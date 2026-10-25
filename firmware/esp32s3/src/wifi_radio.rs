//! The ESP32-S3 WiFi radio: joins the saved network, falling back to the setup
//! hotspot. The control server and discovery on top are in the core crate.

use embassy_net::Runner;
use embassy_time::{Duration, Timer};
use esp_radio::wifi::ap::AccessPointConfig;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{Config as WifiConfig, Interface, WifiController};
use log::{info, warn};
use nxcontroller_core::config::SETUP_SSID;
use nxcontroller_core::setup;

/// Failed connection attempts before the setup hotspot comes up alongside.
const FAILURES_BEFORE_SETUP: u32 = 3;

/// Joins `station` if we have credentials. Without them, or after repeated
/// failures, opens the setup hotspot (while still retrying the network, so a
/// router that was merely slow to boot still gets picked up).
#[allow(
    clippy::large_stack_frames,
    reason = "a task's future lives in static task storage, not on a stack"
)]
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

#[allow(
    clippy::large_stack_frames,
    reason = "a task's future lives in static task storage, not on a stack"
)]
#[embassy_executor::task(pool_size = 2)]
pub async fn net_task(mut runner: Runner<'static, Interface>) -> ! {
    runner.run().await
}
