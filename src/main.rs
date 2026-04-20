#![feature(impl_trait_in_assoc_type)]
// Stabilized in rust 1.95
#![feature(new_range_api)]
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![allow(clippy::pedantic)]

#[cfg(feature = "defmt")]
pub use ::defmt;

mod display;
mod hardware;
mod init;
mod networking;
mod parsing;
mod server;
mod storage;
mod wifi;

use display_interface_spi::SPIInterface;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::TcpClient;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::{
    gpio::{Input, Output},
    spi::master::Spi,
};
use esp_storage::FlashStorage;
use picoserve::AppBuilder;
use portable_atomic::{AtomicU8, AtomicU32};
use weact_studio_epd::WeActStudio420BlackWhiteDriver;
use weact_studio_epd::graphics::Display420BlackWhite;

use crate::hardware::go_to_deep_sleep;
use crate::server::{NetworkStatus, WEB_TASK_POOL_SIZE, web_task};
use crate::storage::NvsConfig;

extern crate alloc;

const NETWORK_FAIL_LIMIT: u8 = 3;

#[esp_hal::ram(unstable(rtc_fast, persistent))]
static DISPLAY_SLEEP_COUNT: AtomicU32 = AtomicU32::new(0);

#[esp_hal::ram(unstable(rtc_fast, persistent))]
pub static BOOT_TYPES: AtomicU8 = AtomicU8::new(BootType::Display as u8);

#[esp_hal::ram(unstable(rtc_fast, persistent))]
pub static NETWORK_FAIL_COUNT: AtomicU8 = AtomicU8::new(0);

static TLS: static_cell::StaticCell<mbedtls_rs::Tls<'static>> = static_cell::StaticCell::new();
static TLS_MUTEX: static_cell::StaticCell<
    embassy_sync::mutex::Mutex<NoopRawMutex, &'static mut mbedtls_rs::Tls<'static>>,
> = static_cell::StaticCell::new();
static DNS_SOCKET: static_cell::StaticCell<DnsSocket<'static>> = static_cell::StaticCell::new();
static TCP_CLIENT: static_cell::StaticCell<TcpClient<'static, 1, 4096, 4096>> =
    static_cell::StaticCell::new();

type EpdDriver = WeActStudio420BlackWhiteDriver<
    SPIInterface<
        ExclusiveDevice<Spi<'static, esp_hal::Async>, Output<'static>, Delay>,
        Output<'static>,
    >,
    Input<'static>,
    Output<'static>,
    Delay,
>;

#[cfg_attr(feature = "defmt", derive(crate::defmt::Format))]
#[derive(PartialEq, Clone, Copy, Debug)]
pub(crate) enum BootType {
    Display = 0,
    Config = 1,
}

