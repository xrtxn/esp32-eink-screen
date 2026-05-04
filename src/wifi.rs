use alloc::string::ToString;

use edge_nal::UdpBind;
use edge_nal_embassy::{Udp, UdpBuffers};
use embassy_net::{DhcpConfig, Runner};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::peripherals::{ADC1, RNG, WIFI};
use esp_radio::wifi::{
    AccessPointConfig, ClientConfig, ModeConfig, WifiController, WifiDevice, WifiEvent,
    WifiStaState,
};
use static_cell::StaticCell;

use crate::storage::WifiCreds;

pub static STOP_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();
pub static STOPPED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();
pub static WIFI_STARTED: portable_atomic::AtomicBool = portable_atomic::AtomicBool::new(false);

pub const AP_IP_ADDR: [u8; 4] = [192, 168, 0, 1];
const WIFI_RETRY_DELAY_MS: u64 = 100;

const NETWORK_STACK_NUM: usize = 5;

static NETWORK_STACK: StaticCell<embassy_net::StackResources<NETWORK_STACK_NUM>> =
    StaticCell::new();
static DHCP_UDP_BUFFERS: StaticCell<UdpBuffers<1>> = StaticCell::new();
static RADIO_CONTROLLER: StaticCell<esp_radio::Controller> = StaticCell::new();
static TRNG: StaticCell<esp_hal::rng::Trng> = StaticCell::new();
static TRNG_SOURCE: StaticCell<esp_hal::rng::TrngSource> = StaticCell::new();

#[embassy_executor::task]
pub async fn connection(
    mut controller: WifiController<'static>,
    mut runner: Runner<'static, WifiDevice<'static>>,
    ssid: heapless::String<32>,
    pass: heapless::String<32>,
) {
    crate::defmt::info!("Device capabilities: {:?}", controller.capabilities());

    let connection_fut = async {
        loop {
            if STOP_SIGNAL.signaled() {
                return;
            }

            if esp_radio::wifi::sta_state() == WifiStaState::Connected {
                // wait until we're no longer connected
                match embassy_futures::select::select(
                    controller.wait_for_event(WifiEvent::StaDisconnected),
                    STOP_SIGNAL.wait(),
                )
                .await
                {
                    embassy_futures::select::Either::First(_) => {
                        crate::defmt::warn!("Disconnected, retrying...");
                        Timer::after(Duration::from_millis(WIFI_RETRY_DELAY_MS)).await;
                    }
                    embassy_futures::select::Either::Second(_) => return,
                }
            } else {
                if !matches!(controller.is_started(), Ok(true)) {
                    let station_config = ModeConfig::Client(
                        ClientConfig::default()
                            .with_ssid(ssid.to_string())
                            .with_password(pass.to_string()),
                    );
                    controller.set_config(&station_config).unwrap();
                    crate::defmt::info!("Starting wifi");
                    controller.start_async().await.unwrap();
                    crate::defmt::info!("Wifi started!");
                }

                match embassy_futures::select::select(
                    controller.connect_async(),
                    STOP_SIGNAL.wait(),
                )
                .await
                {
                    embassy_futures::select::Either::First(Ok(())) => {
                        crate::defmt::info!("Wifi connected!")
                    }
                    embassy_futures::select::Either::First(Err(e)) => {
                        crate::defmt::error!("Failed to connect to wifi: {:?}", e);
                        Timer::after(Duration::from_millis(WIFI_RETRY_DELAY_MS)).await;
                    }
                    embassy_futures::select::Either::Second(_) => return,
                }
            }
        }
    };

    // run both futures. if connection_fut exits, runner is dropped.
    embassy_futures::select::select(runner.run(), connection_fut).await;

    // device is dropped cleanly. now we can stop the controller.
    let _ = controller.stop_async().await;
    STOPPED_SIGNAL.signal(());
}

