# Bluetooth LE transport: findings and plan

Status: research plus a prototype that compiles (`firmware/src/ble.rs`, `--features ble`) and a
Python skeleton (`python/src/nxcontroller/ble.py`, extra `ble`). **Nothing here has run on
hardware yet.** Everything below comes from reading crate sources and from build output.

Goals: (1) encrypted, paired control (today TCP 7700 is open to anyone on the LAN);
(2) control from a browser through Web Bluetooth. WiFi stays. BLE carries the **same frames**.

---

## 1. Crates, features and wiring

| Piece | Choice | Source read |
|---|---|---|
| Controller (HCI) | `esp-radio 1.0.0-beta.1`, features `ble` + `coex` (on top of `wifi`, `unstable`) | `esp-radio-1.0.0-beta.1/Cargo.toml` (`ble = ["esp-hal/__bluetooth", "dep:bt-hci", "dep:bt-hci-transport", …]`), `src/ble/controller/mod.rs` |
| Host stack | `trouble-host 0.7.0`, features `gatt` (default), `security`, `log` | `trouble-host-0.7.0/src/{lib.rs,gatt.rs,connection.rs,security_manager/}` |
| HCI glue | `bt_hci::controller::ExternalController<BleConnector, 20>`, re-exported by `trouble_host::prelude` | `bt-hci-0.9.0/src/controller.rs`; `BleConnector` implements `bt_hci::transport::Transport` (`esp-radio …/ble/controller/mod.rs:331`) |

These are the same versions `esp-generate 1.4.0 -o wifi -o ble-trouble` picks for this esp-hal
generation: esp-hal ~1.2.2, esp-rtos 0.4, esp-radio 1.0.0-beta.1 with `ble` + `coex`,
`trouble-host 0.7.0`, `bt-hci 0.9.0`. We don't need a direct `bt-hci` dependency, because the
prelude re-exports `ExternalController`.

Init, as in the generated reference project and our `main.rs` under `#[cfg(feature = "ble")]`:

```rust
esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);
esp_alloc::heap_allocator!(size: 64 * 1024);          // "COEX needs more RAM" (esp-generate)
esp_rtos::start(timg0.timer0, …);
let wifi = WifiController::new(peripherals.WIFI, …)?;  // WiFi first
let ble  = BleConnector::new(peripherals.BT, Default::default())?;
let controller = ExternalController::<_, 20>::new(ble);
let stack = trouble_host::new(controller, &mut resources)
    .set_random_address(addr).set_io_capabilities(IoCapabilities::NoInputNoOutput).build();
join(stack.runner().run(), app).await;
```

**Coexistence.** The esp-radio README lists ESP32-S3 as supporting "Coex (Wi-Fi + BLE)". The
`coex` feature runs `coex_initialize()` at radio init (`esp-radio src/lib.rs:339`) and hooks the
coexist blob into the WiFi OS adapter (`src/wifi/internal.rs`). Its `build.rs` asserts the chip
has both radios and that `wifi` is enabled. With our versions, WiFi and BLE together **compile and
link** (`cargo clippy --release --features ble -- -D warnings` passes). Caveats:

- There is one 2.4 GHz radio, shared by time division. While BLE advertises or holds a
  connection, WiFi latency and throughput get worse, and the reverse also holds. Long advertising
  intervals help, and so does a sensible connection interval (below).
