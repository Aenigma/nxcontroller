//! Compile-time configuration, injected from `firmware/.env` by the build scripts.
//!
//! WiFi credentials are optional. A build without them (like the web flasher's)
//! starts the setup hotspot on first boot. Credentials saved on the device
//! always take precedence over these.

pub const WIFI_SSID: Option<&str> = match option_env!("WIFI_SSID") {
    Some(ssid) if !ssid.is_empty() => Some(ssid),
    _ => None,
};
pub const WIFI_PASSWORD: &str = match option_env!("WIFI_PASSWORD") {
    Some(password) => password,
    None => "",
};
pub const DEVICE_NAME: &str = match option_env!("NXC_NAME") {
    Some(name) if !name.is_empty() => name,
    _ => "nxcontroller",
};

/// Name of the open hotspot the board starts when it has no working WiFi.
pub const SETUP_SSID: &str = "nxcontroller-setup";