impl BootType {
    pub(crate) fn set(val: BootType) {
        BOOT_TYPES.store(val as u8, core::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn get() -> BootType {
        Self::from_u8(BOOT_TYPES.load(core::sync::atomic::Ordering::Relaxed))
    }

    pub(crate) fn from_u8(val: u8) -> BootType {
        match val {
            0 => BootType::Display,
            1 => BootType::Config,
            _ => panic!("Unknown boot type value: {}", val),
        }
    }
}

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    let w = peripherals.WIFI;

    hardware::apply_wakeup_boot_type();

    let prev_boot_count = DISPLAY_SLEEP_COUNT.load(core::sync::atomic::Ordering::Relaxed);
    crate::defmt::info!("Successful sleep wake count: {}", prev_boot_count + 1);

    let boot_type = BootType::get();

    match boot_type {
        BootType::Display => {
            DISPLAY_SLEEP_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }
        BootType::Config => (),
    }

    let flash = esp_storage::FlashStorage::new(peripherals.FLASH);
    let flash = storage::init_flash(flash);

    crate::defmt::debug!("Initialized flash storage");

    // this affects the remaining stack
    esp_alloc::heap_allocator!(size: 64 * 1024);
    // SSL needs more RAM
    esp_alloc::heap_allocator!(#[unsafe(link_section = ".dram2_uninit")] size: 64 * 1024);

    let mut rtc = esp_hal::rtc_cntl::Rtc::new(peripherals.LPWR);

    let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    let button = peripherals.GPIO0;

    let btn_config = esp_hal::gpio::InputConfig::default().with_pull(esp_hal::gpio::Pull::Up);
    let button = Input::new(button, btn_config);

    spawner.must_spawn(hardware::button_task(button));

    crate::defmt::debug!("Trying to read config");

    let mut stored_config = storage::read_config(flash).await;

    crate::defmt::debug!("Config read complete");

    let mut sync_calendars = alloc::vec::Vec::with_capacity(2);

    if let Some(config) = &mut stored_config {
        if let Some(display_config) = &mut config.display {
            crate::display::DISPLAY_HOURS.store(
                display_config.displayed_hours,
                core::sync::atomic::Ordering::Relaxed,
            );
            sync_calendars.extend(core::mem::take(&mut display_config.calendars));
        } else {
            crate::display::DISPLAY_HOURS.store(8, core::sync::atomic::Ordering::Relaxed);
        }
    }

    crate::defmt::info!("Boot type: {:?}", boot_type);

    let (net_stack, trng, ncreds, network_status) = if boot_type == BootType::Display {
        let config = match stored_config.clone() {
            Some(config) => config,
            _ => {
                #[cfg(debug_assertions)]
                {
                    crate::defmt::warn!(
                        "No config found; using compile-time credentials (debug build)"
                    );
                    let wifi_creds = storage::WifiCreds::new(env!("WIFI_SSID"), env!("WIFI_PASS"));
                    NvsConfig::new(Some(wifi_creds))
                }
                #[cfg(not(debug_assertions))]
                {
                    BootType::set(BootType::Config);
                    esp_hal::system::software_reset();
                }
            }
        };

        if config.wifi.is_none() || config.caldav.is_none() {
            crate::defmt::warn!("Missing credentials (wifi or caldav), rebooting into config mode");
            BootType::set(BootType::Config);
            esp_hal::system::software_reset();
        }

        let wifi_creds = config.wifi.clone().unwrap();
        let ncreds = Some(config);

        let (net_stack, trng) =
            wifi::start_con(spawner, w, wifi_creds, peripherals.RNG, peripherals.ADC1);
        (net_stack, trng, ncreds, NetworkStatus::Network)
    } else {
        let ncreds = stored_config.clone();

        let (net_stack, trng, network_status) = if let Some(config) = stored_config.clone() {
            if let Some(creds) = config.wifi {
                let (net_stack, trng) =
                    wifi::start_con(spawner, w, creds, peripherals.RNG, peripherals.ADC1);
                (net_stack, trng, NetworkStatus::Network)
            } else {
                let (net_stack, trng) =
                    wifi::start_ap(spawner, w, peripherals.RNG, peripherals.ADC1);
                (net_stack, trng, NetworkStatus::AccessPoint)
            }
        } else {
            let (net_stack, trng) = wifi::start_ap(spawner, w, peripherals.RNG, peripherals.ADC1);
            (net_stack, trng, NetworkStatus::AccessPoint)
        };

        (net_stack, trng, ncreds, network_status)
    };

    {
        let timeout = if boot_type == BootType::Display {
            embassy_time::Duration::from_secs(20)
        } else {
            embassy_time::Duration::from_secs(30)
        };

        let to = embassy_time::with_timeout(timeout, net_stack.wait_config_up()).await;

        if to.is_ok() {
            if boot_type == BootType::Display {
                NETWORK_FAIL_COUNT.store(0, core::sync::atomic::Ordering::Relaxed);
            }
        } else {
            let old_count = NETWORK_FAIL_COUNT.load(core::sync::atomic::Ordering::Relaxed);

            let should_reset = match boot_type {
                BootType::Display if old_count <= NETWORK_FAIL_LIMIT => go_to_deep_sleep(&mut rtc),
                BootType::Display => true,
                BootType::Config => network_status == NetworkStatus::Network,
            };

            if should_reset {
                let mut config = stored_config.unwrap();
                config.wifi = None;
                if let Some(display_config) = &mut config.display {
                    display_config.calendars = sync_calendars;
                }
                storage::write_config(flash, config).await;
                esp_hal::system::software_reset();
            }
        }
    }

    let ip_config = net_stack.config_v4().unwrap();
    crate::defmt::info!("Network connected with IP address: {}", ip_config.address);

    crate::defmt::info!("Microcontroller initialized");

    let (mut display, mut driver) = init::init_display(
        peripherals.GPIO12,
        peripherals.GPIO11,
        peripherals.SPI2,
        peripherals.GPIO18,
        peripherals.GPIO4,
        peripherals.GPIO15,
        peripherals.GPIO10,
    )
    .await;

    match boot_type {
        BootType::Display => {
            networking::sync_time(prev_boot_count, net_stack, &mut rtc).await;
            run_display_mode(
                &mut rtc,
                net_stack,
                trng,
                &mut display,
                &mut driver,
                ncreds.as_ref().unwrap(),
                &sync_calendars,
            )
            .await;
        }
        BootType::Config => {
            let text = if network_status == NetworkStatus::Network {
                alloc::format!(
                    "Connected to Wi-Fi!\nSSID: {}\nIP: {}\n",
                    ncreds.as_ref().unwrap().wifi.as_ref().unwrap().ssid,
                    ip_config.address.address()
                )
            } else {
                alloc::format!(
                    "Access point created!\nSSID: {}\nPassword: {}\nIp: {}",
                    env!("AP_SSID"),
                    env!("AP_PASS"),
                    ip_config.address.address()
                )
            };

            join(
                async { run_config_mode(spawner, net_stack, flash, trng) },
                async {
                    display::draw_config(&mut display, text.as_str());
                    driver.full_update(&display).await.unwrap();
                },
            )
            .await;
        }
    }
}

async fn run_display_mode(
    rtc: &mut esp_hal::rtc_cntl::Rtc<'_>,
    net_stack: embassy_net::Stack<'static>,
    trng: &'static mut esp_hal::rng::Trng,
    display: &mut Display420BlackWhite,
    driver: &mut EpdDriver,
    config: &NvsConfig,
    calendars: &[alloc::string::String],
) {
    let caldav = config.caldav.clone().unwrap();

    let tls = TLS.init(mbedtls_rs::Tls::new(trng).unwrap());
    let dns_socket = DNS_SOCKET.init(DnsSocket::new(net_stack));
    let tcp_client = TCP_CLIENT.init(TcpClient::new(
        net_stack,
        crate::networking::CLIENT_STATE.init(embassy_net::tcp::client::TcpClientState::new()),
    ));
    let events = networking::get_events(
        tls.reference(),
        dns_socket,
        tcp_client,
        rtc,
        &caldav,
        calendars,
    )
    .await;

    display::write_to_screen(display, driver, events, rtc).await;
}

fn run_config_mode(
    spawner: Spawner,
    net_stack: embassy_net::Stack<'static>,
    flash: &'static Mutex<NoopRawMutex, FlashStorage<'static>>,
    trng: &'static mut esp_hal::rng::Trng,
) {
    let tls = TLS.init(mbedtls_rs::Tls::new(trng).unwrap());
    let tls_mutex = TLS_MUTEX.init(embassy_sync::mutex::Mutex::new(tls));
    let dns_socket = DNS_SOCKET.init(DnsSocket::new(net_stack));
    let tcp_client = TCP_CLIENT.init(TcpClient::new(
        net_stack,
        crate::networking::CLIENT_STATE.init(embassy_net::tcp::client::TcpClientState::new()),
    ));

    let app = picoserve::make_static!(
        picoserve::AppRouter<server::AppProps>,
        server::AppProps {
            flash_storage: flash,
            tls_mutex,
            dns_socket,
            tcp_client,
        }
        .build_app()
    );

    for task_id in 0..WEB_TASK_POOL_SIZE {
        spawner.must_spawn(web_task(task_id, net_stack, app));
    }
}
