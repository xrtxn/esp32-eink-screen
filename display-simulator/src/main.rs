use embedded_graphics::prelude::*;
use embedded_graphics_simulator::SimulatorDisplay;
use jiff::{Zoned, civil::DateTime, tz::TimeZone};
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
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 300x400 because we rotate the display
    let mut display = SimulatorDisplay::<Color>::with_default_color(
        Size::new(display::DISPLAY_WIDTH, display::DISPLAY_HEIGHT),
        Color::White,
    );

    display::draw_time_row_header(&mut display);

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

    let now = jiff::Zoned::now();
    display::draw_time_ticker(&mut display, &now);
    display::draw_base_calendar(&mut display);
    display::draw_sync_time(&mut display, &now);
    //display::draw_days(&mut display, &now.weekday(), 3);

    for (start, end, title) in &events {
        display::draw_event(&mut display, start, end, title);
    }

    display::add_footer_info(&mut display);

    let output_settings = embedded_graphics_simulator::OutputSettingsBuilder::new()
        .scale(2)
        .build();
    embedded_graphics_simulator::Window::new("Sim window", &output_settings).show_static(&display);
}
