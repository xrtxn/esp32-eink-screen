#![cfg_attr(not(test), no_std)]

#[cfg(not(test))]
extern crate alloc;

#[cfg(not(test))]
use alloc::string::String;
#[cfg(not(test))]
use alloc::vec::Vec;
use time::OffsetDateTime;

#[cfg(test)]
use std::string::String;
#[cfg(test)]
use std::vec::Vec;

use nom::{
    bytes::complete::{tag, take_while1},
    character::complete::{char, line_ending, not_line_ending},
    combinator::opt,
    multi::many0,
    sequence::{preceded, separated_pair},
    IResult, Parser,
};

#[derive(Debug, Clone, PartialEq)]
pub struct VCalendar<'a> {
    pub version: Option<&'a str>,
    pub prodid: Option<&'a str>,
    pub events: Vec<VEvent<'a>>,
    pub timezones: Vec<VTimezone<'a>>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct VEvent<'a> {
    pub uid: Option<&'a str>,
    pub summary: Option<&'a str>,
    pub description: Option<&'a str>,
    pub location: Option<&'a str>,
    pub dtstart: Option<OffsetDateTime>,
    pub dtend: Option<OffsetDateTime>,
    pub dtstamp: Option<&'a str>,
    pub status: Option<&'a str>,
    pub rrule: Option<&'a str>,
    pub categories: Option<&'a str>,
    pub organizer: Option<&'a str>,
    pub url: Option<&'a str>,
    pub priority: Option<&'a str>,
    pub sequence: Option<&'a str>,
    pub transp: Option<&'a str>,
    pub created: Option<&'a str>,
    pub last_modified: Option<&'a str>,
    pub alarms: Vec<VAlarm<'a>>,
}

fn parse_date(dt: &str) -> time::OffsetDateTime {
    use time::format_description::well_known::Iso8601;
    time::OffsetDateTime::parse(dt, &Iso8601::DEFAULT).unwrap()
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct VAlarm<'a> {
    pub trigger: Option<&'a str>,
    pub action: Option<&'a str>,
    pub description: Option<&'a str>,
    pub repeat: Option<&'a str>,
    pub duration: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct VTimezone<'a> {
    pub tzid: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Property<'a> {
    pub name: &'a str,
    pub params: Vec<(&'a str, &'a str)>,
    pub value: &'a str,
}

/// Unfold lines according to RFC 5545.
/// Long lines can be folded by inserting a (CR)LF followed by a single whitespace character.
pub fn unfold_lines(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\r' {
            // Check for CRLF followed by space/tab (folded line)
            if chars.peek() == Some(&'\n') {
                chars.next(); // consume '\n'
                if let Some(&next) = chars.peek() {
                    if next == ' ' || next == '\t' {
                        chars.next();
                        continue;
                    }
                }
                // Not a fold
                result.push('\r');
                result.push('\n');
            } else {
                result.push(c);
            }
        } else if c == '\n' {
            // Handle bare LF followed by space/tab
            if let Some(&next) = chars.peek() {
                if next == ' ' || next == '\t' {
                    chars.next();
                    continue;
                }
            }
            result.push(c);
        } else {
            result.push(c);
        }
    }

    result
}

/// Parse a property name (alphanumeric and hyphens)
fn property_name(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| c.is_alphanumeric() || c == '-').parse(input)
}

/// Parse a parameter (e.g., TZID=Europe/Budapest)
fn parameter(input: &str) -> IResult<&str, (&str, &str)> {
    separated_pair(
        take_while1(|c: char| c.is_alphanumeric() || c == '-'),
        char('='),
        take_while1(|c: char| c != ':' && c != ';' && c != '\r' && c != '\n'),
    )
    .parse(input)
}

/// Parse parameters (semicolon-separated)
fn parameters(input: &str) -> IResult<&str, Vec<(&str, &str)>> {
    many0(preceded(char(';'), parameter)).parse(input)
}

/// Parse a content line: NAME;PARAM=VALUE:VALUE
fn content_line(input: &str) -> IResult<&str, Property<'_>> {
    let (input, name) = property_name(input)?;
    let (input, params) = parameters(input)?;
    let (input, _) = char(':').parse(input)?;
    let (input, value) = not_line_ending.parse(input)?;
    let (input, _) = opt(line_ending).parse(input)?;

    Ok((
        input,
        Property {
            name,
            params,
            value,
        },
    ))
}

