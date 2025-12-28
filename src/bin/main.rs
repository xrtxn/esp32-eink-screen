#![feature(type_alias_impl_trait)]
#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use embassy_executor::Spawner;
use embassy_net::Stack;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::TcpClient;
use embassy_net::{StackResources, tcp::TcpSocket};
use embassy_time::{Duration, Timer};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::prelude::{Dimensions, OriginDimensions, Point, Size};
use embedded_graphics::prelude::{Drawable, Primitive};
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use esp_hal::clock::CpuClock;
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::peripherals::{RSA, SHA};
use esp_hal::rng::Rng;
use esp32_thesis::{connection, net_task};

use esp_hal::{
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    spi::{
        Mode,
        master::{Config, Spi},
    },
    time::Rate,
};
use esp_mbedtls::Tls;

use esp_println::println;
use log::info;

use esp_backtrace as _;
use profont::PROFONT_10_POINT;
use reqwless::client::{HttpClient, TlsConfig};
use reqwless::request::RequestBuilder;
use reqwless::{Certificates, X509};
use smoltcp::socket::dns::DnsQuery;
use smoltcp::wire::{DnsQueryType, DnsQuestion};
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
const START_POS: i32 = 40;

// add missing null byte
const CERT: &[u8] = concat!(include_str!("../../cert.pem"), "\0").as_bytes();

