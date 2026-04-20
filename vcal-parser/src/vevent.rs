#[cfg(test)]
extern crate std;
use alloc::string::{String, ToString};
#[cfg(test)]
use std::println;

use jiff::Timestamp;
use nom::{
    IResult, Parser,
    branch::alt,
    bytes::streaming::{tag, take_till, take_while, take_while1},
    character::streaming::char,
    combinator::opt,
};

#[derive(PartialEq, Clone, Debug)]
pub enum VcalEvent {
    Begin(String),
    End(String),
    Summary(String),
    DtStart(String),
    DtEnd(String),
}

#[derive(Eq, PartialEq, PartialOrd, Default, Clone, Debug)]
pub struct VEventData {
    pub summary: Option<String>,
    pub dtstart: Option<Timestamp>,
    pub dtend: Option<Timestamp>,
}

impl VEventData {
    /// Returns the event duration, or None if either timestamp is missing.
    fn duration(&self) -> Option<i64> {
        match (self.dtstart, self.dtend) {
            (Some(start), Some(end)) => Some(end.as_second() - start.as_second()),
            _ => None,
        }
    }
}

impl Ord for VEventData {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Longer duration comes first → reverse the comparison (b vs a)
        other.duration().cmp(&self.duration())
    }
}

pub fn parse_date(dt: &str) -> Timestamp {
    use jiff::fmt::strtime;
    let s = dt.strip_suffix('Z').unwrap_or(dt);
    let civil_dt = strtime::parse("%Y%m%dT%H%M%S", s)
        .unwrap()
        .to_datetime()
        .unwrap();
    civil_dt
        .to_zoned(jiff::tz::TimeZone::UTC)
        .unwrap()
        .timestamp()
}

fn property_name(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| c.is_alphanumeric() || c == '-').parse(input)
}

