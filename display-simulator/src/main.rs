use embedded_graphics::prelude::*;
use embedded_graphics_simulator::SimulatorDisplay;
use jiff::{
    civil::{Date, DateTime},
    tz::{self, TimeZone},
    SignedDuration, Zoned,
};
use weact_studio_epd::Color;

#[path = "../../src/display.rs"]
mod display;

fn loc_date_to_mins(dt: &DateTime) -> u16 {
    log::info!("Converting local date to minutes: {:?}", dt);
    (dt.hour() as u16 * 60 + dt.minute() as u16) as u16
}

fn sample_event<'a>(
    start_time: &DateTime,
    end_time: &DateTime,
    title: &'a str,
) -> (u16, u16, &'a str) {
    (
        loc_date_to_mins(start_time),
        loc_date_to_mins(end_time),
        title,
    )
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 300x400 because we rotate the display
    let mut display =
        SimulatorDisplay::<Color>::with_default_color(Size::new(300, 400), Color::White);

    display::draw_time_row_header(&mut display);

    let events: &[(u16, u16, &str)] = &[
        sample_event(
            &"2026-07-11T00:00:00".parse().unwrap(),
            &"2026-07-11T01:30:00".parse().unwrap(),
            "Morning",
        ),
        sample_event(
            &"2026-07-11T08:10:00".parse().unwrap(),
            &"2026-07-11T08:20:00".parse().unwrap(),
            "Breakfast",
        ),
        sample_event(
            &"2026-07-11T23:00:00".parse().unwrap(),
            &"2026-07-11T23:59:00".parse().unwrap(),
            "Midnight",
        ),
    ];

    let now = jiff::Zoned::now();
    display::draw_time_ticker(&mut display, &now);
    display::draw_base_calendar(&mut display);
    display::draw_sync_time(&mut display, &now);
    display::draw_days(&mut display, &now.weekday(), 3);

    for &(start, end, title) in events {
        display::draw_event(&mut display, start, end, title);
    }

    display::add_footer_info(&mut display);

    let output_settings = embedded_graphics_simulator::OutputSettingsBuilder::new()
        .scale(2)
        .build();
    embedded_graphics_simulator::Window::new("Sim window", &output_settings).show_static(&display);
}
