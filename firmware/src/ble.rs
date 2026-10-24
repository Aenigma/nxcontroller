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
//! Frames go through the same handler as TCP ([`crate::control`]), so every
//! message works over BLE, including TYPE with TYPE_PROGRESS notifications.
//!
//! Limits (see `docs/ble-findings.md`):
//! - No bonding: each connection pairs afresh (still encrypted). The board
//!   restarts on mode/option/WiFi changes, and a bond kept only in RAM would
//!   leave the client with stale keys it can't reconnect with.
//! - No arbitration with the TCP client: both can drive the USB reports, and
//!   typing progress goes to whichever transport is waiting for it.

use embassy_futures::join::join;
use embassy_futures::select::{Either, select};
use esp_radio::ble::controller::BleConnector;
use log::{info, warn};
use trouble_host::prelude::*;

use crate::config::DEVICE_NAME;
use crate::{SharedSettings, control, restart, usb};

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
pub async fn ble_task(connector: BleConnector<'static>, settings: SharedSettings) {
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
                    let needs_restart = serve(&conn, &server, settings).await;
                    usb::release_all();
                    if needs_restart {
                        info!("BLE: restarting to apply new settings");
                        restart().await;
                    }
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

/// Serves one central until it disconnects. Returns true if the board should
/// restart (settings changed; the reply has already been sent).
async fn serve(
    conn: &GattConnection<'_, '_, DefaultPacketPool>,
    server: &Server<'_>,
    settings: SharedSettings,
) -> bool {
    // Pair, but don't bond: see the module docs.
    if let Err(e) = conn.raw().set_bondable(false) {
        warn!("BLE: set_bondable failed: {e:?}");
    }
    // Ask the central to pair right away, rather than waiting for its first
    // write to fail with "insufficient encryption".
    if let Err(e) = conn.raw().request_security() {
        warn!("BLE: security request failed: {e:?}");
    }

    let mut frames = FrameAssembler::new();
    let mut write = [0u8; MAX_ATT_PAYLOAD];
    let mut out = [0u8; control::REPLY_MAX];
    let mut progress = [0u8; 8];
    loop {
        // Wait for the client, forwarding typing progress in the meantime.
        let event = match select(conn.next(), control::progress(&mut progress)).await {
            Either::First(event) => event,
            Either::Second(None) => continue,
            Either::Second(Some(frame)) => {
                if let Err(e) = send(conn, &server.control.from_device, frame).await {
                    warn!("BLE: notify failed: {e:?}");
                }
                continue;
            }
        };
        let len = match event {
            GattConnectionEvent::Disconnected { reason } => {
                info!("BLE: disconnected: {reason:?}");
                return false;
            }
            GattConnectionEvent::PairingComplete { security_level, .. } => {
                info!("BLE: paired ({security_level:?})");
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
            let mut needs_restart = false;
            let reply =
                control::handle(msg_type, payload, settings, &mut out, &mut needs_restart).await;
            if let Some(reply) = reply
                && let Err(e) = send(conn, &server.control.from_device, reply).await
            {
                warn!("BLE: notify failed: {e:?}");
            }
            frames.clear();
            if needs_restart {
                return true;
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