#[embassy_executor::task]
pub async fn ap_task(
    mut controller: WifiController<'static>,
    mut runner: Runner<'static, WifiDevice<'static>>,
    stack: embassy_net::Stack<'static>,
) {
    crate::defmt::info!("Device capabilities: {:?}", controller.capabilities());

    let ap_fut = async {
        loop {
            if STOP_SIGNAL.signaled() {
                return;
            }
            if !matches!(controller.is_started(), Ok(true)) {
                let ap_config = ModeConfig::AccessPoint(
                    AccessPointConfig::default()
                        .with_max_connections(1)
                        .with_auth_method(esp_radio::wifi::AuthMethod::Wpa2Wpa3Personal)
                        .with_ssid(alloc::string::String::from(env!("AP_SSID")))
                        .with_password(alloc::string::String::from(env!("AP_PASS"))),
                );
                controller.set_config(&ap_config).unwrap();
                crate::defmt::info!("Starting AP");
                controller.start_async().await.unwrap();
                crate::defmt::info!("AP started!");
            }
            match embassy_futures::select::select(
                controller.wait_for_event(WifiEvent::ApStop),
                STOP_SIGNAL.wait(),
            )
            .await
            {
                embassy_futures::select::Either::First(_) => {
                    crate::defmt::warn!("AP stopped, restarting...");
                    Timer::after(Duration::from_millis(1000)).await;
                }
                embassy_futures::select::Either::Second(_) => return,
            }
        }
    };

    let dhcp_fut = async {
        let server_ip = core::net::Ipv4Addr::from_octets(AP_IP_ADDR);
        let mut gw_buf = [server_ip];
        let server_options = edge_dhcp::server::ServerOptions::new(server_ip, Some(&mut gw_buf));
        let mut server = edge_dhcp::server::Server::<_, 4>::new_with_et(server_ip);

        #[allow(clippy::large_stack_frames, reason = "false positive")]
        let buffers = DHCP_UDP_BUFFERS.init_with(UdpBuffers::new);
        let udp = Udp::new(stack, buffers);
        let local =
            core::net::SocketAddr::new(core::net::IpAddr::V4(core::net::Ipv4Addr::UNSPECIFIED), 67);
        let mut socket = udp
            .bind(local)
            .await
            .expect("DHCP: failed to bind to port 67");

        let mut buf = [0u8; 1500];

        loop {
            if let Err(e) =
                edge_dhcp::io::server::run(&mut server, &server_options, &mut socket, &mut buf)
                    .await
            {
                crate::defmt::error!("DHCP server error: {:?}", e);
            }
        }
    };

    embassy_futures::select::select(
        runner.run(),
        embassy_futures::select::select(dhcp_fut, ap_fut),
    )
    .await;

    let _ = controller.stop_async().await;
    STOPPED_SIGNAL.signal(());
}

pub fn start_ap(
    spawner: embassy_executor::Spawner,
    wifi: WIFI<'static>,
    rng_per: RNG<'static>,
    adc1: ADC1<'static>,
) -> (embassy_net::Stack<'static>, &'static mut esp_hal::rng::Trng) {
    let wifi_config = esp_radio::wifi::Config::default()
        .with_power_save_mode(esp_radio::wifi::PowerSaveMode::Minimum);

    let (wifi_controller, interfaces) = esp_radio::wifi::new(
        #[allow(clippy::large_stack_frames, reason = "false positive")]
        RADIO_CONTROLLER
            .init_with(|| esp_radio::init().expect("Failed to initialize Wi-Fi controller")),
        wifi,
        wifi_config,
    )
    .expect("Failed to initialize Wi-Fi controller");

    let wifi_interface = interfaces.ap;

    #[allow(clippy::default_trait_access)]
    let config = embassy_net::Config::ipv4_static(embassy_net::StaticConfigV4 {
        address: embassy_net::Ipv4Cidr::new(embassy_net::Ipv4Address::from_octets(AP_IP_ADDR), 24),
        gateway: Some(embassy_net::Ipv4Address::from_octets(AP_IP_ADDR)),
        dns_servers: Default::default(),
    });

    let _trng_source = TRNG_SOURCE.init(esp_hal::rng::TrngSource::new(rng_per, adc1));

    #[allow(clippy::large_stack_frames, reason = "false positive")]
    let trng = TRNG.init_with(|| esp_hal::rng::Trng::try_new().unwrap());
    let seed = (trng.random() as u64) << 32 | trng.random() as u64;

    // Init network stack
    let (net_stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        #[allow(clippy::large_stack_frames, reason = "false positive")]
        NETWORK_STACK.init_with(embassy_net::StackResources::<NETWORK_STACK_NUM>::new),
        seed,
    );

    STOP_SIGNAL.reset();
    STOPPED_SIGNAL.reset();

    spawner
        .spawn(ap_task(wifi_controller, runner, net_stack))
        .ok();

    WIFI_STARTED.store(true, core::sync::atomic::Ordering::Relaxed);
    (net_stack, trng)
}

