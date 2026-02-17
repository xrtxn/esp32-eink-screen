use embassy_net::Runner;
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_println::println;
use esp_radio::wifi::{
    ClientConfig, ModeConfig, ScanConfig, WifiController, WifiDevice, WifiEvent, WifiStaState,
};

use esp_backtrace as _;
use log::{info, warn};

#[embassy_executor::task]
pub async fn connection(mut controller: WifiController<'static>) {
    println!("Device capabilities: {:?}", controller.capabilities());
    loop {
        match esp_radio::wifi::sta_state() {
            WifiStaState::Connected => {
                // wait until we're no longer connected
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                println!("Disconnected, retrying...");
                Timer::after(Duration::from_millis(5000)).await
            }
            _ => {}
        }

        let ssid = option_env!("WIFI_SSID").unwrap();

        if !matches!(controller.is_started(), Ok(true)) {
            let station_config = ModeConfig::Client(
                ClientConfig::default()
                    .with_ssid(alloc::string::String::from(ssid))
                    .with_password(alloc::string::String::from(
                        option_env!("WIFI_PASS").unwrap(),
                    )),
            );
            controller.set_config(&station_config).unwrap();
            println!("Starting wifi");
            controller.start_async().await.unwrap();
            println!("Wifi started!");

            let scan_config = ScanConfig::default().with_max(10);

            loop {
                let results = controller
                    .scan_with_config_async(scan_config)
                    .await
                    .unwrap();

                if results.iter().any(|f| f.ssid == ssid) {
                    break;
                }

                warn!("Target SSID not found, retrying...");
                Timer::after(Duration::from_millis(2000)).await;
            }

            info!("Target SSID found!");
        }
        println!("About to connect...");

        match controller.connect_async().await {
            Ok(_) => println!("Wifi connected!"),
            Err(e) => {
                println!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}
