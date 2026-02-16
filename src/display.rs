use alloc::format;
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::prelude::{Dimensions, OriginDimensions, Point, Size};
use embedded_graphics::prelude::{Drawable, Primitive};
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use esp_println::println;
use log::info;
use profont::PROFONT_10_POINT;
use weact_studio_epd::graphics::Display420BlackWhite;
use weact_studio_epd::Color;

const DAYS_TO_DISPLAY: u8 = 3;
const HOURS_TO_DISPLAY: u8 = 24;
const MINUTES_IN_A_DAY: u16 = 1440;
const EVENT_FONT: MonoFont = PROFONT_10_POINT;
const START_POS: i32 = 40;

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
