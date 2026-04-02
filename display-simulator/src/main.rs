use crate::display::DISPLAY_HOURS;
use embedded_graphics::prelude::*;
use embedded_graphics_simulator::SimulatorDisplay;
use jiff::{Timestamp, Zoned, civil::DateTime, tz::TimeZone};
use weact_studio_epd::Color;

#[path = "../../src/display.rs"]
mod display;

fn sample_event<'a>(
    start_time: DateTime,
    end_time: DateTime,
    title: &'a str,
) -> (Zoned, Zoned, &'a str) {
    let tz = TimeZone::UTC;
    (
        start_time.to_zoned(tz.clone()).unwrap(),
        end_time.to_zoned(tz).unwrap(),
        title,
    )
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();

    // 300x400 because we rotate the display
    let mut display = SimulatorDisplay::<Color>::with_default_color(
        Size::new(display::DISPLAY_WIDTH, display::DISPLAY_HEIGHT),
        Color::White,
    );

    let now = Zoned::now()
        .with()
        .hour(21)
        .minute(32)
        .second(1)
        .subsec_nanosecond(31) // Zeroes out the fractional seconds
        .build()
        .unwrap();

    let start_display_hour: u8 = now
        .hour()
        .clamp(0, 24 - DISPLAY_HOURS as i8)
        .try_into()
        .unwrap();

    display::draw_time_row_header(&mut display, start_display_hour);

    let events: Vec<(Zoned, Zoned, &str)> = vec![
        sample_event(
            "2026-07-11T00:00:00".parse().unwrap(),
            "2026-07-11T01:30:00".parse().unwrap(),
            "Morning",
        ),
        sample_event(
            "2026-07-11T08:10:00".parse().unwrap(),
            "2026-07-11T08:20:00".parse().unwrap(),
            "Breakfast",
        ),
        sample_event(
            "2026-07-11T08:00:00".parse().unwrap(),
            "2026-07-11T14:00:00".parse().unwrap(),
            "Work",
        ),
        sample_event(
            "2026-07-11T23:00:00".parse().unwrap(),
            "2026-07-11T23:59:00".parse().unwrap(),
            "Midnight",
        ),
        sample_event(
            "2026-07-11T16:00:00".parse().unwrap(),
            "2026-07-11T17:59:00".parse().unwrap(),
            "Very very very long event name",
        ),
    ];

    display::draw_time_ticker(&mut display, &now, start_display_hour);
    display::draw_base_calendar(&mut display, start_display_hour);
    display::draw_sync_time(&mut display, &now);
    //display::draw_days(&mut display, &now.weekday(), 3);

    for (start, end, title) in &events {
        display::draw_event(&mut display, start, end, title, start_display_hour);
    }

    display::add_footer_info(&mut display);

    let output_settings = embedded_graphics_simulator::OutputSettingsBuilder::new()
        .scale(2)
        .build();
    embedded_graphics_simulator::Window::new("Sim window", &output_settings).show_static(&display);
}