// When you are okay with using a nightly compiler it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

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
    let sw_int =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);

    let radio_init = mk_static!(
        esp_radio::Controller<'static>,
        esp_radio::init().expect("Failed to initialize Wi-Fi/BLE controller")
    );
    let (mut wifi_controller, interfaces) =
        esp_radio::wifi::new(radio_init, peripherals.WIFI, Default::default())
            .expect("Failed to initialize Wi-Fi controller");

    let wifi_interface = interfaces.sta;

    let config = embassy_net::Config::dhcpv4(Default::default());

    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // Init network stack
    let (stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        mk_static!(StackResources::<3>, StackResources::<3>::new()),
        seed,
    );

    spawner.spawn(connection(wifi_controller)).ok();
    spawner.spawn(net_task(runner)).ok();

    loop {
        if stack.is_link_up() {
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    println!("Waiting to get IP address...");
    loop {
        if let Some(config) = stack.config_v4() {
            println!("Got IP: {}", config.address);
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    let rsa_peripherals = peripherals.RSA;
    let sha_peripherals = peripherals.SHA;

    let cal_xml = network_req(stack, rsa_peripherals, sha_peripherals).await;
    parse_calendar(&cal_xml).await;

    // let sclk = peripherals.GPIO12;
    // let mosi = peripherals.GPIO11; // SDA -> MOSI

    // let spi_bus = Spi::new(
    //     peripherals.SPI2,
    //     Config::default()
    //         .with_frequency(Rate::from_khz(100))
    //         .with_mode(Mode::_0),
    // )
    // .unwrap()
    // .with_sck(sclk)
    // .with_mosi(mosi);

    // let dc = Output::new(peripherals.GPIO18, Level::Low, OutputConfig::default());
    // let rst = Output::new(peripherals.GPIO4, Level::High, OutputConfig::default());
    // let busy = Input::new(
    //     peripherals.GPIO15,
    //     InputConfig::default().with_pull(Pull::None),
    // );
    // let cs = Output::new(peripherals.GPIO10, Level::High, OutputConfig::default());

    // log::info!("Intializing SPI Device...");
    // let spi_device =
    //     ExclusiveDevice::new(spi_bus, cs, Delay::new()).expect("SPI device initialize error");
    // let spi_interface = SPIInterface::new(spi_device, dc);

    // log::info!("Intializing EPD...");
    // let mut driver = WeActStudio420BlackWhiteDriver::new(spi_interface, busy, rst, Delay::new());
    // let mut display = Display420BlackWhite::new();
    // // set it to be longer not wider
    // display.set_rotation(DisplayRotation::Rotate270);
    // driver.init().unwrap();
    // log::info!("EPD initialized!");
    // // driver.full_update(&display).unwrap();
}

async fn network_req(
    stack: Stack<'_>,
    rsa_peripherial: RSA<'_>,
    sha_peripherial: SHA<'_>,
) -> String {
    Timer::after(Duration::from_millis(1_000)).await;

    let client_state = mk_static!(
        embassy_net::tcp::client::TcpClientState::<1, 4096, 4096>,
        embassy_net::tcp::client::TcpClientState::new()
    );

    let tcp_client = TcpClient::new(stack, client_state);
    let dns_socket = DnsSocket::new(stack);

    let tls = Tls::new(sha_peripherial)
        .unwrap()
        .with_hardware_rsa(rsa_peripherial);
    let mut certs = Certificates::new();
    certs.ca_chain = Some(X509::pem(CERT).unwrap());
    let tls_config = TlsConfig::new(reqwless::TlsVersion::Tls1_3, certs, tls.reference());

    let mut client = HttpClient::new_with_tls(&tcp_client, &dns_socket, tls_config);

    let mut req_buffer = [0; 4096];

    let creds = include_str!("../../passwd.txt");

    let origin = creds.split('\n').nth(2).unwrap();

    let body = r#"<?xml version="1.0" encoding="utf-8" ?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
    <d:prop>
        <d:getetag/>
        <c:calendar-data/>
    </d:prop>
    <c:filter>
        <c:comp-filter name="VCALENDAR">
            <c:comp-filter name="VEVENT">
                <c:time-range start="20251222T000000" end="20251222T235959"/>
            </c:comp-filter>
        </c:comp-filter>
    </c:filter>
</c:calendar-query>"#
        .to_owned();

    let username = creds.split('\n').nth(3).unwrap();
    let password = creds.split('\n').nth(4).unwrap();
    let path = format!("/remote.php/dav/calendars/{}/personal/", username);

    let mut request = client
        .request(reqwless::request::Method::REPORT, &origin)
        .await
        .unwrap()
        .basic_auth(username, password)
        .path(&path)
        .headers(&[("Content-Type", "text/xml; charset=utf-8"), ("Depth", "1")])
        .body(body.as_bytes());

    let response = request.send(&mut req_buffer).await.unwrap();
    println!("Response status: {:?}", response.status);

    let res = response.body().read_to_end().await.unwrap();

    let res = match str::from_utf8(&res) {
        Ok(v) => v,
        Err(_) => {
            println!("Response body (hex): {:02x?}", res);
            todo!()
        }
    };
    res.to_owned()
}

async fn parse_calendar(_data: &str) {
    println!("Parsing calendar data... {:?}", _data);
    let parsed = roxmltree::Document::parse(_data).unwrap();
    let elem = parsed
        .descendants()
        .find(|n| n.has_tag_name("calendar-data"))
        .unwrap();
    let calendar_data = elem.text().unwrap();
    println!("Calendar data: {:?}", calendar_data);
}

#[allow(dead_code)]
async fn manual_socket(stack: Stack<'_>) {
    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];

    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);

    socket.set_timeout(Some(embassy_time::Duration::from_secs(10)));

    loop {
        let addr = stack
            .dns_query("www.httpbin.org", DnsQueryType::A)
            .await
            .unwrap()[0];

        println!("Resolved address: {:?}", addr);

        println!("connecting...");
        let r = socket.connect((addr, 80)).await;
        if let Err(e) = r {
            println!("connect error: {:?}", e);
            continue;
        }
        println!("connected!");
        let mut buf = [0; 1024];
        loop {
            let r = socket
                .write(b"GET /get HTTP/1.0\r\nHost: www.httpbin.org\r\n\r\n")
                .await;
            if let Err(e) = r {
                println!("write error: {:?}", e);
                break;
            }
            let n = match socket.read(&mut buf).await {
                Ok(0) => {
                    println!("read EOF");
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    println!("read error: {:?}", e);
                    break;
                }
            };
            println!("{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
    }
}

fn add_example_events(mut display: &mut Display420BlackWhite) {
    draw_time_row_header(&mut display);
    // draw_base_calendar(&mut display);
    draw_event(&mut display, 360, 420, START_POS, 110, "Állatok etetése");
    draw_event(&mut display, 420, 480, START_POS, 60, "Reggeli");
    draw_event(
        &mut display,
        480,
        720,
        40,
        70,
        "Formális nyelvek, automaták",
    );
    draw_event(&mut display, 720, 860, START_POS, 85, "IoT technológia");
    draw_event(&mut display, 760, 860, 125, 72, "5-ös jegy");
    draw_event(&mut display, 760, 860, 125, 72, "5-ös jegy");
    draw_event(&mut display, 860, 920, START_POS, 36, "Ebéd");
    draw_event(&mut display, 920, 1030, START_POS, 50, "Pihenés");
    draw_event(&mut display, 960, 1030, 90, 45, "Alvás");
    draw_event(&mut display, 1080, 1140, START_POS, 72, "Vacsora");
    draw_event(
        &mut display,
        1140,
        1260,
        START_POS,
        100,
        "Liverpool - FC Barcelona",
    );
    draw_event(&mut display, 1320, 1600, START_POS, 72, "Alvás");
    //add_footer_info(&mut display);
    //draw_days(&mut display, DAYS_TO_DISPLAY);
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
    // calculate how many could fit at max
    let s = full_size as i32 - (text_size * item_count);
    s / item_count
}

fn calculate_left_side_width(font_size: u32, char_count: i32) -> i32 {
    font_size as i32 * char_count
}

fn draw_time_row_header(display: &mut Display420BlackWhite) {
    let text_height = EVENT_FONT.character_size.height as i32;
    let extra_bottom_space = EVENT_FONT.character_size.height;
    let mut exceeded_height: i32 = 0;
    let mut hour = 0;

    let padding = calculate_padding(
        display.size().height - extra_bottom_space,
        text_height,
        HOURS_TO_DISPLAY as i32,
    );
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
    let text_height = EVENT_FONT.character_size.height as i32;
    let extra_bottom_space = EVENT_FONT.character_size.height;
    let padding = calculate_padding(
        display_height - extra_bottom_space,
        text_height,
        HOURS_TO_DISPLAY as i32,
    );

    let one_hour_height = text_height + padding;
    let one_minute = one_hour_height as f32 / 60.0;

    println!("one_minute: {}", one_minute);
    (one_minute * start_minute as f32) as u32
}

fn calculate_event_width(display_width: i32, left_offset: i32) -> i32 {
    display_width - (display_width - left_offset)
}

/// Calculates the ending position of the event based on the screen size
fn calculate_end_height(display_height: u32, end_minute: u16) -> u32 {
    println!("display_height: {}", display_height);
    let text_height = EVENT_FONT.character_size.height as i32;
    let extra_bottom_space = EVENT_FONT.character_size.height;
    let padding = calculate_padding(
        display_height - extra_bottom_space,
        text_height,
        HOURS_TO_DISPLAY as i32,
    );
    let one_hour_height = text_height + padding;
    let one_minute = one_hour_height as f32 / 60.0;
    (one_minute * end_minute as f32) as u32
}

fn draw_event(
    display: &mut Display420BlackWhite,
    start_minute: u16,
    end_minute: u16,
    x: i32,
    end_x: i32,
    text: &str,
) {
    let y = calculate_start_height(display.size().height, start_minute);

    let end_y = calculate_end_height(display.size().height, end_minute);

    Rectangle::new(Point::new(x, y as i32), Size::new(end_x as u32, end_y - y))
        .into_styled(PrimitiveStyle::with_stroke(Color::Black, 1))
        .draw(display)
        .unwrap();

    info!(
        "Drawing event '{}' at position ({}, {}), ending at y {}",
        text, x, y, end_y
    );

    let text_style = MonoTextStyle::new(&EVENT_FONT, Color::Black);
    let char_width = EVENT_FONT.character_size.width;
    let max_chars_per_line = (end_x - 4).max(0) as u32 / char_width;

    if max_chars_per_line == 0 {
        return;
    }

    info!("max_chars_per_line: {}", max_chars_per_line);

    let mut wrapped_text = alloc::string::String::new();
    let mut current_line_len = 0;

    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        let space_len = if current_line_len > 0 { 1 } else { 0 };

        if current_line_len + space_len + word_len > max_chars_per_line as usize
            && current_line_len > 0
        {
            wrapped_text.push('\n');
            current_line_len = 0;
        }

        if current_line_len > 0 {
            wrapped_text.push(' ');
            current_line_len += 1;
        }

        wrapped_text.push_str(word);
        current_line_len += word_len;
    }

    Text::with_baseline(
        &wrapped_text,
        Point::new(x + 2, y as i32 + 2),
        text_style,
        embedded_graphics::text::Baseline::Top,
    )
    .draw(display)
    .unwrap();
}

fn draw_days(display: &mut Display420BlackWhite, count: u8) {
    let left_padding: i32 = calculate_left_side_width(EVENT_FONT.character_size.width, 5 + 1);
    let text_style = MonoTextStyle::new(&EVENT_FONT, Color::Black);
    let starting_x = 0 + left_padding + 5;
    let y = display.bounding_box().size.height - EVENT_FONT.character_size.height;
    let mut x_offset = 0;
    for day in 0..count {
        let day_text = match day {
            0 => "Today",
            1 => "Tomorrow",
            2 => "Overmorrow",
            _ => &format!("Day {}", day),
        };
        x_offset += day_text.chars().count() as i32 * EVENT_FONT.character_size.width as i32 + 15;
        let pos = Point::new(starting_x + x_offset, y as i32);
        info!("Drawing day '{}' at position {:?}", day_text, pos);

        Text::with_baseline(
            &day_text,
            pos,
            text_style,
            embedded_graphics::text::Baseline::Top,
        )
        .draw(display)
        .unwrap();
    }
}
