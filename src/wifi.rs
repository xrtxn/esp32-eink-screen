use alloc::string::ToString;
use edge_nal::UdpBind;
use edge_nal_embassy::{Udp, UdpBuffers};
use embassy_net::Runner;
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::peripherals::{ADC1, RNG, WIFI};
use esp_radio::wifi::{
    AccessPointConfig, ClientConfig, ModeConfig, WifiController, WifiDevice, WifiEvent,
    WifiStaState,
};
use static_cell::StaticCell;

use crate::storage::NvsConfig;

static NETWORK_STACK: StaticCell<embassy_net::StackResources<5>> = StaticCell::new();
static DHCP_UDP_BUFFERS: StaticCell<UdpBuffers<1>> = StaticCell::new();
static RADIO_CONTROLLER: StaticCell<esp_radio::Controller> = StaticCell::new();
static TRNG: StaticCell<esp_hal::rng::Trng> = StaticCell::new();

#[embassy_executor::task]
pub async fn connection(
    mut controller: WifiController<'static>,
    ssid: heapless::String<32>,
    pass: heapless::String<32>,
) {
    log::info!("Device capabilities: {:?}", controller.capabilities());
    loop {
        if esp_radio::wifi::sta_state() == WifiStaState::Connected {
            // wait until we're no longer connected
            controller.wait_for_event(WifiEvent::StaDisconnected).await;
            log::warn!("Disconnected, retrying...");
            Timer::after(Duration::from_millis(1000)).await
        }

        if !matches!(controller.is_started(), Ok(true)) {
            let station_config = ModeConfig::Client(
                ClientConfig::default()
                    .with_ssid(ssid.to_string())
                    .with_password(pass.to_string()),
            );
            controller.set_config(&station_config).unwrap();
            log::info!("Starting wifi");
            controller.start_async().await.unwrap();
            log::info!("Wifi started!");
        }

        match controller.connect_async().await {
            Ok(_) => log::info!("Wifi connected!"),
            Err(e) => {
                log::error!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(1000)).await
            }
        }
    }
}

#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

#[embassy_executor::task]
pub async fn dhcp_server_task(stack: embassy_net::Stack<'static>) {
    let server_ip = core::net::Ipv4Addr::new(192, 168, 0, 1);
    let mut gw_buf = [server_ip];
    let server_options =
        edge_dhcp::server::ServerOptions::new(server_ip, Some(&mut gw_buf));
    let mut server = edge_dhcp::server::Server::<_, 4>::new_with_et(server_ip);

    let buffers = DHCP_UDP_BUFFERS.init(UdpBuffers::new());
    let udp = Udp::new(stack, buffers);
    let local = core::net::SocketAddr::new(
        core::net::IpAddr::V4(core::net::Ipv4Addr::UNSPECIFIED),
        67,
    );
    let mut socket = udp
        .bind(local)
        .await
        .expect("DHCP: failed to bind to port 67");

    let mut buf = [0u8; 1500];

    loop {
        if let Err(e) =
            edge_dhcp::io::server::run(&mut server, &server_options, &mut socket, &mut buf).await
        {
            log::error!("DHCP server error: {:?}", e);
        }
    }
}

#[embassy_executor::task]
pub async fn ap_task(mut controller: WifiController<'static>) {
    log::info!("Device capabilities: {:?}", controller.capabilities());
    loop {
        if !matches!(controller.is_started(), Ok(true)) {
            let ap_config = ModeConfig::AccessPoint(
                AccessPointConfig::default()
                    .with_max_connections(1)
                    .with_auth_method(esp_radio::wifi::AuthMethod::Wpa2Personal)
                    .with_ssid(alloc::string::String::from(env!("AP_SSID")))
                    .with_password(alloc::string::String::from(env!("AP_PASS"))),
            );
            controller.set_config(&ap_config).unwrap();
            log::info!("Starting AP");
            controller.start_async().await.unwrap();
            log::info!("AP started!");
        }
        controller.wait_for_event(WifiEvent::ApStop).await;
        log::warn!("AP stopped, restarting...");
        Timer::after(Duration::from_millis(1000)).await;
    }
}

pub fn start_ap(
    spawner: embassy_executor::Spawner,
    wifi: WIFI<'static>,
    rng_per: RNG,
    adc1: ADC1,
) -> (embassy_net::Stack<'static>, &'static mut esp_hal::rng::Trng) {
    let wifi_config = esp_radio::wifi::Config::default()
        .with_power_save_mode(esp_radio::wifi::PowerSaveMode::Minimum);

    let (wifi_controller, interfaces) = esp_radio::wifi::new(
        RADIO_CONTROLLER.init(esp_radio::init().expect("Failed to initialize Wi-Fi controller")),
        wifi,
        wifi_config,
    )
    .expect("Failed to initialize Wi-Fi controller");

    let wifi_interface = interfaces.ap;

    let config = embassy_net::Config::ipv4_static(embassy_net::StaticConfigV4 {
        address: embassy_net::Ipv4Cidr::new(embassy_net::Ipv4Address::new(192, 168, 0, 1), 24),
        gateway: Some(embassy_net::Ipv4Address::new(192, 168, 0, 1)),
        dns_servers: Default::default(),
    });

    let _trng_source = esp_hal::rng::TrngSource::new(rng_per, adc1);

    let trng = TRNG.init(esp_hal::rng::Trng::try_new().unwrap());
    let seed = (trng.random() as u64) << 32 | trng.random() as u64;

    // Init network stack
    let (net_stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        NETWORK_STACK.init(embassy_net::StackResources::<5>::new()),
        seed,
    );

    spawner.spawn(ap_task(wifi_controller)).ok();
    spawner.spawn(net_task(runner)).ok();
    spawner.spawn(dhcp_server_task(net_stack)).ok();

    (net_stack, trng)
}

pub fn start_con(
    spawner: embassy_executor::Spawner,
    wifi: WIFI<'static>,
    creds: NvsConfig,
    rng_per: RNG,
    adc1: ADC1,
) -> (embassy_net::Stack<'static>, &'static mut esp_hal::rng::Trng) {
    let wifi_config = esp_radio::wifi::Config::default()
        .with_power_save_mode(esp_radio::wifi::PowerSaveMode::Minimum);

    let (wifi_controller, interfaces) = esp_radio::wifi::new(
        RADIO_CONTROLLER.init(esp_radio::init().expect("Failed to initialize Wi-Fi controller")),
        wifi,
        wifi_config,
    )
    .expect("Failed to initialize Wi-Fi controller");

    let wifi_interface = interfaces.sta;

    let config = embassy_net::Config::dhcpv4(Default::default());

    let _trng_source = esp_hal::rng::TrngSource::new(rng_per, adc1);

    let trng = TRNG.init(esp_hal::rng::Trng::try_new().unwrap());
    let seed = (trng.random() as u64) << 32 | trng.random() as u64;

    // Init network stack
    let (net_stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        NETWORK_STACK.init(embassy_net::StackResources::<5>::new()),
        seed,
    );

    let wifi_creds = creds.wifi.unwrap();

    spawner
        .spawn(connection(
            wifi_controller,
            wifi_creds.ssid,
            wifi_creds.password,
        ))
        .ok();
    spawner.spawn(net_task(runner)).ok();

    (net_stack, trng)
}
