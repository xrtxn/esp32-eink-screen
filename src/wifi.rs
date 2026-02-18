use embassy_net::Runner;
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_println::println;
use esp_radio::wifi::{
    ClientConfig, ModeConfig, WifiController, WifiDevice, WifiEvent, WifiStaState,
};

#[embassy_executor::task]
pub async fn connection(mut controller: WifiController<'static>) {
    println!("Device capabilities: {:?}", controller.capabilities());
    loop {
        if esp_radio::wifi::sta_state() == WifiStaState::Connected {
            // wait until we're no longer connected
            controller.wait_for_event(WifiEvent::StaDisconnected).await;
            println!("Disconnected, retrying...");
            Timer::after(Duration::from_millis(5000)).await
        }

        let ssid = env!("WIFI_SSID");

        if !matches!(controller.is_started(), Ok(true)) {
            let station_config = ModeConfig::Client(
                ClientConfig::default()
                    .with_ssid(alloc::string::String::from(ssid))
                    .with_password(alloc::string::String::from(env!("WIFI_PASS"))),
            );
            controller.set_config(&station_config).unwrap();
            println!("Starting wifi");
            controller.start_async().await.unwrap();
            println!("Wifi started!");
        }
        println!("About to connect...");

        match controller.connect_async().await {
            Ok(_) => println!("Wifi connected!"),
            Err(e) => {
                println!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(1000)).await
            }
        }
    }
}

#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}
