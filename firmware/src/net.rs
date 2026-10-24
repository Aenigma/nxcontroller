//! WiFi (station, with the setup hotspot as fallback), TCP control server and
//! UDP discovery responder.

use embassy_futures::select::{Either3, select3};
use embassy_net::tcp::TcpSocket;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{Runner, Stack};
use embassy_time::{Duration, Timer};
use embedded_io_async::{Read, Write};
use esp_radio::wifi::ap::AccessPointConfig;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{Config as WifiConfig, Interface, WifiController};
use log::{info, warn};

use crate::config::{DEVICE_NAME, SETUP_SSID};
use crate::logbuf::Reader;
use crate::protocol;
use crate::{SharedSettings, control, restart, setup, usb};

/// Failed connection attempts before the setup hotspot comes up alongside.
const FAILURES_BEFORE_SETUP: u32 = 3;

/// Joins `station` if we have credentials. Without them, or after repeated
/// failures, opens the setup hotspot (while still retrying the network, so a
/// router that was merely slow to boot still gets picked up).
#[embassy_executor::task]
pub async fn wifi_task(mut controller: WifiController<'static>, station: Option<StationConfig>) {
    let ap = AccessPointConfig::default().with_ssid(SETUP_SSID.try_into().unwrap());

    let Some(station) = station else {
        info!("WiFi: no credentials saved");
        match controller.set_config(&WifiConfig::AccessPoint(ap)) {
            Ok(()) => setup::announce(),
            Err(e) => warn!("WiFi: starting setup hotspot failed: {e:?}"),
        }
        // Keep the controller alive: dropping it shuts WiFi down.
        loop {
            Timer::after(Duration::from_secs(3600)).await;
        }
    };

    let mut failures = 0;
    let mut hotspot_on = false;
    loop {
        info!("WiFi: connecting");
        match controller.connect_async().await {
            Ok(_) => {
                info!("WiFi: connected");
                failures = 0;
                if hotspot_on {
                    // Back to station-only. Changing the config drops the link; reconnect.
                    info!("WiFi: closing setup hotspot");
                    let _ = controller.set_config(&WifiConfig::Station(station.clone()));
                    hotspot_on = false;
                    continue;
                }
                let _ = controller.wait_for_disconnect_async().await;
                warn!("WiFi: disconnected");
            }
            Err(e) => {
                warn!("WiFi: connect failed: {e:?}");
                failures += 1;
                if failures >= FAILURES_BEFORE_SETUP && !hotspot_on {
                    let both = WifiConfig::AccessPointStation(station.clone(), ap.clone());
                    match controller.set_config(&both) {
                        Ok(()) => {
                            hotspot_on = true;
                            setup::announce();
                        }
                        Err(e) => warn!("WiFi: starting setup hotspot failed: {e:?}"),
                    }
                }
            }
        }
        Timer::after(Duration::from_secs(3)).await;
    }
}

#[embassy_executor::task(pool_size = 2)]
pub async fn net_task(mut runner: Runner<'static, Interface>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
pub async fn control_server_task(stack: Stack<'static>, settings: SharedSettings) {
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

        let needs_restart = serve(&mut socket, settings).await;

        usb::release_all();
        if needs_restart {
            // Let the ack reach the client, then reboot with the new settings.
            socket.close();
            let _ = socket.flush().await;
            info!("restarting to apply new settings");
            restart().await;
        }
        socket.abort();
        let _ = socket.flush().await;
        info!("client disconnected, inputs released");
    }
}

/// Handle one client. Returns true if the device should restart (new settings).
async fn serve(socket: &mut TcpSocket<'_>, settings: SharedSettings) -> bool {
    let mut header = [0u8; 2];
    let mut payload = [0u8; 255];
    let mut out = [0u8; control::REPLY_MAX];
    let mut log_out = [0u8; control::LOG_FRAME_MAX];
    let mut session = control::Session::new(Reader::Tcp);

    loop {
        // Wait for the next message, sending typing progress and log output in the
        // meantime: the client holds back further TYPE messages until it hears
        // about progress. (TcpSocket::read is cancel-safe: losing the race drops no data.)
        let frame = match select3(
            socket.read(&mut header[..1]),
            control::progress(&mut out),
            control::next_log(&mut session, &mut log_out),
        )
        .await
        {
            Either3::First(Ok(n)) if n > 0 => None,
            Either3::First(_) => return false,
            Either3::Second(frame) => Some(frame),
            Either3::Third(frame) => Some(Some(frame)),
        };
        if let Some(frame) = frame {
            if let Some(frame) = frame
                && (socket.write_all(frame).await.is_err() || socket.flush().await.is_err())
            {
                return false;
            }
            continue;
        }
        if socket.read_exact(&mut header[1..]).await.is_err() {
            return false;
        }
        let len = header[1] as usize;
        if socket.read_exact(&mut payload[..len]).await.is_err() {
            return false;
        }
        let reply =
            control::handle(header[0], &payload[..len], settings, &mut out, &mut session).await;

        if let Some(reply) = reply
            && (socket.write_all(reply).await.is_err() || socket.flush().await.is_err())
        {
            return session.restart;
        }
        if session.restart {
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