/// Parse BEGIN:
fn begin_component<'a>(input: &'a str, name: &'static str) -> IResult<&'a str, ()> {
    let (input, _) = tag("BEGIN:").parse(input)?;
    let (input, _) = tag(name).parse(input)?;
    let (input, _) = opt(line_ending).parse(input)?;
    Ok((input, ()))
}

/// Parse END:
fn end_component<'a>(input: &'a str, name: &'static str) -> IResult<&'a str, ()> {
    let (input, _) = tag("END:").parse(input)?;
    let (input, _) = tag(name).parse(input)?;
    let (input, _) = opt(line_ending).parse(input)?;
    Ok((input, ()))
}

/// Create a DateTime from a property
fn make_datetime<'a>(prop: &Property<'a>) -> OffsetDateTime {
    log::trace!("Creating datetime from property: {:?}", prop);
    let mut s: heapless::String<16> = heapless::String::new();
    s.push_str(&prop.value)
        .expect("Value too long for heapless string");
    parse_date(&s)
}

fn parse_valarm(input: &str) -> IResult<&str, VAlarm<'_>> {
    let (input, _) = begin_component(input, "VALARM")?;

    let mut alarm = VAlarm::default();
    let mut remaining = input;

    loop {
        if let Ok((new_input, _)) = end_component(remaining, "VALARM") {
            remaining = new_input;
            break;
        }

        let (new_input, prop) = content_line(remaining)?;
        match prop.name {
            "TRIGGER" => alarm.trigger = Some(prop.value),
            "ACTION" => alarm.action = Some(prop.value),
            "DESCRIPTION" => alarm.description = Some(prop.value),
            "REPEAT" => alarm.repeat = Some(prop.value),
            "DURATION" => alarm.duration = Some(prop.value),
            _ => {}
        }
        remaining = new_input;
    }

    Ok((remaining, alarm))
}

fn parse_vevent(input: &str) -> IResult<&str, VEvent<'_>> {
    let (input, _) = begin_component(input, "VEVENT")?;

    let mut event = VEvent::default();
    let mut remaining = input;

    loop {
        // Parse VALARM
        if let Ok((new_input, alarm)) = parse_valarm(remaining) {
            event.alarms.push(alarm);
            remaining = new_input;
            continue;
        }

        // Parse END:VEVENT
        if let Ok((new_input, _)) = end_component(remaining, "VEVENT") {
            remaining = new_input;
            break;
        }

        let (new_input, prop) = content_line(remaining)?;
        match prop.name {
            "UID" => event.uid = Some(prop.value),
            "SUMMARY" => event.summary = Some(prop.value),
            "DESCRIPTION" => event.description = Some(prop.value),
            "LOCATION" => event.location = Some(prop.value),
            "DTSTAMP" => event.dtstamp = Some(prop.value),
            "STATUS" => event.status = Some(prop.value),
            "RRULE" => event.rrule = Some(prop.value),
            "CATEGORIES" => event.categories = Some(prop.value),
            "ORGANIZER" => event.organizer = Some(prop.value),
            "URL" => event.url = Some(prop.value),
            "PRIORITY" => event.priority = Some(prop.value),
            "SEQUENCE" => event.sequence = Some(prop.value),
            "TRANSP" => event.transp = Some(prop.value),
            "CREATED" => event.created = Some(prop.value),
            "LAST-MODIFIED" => event.last_modified = Some(prop.value),
            "DTSTART" => event.dtstart = Some(make_datetime(&prop)),
            "DTEND" => event.dtend = Some(make_datetime(&prop)),
            _ => {}
        }
        remaining = new_input;
    }

    Ok((remaining, event))
}

fn parse_vtimezone(input: &str) -> IResult<&str, VTimezone<'_>> {
    let (input, _) = begin_component(input, "VTIMEZONE")?;

    let mut tz = VTimezone::default();
    let mut remaining = input;
    let mut depth = 1;

    loop {
        if remaining.starts_with("BEGIN:") {
            depth += 1;
            let (new_input, _) = not_line_ending.parse(remaining)?;
            let (new_input, _) = opt(line_ending).parse(new_input)?;
            remaining = new_input;
            continue;
        }

        if remaining.starts_with("END:VTIMEZONE") {
            let (new_input, _) = end_component(remaining, "VTIMEZONE")?;
            remaining = new_input;
            break;
        }

        if remaining.starts_with("END:") {
            depth -= 1;
            let (new_input, _) = not_line_ending.parse(remaining)?;
            let (new_input, _) = opt(line_ending).parse(new_input)?;
            remaining = new_input;
            continue;
        }

        let (new_input, prop) = content_line(remaining)?;
        if prop.name == "TZID" && depth == 1 {
            tz.tzid = Some(prop.value);
        }
        remaining = new_input;
    }

    Ok((remaining, tz))
}

