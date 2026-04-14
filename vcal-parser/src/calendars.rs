use nom::{
    IResult, Parser,
    branch::alt,
    bytes::streaming::{tag, take_till, take_until, take_while},
    character::streaming::{char, line_ending},
    combinator::opt,
    sequence::delimited,
};

use alloc::string::{String, ToString};
#[cfg(test)]
use std::println;

#[derive(Debug, PartialEq, Clone)]
pub enum XmlEvent {
    Open(Namespace),
    Close(Namespace),
    SelfClosing(Namespace),
    Text(String),
}

#[derive(Debug, PartialEq, Clone)]
pub struct CalendarData {
    pub href: Option<String>,
    pub display_name: Option<String>,
}

impl CalendarData {
    pub const fn new() -> Self {
        CalendarData {
            href: None,
            display_name: None,
        }
    }

    pub fn reset(&mut self) {
        self.href = None;
        self.display_name = None;
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum Namespace {
    D(DNamespace),
    Cal(CalNamespace),
    Other(String, String),
}

#[derive(Debug, PartialEq, Clone)]
pub enum DNamespace {
    Multistatus,
    Response,
    Href,
    PropStat,
    Prop,
    DisplayName,
    Status,
    ResourceType,
    Collection,
    Other(String),
}

#[derive(Debug, PartialEq, Clone)]
pub enum CalNamespace {
    SupportedCalendarComponentSet,
    Comp,
    Calendar,
    Other(String),
}

fn tag_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_' || c == '.'
}

fn classify_namespace(ns: &str, name: &str) -> Namespace {
    match ns {
        "d" => Namespace::D(match name {
            "multistatus" => DNamespace::Multistatus,
            "response" => DNamespace::Response,
            "href" => DNamespace::Href,
            "propstat" => DNamespace::PropStat,
            "prop" => DNamespace::Prop,
            "displayname" => DNamespace::DisplayName,
            "status" => DNamespace::Status,
            "resourcetype" => DNamespace::ResourceType,
            "collection" => DNamespace::Collection,
            other => DNamespace::Other(other.to_string()),
        }),
        "cal" => Namespace::Cal(match name {
            "supported-calendar-component-set" => CalNamespace::SupportedCalendarComponentSet,
            "comp" => CalNamespace::Comp,
            "calendar" => CalNamespace::Calendar,
            other => CalNamespace::Other(other.to_string()),
        }),
        _ => Namespace::Other(ns.to_string(), name.to_string()),
    }
}

pub fn parse_xml_version(input: &str) -> IResult<&str, ()> {
    let (input, _) = delimited(tag("<?"), take_until("?>"), tag("?>")).parse(input)?;
    let (input, _) = opt(line_ending).parse(input)?;
    Ok((input, ()))
}

/// Parses `ns:name` or a bare `name`, returning `("ns", "name")` or `("", "name")`.
fn parse_qualified_name(input: &str) -> IResult<&str, (&str, &str)> {
    let (input, first) =
        take_till(|c: char| c == ':' || c == '>' || c.is_whitespace() || c == '/').parse(input)?;
    match char::<_, nom::error::Error<&str>>(':').parse(input) {
        Ok((rest, _)) => {
            let (rest, second) = take_while(tag_name_char).parse(rest)?;
            Ok((rest, (first, second)))
        }
        Err(_) => Ok((input, ("", first))),
    }
}

fn parse_open_tag(input: &str) -> IResult<&str, XmlEvent> {
    let (input, _) = char('<').parse(input)?;
    let (input, (ns, name)) = parse_qualified_name(input)?;
    let (input, attrs) = take_till(|c| c == '>').parse(input)?;
    let self_closing = attrs.ends_with('/');
    let (input, _) = char('>').parse(input)?;
    let ns_enum = classify_namespace(ns, name);
    Ok((
        input,
        if self_closing {
            XmlEvent::SelfClosing(ns_enum)
        } else {
            XmlEvent::Open(ns_enum)
        },
    ))
}

fn parse_close_tag(input: &str) -> IResult<&str, XmlEvent> {
    let (input, _) = tag("</").parse(input)?;
    let (input, (ns, name)) = parse_qualified_name(input)?;
    let (input, _) = char('>').parse(input)?;
    Ok((input, XmlEvent::Close(classify_namespace(ns, name))))
}

fn parse_text(input: &str) -> IResult<&str, XmlEvent> {
    let (input, text) = take_till(|c| c == '<').parse(input)?;
    if text.is_empty() {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::TakeTill1,
        )));
    }
    Ok((input, XmlEvent::Text(text.to_string())))
}