pub fn start_con(
    spawner: embassy_executor::Spawner,
    wifi: WIFI<'static>,
    wifi_creds: WifiCreds,
    rng_per: RNG<'static>,
    adc1: ADC1<'static>,
) -> (embassy_net::Stack<'static>, &'static mut esp_hal::rng::Trng) {
    let wifi_config = esp_radio::wifi::Config::default()
        .with_power_save_mode(esp_radio::wifi::PowerSaveMode::Minimum);

    let (wifi_controller, interfaces) = esp_radio::wifi::new(
        #[allow(clippy::large_stack_frames, reason = "false positive")]
        RADIO_CONTROLLER
            .init_with(|| esp_radio::init().expect("Failed to initialize Wi-Fi controller")),
        wifi,
        wifi_config,
    )
    .expect("Failed to initialize Wi-Fi controller");

    let wifi_interface = interfaces.sta;

    let config = embassy_net::Config::dhcpv4(DhcpConfig::default());

    let _trng_source = TRNG_SOURCE.init(esp_hal::rng::TrngSource::new(rng_per, adc1));

    #[allow(clippy::large_stack_frames, reason = "false positive")]
    let trng = TRNG.init_with(|| esp_hal::rng::Trng::try_new().unwrap());
    let seed = (trng.random() as u64) << 32 | trng.random() as u64;

    // Init network stack
    let (net_stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        #[allow(clippy::large_stack_frames, reason = "false positive")]
        NETWORK_STACK.init_with(embassy_net::StackResources::<NETWORK_STACK_NUM>::new),
        seed,
    );

    STOP_SIGNAL.reset();
    STOPPED_SIGNAL.reset();

    spawner
        .spawn(connection(
            wifi_controller,
            runner,
            wifi_creds.ssid,
            wifi_creds.password,
        ))
        .ok();

    WIFI_STARTED.store(true, core::sync::atomic::Ordering::Relaxed);
    (net_stack, trng)
}

/// Mostly should be used when error occurred
pub async fn stop_wifi_and_reset() -> ! {
    if WIFI_STARTED.load(core::sync::atomic::Ordering::Relaxed) {
        crate::defmt::info!("Stopping wifi...");
        STOP_SIGNAL.signal(());
        STOPPED_SIGNAL.wait().await;
        WIFI_STARTED.store(false, core::sync::atomic::Ordering::Relaxed);
        crate::defmt::info!("Wifi stopped!");
    }
    esp_hal::system::software_reset();
}

pub async fn stop_wifi() {
    if WIFI_STARTED.load(core::sync::atomic::Ordering::Relaxed) {
        crate::defmt::info!("Stopping wifi...");
        STOP_SIGNAL.signal(());
    }
}

pub async fn wait_until_wifi_stop() {
    STOPPED_SIGNAL.wait().await;
    WIFI_STARTED.store(false, core::sync::atomic::Ordering::Relaxed);
    crate::defmt::info!("Wifi stopped!");
}
