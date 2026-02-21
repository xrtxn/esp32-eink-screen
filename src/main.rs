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

use crate::server::{web_task, WEB_TASK_POOL_SIZE};
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use picoserve::AppBuilder;

use display_interface_spi::SPIInterface;
use embassy_executor::Spawner;
use embassy_net::tcp::client::TcpClient;
use embassy_net::StackResources;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::clock::CpuClock;
use esp_hal::{
    delay::Delay,
    gpio::{Input, Output},
    spi::master::Spi,
};
use static_cell::StaticCell;
use time::OffsetDateTime;
use weact_studio_epd::WeActStudio420BlackWhiteDriver;

use esp_backtrace as _;

use crate::hardware::go_to_deep_sleep;

extern crate alloc;

// This is one event every half our
pub const MAX_DAILY_EVENTS: usize = 4;
pub const MAX_VCALENDAR_BYTES: usize = 2000;

const TOTAL_VCAL_BUFFER: usize = MAX_DAILY_EVENTS * MAX_VCALENDAR_BYTES;

static NETWORK_STACK: StaticCell<StackResources<4>> = StaticCell::new();
static RADIO_CONTROLLER: StaticCell<esp_radio::Controller> = StaticCell::new();
static TRNG: StaticCell<esp_hal::rng::Trng> = StaticCell::new();

static CAL_XML_BUF: StaticCell<heapless::String<TOTAL_VCAL_BUFFER>> = StaticCell::new();
static REQ_BUFFER: StaticCell<[u8; 8192]> = StaticCell::new();
static CAL_STRINGS: StaticCell<
    heapless::Vec<heapless::String<MAX_VCALENDAR_BYTES>, MAX_DAILY_EVENTS>,
> = StaticCell::new();

#[esp_hal::ram(unstable(rtc_fast))]
static BOOT_COUNT: AtomicU32 = AtomicU32::new(0);

#[esp_hal::ram(unstable(rtc_fast))]
static BOOT_TYPES: AtomicU8 = AtomicU8::new(BootType::Config as u8);

type VcalsType<'a> = heapless::Vec<vcal_parser::VCalendar<'a>, MAX_DAILY_EVENTS>;

type EpdDriver = WeActStudio420BlackWhiteDriver<
    SPIInterface<
        ExclusiveDevice<Spi<'static, esp_hal::Blocking>, Output<'static>, Delay>,
        Output<'static>,
    >,
    Input<'static>,
    Output<'static>,
    Delay,
>;

enum BootType {
    Display = 0,
    Config = 1,
}

impl BootType {
    #[allow(unused)]
    fn set_boot_type(val: BootType) {
        BOOT_TYPES.store(val as u8, Ordering::Relaxed);
    }

    fn get_boot_type() -> BootType {
        match BOOT_TYPES.load(Ordering::Relaxed) {
            0 => BootType::Display,
            1 => BootType::Config,
            _ => panic!(),
        }
    }
}

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    let flash = esp_storage::FlashStorage::new(peripherals.FLASH);
    storage::read_config(flash).await;

    let prev_boot_count = BOOT_COUNT.fetch_add(1, Ordering::SeqCst);
    log::info!("Boot count: {}", prev_boot_count + 1);

    // this affects the remaining stack
    esp_alloc::heap_allocator!(size: 64 * 1024);
    // SSL needs more RAM
    esp_alloc::heap_allocator!(#[unsafe(link_section = ".dram2_uninit")] size: 64 * 1024);

    let mut rtc = esp_hal::rtc_cntl::Rtc::new(peripherals.LPWR);

    let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    let mut io = esp_hal::gpio::Io::new(peripherals.IO_MUX);
    io.set_interrupt_handler(hardware::handler);

    let button = peripherals.GPIO0;

    let config = esp_hal::gpio::InputConfig::default().with_pull(esp_hal::gpio::Pull::Up);
    let mut button = Input::new(button, config);

    critical_section::with(|cs| {
        button.listen(esp_hal::gpio::Event::FallingEdge);
        hardware::BUTTON.borrow_ref_mut(cs).replace(button)
    });

    let (wifi_controller, interfaces) = esp_radio::wifi::new(
        RADIO_CONTROLLER.init(esp_radio::init().expect("Failed to initialize Wi-Fi controller")),
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
        NETWORK_STACK.init(StackResources::<4>::new()),
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

    log::info!("Microcontroller initilized");

    match BootType::get_boot_type() {
        BootType::Display => {
            let events = get_events(trng, &mut rtc, stack).await;

            let sclk = peripherals.GPIO12;
            let mosi = peripherals.GPIO11; // SDA -> MOSI
            let spi = peripherals.SPI2;
            let dc = peripherals.GPIO18;
            let rst = peripherals.GPIO4;
            let busy = peripherals.GPIO15;
            let cs = peripherals.GPIO10;
            let (mut display, mut driver) =
                init::init_display(sclk, mosi, spi, dc, rst, busy, cs).await;
            display::write_to_screen(&mut display, &mut driver, events, &mut rtc).await;
        }
        BootType::Config => {
            let app = picoserve::make_static!(
                picoserve::AppRouter<server::AppProps>,
                server::AppProps.build_app()
            );

            for task_id in 0..WEB_TASK_POOL_SIZE {
                spawner.must_spawn(web_task(task_id, stack, app));
            }
        }
    };
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

fn date_to_mins(dt: OffsetDateTime) -> u16 {
    dt.hour() as u16 * 60 + dt.minute() as u16
}

async fn get_events<'a>(
    trng: &mut esp_hal::rng::Trng,
    rtc: &mut esp_hal::rtc_cntl::Rtc<'_>,
    stack: embassy_net::Stack<'_>,
) -> VcalsType<'a> {
    let tls = mbedtls_rs::Tls::new(trng).unwrap();

    let cal_xml_buf = CAL_XML_BUF.init(heapless::String::new());
    let req_buffer = REQ_BUFFER.init([0u8; 8192]);

    let tcp_client = TcpClient::new(
        stack,
        networking::CLIENT_STATE.init(embassy_net::tcp::client::TcpClientState::new()),
    );
    let time_from_rtc =
        time::OffsetDateTime::from_unix_timestamp(rtc.current_time_us() as i64 / 1_000_000)
            .unwrap();

    let mut success = false;
    for tries in 1..=3 {
        req_buffer.fill(0);
        let req = networking::network_req(
            stack,
            &tcp_client,
            tls.reference(),
            time_from_rtc.date(),
            cal_xml_buf,
            req_buffer,
        );
        if let Ok(()) = embassy_time::with_timeout(embassy_time::Duration::from_secs(30), req).await
        {
            success = true;
            break;
        }
        log::warn!("Failed to get calendar data on attempt {tries}, retrying...");
    }

    if !success {
        log::error!("Failed after 3 attempts, entering deep sleep");
        go_to_deep_sleep(rtc);
    }

    log::trace!("Received calendar data len: {}", cal_xml_buf.len());
    let cal_strings = CAL_STRINGS.init(extract_calendar_data(cal_xml_buf));
    // todo do unfolding
    let events: VcalsType<'static> = cal_strings
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
    events
}
