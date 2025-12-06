#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use alloc::format;
use display_interface_spi::SPIInterface;
use embassy_executor::Spawner;
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::prelude::{Dimensions, OriginDimensions, Point, Size};
use embedded_graphics::prelude::{Drawable, Primitive};
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
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

use esp_println::println;
use log::info;

use esp_backtrace as _;
use profont::PROFONT_10_POINT;
use weact_studio_epd::graphics::{Display420BlackWhite, DisplayRotation};
use weact_studio_epd::{Color, WeActStudio420BlackWhiteDriver};

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

const DAYS_TO_DISPLAY: u8 = 3;
const HOURS_TO_DISPLAY: u8 = 24;
const MINUTES_IN_A_DAY: u16 = 1440;
const EVENT_FONT: MonoFont = PROFONT_10_POINT;

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(size: 64 * 1024);
    // COEX needs more RAM - so we've added some more
    esp_alloc::heap_allocator!(#[unsafe(link_section = ".dram2_uninit")] size: 64 * 1024);

    let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

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
    // set it to be longer not wider
    display.set_rotation(DisplayRotation::Rotate270);
    driver.init().unwrap();
    log::info!("EPD initialized!");

    draw_time_row_header(&mut display);
    // draw_base_calendar(&mut display);
    draw_event(&mut display, 0, MINUTES_IN_A_DAY);
    add_footer_info(&mut display);

    driver.full_update(&display).unwrap();

    let _ = spawner;
}

fn add_footer_info(display: &mut Display420BlackWhite) {
    use embedded_graphics::text::{Baseline, Text};

    let git_commit = option_env!("GIT_SHORT").unwrap_or("unknown");
    let git_dirty: bool = option_env!("GIT_DIRTY")
        .unwrap_or("false")
        .parse()
        .unwrap_or_default();
    let mut build_info = format!("commit: {git_commit}");
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

fn calculate_padding(full_size: u32, text_size: i32, item_count: i32) -> i32 {
    info!("{} {} {}", full_size, text_size, item_count);
    // calculate how many could fit at max
    let s = full_size as i32 - (text_size * item_count);
    s / item_count
}

fn calculate_left_side_width(font_size: u32, char_count: i32) -> i32 {
    font_size as i32 * char_count
}

fn draw_time_row_header(display: &mut Display420BlackWhite) {
    let text_height = EVENT_FONT.character_size.height as i32;
    let mut exceeded_height: i32 = 0;
    let mut hour = 0;

    let padding = calculate_padding(display.size().height, text_height, HOURS_TO_DISPLAY as i32);
    info!("padding: {}", padding);

    let text_style = MonoTextStyle::new(&EVENT_FONT, Color::Black);
    let position = display.bounding_box().top_left;

    for _ in 0..=HOURS_TO_DISPLAY {
        Text::with_baseline(
            &format!("{:0>2}:00", hour),
            position + Point::new(0, exceeded_height),
            text_style,
            embedded_graphics::text::Baseline::Top,
        )
        .draw(display)
        .unwrap();
        exceeded_height += text_height + padding;
        hour += 1;
    }
}

fn draw_base_calendar(display: &mut Display420BlackWhite) {
    let event_width = display.size().width / (DAYS_TO_DISPLAY + 1) as u32;
    let start_x = calculate_left_side_width(EVENT_FONT.character_size.width, 5 + 1);
    let start_y = EVENT_FONT.character_size.height / 2;

    let mut start_pos = Point::new(start_x, start_y as i32);
    let mut finish_pos = Point::new(display.size().width as i32, start_pos.y);

    Line::new(start_pos, finish_pos)
        .into_styled(PrimitiveStyle::with_stroke(Color::Black, 1))
        .draw(display)
        .unwrap();
}

/// Calculates the starting position of the event based on the screen size
fn calculate_start_height(display_height: u32, start_minute: u16) -> u32 {
    println!("display_height: {}", display_height);
    let padding = calculate_padding(
        display_height,
        EVENT_FONT.character_size.height as i32,
        HOURS_TO_DISPLAY as i32,
    );
    let offset = EVENT_FONT.character_size.height / 2;
    // Available height for the calendar content
    let available_height = display_height - (padding as u32 + offset);
    // one minute in pixels
    let one_minute = available_height as f32 / MINUTES_IN_A_DAY as f32;
    println!("one_minute: {}", one_minute);
    offset + (one_minute * start_minute as f32) as u32
}

fn calculate_event_width(display_width: i32, left_offset: i32) -> i32 {
    display_width - (display_width - left_offset)
}

/// Calculates the ending position of the event based on the screen size
fn calculate_end_height(display_height: u32, end_minute: u16) -> u32 {
    println!("display_height: {}", display_height);
    let padding = calculate_padding(
        display_height,
        EVENT_FONT.character_size.height as i32,
        HOURS_TO_DISPLAY as i32,
    );
    // Available height for the calendar content
    let available_height = display_height - padding as u32;
    // one minute in pixels
    let one_minute = available_height as f32 / MINUTES_IN_A_DAY as f32;
    println!("one_minute: {}", one_minute);
    (one_minute * end_minute as f32) as u32 - EVENT_FONT.character_size.height / 2
}

fn draw_event(display: &mut Display420BlackWhite, start_minute: u16, end_minute: u16) {
    let x = 50;
    let y = calculate_start_height(display.size().height, start_minute);

    let end_x = 50;
    let end_y = calculate_end_height(display.size().height, end_minute);

    Rectangle::new(Point::new(x, y as i32), Size::new(end_x, end_y - y))
        .into_styled(PrimitiveStyle::with_stroke(Color::Black, 1))
        .draw(display)
        .unwrap();
}
