#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

mod display;
mod hardware;
mod init;
mod networking;
mod server;
mod storage;
mod wifi;

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;

use crate::networking::{MAX_DAILY_EVENTS, MAX_VCALENDAR_BYTES};
use crate::server::{web_task, WEB_TASK_POOL_SIZE};
use crate::storage::NvsConfig;
use esp_storage::FlashStorage;
use picoserve::AppBuilder;
use portable_atomic::{AtomicU32, AtomicU8};

use display_interface_spi::SPIInterface;
use embassy_executor::Spawner;
use embassy_time::Delay;

use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::peripherals::SPI2;
use esp_hal::{
    gpio::{Input, Output},
    spi::master::Spi,
};
use weact_studio_epd::WeActStudio420BlackWhiteDriver;

use esp_backtrace as _;

use crate::hardware::go_to_deep_sleep;

extern crate alloc;

#[esp_hal::ram(unstable(rtc_fast, persistent))]
static BOOT_COUNT: AtomicU32 = AtomicU32::new(0);

#[esp_hal::ram(unstable(rtc_fast, persistent))]
pub static BOOT_TYPES: AtomicU8 = AtomicU8::new(BootType::Display as u8);

type EpdDriver = WeActStudio420BlackWhiteDriver<
    SPIInterface<
        ExclusiveDevice<Spi<'static, esp_hal::Async>, Output<'static>, Delay>,
        Output<'static>,
    >,
    Input<'static>,
    Output<'static>,
    Delay,
>;

#[derive(PartialEq, Clone, Copy)]
pub(crate) enum BootType {
    Display = 0,
    Config = 1,
}

impl BootType {
    /// Store the boot type into RTC-persistent memory.
    pub(crate) fn set(val: BootType) {
        BOOT_TYPES.store(val as u8, core::sync::atomic::Ordering::Relaxed);
    }

    /// Read the boot type from RTC-persistent memory.
    pub(crate) fn get() -> BootType {
        Self::from_u8(BOOT_TYPES.load(core::sync::atomic::Ordering::Relaxed))
    }

    /// Convert a raw `u8` (as stored in the atomic) to a `BootType`.
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
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    let w = peripherals.WIFI;

    #[cfg(not(debug_assertions))]
    hardware::apply_wakeup_boot_type();

    let boot_type = BootType::get();

    let flash = esp_storage::FlashStorage::new(peripherals.FLASH);
    let flash = storage::init_flash(flash);
    let stored_config = storage::read_config(flash).await;

    let creds = get_credentials(stored_config);

    let prev_boot_count = BOOT_COUNT.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    log::info!("Boot count: {}", prev_boot_count + 1);

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

    let (net_stack, trng) = if boot_type == BootType::Display {
        wifi::start_con(spawner, w, creds, peripherals.RNG, peripherals.ADC1)
    } else {
        wifi::start_ap(spawner, w, peripherals.RNG, peripherals.ADC1)
    };

    {
        let to = embassy_time::with_timeout(
            embassy_time::Duration::from_secs(30),
            net_stack.wait_config_up(),
        )
        .await;
        if to.is_err() {
            go_to_deep_sleep(&mut rtc)
        }
    }

    let ip_config = net_stack.config_v4().unwrap();
    log::info!("Network connected with IP address: {}", ip_config.address);

    log::info!("Microcontroller initialized");

    match boot_type {
        BootType::Display => {
            log::info!("Boot type is display");
            run_display_mode(
                prev_boot_count,
                &mut rtc,
                net_stack,
                trng,
                peripherals.GPIO12,
                peripherals.GPIO11,
                peripherals.SPI2,
                peripherals.GPIO18,
                peripherals.GPIO4,
                peripherals.GPIO15,
                peripherals.GPIO10,
            )
            .await;
        }
        BootType::Config => {
            log::info!("Boot type is config");
            run_config_mode(spawner, net_stack, flash).await;
        }
    }
}

/// - If credentials exist and wifi is set, returns them.
/// - If wifi is not set reboot to config mode
fn get_credentials(stored_config: Option<NvsConfig>) -> NvsConfig {
    match stored_config {
        Some(ok) => match ok.wifi {
            None => {
                // WiFi credentials are missing; switch to config mode so the user can set them.
                log::warn!("No WiFi credentials stored, rebooting into config mode");
                BootType::set(BootType::Config);
                esp_hal::system::software_reset();
            }
            _ => ok,
        },
        #[cfg(debug_assertions)]
        None => {
            log::warn!("No config found; using compile-time credentials (debug build)");
            let wifi_creds = storage::WifiCreds::new(env!("WIFI_SSID"), env!("WIFI_PASS"));
            NvsConfig::new(Some(wifi_creds))
        }
        #[cfg(not(debug_assertions))]
        None => {
            log::warn!("No config found, rebooting into config mode");
            BootType::set(BootType::Config);
            esp_hal::system::software_reset();
        }
    }
}

async fn run_display_mode(
    prev_boot_count: u32,
    rtc: &mut esp_hal::rtc_cntl::Rtc<'_>,
    net_stack: embassy_net::Stack<'_>,
    trng: &mut esp_hal::rng::Trng,
    sclk: impl OutputPin + 'static,
    mosi: impl OutputPin + 'static,
    spi: SPI2<'static>,
    dc: impl OutputPin + 'static,
    rst: impl OutputPin + 'static,
    busy: impl InputPin + 'static,
    cs: impl OutputPin + 'static,
) {
    // The RTC clock drifts, so every 5th boot we resync it with the NTP time.
    if prev_boot_count.is_multiple_of(5) {
        log::info!("Syncing RTC with NTP (boot {})", prev_boot_count + 1);
        let time = networking::get_time(net_stack).await;
        // set_current_time_us expects microseconds
        rtc.set_current_time_us(
            (time.as_second() as u64 * 1_000_000) + (time.subsec_microsecond() as u64),
        );
    }

    let events = networking::get_events(trng, rtc, net_stack).await;

    let (mut display, mut driver) = init::init_display(sclk, mosi, spi, dc, rst, busy, cs).await;
    display::write_to_screen(&mut display, &mut driver, events, rtc).await;
}

async fn run_config_mode(
    spawner: Spawner,
    net_stack: embassy_net::Stack<'static>,
    flash: &'static Mutex<NoopRawMutex, FlashStorage<'static>>,
) {
    let app = picoserve::make_static!(
        picoserve::AppRouter<server::AppProps>,
        server::AppProps {
            flash_storage: flash
        }
        .build_app()
    );

    for task_id in 0..WEB_TASK_POOL_SIZE {
        spawner.must_spawn(web_task(task_id, net_stack, app));
    }
}

fn extract_calendar_data(
    data: &str,
) -> heapless::Vec<heapless::String<MAX_VCALENDAR_BYTES>, MAX_DAILY_EVENTS> {
    let parsed = roxmltree::Document::parse(data).unwrap();
    parsed
        .descendants()
        .filter(|n| n.has_tag_name("calendar-data"))
        .filter_map(|e| {
            e.text().map(|t| {
                heapless::String::try_from(t)
                    .expect("Unable to store calendar data into heapless string")
            })
        })
        .collect()
}
