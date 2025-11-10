#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use alloc::format;
use display_interface_spi::SPIInterface;
use embedded_graphics::prelude::{Dimensions, Point};
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::clock::CpuClock;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::{
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    spi::{
        master::{Config, Spi},
        Mode,
    },
    time::Rate,
};

use log::info;

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};

use esp_backtrace as _;
use weact_studio_epd::graphics::{Display420BlackWhite, DisplayRotation};
use weact_studio_epd::{Color, WeActStudio420BlackWhiteDriver};

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(size: 64 * 1024);
    // COEX needs more RAM - so we've added some more
    esp_alloc::heap_allocator!(#[unsafe(link_section = ".dram2_uninit")] size: 64 * 1024);

    let timer0 = SystemTimer::new(peripherals.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);

    info!("Embassy initialized!");

    let sclk = peripherals.GPIO12;
    let mosi = peripherals.GPIO11; // SDA -> MOSI

    let spi_bus = Spi::new(
        peripherals.SPI2,
        Config::default()
            .with_frequency(Rate::from_khz(100))
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
    display.set_rotation(DisplayRotation::Rotate90);
    driver.init().unwrap();
    log::info!("EPD initialized!");

    add_footer_info(&mut display);

    driver.full_update(&display).unwrap();

    let _ = spawner;

    loop {
        info!("Hello world!");
        Timer::after(Duration::from_secs(1)).await;
    }
}

fn add_footer_info(display: &mut Display420BlackWhite) {
    use embedded_graphics::mono_font::MonoTextStyle;
    use embedded_graphics::prelude::Drawable;
    use embedded_graphics::text::{Baseline, Text};

    let build_date = option_env!("GIT_SHORT").unwrap_or("unknown");
    let git_dirty: bool = option_env!("GIT_DIRTY")
        .unwrap_or("false")
        .parse()
        .unwrap_or_default();
    let mut build_info = format!("commit: {build_date}");
    if git_dirty {
        build_info.push_str("*");
    }

    let font = profont::PROFONT_7_POINT;
    let text_style = MonoTextStyle::new(&font, Color::Black);

    let br = display.bounding_box().bottom_right().unwrap();

    let text_width = build_info.chars().count() as i32 * font.character_size.width as i32;
    let pos = Point::new(br.x - text_width, br.y - font.character_size.height as i32);

    Text::with_baseline(&build_info, pos, text_style, Baseline::Top)
        .draw(display)
        .unwrap();
}
