//! WiFi station, TCP control server and UDP discovery responder.

use embassy_net::tcp::TcpSocket;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{Runner, Stack};
use embassy_time::{Duration, Timer};
use embedded_io_async::{Read, Write};
use esp_radio::wifi::{Interface, WifiController};
use log::{info, warn};

use crate::config::DEVICE_NAME;
use crate::protocol::{self, Command, error, msg};
use crate::settings::{Mode, Settings};
use crate::usb;

#[embassy_executor::task]
pub async fn wifi_task(mut controller: WifiController<'static>) {
    loop {
        info!("WiFi: connecting to '{}'", crate::config::WIFI_SSID);
        match controller.connect_async().await {
            Ok(_) => {
                info!("WiFi: connected");
                let _ = controller.wait_for_disconnect_async().await;
                warn!("WiFi: disconnected");
            }
            Err(e) => warn!("WiFi: connect failed: {e:?}"),
        }
        Timer::after(Duration::from_secs(3)).await;
    }
}

#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, Interface>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
pub async fn control_server_task(stack: Stack<'static>, mut settings: Settings) {
    let mut rx_buf = [0u8; 1024];
    let mut tx_buf = [0u8; 512];

    stack.wait_config_up().await;
    if let Some(cfg) = stack.config_v4() {
        info!(
            "Ready: '{}' at {} (tcp {}, discovery udp {})",
            DEVICE_NAME,
            cfg.address.address(),
            protocol::TCP_PORT,
            protocol::DISCOVERY_PORT
        );
    }

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
        // Keep-alives let us notice a client that vanished (laptop asleep, WiFi
        // dropped) so held buttons get released.
        socket.set_keep_alive(Some(Duration::from_secs(5)));
        socket.set_timeout(Some(Duration::from_secs(15)));
        socket.set_nagle_enabled(false);

        if let Err(e) = socket.accept(protocol::TCP_PORT).await {
            warn!("accept failed: {e:?}");
            continue;
        }
        info!("client connected from {:?}", socket.remote_endpoint());

        let restart = serve(&mut socket, &mut settings).await;

        usb::release_all();
        if restart {
            // Let the MODE_ACK reach the client, then reboot into the new mode.
            socket.close();
            let _ = socket.flush().await;
            Timer::after(Duration::from_millis(300)).await;
            info!("restarting into {:?} mode", settings.mode());
            esp_hal::system::software_reset();
        }
        socket.abort();
        let _ = socket.flush().await;
        info!("client disconnected, inputs released");
    }
}

/// Handle one client. Returns true if the device should restart (mode change).
async fn serve(socket: &mut TcpSocket<'_>, settings: &mut Settings) -> bool {
    let mut header = [0u8; 2];
    let mut payload = [0u8; 255];
    let mut out = [0u8; 64];

    loop {
        if socket.read_exact(&mut header).await.is_err() {
            return false;
        }
        let len = header[1] as usize;
        if socket.read_exact(&mut payload[..len]).await.is_err() {
            return false;
        }
        let mut restart = false;

        let reply = match protocol::parse(header[0], &payload[..len]) {
            Ok(Command::Hello { version }) if version == protocol::PROTOCOL_VERSION => {
                let mut body = [0u8; 34];
                let features = usb::mode() as u8; // mode values are the feature bits
                let name = &DEVICE_NAME.as_bytes()[..DEVICE_NAME.len().min(32)];
                body[0] = protocol::PROTOCOL_VERSION;
                body[1] = features;
                body[2..2 + name.len()].copy_from_slice(name);
                Some(protocol::frame(
                    &mut out,
                    msg::HELLO_ACK,
                    &body[..2 + name.len()],
                ))
            }
            Ok(Command::Hello { .. }) => Some(protocol::frame(
                &mut out,
                msg::ERROR,
                &[error::VERSION_MISMATCH],
            )),
            Ok(Command::Ping { seq }) => {
                let s = seq.to_le_bytes();
                let body = [s[0], s[1], s[2], s[3], usb::is_configured() as u8];
                Some(protocol::frame(&mut out, msg::PONG, &body))
            }
            Ok(Command::SetMode { mode }) => match Mode::from_u8(mode) {
                None => Some(protocol::frame(
                    &mut out,
                    msg::ERROR,
                    &[error::INVALID_VALUE],
                )),
                Some(new) if new == usb::mode() => {
                    Some(protocol::frame(&mut out, msg::MODE_ACK, &[mode, 0]))
                }
                Some(new) => match settings.set_mode(new) {
                    Ok(()) => {
                        info!("mode set to {new:?}");
                        restart = true;
                        Some(protocol::frame(&mut out, msg::MODE_ACK, &[mode, 1]))
                    }
                    Err(e) => {
                        warn!("saving mode failed: {e:?}");
                        Some(protocol::frame(&mut out, msg::ERROR, &[error::STORAGE]))
                    }
                },
            },
            Ok(Command::Gamepad(report)) if usb::mode().has_gamepad() => {
                usb::submit_gamepad(report);
                None
            }
            Ok(Command::Keyboard(report)) if usb::mode().has_keyboard() => {
                usb::submit_keyboard(report);
                None
            }
            Ok(Command::Gamepad(_)) | Ok(Command::Keyboard(_)) => {
                Some(protocol::frame(&mut out, msg::ERROR, &[error::UNSUPPORTED]))
            }
            Ok(Command::ReleaseAll) => {
                usb::release_all();
                None
            }
            Err(code) => Some(protocol::frame(&mut out, msg::ERROR, &[code])),
        };

        if let Some(reply) = reply
            && (socket.write_all(reply).await.is_err() || socket.flush().await.is_err())
        {
            return restart;
        }
        if restart {
            return true;
        }
    }
}

#[embassy_executor::task]
pub async fn discovery_task(stack: Stack<'static>) {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; 128];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_buf = [0u8; 256];

    stack.wait_config_up().await;

    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if let Err(e) = socket.bind(protocol::DISCOVERY_PORT) {
        warn!("discovery bind failed: {e:?}");
        return;
    }

    let mut reply = [0u8; 64];
    let name = &DEVICE_NAME.as_bytes()[..DEVICE_NAME.len().min(32)];
    reply[..4].copy_from_slice(protocol::DISCOVER_REPLY_MAGIC);
    reply[4] = protocol::PROTOCOL_VERSION;
    reply[5..7].copy_from_slice(&protocol::TCP_PORT.to_le_bytes());
    reply[7..7 + name.len()].copy_from_slice(name);
    let reply = &reply[..7 + name.len()];

    let mut buf = [0u8; 64];
    loop {
        let Ok((n, meta)) = socket.recv_from(&mut buf).await else {
            continue;
        };
        // Reply to the sender only. Echoing `meta` back would reuse the broadcast
        // destination as our source address.
        if &buf[..n] == protocol::DISCOVER_REQUEST
            && let Err(e) = socket.send_to(reply, meta.endpoint).await
        {
            warn!("discovery reply failed: {e:?}");
        }
    }
}