/// Parse a complete VCALENDAR from an unfolded string
pub fn parse_vcalendar(input: &str) -> IResult<&str, VCalendar<'_>> {
    let (input, _) = begin_component(input, "VCALENDAR")?;

    let mut calendar = VCalendar {
        version: None,
        prodid: None,
        events: Vec::new(),
        timezones: Vec::new(),
    };

    let mut remaining = input;
    loop {
        // Parse VEVENT
        if let Ok((new_input, event)) = parse_vevent(remaining) {
            calendar.events.push(event);
            remaining = new_input;
            continue;
        }

        // Parse VTIMEZONE
        if let Ok((new_input, tz)) = parse_vtimezone(remaining) {
            calendar.timezones.push(tz);
            remaining = new_input;
            continue;
        }

        // Parse END:VCALENDAR
        if let Ok((new_input, _)) = end_component(remaining, "VCALENDAR") {
            remaining = new_input;
            break;
        }

        // Parse properties
        let (new_input, prop) = content_line(remaining)?;
        match prop.name {
            "VERSION" => calendar.version = Some(prop.value),
            "PRODID" => calendar.prodid = Some(prop.value),
            _ => {}
        }
        remaining = new_input;
    }

    Ok((remaining, calendar))
}

// Some tests added by LLMs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unfold_lines_crlf() {
        let input =
            "DESCRIPTION:This is a long\r\n  description that spans\r\n \tmultiple lines.\r\n";
        let result = unfold_lines(input);
        assert_eq!(
            result,
            "DESCRIPTION:This is a long description that spans\tmultiple lines.\r\n"
        );
    }

    #[test]
    fn test_unfold_lines_lf_only() {
        let input = "DESCRIPTION:This is a long\n  description that spans\n \tmultiple lines.\n";
        let result = unfold_lines(input);
        assert_eq!(
            result,
            "DESCRIPTION:This is a long description that spans\tmultiple lines.\n"
        );
    }

    #[test]
    fn test_unfold_no_folding() {
        let input = "SUMMARY:Simple line\r\nDTSTART:20251222T170000\r\n";
        let result = unfold_lines(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_content_line_simple() {
        let input = "VERSION:2.0\r\n";
        let (_, prop) = content_line(input).unwrap();
        assert_eq!(prop.name, "VERSION");
        assert_eq!(prop.value, "2.0");
        assert!(prop.params.is_empty());
    }

    #[test]
    fn test_content_line_with_params() {
        let input = "DTSTART;TZID=Europe/Budapest:20251222T170000\r\n";
        let (_, prop) = content_line(input).unwrap();
        assert_eq!(prop.name, "DTSTART");
        assert_eq!(prop.value, "20251222T170000");
        assert_eq!(prop.params, vec![("TZID", "Europe/Budapest")]);
    }

    #[test]
    fn test_content_line_multiple_params() {
        let input = "DTSTART;TZID=Europe/Budapest;VALUE=DATE-TIME:20251222T170000\r\n";
        let (_, prop) = content_line(input).unwrap();
        assert_eq!(prop.name, "DTSTART");
        assert_eq!(prop.value, "20251222T170000");
        assert_eq!(
            prop.params,
            vec![("TZID", "Europe/Budapest"), ("VALUE", "DATE-TIME")]
        );
    }

    #[test]
    fn test_parse_valarm() {
        let input = "BEGIN:VALARM\r\n\
TRIGGER:-P1D\r\n\
ACTION:DISPLAY\r\n\
DESCRIPTION:Reminder\r\n\
END:VALARM\r\n";

        let (_, alarm) = parse_valarm(input).unwrap();
        assert_eq!(alarm.trigger, Some("-P1D"));
        assert_eq!(alarm.action, Some("DISPLAY"));
        assert_eq!(alarm.description, Some("Reminder"));
    }

    #[test]
    fn test_parse_vevent_simple() {
        let input = "BEGIN:VEVENT\r\n\
UID:test-uid-123\r\n\
SUMMARY:Test Event\r\n\
DTSTART:20251222T170000Z\r\n\
DTEND:20251222T180000Z\r\n\
END:VEVENT\r\n";

        let (_, event) = parse_vevent(input).unwrap();
        assert_eq!(event.uid, Some("test-uid-123"));
        assert_eq!(event.summary, Some("Test Event"));
        assert!(event.dtstart.is_some());
        assert_eq!(event.dtstart.as_ref().unwrap().value, "20251222T170000Z");
        assert!(event.dtstart.as_ref().unwrap().tzid.is_none());
    }

    #[test]
    fn test_parse_vevent_with_alarm() {
        let input = "BEGIN:VEVENT\r\n\
UID:test-uid-456\r\n\
SUMMARY:Event with Alarm\r\n\
BEGIN:VALARM\r\n\
TRIGGER:-PT15M\r\n\
ACTION:DISPLAY\r\n\
END:VALARM\r\n\
END:VEVENT\r\n";

        let (_, event) = parse_vevent(input).unwrap();
        assert_eq!(event.uid, Some("test-uid-456"));
        assert_eq!(event.alarms.len(), 1);
        assert_eq!(event.alarms[0].trigger, Some("-PT15M"));
    }

    #[test]
    fn test_parse_vevent_with_rrule() {
        let input = "BEGIN:VEVENT\r\n\
UID:recurring-event\r\n\
SUMMARY:Weekly Meeting\r\n\
RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR\r\n\
LOCATION:Conference Room A\r\n\
END:VEVENT\r\n";

        let (_, event) = parse_vevent(input).unwrap();
        assert_eq!(event.rrule, Some("FREQ=WEEKLY;BYDAY=MO,WE,FR"));
        assert_eq!(event.location, Some("Conference Room A"));
    }

    #[test]
    fn test_parse_vtimezone() {
        let input = "BEGIN:VTIMEZONE\r\n\
TZID:Europe/Budapest\r\n\
BEGIN:STANDARD\r\n\
TZNAME:CET\r\n\
TZOFFSETFROM:+0200\r\n\
TZOFFSETTO:+0100\r\n\
END:STANDARD\r\n\
END:VTIMEZONE\r\n";

        let (_, tz) = parse_vtimezone(input).unwrap();
        assert_eq!(tz.tzid, Some("Europe/Budapest"));
    }

    #[test]
    fn test_parse_vcalendar_full() {
        let vcal = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:DAVx5/4.5.7.1-ose ical4j/3.2.19\r\n\
BEGIN:VEVENT\r\n\
DTSTAMP:20251221T081106Z\r\n\
UID:481b79c2-cc82-47bd-bf60-6082bab80e99\r\n\
SUMMARY:Fürdő\r\n\
DTSTART;TZID=Europe/Budapest:20251222T170000\r\n\
DTEND;TZID=Europe/Budapest:20251222T230000\r\n\
STATUS:CONFIRMED\r\n\
BEGIN:VALARM\r\n\
TRIGGER:-P1D\r\n\
ACTION:DISPLAY\r\n\
DESCRIPTION:Fürdő\r\n\
END:VALARM\r\n\
END:VEVENT\r\n\
BEGIN:VTIMEZONE\r\n\
TZID:Europe/Budapest\r\n\
BEGIN:STANDARD\r\n\
TZNAME:CET\r\n\
TZOFFSETFROM:+0200\r\n\
TZOFFSETTO:+0100\r\n\
DTSTART:19961027T030000\r\n\
RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\n\
END:STANDARD\r\n\
BEGIN:DAYLIGHT\r\n\
TZNAME:CEST\r\n\
TZOFFSETFROM:+0100\r\n\
TZOFFSETTO:+0200\r\n\
DTSTART:19840325T020000\r\n\
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r\n\
END:DAYLIGHT\r\n\
END:VTIMEZONE\r\n\
END:VCALENDAR\r\n";

        let (remaining, cal) = parse_vcalendar(vcal).unwrap();

        assert!(remaining.is_empty());
        assert_eq!(cal.version, Some("2.0"));
        assert_eq!(cal.prodid, Some("DAVx5/4.5.7.1-ose ical4j/3.2.19"));
        assert_eq!(cal.events.len(), 1);
        assert_eq!(cal.timezones.len(), 1);

        let event = &cal.events[0];
        assert_eq!(event.summary, Some("Fürdő"));
        assert_eq!(event.uid, Some("481b79c2-cc82-47bd-bf60-6082bab80e99"));
        assert_eq!(event.status, Some("CONFIRMED"));
        assert_eq!(
            event.dtstart.as_ref().unwrap().tzid,
            Some("Europe/Budapest")
        );
        assert_eq!(event.dtstart.as_ref().unwrap().value, "20251222T170000");
        assert_eq!(event.alarms.len(), 1);
        assert_eq!(event.alarms[0].trigger, Some("-P1D"));
        assert_eq!(event.alarms[0].action, Some("DISPLAY"));

        let tz = &cal.timezones[0];
        assert_eq!(tz.tzid, Some("Europe/Budapest"));
    }

    #[test]
    fn test_parse_vcalendar_with_lf_line_endings() {
        // Test with LF-only line endings (common in some systems)
        let vcal = "BEGIN:VCALENDAR\n\
VERSION:2.0\n\
BEGIN:VEVENT\n\
UID:test-lf\n\
SUMMARY:LF Test\n\
END:VEVENT\n\
END:VCALENDAR\n";

        let (_, cal) = parse_vcalendar(vcal).unwrap();
        assert_eq!(cal.version, Some("2.0"));
        assert_eq!(cal.events.len(), 1);
        assert_eq!(cal.events[0].uid, Some("test-lf"));
    }

    #[test]
    fn test_parse_with_unfolding() {
        // Simulate a folded line that would be unfolded before parsing
        let folded = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
BEGIN:VEVENT\r\n\
UID:folded-test\r\n\
DESCRIPTION:This is a very long description that has been\r\n  folded across multiple lines for readability.\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

        let unfolded = unfold_lines(folded);
        let (_, cal) = parse_vcalendar(&unfolded).unwrap();

        assert_eq!(cal.events.len(), 1);
        assert_eq!(
            cal.events[0].description,
            Some(
                "This is a very long description that has been folded across multiple lines for readability."
            )
        );
    }

    #[test]
    fn test_original_sample() {
        // Test with the original sample from the user (with LF line endings)
        let vcal = r#"BEGIN:VCALENDAR
VERSION:2.0
PRODID:DAVx5/4.5.7.1-ose ical4j/3.2.19
BEGIN:VEVENT
DTSTAMP:20251221T081106Z
UID:481b79c2-cc82-47bd-bf60-6082bab80e99
SUMMARY:Fürdő
DTSTART;TZID=Europe/Budapest:20251222T170000
DTEND;TZID=Europe/Budapest:20251222T230000
STATUS:CONFIRMED
BEGIN:VALARM
TRIGGER:-P1D
ACTION:DISPLAY
DESCRIPTION:Fürdő
END:VALARM
END:VEVENT
BEGIN:VTIMEZONE
TZID:Europe/Budapest
BEGIN:STANDARD
TZNAME:CET
TZOFFSETFROM:+0200
TZOFFSETTO:+0100
DTSTART:19961027T030000
RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU
END:STANDARD
BEGIN:DAYLIGHT
TZNAME:CEST
TZOFFSETFROM:+0100
TZOFFSETTO:+0200
DTSTART:19840325T020000
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU
END:DAYLIGHT
END:VTIMEZONE
END:VCALENDAR
"#;
        let (_, cal) = parse_vcalendar(vcal).unwrap();

        assert_eq!(cal.version, Some("2.0"));
        assert_eq!(cal.events.len(), 1);
        assert_eq!(cal.timezones.len(), 1);
        assert_eq!(cal.events[0].summary, Some("Fürdő"));
        assert_eq!(cal.events[0].alarms.len(), 1);
    }

    #[test]
    fn test_multiple_events() {
        let vcal = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
BEGIN:VEVENT\r\n\
UID:event-1\r\n\
SUMMARY:First Event\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
UID:event-2\r\n\
SUMMARY:Second Event\r\n\
LOCATION:Room 101\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
UID:event-3\r\n\
SUMMARY:Third Event\r\n\
PRIORITY:1\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

        let (_, cal) = parse_vcalendar(vcal).unwrap();

        assert_eq!(cal.events.len(), 3);
        assert_eq!(cal.events[0].uid, Some("event-1"));
        assert_eq!(cal.events[1].uid, Some("event-2"));
        assert_eq!(cal.events[1].location, Some("Room 101"));
        assert_eq!(cal.events[2].uid, Some("event-3"));
        assert_eq!(cal.events[2].priority, Some("1"));
    }
}