pub fn parse_vcal_event(input: &str) -> IResult<&str, Option<VcalEvent>> {
    let (input, _) = take_while(|c: char| c == ' ' || c == '\t').parse(input)?;
    let (input, name) = property_name(input)?;
    let (input, _) = take_till(|c| c == ':' || c == '\r' || c == '\n').parse(input)?;
    let (input, has_colon) = opt(char(':')).parse(input)?;

    let (input, value) = if has_colon.is_some() {
        take_till(|c| c == '\r' || c == '\n').parse(input)?
    } else {
        (input, "")
    };

    let value = value.strip_suffix("&#13;").unwrap_or(value);

    let (input, _) = alt((tag("\r\n"), tag("\n"))).parse(input)?;

    let event = match name {
        "BEGIN" => Some(VcalEvent::Begin(value.to_string())),
        "END" => Some(VcalEvent::End(value.to_string())),
        "SUMMARY" => Some(VcalEvent::Summary(value.to_string())),
        "DTSTART" => Some(VcalEvent::DtStart(value.to_string())),
        "DTEND" => Some(VcalEvent::DtEnd(value.to_string())),
        _ => None,
    };

    Ok((input, event))
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_parse_vcal_event() {
        let input = r#"VERSION:2.0&#13;
        PRODID:-//Sabre//Sabre VObject 4.5.6//EN&#13;
        CALSCALE:GREGORIAN&#13;
        BEGIN:VEVENT&#13;
        DTSTAMP:20260312T063325Z&#13;
        UID:1d1a6701-97b5-40c0-933a-2f158030dbe4&#13;
        SUMMARY:Este&#13;
        DTSTART:20260416T210000Z&#13;
        DTEND:20260416T215900Z&#13;
        STATUS:CONFIRMED&#13;
        SEQUENCE:4&#13;
        CREATED:20260219T105853Z&#13;
        RECURRENCE-ID:20260416T210000Z&#13;
        END:VEVENT&#13;
        END:VCALENDAR&#13;"#;

        let (input, event) = parse_vcal_event(input).unwrap();
        assert_eq!(event, Some(VcalEvent::Summary("Este".to_string())));

        let (input, event) = parse_vcal_event(input).unwrap();
        assert_eq!(
            event,
            Some(VcalEvent::DtStart("20251222T170000".to_string()))
        );

        let (input, event) = parse_vcal_event(input).unwrap();
        assert_eq!(event, Some(VcalEvent::DtEnd("20251222T180000".to_string())));

        let (input, event) = parse_vcal_event(input).unwrap();
        assert_eq!(event, Some(VcalEvent::End("VEVENT".to_string())));

        assert_eq!(input, "");
    }

    #[test]
    fn test_incomplete() {
        let input = "<d:multistatus xmlns:d=\"DAV:\" xmlns:s=\"http://sabredav.org/ns\" xmlns:cal=\"urn:ietf:params:xml:ns:caldav\" xmlns:cs=\"http://calendarserver.org/ns/\" xmlns:oc=\"http://owncloud.org/ns\" xmlns:nc=\"http://nextcloud.org/ns\"><d:response><d:href>/remote.php/dav/calendars/mmartin/szakdoga-teszt/65E6F4B7-4CEF-4CD0-BEDC-77734C0D5A61.ics</d:href><d:propstat><d:prop><d:getetag>&quot;19cd4124f96694c0be3b8c5ed8798a25&quot;</d:getetag><cal:calendar-data>BEGIN:VCALENDAR&#13;\nVERSION:2.0&#13;\nPRODID:-//Sabre//Sabre VObject 4.5.6//EN&#13;\nCALSCALE:GREGORIAN&#13;\nBEGIN:VEVENT&#13;\nDTSTAMP:20260312T063325Z&#13;\nUID:7e784d46-957c-4edd-9a4f-7179ebd5809c&#13;\nSUMMARY:Szieszta&#13;\nDTSTART:20260415T103000Z&#13;\nDTEND:20260415T113000Z&#13;\nSTATUS:CONFIRMED&#13;\nSEQUENCE:4&#13;\nCREATED:20260219T111359Z&#13;\nRECURRENCE-ID:20260415T103000Z&#13;\nEND:VEVENT&#13;\nEND:VCALENDAR&#13;\n</cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/remote.php/dav/calendars/mmartin/szakdoga-teszt/8329A57E-DAFE-45CA-8DCC-E52998614CD1.ics</d:href><d:propstat><d:prop><d:getetag>&quot;6c1b7c1cc4f3dc040bc4e4897d0b9cbc&quot;</d:getetag><cal:calendar-data>BEGIN:VCALENDAR&#13;\nVERSION:2.0&#13;\nPRODID:-//Sabre//Sabre VObject 4.5.6//EN&#13;\nCALSCALE:GREGORIAN&#13;\nBEGIN:VEVENT&#13;\nDTSTAMP:20260312T063325Z&#13;\nUID:0401437d-1f41-4bbe-8d73-c4c60b191f20&#13;\nSUMMARY:Éjfél&#13;\nDTSTART:20260415T220000Z&#13;\nDTEND:20260415T230000Z&#13;\nSTATUS:CONFIRMED&#13;\nSEQUENCE:7&#13;\nCREATED:20260218T125559Z&#13;\nRECURRENCE-ID:20260415T220000Z&#13;\nEND:VEVENT&#13;\nEND:VCALENDAR&#13;\n</cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/remote.php/dav/calendars/mmartin/szakdoga-teszt/AC4DC0A8-CE09-4AA0-AE0A-DDD88FAF24F5.ics</d:href><d:propstat><d:prop><d:getetag>&quot;8eee8c86bb5fbda441052450b01e9fe2&quot;</d:getetag><cal:calendar-data>BEGIN:VCALENDAR&#13;\nVERSION:2.0&#13;\nPRODID:-//Sabre//Sabre VObject 4.5.6//EN&#13;\nCALSCALE:GREGORIAN&#13;\nBEGIN:VEVENT&#13;\nDTSTAMP:20260312T063325Z&#13;\nUID:1d1a6701-97b5-40c0-933a-2f158030dbe4&#13;\nSUMMARY:Este&#13;\nDTSTART:20260415T210000Z&#13;\nDTEND:20260415T215900Z&#13;\nSTATUS:CONFIRMED&#13;\nSEQUENCE:4&#13;\nCREATED:20260219T105853Z&#13;\nRECURRENCE-ID:20260415T210000Z&#13;\nEND:VEVENT&#13;\nEND:VCALENDAR&#13;\n</cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>\n";
        let res = parse_vcal_event(input);
        assert!(res.is_err());
    }

    #[test]
    fn test_parse_with_html_entity() {
        use crate::calendars::{
            CalNamespace, Namespace, XmlEvent, parse_xml_event, parse_xml_version,
        };
        let input = r#"<?xml version="1.0"?>
        <d:multistatus xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/" xmlns:oc="http://owncloud.org/ns" xmlns:nc="http://nextcloud.org/ns"><d:response><d:href>/remote.php/dav/calendars/mmartin/szakdoga-teszt/65E6F4B7-4CEF-4CD0-BEDC-77734C0D5A61.ics</d:href><d:propstat><d:prop><d:getetag>&quot;19cd4124f96694c0be3b8c5ed8798a25&quot;</d:getetag><cal:calendar-data>BEGIN:VCALENDAR&#13;
        VERSION:2.0&#13;
        PRODID:-//Sabre//Sabre VObject 4.5.6//EN&#13;
        CALSCALE:GREGORIAN&#13;
        BEGIN:VEVENT&#13;
        DTSTAMP:20260312T063325Z&#13;
        UID:7e784d46-957c-4edd-9a4f-7179ebd5809c&#13;
        SUMMARY:Szieszta&#13;
        DTSTART:20260415T103000Z&#13;
        DTEND:20260415T113000Z&#13;
        STATUS:CONFIRMED&#13;
        SEQUENCE:4&#13;
        CREATED:20260219T111359Z&#13;
        RECURRENCE-ID:20260415T103000Z&#13;
        END:VEVENT&#13;
        END:VCALENDAR&#13;
        </cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/remote.php/dav/calendars/mmartin/szakdoga-teszt/8329A57E-DAFE-45CA-8DCC-E52998614CD1.ics</d:href><d:propstat><d:prop><d:getetag>&quot;6c1b7c1cc4f3dc040bc4e4897d0b9cbc&quot;</d:getetag><cal:calendar-data>BEGIN:VCALENDAR&#13;
        VERSION:2.0&#13;
        PRODID:-//Sabre//Sabre VObject 4.5.6//EN&#13;
        CALSCALE:GREGORIAN&#13;
        BEGIN:VEVENT&#13;
        DTSTAMP:20260312T063325Z&#13;
        UID:0401437d-1f41-4bbe-8d73-c4c60b191f20&#13;
        SUMMARY:Éjfél&#13;
        DTSTART:20260415T220000Z&#13;
        DTEND:20260415T230000Z&#13;
        STATUS:CONFIRMED&#13;
        SEQUENCE:7&#13;
        CREATED:20260218T125559Z&#13;
        RECURRENCE-ID:20260415T220000Z&#13;
        END:VEVENT&#13;
        END:VCALENDAR&#13;
        </cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/remote.php/dav/calendars/mmartin/szakdoga-teszt/AC4DC0A8-CE09-4AA0-AE0A-DDD88FAF24F5.ics</d:href><d:propstat><d:prop><d:getetag>&quot;8eee8c86bb5fbda441052450b01e9fe2&quot;</d:getetag><cal:calendar-data>BEGIN:VCALENDAR&#13;
        VERSION:2.0&#13;
        PRODID:-//Sabre//Sabre VObject 4.5.6//EN&#13;
        CALSCALE:GREGORIAN&#13;
        BEGIN:VEVENT&#13;
        DTSTAMP:20260312T063325Z&#13;
        UID:1d1a6701-97b5-40c0-933a-2f158030dbe4&#13;
        SUMMARY:Este&#13;
        DTSTART:20260415T210000Z&#13;
        DTEND:20260415T215900Z&#13;
        STATUS:CONFIRMED&#13;
        SEQUENCE:4&#13;
        CREATED:20260219T105853Z&#13;
        RECURRENCE-ID:20260415T210000Z&#13;
        END:VEVENT&#13;
        END:VCALENDAR&#13;
        </cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>
"#;

        let (mut remaining, _) = parse_xml_version(input).unwrap();
        let mut in_calendar_data = false;
        let mut parsed_events = alloc::vec::Vec::new();

        while !remaining.is_empty() {
            match parse_xml_event(remaining) {
                Ok((rem, event)) => {
                    remaining = rem;
                    match event {
                        XmlEvent::Open(Namespace::Cal(CalNamespace::CalendarData)) => {
                            in_calendar_data = true;
                        }
                        XmlEvent::Close(Namespace::Cal(CalNamespace::CalendarData)) => {
                            in_calendar_data = false;
                        }
                        XmlEvent::Text(text) if in_calendar_data => {
                            let mut vcal_rem: &str = &text;
                            while !vcal_rem.is_empty() {
                                match parse_vcal_event(vcal_rem) {
                                    Ok((next_rem, Some(ev))) => {
                                        vcal_rem = next_rem;
                                        parsed_events.push(ev);
                                    }
                                    Ok((next_rem, None)) => {
                                        vcal_rem = next_rem;
                                    }
                                    Err(_) => break,
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Err(_) => break,
            }
        }

        assert!(parsed_events.contains(&VcalEvent::Begin("VEVENT".to_string())));
        assert!(parsed_events.contains(&VcalEvent::Summary("Szieszta".to_string())));
        assert!(parsed_events.contains(&VcalEvent::DtStart("20260415T103000Z".to_string())));
        assert!(parsed_events.contains(&VcalEvent::DtEnd("20260415T113000Z".to_string())));
        assert!(parsed_events.contains(&VcalEvent::End("VEVENT".to_string())));
    }
}