pub fn parse_xml_event(input: &str) -> IResult<&str, XmlEvent> {
    alt((parse_close_tag, parse_open_tag, parse_text)).parse(input)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn minimal_test() {
        let input = "<?xml version=\"1.0\"?>\n<d:multistatus xmlns:d=\"DAV:\" xmlns:s=\"http://sabredav.org/ns\"><d:response><d:href>/remote.php/dav/calendars/tesztelek/</d:href></d:response></d:multistatus>";

        let (input, _) = parse_xml_version(input).unwrap();
        assert_eq!(input, input.replace("<?xml version=\"1.0\"?>\n", ""));
        let (input, event) = parse_xml_event(input).unwrap();
        assert_eq!(event, XmlEvent::Open(Namespace::D(DNamespace::Multistatus)));
        let (input, event) = parse_xml_event(input).unwrap();
        assert_eq!(event, XmlEvent::Open(Namespace::D(DNamespace::Response)));
        let (input, event) = parse_xml_event(input).unwrap();
        assert_eq!(event, XmlEvent::Open(Namespace::D(DNamespace::Href)));
        let (input, event) = parse_xml_event(input).unwrap();
        assert_eq!(
            event,
            XmlEvent::Text("/remote.php/dav/calendars/tesztelek/".to_string())
        );
        let (input, event) = parse_xml_event(input).unwrap();
        assert_eq!(event, XmlEvent::Close(Namespace::D(DNamespace::Href)));
        let (input, event) = parse_xml_event(input).unwrap();
        assert_eq!(event, XmlEvent::Close(Namespace::D(DNamespace::Response)));
        let (input, event) = parse_xml_event(input).unwrap();
        assert_eq!(
            event,
            XmlEvent::Close(Namespace::D(DNamespace::Multistatus))
        );
    }

    #[test]
    fn href_test() {
        let input = "/remote.php/dav/calendars/tesztelek/</d:href>";
        let (input, event) = parse_xml_event(input).unwrap();
        assert_eq!(
            event,
            XmlEvent::Text("/remote.php/dav/calendars/tesztelek/".to_string())
        );
    }

    #[test]
    fn incomplete_test() {
        let input = "<?xml version=\"1.0\"?>\n<d:multistatus xmlns:d=\"DAV:\" xmlns:s=\"http://sabredav.org/ns\" xmlns:cal=\"urn:ietf:params:xml:ns:caldav\" xmlns:cs=\"http://calendarserver.org/ns/\" xmlns:oc=\"http://owncloud.org/ns\" xmlns:nc=\"http://nextcloud.org/ns\"><d:response><d:href>/remote.php/dav/calendars/tesztelek/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat><d:propstat><d:prop><d:displayname/><cal:supported-calendar-component-set/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat></d:response><d:response><d:href>/remote.php/dav/calendars/tesztelek/75aa29b5-1567-4d51-bcea-9d7e59c87101/</d:href><d:propstat><d:prop><d:displayname>University</d:displayname><d:resourcetype><d:collection/><cal:calendar/></d:resourcetype><cal:supported-calendar-component-set><cal:comp name=\"VEVENT\"/><cal:comp name=\"VTODO\"/><cal:comp name=\"VJOURNAL\"/></cal:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/remote.php/dav/calendars/tesztelek/verylonglonglonglonglonglonglonglonglonglonglonglonglonglong/</d:href><d:propstat><d:prop><d:displayname>Verylonglonglonglonglonglonglonglonglonglonglonglonglonglonglonglonglonglonglonglong</d:displayname><d:resourcetype><d:collection/><cal:calendar/></d:resourcetype><cal:supported-calendar-component-set><cal:comp name=\"VEVENT\"/></cal:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/remote.php/dav/calendars/tesztelek/40193e3b-e4f2-43b0-8b23-540145c9108f/</d:href><d:propstat><d:prop><d:displayname>Company</d:displayname><d:resourcetype><d:collection/><cal:calendar/></d:resourcetype><cal:supported-calendar-component-set><cal:comp name=\"VEVENT\"/><cal:comp name=\"VTODO\"/><cal:comp name=\"VJOURNAL\"/></cal:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/remote.php/dav/calendars/tesztelek/time-events/</d:href><d:propstat><d:prop><d:displayname>Limited events</d:displayname><d:resourcetype><d:collection/><cal:calendar/></d:resourcetype><cal:supported-calendar-component-set><cal:comp name=\"VEVENT\"/></cal:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/remote.php/dav/calendars/tesztelek/6f3e6d8a-f8a9-454b-a793-8515965dcf1d/</d:href><d:propstat><d:prop><d:displayname>Calendar name</d:displayname><d:resourcetype><d:collection/><cal:calendar/></d:resourcetype><cal:supported-calendar-component-set><cal:comp name=\"VEVENT\"/><cal:comp name=\"VTODO\"/><cal:comp name=\"VJOURNAL\"/></cal:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/re";

        let (mut remaining, _) = parse_xml_version(input).unwrap();
        loop {
            match parse_xml_event(remaining) {
                Ok((i, _)) => remaining = i,
                Err(nom::Err::Incomplete(_)) => break,
                Err(nom::Err::Error(_)) => break,
                Err(e) => panic!("Unexpected error {:?}", e),
            }
        }
        assert_eq!(remaining, "/re");
    }
}