- **Risk (check first on hardware):** we turn WiFi power saving off (`PowerSaveMode::None`) for
  latency. ESP-IDF refuses that combination when BT is enabled ("Should enable WiFi modem sleep
  when both WiFi and Bluetooth are enabled"), because coex relies on modem sleep to give BT radio
  time. esp-radio's `apply_power_saving` (`src/wifi/mod.rs:2286`) passes the value straight to
  `esp_wifi_set_ps` and adds no such check. So we either get away with it, or the blob errors
  or misbehaves. If it does, BLE builds need `PowerSaveMode::Minimum`, which costs WiFi latency
  (packets wait for the DTIM beacon). That makes BLE the low-latency path in those builds.
- esp-radio tasks are pinned to core 0. Not a problem: we run everything on one executor.

**trouble-host 0.7.0 gotcha:** `security` without `central` does not compile
(`gap::GAP_SERVICE_ATTRIBUTE_COUNT` is cfg'd out for `security && !central`). So we keep the
default features (peripheral + central).

## 2. RAM and flash cost (release, measured)

`xtensa-esp32s3-elf-size -A`, default build vs `--features ble` (prototype with `security`):

| | default | `ble` | delta |
|---|---:|---:|---:|
| `.text` (flash code) | 412,933 | 702,549 | **+283 KB** |
| `.rodata` (+ `.rodata.wifi`) | 91,784 | 138,720 | +46 KB |
| IRAM (`.rwtext*`, `.vectors`) | 44,588 | 66,616 | +22 KB |
| `.data` | 13,584 | 20,412 | +7 KB |
| `.bss` (includes the 64 KB extra heap) | 37,048 | 111,200 | +72 KB |
| main stack (whatever DRAM is left) | 246,536 | 143,520 | −101 KB |
| **flash image (approx.)** | **~550 KB** | **~907 KB** | **+357 KB** |

From symbol sizes (fuzzy because of LTO): trouble-host about 136 KB, the BT controller blob
about 60 KB (more lives in ROM), our `ble.rs` with inlined async code about 24 KB,
p256/aes/cmac (LESC crypto) about 22 KB, bt-hci about 8 KB. The image still fits easily in the
factory partition. The default build's loadable sections are byte-for-byte the same size as
before, so the default build is unchanged.

Heap: 73.7 KB reclaimed plus 64 KB extra, as esp-generate recommends for coex. The controller
blob allocates its buffers at `BleConnector::new`. Static host buffers are small:
`DefaultPacketPool` is 16 × 251 B (about 4 KB), plus 1 connection and 1 L2CAP channel. **To do
on hardware:** log `esp_alloc::HEAP.stats()` after WiFi + BLE come up and while a client is
connected, then size the extra region from that. 140 KB of main stack is still plenty.

## 3. Security

What trouble-host 0.7.0 offers with `security`:

- **LE Secure Connections** pairing (P-256 ECDH). Legacy pairing only comes with the extra
  `legacy-pairing` feature, so with our feature set the stack is effectively SC-only.
  `set_secure_connections_only()` exists when legacy is compiled in.
- IO capabilities: `set_io_capabilities(NoInputNoOutput | DisplayOnly | …)`. The events
  `PassKeyDisplay`, `PassKeyConfirm`, `PassKeyInput`, `PairingComplete { security_level, bond }`,
  `PairingFailed`, `Encrypted { .. }`, `BondLost` arrive on `GattConnection::next()`.
- **Bonding:** `conn.raw().set_bondable(true)` must be called **per connection**; the default is
  not bondable. `PairingComplete.bond: Option<BondInformation>` holds `{ identity: { addr, irk },
  ltk: u128, security_level, is_bonded }`. Restore bonds at boot with
  `stack.add_bond_information()`. `remove_bond_information()` and `with_bond_information()` also
  exist. `HostResources<…, BONDS = 10>` caps the bonds kept in RAM.
- Peripheral-initiated pairing: `conn.raw().request_security()` sends a Security Request right
  after the central connects.
- Per-characteristic permissions in the derive macro: `permissions(encrypted)` or
  `permissions(authenticated)` (MITM), applied to read, write and CCCD. A write or subscribe on an
  unencrypted link becomes a `GattEvent::NotAllowed` and the client gets "insufficient
  encryption", which makes Android, Windows and macOS start pairing on their own.
- Privacy (RPA) via `enable_privacy(irk)`. We don't need it.
- `server.get_client_att_table(conn)` / `set_client_att_table()` save and restore a bonded
  client's CCCD (subscription) state.

**Where to persist bonds.** Settings live in sector 0 (bytes 0x0000–0x0FFF) of the `nvs`
partition, which is 24 KB (6 sectors) in espflash's default table. Proposal: **bond store in
sectors 1–2 (offsets 0x1000 and 0x2000), A/B**. Each sector holds a record of
`magic "NXCB" | version | seq u32 | count | entries[]`. Write the sector that is not current and
bump `seq`. At boot, take the valid record with the higher `seq`. A power cut while saving then
never loses the old bonds. One entry is about 64 B: addr kind + 6 bytes, IRK flag + 16 bytes,
LTK 16 bytes, security level, then the client CCCD table, about 16 bytes for our two
characteristics. Cap it at 4–8 bonds. It reuses `Settings`' `FlashStorage` / `PartitionEntry`
(move bonds into `settings.rs`, or share the flash handle), so flash writes stay serialized
under the same mutex. The web flasher's "erase" wipes the bonds along with the settings, which
is consistent.

**No display, no buttons, so no passkey.** With `NoInputNoOutput`, LESC uses **Just Works**:
the link is encrypted and safe from passive sniffing, but not authenticated. Anyone in range can
pair, and an active MITM during that first pairing goes undetected. `permissions(authenticated)`
is impossible without a passkey or OOB. Mitigations, in the order we recommend:

1. **Pairing window.** Accept new pairings only for N minutes after boot, or after pressing the
   board's BOOT button (GPIO0 is usable at runtime). Outside the window, reject pairing from
   unknown peers. Bonded peers re-encrypt with their stored LTK.
2. **Bond cap and reset.** Keep at most a few bonds. Add a `CLEAR_BONDS` command over an
   already-bonded link, and clear them when the WiFi setup hotspot saves new settings.
3. Optional: an application-level PIN, set on the setup page and checked in HELLO, for
   environments where Just Works isn't enough.
4. `DisplayOnly` with the passkey printed on the serial log is only workable for development.

Note: TCP stays unauthenticated. For the LAN threat, BLE only helps once there is an option to
switch TCP control off, or to require a BLE-provisioned token.

## 4. GATT design (implemented in the prototype)

- Service `a6240001-7a84-4692-ab2b-1fe245e78947`
  - `to_device` `a6240002-…`: **write without response** (plus write), `permissions(encrypted)`.
    Client → device frames.
  - `from_device` `a6240003-…`: **notify**, CCCD `permissions(encrypted)`. Device → client frames.
- Both characteristics carry **byte streams** in the existing `[type][len][payload]` framing. A
  frame can span several ATT PDUs, and one PDU can carry several frames. `ble::FrameAssembler`
  (firmware) and `ble.FrameBuffer` (Python) reassemble them. The device sends each reply as
  `ATT_MTU-3`-sized notifications.
- **MTU:** the trouble-host default packet pool is 251 B, so ATT MTU is up to 247 and payloads up
  to 244 bytes. Our hot-path frames are 9 bytes (GAMEPAD, KEYBOARD) and fit even the minimum MTU
  (23 bytes, 20-byte payload). A frame bigger than the negotiated payload (SET_WIFI, long TYPE
  text) is chunked by the client. Recommended: ask for MTU 247, but stay correct at 23. Browsers
  don't expose the MTU, so the web client chunks at 20 bytes.
- Write without response, not write-with-response: no ATT round trip, so several writes fit in
  one connection event. That is the right fit for state reports. Requests (HELLO, PING,
  SET_MODE…) already get protocol-level replies.
- **Flow control (TYPE_PROGRESS 0x85):** it maps directly onto notifications. Write commands are
  reliable at the link layer, but the ATT spec lets the server drop them when its buffers are
  full, so device pacing matters *more* over BLE. Rules: (a) the client must subscribe before
  HELLO, because `notify_raw` quietly drops notifications to an unsubscribed client; (b) the
  client keeps no more unacknowledged TYPE bytes in flight than the window TYPE_PROGRESS reports;
  (c) the device answers HELLO with feature bit 0x80 on both transports. Notifications rather
  than indications: indications need an ATT confirm per PDU, about one connection interval each.
- **Connection parameters:** after connecting, request a 7.5–15 ms interval, latency 0 and a 4 s
  supervision timeout (`conn.raw().update_connection_params(&stack, …)`). Hosts may clamp it:
  Apple usually allows 15 ms at best, often 30 ms. A 4 s supervision timeout also gives a quick
  "client vanished → release all inputs".
- **Advertising:** the adv data holds Flags + the complete 128-bit service UUID (21 of 31 bytes).
  The scan response holds the complete local name, `DEVICE_NAME` (`NXC_NAME`, default
  "nxcontroller", truncated to 22 bytes for GAP). Appearance is `GENERIC_CONTROL_DEVICE`. We
  avoid the HID gamepad appearance so hosts don't try HID-over-GATT. The address is static
  random, derived from the eFuse BT MAC (`static_address()`), so bonds survive reboots.

## 5. Web Bluetooth

- **Support:** Chrome and Edge on Windows, macOS and ChromeOS, plus Chrome on Android (6+), and
  Opera. Linux desktop is partial and may need `chrome://flags/#enable-experimental-web-platform-features`.
  **No support** in Firefox or Safari, and none on iOS/iPadOS, where every browser is WebKit.
  Third-party "Bluefy" is the iOS workaround.
- Requires a **secure context** (https, or `http://localhost`) and a user gesture.
  `navigator.bluetooth.requestDevice({ filters: [{ services: ["a6240001-7a84-4692-ab2b-1fe245e78947"] }] })`
  opens the browser's chooser. The UUID has to be in the filter or in `optionalServices` to be
  usable, and must appear in the advertisement or scan response (it does). An extra
  `namePrefix` filter is optional.
- Then `device.gatt.connect()` → `getPrimaryService` → `getCharacteristic`, plus
  `fromDevice.startNotifications()` and `toDevice.writeValueWithoutResponse(chunk)`. Chunk writes
  at 20 bytes: the MTU is hidden, and a too-large write-without-response errors on some
  platforms. Long writes (prepare/execute) would need trouble-host's `att-queued-writes`.
- **Pairing:** there is no pairing API. When an encrypted characteristic is accessed (the
  `startNotifications` CCCD write fails with insufficient encryption, or after our Security
  Request), the OS pairs. Android shows a "Pair with nxcontroller?" system dialog. Windows pairs
  with Just Works, possibly behind a Chrome or OS prompt. macOS pairs through CoreBluetooth,
  maybe with a system prompt depending on the version. After that, the OS keeps the bond and
  later connections re-encrypt silently. This is why bonds must persist on the device: a device
  that forgot its bonds makes macOS and Windows fail to re-encrypt until the user removes the
  device in OS Bluetooth settings.
- Reconnecting without the chooser (`navigator.bluetooth.getDevices()`, `watchAdvertisements`)
  depends on the browser version and flags. Assume one chooser click per page load.
- On Chrome for macOS, the browser itself needs Bluetooth permission in System Settings → Privacy.
- Hosting: the existing `web/` installer is already served over https/localhost, so a
  `web/control.html` fits there.

## 6. Python (`bleak`)

- `bleak` (3.0.2 at the time of writing) is asyncio-based and runs on macOS (CoreBluetooth),
  Windows (WinRT) and Linux (BlueZ). It's an **optional extra**: `pip install 'nxcontroller[ble]'`.
  `nxcontroller.ble` imports without it, and the 16 existing and new tests pass without it.
- **macOS:** the terminal or IDE running Python needs Bluetooth permission (a prompt on first use,
  then Privacy & Security → Bluetooth). Addresses show up as per-Mac CoreBluetooth UUIDs, not
  MACs, so scan by the service UUID (`BleakScanner.discover(service_uuids=[…])`, which macOS
  12.0–12.2 *requires*). `BleakClient.pair()` doesn't exist on macOS. Pairing happens implicitly
  when `start_notify` writes the protected CCCD. That's how the skeleton does it.
- **Latency:** a write-without-response arrives on the next connection event, about half to one
  interval. Expect about 8–15 ms one way on Android/Windows at 7.5–15 ms intervals, and about
  15–30 ms on macOS. A PING round trip is about 2 intervals (macOS: 30–60 ms). TCP over WiFi
  with power saving off measures a few ms. With coex on, WiFi latency gets worse while BLE is
  connected.
- **Throughput:** far more than we need. 9-byte reports at 125 Hz is about 1.1 kB/s, and BLE 1M
  PHY does tens of kB/s even with conservative parameters. TYPE text is bounded by USB report
  pacing, not by BLE.
- Integration: `Controller` is built around a blocking socket plus a reader thread. Cleanest
  path: extract a small `Link` interface (`send(bytes)`, `recv()` → frames), with `TcpLink` and
  `BleLink`. `BleLink` runs bleak on a private asyncio loop in a background thread and exposes
  blocking `sendall` / queue-based receive. Then `Controller.connect_ble()` reuses all the
  input logic unchanged.

## 7. Making `serve()` transport-agnostic (for the main session)

Don't duplicate the dispatcher. `ble::handle_frame` is a stopgap subset: HELLO, PING, GAMEPAD,
KEYBOARD, RELEASE_ALL, and ERROR for the rest. Proposed shape, in a new `firmware/src/control.rs`:

```rust
/// What the transport should do after a frame.
pub enum Flow { Continue, Restart }

/// Sink for device -> client frames (TCP socket writer, BLE notifier).
pub trait FrameSink {
    async fn send(&mut self, frame: &[u8]) -> Result<(), ()>;
}

/// Per-connection state: typing queue/window, seq, etc.
pub struct Session { /* … */ }

impl Session {
    pub fn new(settings: SharedSettings) -> Self;
    /// Handle one complete client frame; write any reply (or replies) to `sink`.
    pub async fn handle(&mut self, msg_type: u8, payload: &[u8], sink: &mut impl FrameSink) -> Flow;
    /// Device-initiated frames (e.g. TYPE_PROGRESS) that became due; the transport
    /// `select`s on this alongside reading the next frame.
    pub async fn next_event(&mut self, sink: &mut impl FrameSink);
    /// Connection gone: release inputs, drop queued typing.
    pub fn end(&mut self);
}
```

- TCP `serve()` becomes: read header + payload → `session.handle(...)`, with a
  `select(read, session.next_event())` loop. `Flow::Restart` → close + `restart()`. This is
  exactly today's body, moved, with `out`/`write_all` behind `FrameSink`.
- BLE `serve()`: a GATT write → `FrameAssembler` → `session.handle(...)`, with the sink calling
  `notify_raw` in MTU-sized chunks. Disconnect → `session.end()`.
- **Single-client arbitration:** today TCP accepts one client. Add a global "control owner"
  (e.g. `AtomicU8` none/tcp/ble) taken at HELLO and released at disconnect. A second transport
  gets `ERROR busy`, which needs a new error code 7, a protocol change, so bump the docs. That
  stops a TCP and a BLE client from fighting over the same report. Releasing inputs on
  disconnect stays per-owner.

## 8. Plan to finish BLE (after paced typing merges)

1. **Hardware smoke test** of the prototype: boot with WiFi + BLE, check that WiFi still joins
   with `PowerSaveMode::None` (the coex risk above), log heap stats, connect with nRF Connect,
   pair (Just Works), and PING over notifications.
2. Extract `control::Session` (section 7) from `net::serve()`, including TYPE/TYPE_PROGRESS, and
   switch TCP over with no behaviour change. Delete `ble::handle_frame`.
3. BLE uses `Session`; add the control-owner arbitration and the protocol doc for the BLE
   transport (UUIDs, chunking, "subscribe before HELLO").
4. Bond persistence (nvs sectors 1–2, A/B), restored with `add_bond_information` at boot, and
   saving `PairingComplete.bond` plus the CCCD table. Add the pairing window and the
   BOOT-button gesture.
5. Connection-parameter request, and tuning of the advertising interval (fast for 30 s after
   boot or disconnect, then slow, to spare WiFi airtime).
6. Python: the `Link` abstraction and `Controller.connect_ble()`, `nxc --ble …` in the CLI, and
   BLE in `mock.py` (or a loopback link) for tests.
7. `web/control.html`: a Web Bluetooth gamepad/keyboard page using the same framing.
8. Build matrix: CI/clippy for both `default` and `--features ble`. The web flasher stays
   WiFi-only unless we ship a second image.

**Risks:** coex vs WiFi power saving (WiFi latency in BLE builds); the heap needed under coex;
trouble-host and esp-radio are pre-1.0 and BLE in esp-radio is `unstable` (API churn on
updates); Just Works can't authenticate (we depend on the pairing window); macOS connection
intervals limit BLE latency to about 15–30 ms; image size +357 KB (fine for 4 MB+ flash, but
OTA would need two such slots).
