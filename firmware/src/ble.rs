//! Bluetooth LE control transport (cargo feature `ble`). **Prototype.**
//!
//! One custom GATT service carries the same `[type u8][len u8][payload]` frames
//! as the TCP control channel (see `protocol/PROTOCOL.md`):
//!
//! - `to_device` (write, write without response): client -> device byte stream.
//! - `from_device` (notify): device -> client byte stream.
//!
//! Both directions are byte streams: a frame may span several ATT writes or
//! notifications, and one write may hold several frames. Both characteristics
//! (including the notify CCCD) require an encrypted link, so a client must pair
//! (LE Secure Connections, Just Works: the board has no display or buttons)
//! before it can subscribe or send input.
//!
//! Prototype limits (see `docs/ble-findings.md`):
//! - Bonds live in RAM only and are forgotten on reboot.
//! - Only HELLO, PING, GAMEPAD, KEYBOARD and RELEASE_ALL are handled here.
//!   SET_MODE / SET_WIFI answer `ERROR unsupported`. The plan is to replace
//!   [`handle_frame`] with the transport-agnostic handler extracted from
//!   `net::serve()`.
//! - No arbitration with the TCP client: both can drive the USB reports.

use embassy_futures::join::join;
use esp_radio::ble::controller::BleConnector;
use log::{info, warn};
use trouble_host::prelude::*;

use crate::config::DEVICE_NAME;
use crate::protocol::{self, Command, error, msg};
use crate::usb;

/// `a6240001-7a84-4692-ab2b-1fe245e78947`
pub const SERVICE_UUID: u128 = 0xa6240001_7a84_4692_ab2b_1fe245e78947;
/// `a6240002-...`: client -> device frames (write / write without response).
pub const TO_DEVICE_UUID: u128 = 0xa6240002_7a84_4692_ab2b_1fe245e78947;
/// `a6240003-...`: device -> client frames (notify).
pub const FROM_DEVICE_UUID: u128 = 0xa6240003_7a84_4692_ab2b_1fe245e78947;

/// One central at a time, like the TCP server.
const CONNECTIONS_MAX: usize = 1;
/// L2CAP channels: ATT and SMP use fixed channels, no CoC needed.
const L2CAP_CHANNELS_MAX: usize = 1;
/// Command slots in the HCI controller adapter.
const HCI_SLOTS: usize = 20;
/// Largest ATT payload with the default packet pool (251-byte L2CAP SDU,
/// ATT MTU 247, minus the 3-byte ATT header).
const MAX_ATT_PAYLOAD: usize = 244;
/// GAP device name limit (it has to fit the scan response).
const MAX_GAP_NAME: usize = 22;

#[gatt_server(connections_max = CONNECTIONS_MAX)]
struct Server {
    control: ControlService,
}

#[gatt_service(uuid = SERVICE_UUID)]
struct ControlService {
    /// Client -> device frames. Writes are consumed, never stored.
    #[characteristic(uuid = TO_DEVICE_UUID, write, write_without_response, value = [0; MAX_ATT_PAYLOAD], permissions(encrypted))]
    to_device: [u8; MAX_ATT_PAYLOAD],
    /// Device -> client frames.
    #[characteristic(uuid = FROM_DEVICE_UUID, notify, value = [0; MAX_ATT_PAYLOAD], permissions(encrypted))]
    from_device: [u8; MAX_ATT_PAYLOAD],
}

/// Runs the BLE host: advertises, accepts one central at a time and serves it.
#[embassy_executor::task]
pub async fn ble_task(connector: BleConnector<'static>) {
    let controller: ExternalController<_, HCI_SLOTS> = ExternalController::new(connector);
    let mut resources: HostResources<_, DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX> =
        HostResources::new();
    let stack = trouble_host::new(controller, &mut resources)
        .set_random_address(static_address())
        .set_io_capabilities(IoCapabilities::NoInputNoOutput)
        .build();
    let mut runner = stack.runner();
    let mut peripheral = stack.peripheral();

    let name = &DEVICE_NAME[..DEVICE_NAME.len().min(MAX_GAP_NAME)];
    let server = match Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name,
        // Not a HID appearance: hosts must not treat this as a BLE HID device.
        appearance: &appearance::control_device::GENERIC_CONTROL_DEVICE,
    })) {
        Ok(server) => server,
        Err(e) => {
            warn!("BLE: building the GATT server failed: {e}");
            return;
        }
    };

    let app = async {
        loop {
            match advertise(name, &mut peripheral, &server).await {
                Ok(conn) => {
                    serve(&conn, &server).await;
                    usb::release_all();
                    info!("BLE: client disconnected, inputs released");
                }
                Err(e) => warn!("BLE: advertising failed: {e:?}"),
            }
        }
    };
    let (result, ()) = join(runner.run(), app).await;
    if let Err(e) = result {
        warn!("BLE: host stopped: {e:?}");
    }
}

