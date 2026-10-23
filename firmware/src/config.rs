//! Compile-time configuration, injected from `firmware/.env` by `scripts/build.sh`.

pub const WIFI_SSID: &str = env!(
    "WIFI_SSID",
    "WIFI_SSID is not set: copy firmware/.env.example to firmware/.env and build via scripts/build.sh"
);
pub const WIFI_PASSWORD: &str = env!(
    "WIFI_PASSWORD",
    "WIFI_PASSWORD is not set: copy firmware/.env.example to firmware/.env and build via scripts/build.sh"
);
pub const DEVICE_NAME: &str = match option_env!("NXC_NAME") {
    Some(name) if !name.is_empty() => name,
    _ => "nxcontroller",
};
