use core::u32;

#[cfg(target_arch = "xtensa")]
use alloc::format;

use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::prelude::{Dimensions, DrawTarget, OriginDimensions, Point};
use embedded_graphics::prelude::{Drawable, Primitive};
use embedded_graphics::primitives::{Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::Text;
use heapless::format as hformat;
use log::info;
#[cfg(target_arch = "xtensa")]
use weact_studio_epd::graphics::Display420BlackWhite;
use weact_studio_epd::Color as EpdColor;

const START_DISPLAY_HOUR: u8 = 10;
const DISPLAY_HOURS: u8 = validate_hours(START_DISPLAY_HOUR, 6);
pub const DISPLAY_WIDTH: u32 = 300;
pub const DISPLAY_HEIGHT: u32 = 400;
const EXTRA_BOTTOM_SPACE: i32 = 15;
const EVENT_FONT: MonoFont = profont::PROFONT_10_POINT;
const MINI_FONT: MonoFont = profont::PROFONT_7_POINT;
const START_POS: i32 = calculate_text_width(5, EVENT_FONT) as i32;
const CHARACTER_STYLE: MonoTextStyle<'static, EpdColor> =
    MonoTextStyle::new(&EVENT_FONT, EpdColor::Black);
const MINI_CHARACTER_STYLE: MonoTextStyle<'static, EpdColor> =
    MonoTextStyle::new(&MINI_FONT, EpdColor::Black);

const OVERWRITE_STYLE: PrimitiveStyle<EpdColor> = PrimitiveStyleBuilder::new()
    .fill_color(EpdColor::White)
    .stroke_color(EpdColor::Black)
    .stroke_width(1)
    .build();

const BORDERLESS_OVERWRITE_STYLE: PrimitiveStyle<EpdColor> =
    PrimitiveStyle::with_fill(EpdColor::White);

const fn calculate_row_padding(start_hour: u8, end_hour: u8) -> i32 {
    assert!(
        end_hour > start_hour,
        "End hour must be greater than start hour"
    );
    let text_height = EVENT_FONT.character_size.height as i32;
    let item_count = (end_hour - start_hour) as i32;
    let total_text_size = text_height * item_count;
    let remaining_space = DISPLAY_HEIGHT as i32 - EXTRA_BOTTOM_SPACE - total_text_size;
    remaining_space / (item_count)
}

const fn validate_hours(start: u8, duration: u8) -> u8 {
    assert!(start + duration <= 24, "Display hours exceed 24-hour limit");
    duration
}

pub(crate) fn add_footer_info<D>(display: &mut D)
where
    D: DrawTarget<Color = EpdColor> + OriginDimensions,
    D::Error: core::fmt::Debug,
{
    use embedded_graphics::text::Text;

    let git_commit = env!("GIT_SHORT");
    let git_dirty: bool = env!("GIT_DIRTY").parse().unwrap_or(false);
    // 8 is the text + 8 is short hash in build.rs + 1 is possible *
    let mut build_info: heapless::String<17> = hformat!("commit: {git_commit}").unwrap();
    if git_dirty {
        build_info.push_str("*").unwrap();
    }

    let text_style = embedded_graphics::text::TextStyleBuilder::new()
        .alignment(embedded_graphics::text::Alignment::Right)
        .baseline(embedded_graphics::text::Baseline::Bottom)
        .build();

    let etext = Text::with_text_style(
        &build_info,
        display.bounding_box().bottom_right().unwrap(),
        MINI_CHARACTER_STYLE,
        text_style,
    );

    let mut text_bb = etext.bounding_box();
    text_bb.size.width += 2;
    text_bb.size.height += 1;
    text_bb.top_left.x -= 1;

    text_bb.into_styled(OVERWRITE_STYLE).draw(display).unwrap();

    etext.draw(display).unwrap();
}

pub(crate) fn draw_time_row_header<D>(display: &mut D)
where
    D: DrawTarget<Color = EpdColor> + OriginDimensions,
    D::Error: core::fmt::Debug,
{
    let text_height = EVENT_FONT.character_size.height as i32;
    let mut exceeded_height: i32 = 0;

    let position = display.bounding_box().top_left;

    for hour in START_DISPLAY_HOUR..=START_DISPLAY_HOUR + DISPLAY_HOURS {
        let fmt_hour: heapless::String<5> = hformat!("{:0>2}:00", hour).unwrap();
        Text::with_baseline(
            &fmt_hour,
            position + Point::new(0, exceeded_height),
            CHARACTER_STYLE,
            embedded_graphics::text::Baseline::Top,
        )
        .draw(display)
        .unwrap();
        let row_padding =
            calculate_row_padding(START_DISPLAY_HOUR, START_DISPLAY_HOUR + DISPLAY_HOURS);
        exceeded_height += text_height + row_padding;
    }
    // height is at max
}

/// Calculates the starting position of the event based on the screen size
fn calculate_start_height(start_minute: u16) -> u32 {
    let text_height = EVENT_FONT.character_size.height as i32;

    let row_padding = calculate_row_padding(START_DISPLAY_HOUR, START_DISPLAY_HOUR + DISPLAY_HOURS);
    let one_hour_height = text_height + row_padding;
    let one_minute = one_hour_height as f32 / 60.0;

    (one_minute * start_minute as f32) as u32 + (text_height / 2) as u32
}

/// Calculates the ending position of the event based on the screen size
const fn calculate_end_height(end_minute: u16) -> u32 {
    let text_height = EVENT_FONT.character_size.height as i32;

    let row_padding = calculate_row_padding(START_DISPLAY_HOUR, START_DISPLAY_HOUR + DISPLAY_HOURS);
    let one_hour_height = text_height + row_padding;
    let one_minute = one_hour_height as f32 / 60.0;

    let e = (one_minute * end_minute as f32) as u32 + (text_height / 2) as u32;
    e
}

const fn calculate_text_width(char_count: u16, font: MonoFont) -> u16 {
    if char_count == 0 {
        return 0;
    }

    (char_count * font.character_size.width as u16)
        + ((char_count - 1) * font.character_spacing as u16)
        + font.character_size.width as u16 / 2
}

pub(crate) fn draw_sync_time<D>(display: &mut D, time: &jiff::Zoned)
where
    D: DrawTarget<Color = EpdColor> + OriginDimensions,
    D::Error: core::fmt::Debug,
{
    log::info!("Calendar sync time: {}", time);
    let fmt_time: heapless::String<11> =
        hformat!("Sync: {:02}:{:02}", time.hour(), time.minute()).unwrap();

    let text_style = embedded_graphics::text::TextStyleBuilder::new()
        .alignment(embedded_graphics::text::Alignment::Right)
        .baseline(embedded_graphics::text::Baseline::Top)
        .build();

    let pos = Point::new(
        display.bounding_box().bottom_right().unwrap().x,
        display.bounding_box().top_left.y,
    );

    let etext = Text::with_text_style(&fmt_time, pos, MINI_CHARACTER_STYLE, text_style);

    let mut text_bb = etext.bounding_box();
    extend_rectangle(&mut text_bb);

    text_bb
        .bounding_box()
        .into_styled(BORDERLESS_OVERWRITE_STYLE)
        .draw(display)
        .unwrap();

    etext.draw(display).unwrap();
}

pub fn draw_base_calendar<D>(display: &mut D)
where
    D: DrawTarget<Color = EpdColor> + OriginDimensions,
    D::Error: core::fmt::Debug,
{
    let text_height = EVENT_FONT.character_size.height as i32;
    let text_width = EVENT_FONT.character_size.width as i32;
    let mut exceeded_height: i32 = 0;

    let position = display.bounding_box().top_left;

    for _ in START_DISPLAY_HOUR..=START_DISPLAY_HOUR + DISPLAY_HOURS {
        let start_pos = position + Point::new(text_width * 6, exceeded_height + text_height / 2);
        let finish_pos = position
            + Point::new(
                display.size().width as i32,
                exceeded_height + text_height / 2,
            );

        Line::new(start_pos, finish_pos)
            .into_styled(PrimitiveStyle::with_stroke(EpdColor::Black, 1))
            .draw(display)
            .unwrap();
        let row_padding =
            calculate_row_padding(START_DISPLAY_HOUR, START_DISPLAY_HOUR + DISPLAY_HOURS);
        exceeded_height += text_height + row_padding;
    }
    // height is at max
}

pub(crate) fn draw_time_ticker<D>(display: &mut D, time: &jiff::Zoned)
where
    D: DrawTarget<Color = EpdColor> + OriginDimensions,
    D::Error: core::fmt::Debug,
{
    let x = START_POS;
    let end_x = START_POS + 40;

    if time.hour() < START_DISPLAY_HOUR as i8
        || time.hour() > (START_DISPLAY_HOUR + DISPLAY_HOURS) as i8
    {
        log::warn!(
            "Current time {} is out of display bounds ({}-{}), skipping time ticker",
            time,
            START_DISPLAY_HOUR,
            START_DISPLAY_HOUR + DISPLAY_HOURS
        );
        return;
    }

    let y = calculate_start_height(date_to_mins(time) - START_DISPLAY_HOUR as u16 * 60);

    let end_y = y;

    Line::new(Point::new(x, y as i32), Point::new(end_x, end_y as i32))
        .into_styled(PrimitiveStyle::with_stroke(EpdColor::Black, 1))
        .draw(display)
        .unwrap();
}

pub(crate) fn draw_event<D>(display: &mut D, start: &jiff::Zoned, end: &jiff::Zoned, text: &str)
where
    D: DrawTarget<Color = EpdColor> + OriginDimensions,
    D::Error: core::fmt::Debug,
{
    if end.hour() < START_DISPLAY_HOUR as i8
        || start.hour() > (START_DISPLAY_HOUR + DISPLAY_HOURS) as i8
    {
        log::warn!(
            "Event '{}' is out of display bounds ({}-{}), skipping",
            text,
            start,
            end
        );
        return;
    }

    let x = START_POS;
    let end_x = calculate_text_width(text.chars().count() as u16, EVENT_FONT);

    let y =
        calculate_start_height(date_to_mins(start).saturating_sub(START_DISPLAY_HOUR as u16 * 60));
    let mut end_y =
        calculate_end_height(date_to_mins(end).saturating_sub(START_DISPLAY_HOUR as u16 * 60))
            .clamp(0, calculate_end_height(DISPLAY_HOURS as u16 * 60));

    let available_height = end_y - y;

    let single_line_height = EVENT_FONT.character_size.height;
    let double_line_height = single_line_height + MINI_FONT.character_size.height;

    let oneline = available_height < double_line_height;

    if available_height < single_line_height {
        end_y = y + single_line_height;
    }

    let time_str = format!("{}-{}", start.strftime("%H:%M"), end.strftime("%H:%M"));

    let time_text = Text::with_baseline(
        &time_str,
        Point::new(x, y as i32),
        MINI_CHARACTER_STYLE,
        embedded_graphics::text::Baseline::Top,
    );

    let title_point = if oneline {
        let time_width = time_text.bounding_box().size.width as i32;
        Point::new(x + time_width + 3, y as i32)
    } else {
        Point::new(x, y as i32 + MINI_FONT.character_size.height as i32)
    };

    let title_text = Text::with_baseline(
        text,
        title_point,
        CHARACTER_STYLE,
        embedded_graphics::text::Baseline::Top,
    );

    let time_bb = time_text.bounding_box();
    let title_bb = title_text.bounding_box();

    let min_x = time_bb.top_left.x.min(title_bb.top_left.x);
    let max_x = (time_bb.top_left.x + time_bb.size.width as i32)
        .max(title_bb.top_left.x + title_bb.size.width as i32);

    let mut ebb = Rectangle::with_corners(
        Point::new(min_x, y as i32),
        Point::new(max_x as i32, end_y as i32),
    );

    extend_rectangle(&mut ebb);

    ebb.into_styled(OVERWRITE_STYLE).draw(display).unwrap();

    time_text.draw(display).unwrap();
    title_text.draw(display).unwrap();
}

pub const fn extend_rectangle(rec: &mut Rectangle) {
    rec.size.width += 3;
    rec.top_left.x -= 2;
}

pub fn draw_days<D>(display: &mut D, current_day: &jiff::civil::Weekday, count: u8)
where
    D: DrawTarget<Color = EpdColor> + OriginDimensions,
    D::Error: core::fmt::Debug,
{
    let starting_x = START_POS;
    let y = display.bounding_box().size.height - EVENT_FONT.character_size.height;
    let mut x_offset = 0;
    // max 7 days supported
    let mut days: heapless::Vec<jiff::civil::Weekday, 7> = heapless::Vec::new();
    for i in 0..count.clamp(1, 7) {
        days.push(current_day.wrapping_add(i as i64)).unwrap();
    }
    for day in days {
        let day_text = match day {
            jiff::civil::Weekday::Monday => "Monday",
            jiff::civil::Weekday::Tuesday => "Tuesday",
            jiff::civil::Weekday::Wednesday => "Wednesday",
            jiff::civil::Weekday::Thursday => "Thursday",
            jiff::civil::Weekday::Friday => "Friday",
            jiff::civil::Weekday::Saturday => "Saturday",
            jiff::civil::Weekday::Sunday => "Sunday",
        };
        x_offset += day_text.chars().count() as i32 * EVENT_FONT.character_size.width as i32 + 15;
        let pos = Point::new(starting_x + x_offset, y as i32);
        info!("Drawing day '{}' at position {:?}", day_text, pos);

        Text::with_baseline(
            day_text,
            pos,
            CHARACTER_STYLE,
            embedded_graphics::text::Baseline::Top,
        )
        .draw(display)
        .unwrap();
    }
}

pub(crate) async fn draw_config<D>(display: &mut D, text: &str)
where
    D: DrawTarget<Color = EpdColor> + OriginDimensions,
    D::Error: core::fmt::Debug,
{
    let text_style = embedded_graphics::text::TextStyleBuilder::new()
        .alignment(embedded_graphics::text::Alignment::Center)
        .baseline(embedded_graphics::text::Baseline::Middle)
        .build();

    Text::with_text_style(
        text,
        display.bounding_box().center(),
        CHARACTER_STYLE,
        text_style,
    )
    .draw(display)
    .unwrap();
}

#[cfg(target_arch = "xtensa")]
use display_interface::AsyncWriteOnlyDataCommand;
#[cfg(target_arch = "xtensa")]
use embedded_hal::digital::OutputPin as EhalOutputPin;
#[cfg(target_arch = "xtensa")]
use embedded_hal_async::{delay::DelayNs, digital::Wait};
#[cfg(target_arch = "xtensa")]
use esp_hal::rtc_cntl::Rtc;
#[cfg(target_arch = "xtensa")]
use weact_studio_epd::WeActStudio420BlackWhiteDriver;

#[cfg(target_arch = "xtensa")]
use crate::hardware;

#[cfg(target_arch = "xtensa")]
pub(crate) async fn write_to_screen<DI, BSY, RST, DELAY>(
    display: &mut Display420BlackWhite,
    driver: &mut WeActStudio420BlackWhiteDriver<DI, BSY, RST, DELAY>,
    events: crate::networking::VcalsType<'_>,
    rtc: &mut Rtc<'_>,
) where
    DI: AsyncWriteOnlyDataCommand,
    BSY: embedded_hal::digital::InputPin + Wait,
    RST: EhalOutputPin,
    DELAY: DelayNs,
{
    crate::display::draw_time_row_header(display);
    let tz = jiff::tz::TimeZone::fixed(jiff::tz::offset(1));
    for event in events {
        for eevent in event.events {
            let start_dt = eevent.dtstart.unwrap().to_zoned(tz.clone());
            let end_dt = eevent.dtend.unwrap().to_zoned(tz.clone());
            draw_event(
                display,
                &start_dt,
                &end_dt,
                eevent.summary.unwrap_or("No summary"),
            );
        }
    }
    #[cfg(debug_assertions)]
    crate::display::add_footer_info(display);

    let time = hardware::get_time(rtc);
    crate::display::draw_sync_time(display, &time);
    crate::display::draw_time_ticker(display, &time);
    driver.full_update(display).await.unwrap();

    crate::hardware::go_to_deep_sleep(rtc);
}

pub fn date_to_mins(dt: &jiff::Zoned) -> u16 {
    dt.hour() as u16 * 60 + dt.minute() as u16
}