/// A static random address derived from the chip's Bluetooth MAC, so it stays
/// the same across reboots (bonded peers find us by it).
fn static_address() -> Address {
    let mac = esp_hal::efuse::interface_mac_address(esp_hal::efuse::InterfaceMacAddress::Bluetooth);
    let mut addr = [0u8; 6];
    // BdAddr is little-endian; the MAC prints most significant byte first.
    for (dst, src) in addr.iter_mut().zip(mac.as_bytes().iter().rev()) {
        *dst = *src;
    }
    addr[5] |= 0xC0; // static random address: the two top bits are set
    Address::random(addr)
}

async fn advertise<'values, 'server, C: Controller>(
    name: &str,
    peripheral: &mut Peripheral<'values, C, DefaultPacketPool>,
    server: &'server Server<'values>,
) -> Result<GattConnection<'values, 'server, DefaultPacketPool>, BleHostError<C::Error>> {
    let service = SERVICE_UUID.to_le_bytes();
    let mut adv_data = [0; 31];
    let adv_len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::CompleteServiceUuids128(&[service]),
        ],
        &mut adv_data[..],
    )?;
    let mut scan_data = [0; 31];
    let scan_len = AdStructure::encode_slice(
        &[AdStructure::CompleteLocalName(name.as_bytes())],
        &mut scan_data[..],
    )?;
    info!("BLE: advertising as '{name}'");
    let acceptor = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &adv_data[..adv_len],
                scan_data: &scan_data[..scan_len],
            },
        )
        .await?;
    let conn = acceptor.accept().await?.with_attribute_server(server)?;
    info!("BLE: connected to {:?}", conn.raw().peer_address());
    Ok(conn)
}

/// Serves one central until it disconnects.
async fn serve(conn: &GattConnection<'_, '_, DefaultPacketPool>, server: &Server<'_>) {
    // Bonding needs both sides bondable. The bond is only kept in RAM for now.
    if let Err(e) = conn.raw().set_bondable(true) {
        warn!("BLE: set_bondable failed: {e:?}");
    }
    // Ask the central to pair right away, rather than waiting for its first
    // write to fail with "insufficient encryption".
    if let Err(e) = conn.raw().request_security() {
        warn!("BLE: security request failed: {e:?}");
    }

    let mut frames = FrameAssembler::new();
    let mut write = [0u8; MAX_ATT_PAYLOAD];
    loop {
        let len = match conn.next().await {
            GattConnectionEvent::Disconnected { reason } => {
                info!("BLE: disconnected: {reason:?}");
                return;
            }
            GattConnectionEvent::PairingComplete {
                security_level,
                bond,
            } => {
                info!(
                    "BLE: paired ({security_level:?}, bonded: {})",
                    bond.is_some()
                );
                continue;
            }
            GattConnectionEvent::PairingFailed(e) => {
                warn!("BLE: pairing failed: {e:?}");
                continue;
            }
            GattConnectionEvent::Gatt {
                event: GattEvent::Write(event),
            } if event.handle() == server.control.to_device.handle => {
                let len = event.with_data(|offset, data| {
                    let len = data.len().min(write.len());
                    write[..len].copy_from_slice(&data[..len]);
                    if offset == 0 { len } else { 0 }
                });
                // The bytes are a stream for us, not a stored value.
                match event.accept_unprocessed() {
                    Ok(reply) => reply.send().await,
                    Err(e) => warn!("BLE: write reply failed: {e:?}"),
                }
                len
            }
            // Everything else (reads of the GAP service, CCCD writes, permission
            // failures) is handled by the attribute server when dropped.
            _ => continue,
        };

        let mut data = &write[..len];
        while !data.is_empty() {
            let taken = frames.push(data);
            data = &data[taken..];
            let Some((msg_type, payload)) = frames.frame() else {
                continue;
            };
            let mut out = [0u8; 64];
            let reply = handle_frame(msg_type, payload, &mut out);
            frames.clear();
            if let Some(reply) = reply
                && let Err(e) = send(conn, &server.control.from_device, reply).await
            {
                warn!("BLE: notify failed: {e:?}");
            }
        }
    }
}

