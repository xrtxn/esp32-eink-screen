#![feature(type_alias_impl_trait)]
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

mod display;
mod hardware;
mod networking;
mod wifi;

use core::sync::atomic::{AtomicU32, Ordering};

use display_interface_spi::SPIInterface;
use embassy_executor::Spawner;
use embassy_net::StackResources;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::clock::CpuClock;
use esp_hal::{
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    spi::{
        master::{Config, Spi},
        Mode,
    },
    time::Rate,
};
use static_cell::StaticCell;
use time::OffsetDateTime;
use weact_studio_epd::graphics::{Display420BlackWhite, DisplayRotation};
use weact_studio_epd::WeActStudio420BlackWhiteDriver;

use esp_backtrace as _;

use crate::display::{add_footer_info, draw_event};
use crate::hardware::go_to_deep_sleep;

extern crate alloc;

// This is one event every half our
pub const MAX_DAILY_EVENTS: usize = 16;
pub const MAX_VCALENDAR_BYTES: usize = 2000;

static NETWORK_STACK: StaticCell<StackResources<3>> = StaticCell::new();
static RADIO_CONTROLLER: StaticCell<esp_radio::Controller> = StaticCell::new();
static TRNG: StaticCell<esp_hal::rng::Trng> = StaticCell::new();

#[unsafe(link_section = ".rtc_slow.data")]
static BOOT_COUNT: AtomicU32 = AtomicU32::new(0);

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    let prev_boot_count = BOOT_COUNT.fetch_add(1, Ordering::SeqCst);
    log::info!("Boot count: {}", prev_boot_count + 1);

    esp_alloc::heap_allocator!(size: 64 * 1024);
    // COEX needs more RAM - so we've added some more
    esp_alloc::heap_allocator!(#[unsafe(link_section = ".dram2_uninit")] size: 64 * 1024);

    let mut rtc = esp_hal::rtc_cntl::Rtc::new(peripherals.LPWR);

    let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);
    let sw_int =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);

    let (wifi_controller, interfaces) = esp_radio::wifi::new(
        RADIO_CONTROLLER
            .init(esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller")),
        peripherals.WIFI,
        Default::default(),
    )
    .expect("Failed to initialize Wi-Fi controller");

    let wifi_interface = interfaces.sta;

    let config = embassy_net::Config::dhcpv4(Default::default());

    let _trng_source = esp_hal::rng::TrngSource::new(peripherals.RNG, peripherals.ADC1);

    let trng = TRNG.init(esp_hal::rng::Trng::try_new().unwrap());
    let seed = (trng.random() as u64) << 32 | trng.random() as u64;

    // Init network stack
    let (stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        NETWORK_STACK.init(StackResources::<3>::new()),
        seed,
    );

    spawner.spawn(wifi::connection(wifi_controller)).ok();
    spawner.spawn(wifi::net_task(runner)).ok();
    {
        let to = embassy_time::with_timeout(
            embassy_time::Duration::from_secs(30),
            stack.wait_config_up(),
        )
        .await;
        if to.is_err() {
            go_to_deep_sleep(&mut rtc)
        }
    }

    let config = stack.config_v4().unwrap();
    log::info!("Network connected with IP address: {}", config.address);

    // The RTC clock drifts, so every 5th boot we should resync it with the NTP time.
    if prev_boot_count.is_multiple_of(5) {
        let time = networking::get_time(stack).await;

        // it uses microseconds, so we should convert it before setting
        rtc.set_current_time_us(
            (time.unix_timestamp() as u64 * 1_000_000) + (time.microsecond() as u64),
        );
    }

    let tls = mbedtls_rs::Tls::new(trng).unwrap();

    let time_from_rtc =
        time::OffsetDateTime::from_unix_timestamp(rtc.current_time_us() as i64 / 1_000_000)
            .unwrap();

    let mut cal_xml = None;

    for tries in 1..=3 {
        let req = networking::network_req(stack, tls.reference(), time_from_rtc.date());
        if let Ok(res) =
            embassy_time::with_timeout(embassy_time::Duration::from_secs(30), req).await
        {
            cal_xml = Some(res);
            break;
        }
        log::warn!("Failed to get calendar data on attempt {tries}, retrying...");
    }

    let cal_xml = cal_xml.unwrap_or_else(|| {
        log::error!("Failed after 3 attempts, entering deep sleep");
        go_to_deep_sleep(&mut rtc)
    });

    log::trace!("Received calendar data len: {}", cal_xml.len());
    let cal_strings = extract_calendar_data(&cal_xml);
    // todo do unfolding
    let events: heapless::Vec<vcal_parser::VCalendar<'_>, MAX_DAILY_EVENTS> = cal_strings
        .iter()
        .map(|s| vcal_parser::parse_vcalendar(s).unwrap().1)
        .collect();

    log::trace!(
        "Parsed: {:?}",
        events
            .iter()
            .map(|e| &e.events.first().unwrap().summary)
            .collect::<heapless::Vec<_, MAX_DAILY_EVENTS>>()
    );

    let sclk = peripherals.GPIO12;
    let mosi = peripherals.GPIO11; // SDA -> MOSI

    let spi_bus = Spi::new(
        peripherals.SPI2,
        Config::default()
            .with_frequency(Rate::from_mhz(4))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(sclk)
    .with_mosi(mosi);

    let dc = Output::new(peripherals.GPIO18, Level::Low, OutputConfig::default());
    let rst = Output::new(peripherals.GPIO4, Level::High, OutputConfig::default());
    let busy = Input::new(
        peripherals.GPIO15,
        InputConfig::default().with_pull(Pull::None),
    );
    let cs = Output::new(peripherals.GPIO10, Level::High, OutputConfig::default());

    log::info!("Intializing SPI Device...");
    let spi_device =
        ExclusiveDevice::new(spi_bus, cs, Delay::new()).expect("SPI device initialize error");
    let spi_interface = SPIInterface::new(spi_device, dc);

    log::info!("Intializing EPD...");
    let mut driver = WeActStudio420BlackWhiteDriver::new(spi_interface, busy, rst, Delay::new());
    let mut display = Display420BlackWhite::new();
    // set it to be longer not wider
    display.set_rotation(DisplayRotation::Rotate270);
    driver.init().unwrap();
    log::info!("EPD initialized!");
    display::draw_time_row_header(&mut display);
    let tz_offset = time::UtcOffset::from_hms(1, 0, 0).unwrap();
    for event in events {
        for eevent in event.events {
            let start_dt = time::OffsetDateTime::to_offset(eevent.dtstart.unwrap(), tz_offset);
            let end_dt = time::OffsetDateTime::to_offset(eevent.dtend.unwrap(), tz_offset);
            let start_minute = date_to_mins(start_dt);
            let end_minute = date_to_mins(end_dt);
            log::info!(
                "Event: {}, start_minute: {}, end_minute: {}",
                eevent.summary.unwrap_or("No summary"),
                start_minute,
                end_minute
            );
            draw_event(
                &mut display,
                start_minute,
                end_minute,
                eevent.summary.unwrap_or("No summary"),
            );
        }
    }
    add_footer_info(&mut display);
    driver.full_update(&display).unwrap();
    log::info!("Display updated!");

    hardware::go_to_deep_sleep(&mut rtc);
}

// this is overkill but may be necessary
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

fn date_to_mins(dt: OffsetDateTime) -> u16 {
    dt.hour() as u16 * 60 + dt.minute() as u16
}