/// Sends `frame` as one or more notifications of at most ATT_MTU - 3 bytes.
///
/// Silently does nothing while the client hasn't subscribed.
async fn send(
    conn: &GattConnection<'_, '_, DefaultPacketPool>,
    characteristic: &Characteristic<[u8; MAX_ATT_PAYLOAD]>,
    frame: &[u8],
) -> Result<(), Error> {
    let chunk = (conn.raw().att_mtu() as usize).saturating_sub(3).max(20);
    for part in frame.chunks(chunk) {
        characteristic.notify_raw(conn, part, false).await?;
    }
    Ok(())
}

/// Prototype dispatcher, to be replaced by the handler shared with TCP.
fn handle_frame<'a>(msg_type: u8, payload: &[u8], out: &'a mut [u8]) -> Option<&'a [u8]> {
    match protocol::parse(msg_type, payload) {
        Ok(Command::Hello { version }) if version == protocol::PROTOCOL_VERSION => {
            let mut body = [0u8; 34];
            let name = &DEVICE_NAME.as_bytes()[..DEVICE_NAME.len().min(32)];
            body[0] = protocol::PROTOCOL_VERSION;
            body[1] = usb::mode() as u8;
            body[2..2 + name.len()].copy_from_slice(name);
            Some(protocol::frame(
                out,
                msg::HELLO_ACK,
                &body[..2 + name.len()],
            ))
        }
        Ok(Command::Hello { .. }) => {
            Some(protocol::frame(out, msg::ERROR, &[error::VERSION_MISMATCH]))
        }
        Ok(Command::Ping { seq }) => {
            let s = seq.to_le_bytes();
            let body = [s[0], s[1], s[2], s[3], usb::is_configured() as u8];
            Some(protocol::frame(out, msg::PONG, &body))
        }
        Ok(Command::Gamepad(report)) if usb::mode().has_gamepad() => {
            usb::submit_gamepad(report);
            None
        }
        Ok(Command::Keyboard(report)) if usb::mode().has_keyboard() => {
            usb::submit_keyboard(report);
            None
        }
        Ok(Command::ReleaseAll) => {
            usb::release_all();
            None
        }
        // SET_MODE / SET_WIFI need the shared settings and a restart; they come
        // with the handler shared with TCP.
        Ok(_) => Some(protocol::frame(out, msg::ERROR, &[error::UNSUPPORTED])),
        Err(code) => Some(protocol::frame(out, msg::ERROR, &[code])),
    }
}

/// Reassembles `[type][len][payload]` frames from a byte stream.
pub struct FrameAssembler {
    buf: [u8; 2 + 255],
    len: usize,
}

impl Default for FrameAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameAssembler {
    pub const fn new() -> Self {
        Self {
            buf: [0; 2 + 255],
            len: 0,
        }
    }

    /// Bytes the frame in progress still needs.
    fn needed(&self) -> usize {
        if self.len < 2 {
            2 - self.len
        } else {
            2 + self.buf[1] as usize - self.len
        }
    }

    /// Takes bytes from `data` until the current frame is complete, and returns
    /// how many it took. Call [`Self::frame`] afterwards.
    pub fn push(&mut self, data: &[u8]) -> usize {
        let mut taken = 0;
        while taken < data.len() && self.frame().is_none() {
            let n = self.needed().min(data.len() - taken);
            self.buf[self.len..self.len + n].copy_from_slice(&data[taken..taken + n]);
            self.len += n;
            taken += n;
        }
        taken
    }

    /// The complete frame, if there is one: `(type, payload)`.
    pub fn frame(&self) -> Option<(u8, &[u8])> {
        (self.len >= 2 && self.needed() == 0).then(|| (self.buf[0], &self.buf[2..self.len]))
    }

    /// Drops the complete frame (or any partial one).
    pub fn clear(&mut self) {
        self.len = 0;
    }
}
