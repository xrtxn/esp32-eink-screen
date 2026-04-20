use vcal_parser::{
    calendars::CalendarData,
    vevent::{VEventData, parse_date},
};

/// The internal nom parser for calendar bodies
pub(crate) async fn parse_body<B>(
    body_reader: &mut reqwless::response::BodyReader<B>,
) -> Result<alloc::vec::Vec<CalendarData>, reqwless::Error>
where
    B: embedded_io_async::Read + embedded_io_async::BufRead,
{
    if let reqwless::response::BodyReader::Empty = body_reader {
        return Ok(alloc::vec::Vec::new());
    }
    let mut spill_buffer: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let handled_start = false;
    let mut cal_data = CalendarData::default();
    let mut calendars: alloc::vec::Vec<CalendarData> = alloc::vec::Vec::new();
    let mut next_href = false;
    let mut next_name = false;
    loop {
        let buf = embedded_io_async::BufRead::fill_buf(body_reader)
            .await
            .unwrap();
        let len = buf.len();
        if len == 0 {
            break;
        }

        let parse_slice = if spill_buffer.is_empty() {
            buf
        } else {
            spill_buffer.extend_from_slice(buf);
            &spill_buffer
        };

        let mut parsed_bytes = 0;

        // TODO: handle if split inside a utf-8 character
        if let Ok(mut current_str) = core::str::from_utf8(parse_slice) {
            crate::defmt::debug!("Parsing chunked calendar data: {}", current_str);
            if !handled_start && current_str.starts_with("<?") {
                match vcal_parser::calendars::parse_xml_version(current_str) {
                    Ok((rest, _)) => {
                        parsed_bytes += current_str.len() - rest.len();
                        current_str = rest;
                    }
                    Err(nom::Err::Incomplete(_)) => {}
                    Err(e) => {
                        crate::defmt::error!(
                            "Failed parsing XML version: {}",
                            crate::defmt::Debug2Format(&e)
                        )
                    }
                }
            }

            loop {
                if current_str.is_empty() {
                    break;
                }

                match vcal_parser::calendars::parse_xml_event(current_str) {
                    Ok((remaining, event)) => {
                        use vcal_parser::calendars::XmlEvent;
                        use vcal_parser::calendars::{DNamespace, Namespace};

                        match event {
                            XmlEvent::Open(Namespace::D(DNamespace::DisplayName)) => {
                                next_name = true
                            }
                            XmlEvent::Open(Namespace::D(DNamespace::Href)) => next_href = true,
                            XmlEvent::Close(Namespace::D(DNamespace::Response)) => {
                                if cal_data.href.is_none() {
                                    crate::defmt::warn!("Calendar response without href, skipping");
                                } else if cal_data.display_name.is_none() {
                                    crate::defmt::warn!(
                                        "Calendar response without display name, skipping"
                                    );
                                } else {
                                    calendars.push(core::mem::take(&mut cal_data));
                                }
                            }
                            XmlEvent::Close(Namespace::D(DNamespace::Multistatus)) => {
                                if remaining.trim().is_empty() {
                                    crate::defmt::info!("Finished parsing all calendar data");
                                } else {
                                    crate::defmt::warn!("Leftover data: {}", remaining);
                                }
                                break;
                            }
                            XmlEvent::Text(mut text) => {
                                if next_name {
                                    cal_data.display_name = Some(text);
                                    next_name = false;
                                } else if next_href {
                                    if let Some(idx) = text.trim_end_matches('/').rfind('/') {
                                        text.drain(..idx);
                                        if !text.ends_with('/') {
                                            text.push('/');
                                        }
                                    }
                                    cal_data.href = Some(text);
                                    next_href = false;
                                }
                            }
                            _ => (),
                        }
                        parsed_bytes += current_str.len() - remaining.len();
                        current_str = remaining;
                    }
                    Err(nom::Err::Incomplete(_)) => {
                        crate::defmt::warn!(
                            "Incomplete chunked calendar data, waiting for more data to arrive"
                        );
                        break;
                    }
                    Err(nom::Err::Error(err)) => {
                        crate::defmt::error!(
                            "Failed to parse chunked calendar data: {}",
                            crate::defmt::Debug2Format(&err)
                        );
                        break;
                    }
                    Err(nom::Err::Failure(fail)) => {
                        crate::defmt::error!(
                            "Failed to parse chunked calendar data: {}",
                            crate::defmt::Debug2Format(&fail)
                        );
                        break;
                    }
                }
            }
        }

        if spill_buffer.is_empty() {
            // Copy the remaining unparsed bytes into the spill buffer
            if parsed_bytes < len {
                spill_buffer.extend_from_slice(&buf[parsed_bytes..]);
            }
        } else {
            spill_buffer.drain(..parsed_bytes);
        }

        // Consume all remaining bytes, it only fetches new data if we consumed everything that was previously fetched
        embedded_io_async::BufRead::consume(body_reader, len);
    }
    Ok(calendars)
}

pub(crate) async fn parse_body_cal<B>(
    body_reader: &mut reqwless::response::BodyReader<B>,
) -> Result<alloc::vec::Vec<VEventData>, reqwless::Error>
where
    B: embedded_io_async::Read + embedded_io_async::BufRead,
{
    if let reqwless::response::BodyReader::Empty = body_reader {
        return Ok(alloc::vec::Vec::new());
    }
    let mut spill_buffer: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let handled_start = false;
    let mut cal_data = VEventData::default();
    let mut events: alloc::vec::Vec<VEventData> = alloc::vec::Vec::new();
    let mut in_calendar_data = false;
    loop {
        let buf = embedded_io_async::BufRead::fill_buf(body_reader)
            .await
            .unwrap();
        let len = buf.len();
        if len == 0 {
            break;
        }

        let parse_slice = if spill_buffer.is_empty() {
            buf
        } else {
            spill_buffer.extend_from_slice(buf);
            &spill_buffer
        };

        let mut parsed_bytes = 0;

        // TODO: handle if split inside a utf-8 character
        if let Ok(mut current_str) = core::str::from_utf8(parse_slice) {
            if !handled_start && current_str.starts_with("<?") {
                match vcal_parser::calendars::parse_xml_version(current_str) {
                    Ok((rest, _)) => {
                        parsed_bytes += current_str.len() - rest.len();
                        current_str = rest;
                    }
                    Err(nom::Err::Incomplete(_)) => {}
                    Err(e) => {
                        crate::defmt::error!(
                            "Failed parsing XML version: {}",
                            crate::defmt::Debug2Format(&e)
                        )
                    }
                }
            }

            loop {
                if current_str.is_empty() {
                    break;
                }

                match vcal_parser::calendars::parse_xml_event(current_str) {
                    Ok((remaining, event)) => {
                        use vcal_parser::calendars::XmlEvent;
                        use vcal_parser::calendars::{CalNamespace, Namespace};

                        match event {
                            XmlEvent::Open(Namespace::Cal(CalNamespace::CalendarData)) => {
                                in_calendar_data = true;
                            }
                            XmlEvent::Close(Namespace::Cal(CalNamespace::CalendarData)) => {
                                in_calendar_data = false;
                            }
                            XmlEvent::Text(text) => {
                                let mut txt = text.as_str();
                                if in_calendar_data {
                                    if text.is_empty() {
                                        break;
                                    }
                                    loop {
                                        match vcal_parser::vevent::parse_vcal_event(txt) {
                                            Ok((rem, vevent)) => {
                                                if let Some(vevent) = vevent {
                                                    match vevent {
                                                        vcal_parser::vevent::VcalEvent::Begin(
                                                            _,
                                                        ) => {
                                                            cal_data = VEventData::default();
                                                        }
                                                        vcal_parser::vevent::VcalEvent::End(_) => {
                                                            events.push(core::mem::take(
                                                                &mut cal_data,
                                                            ));
                                                            break;
                                                        }
                                                        vcal_parser::vevent::VcalEvent::Summary(
                                                            summary,
                                                        ) => {
                                                            cal_data.summary = Some(summary);
                                                        }
                                                        vcal_parser::vevent::VcalEvent::DtStart(
                                                            ref dtstart,
                                                        ) => {
                                                            cal_data.dtstart =
                                                                Some(parse_date(dtstart.as_str()));
                                                        }
                                                        vcal_parser::vevent::VcalEvent::DtEnd(
                                                            ref dtend,
                                                        ) => {
                                                            cal_data.dtend =
                                                                Some(parse_date(dtend.as_str()));
                                                        }
                                                    }
                                                }
                                                txt = rem;
                                            }
                                            Err(e) => {
                                                crate::defmt::error!(
                                                    "Failed to parse VEVENT data: {}",
                                                    crate::defmt::Debug2Format(&e)
                                                )
                                            }
                                        }
                                    }
                                }
                            }
                            _ => (),
                        }
                        current_str = remaining;
                    }
                    Err(nom::Err::Incomplete(_)) => {
                        crate::defmt::warn!(
                            "Incomplete chunked vcal data, waiting for more data to arrive"
                        );
                        break;
                    }
                    Err(nom::Err::Error(err)) => {
                        crate::defmt::error!(
                            "Failed to parse chunked vcal data: {}",
                            crate::defmt::Debug2Format(&err)
                        );
                        break;
                    }
                    Err(nom::Err::Failure(fail)) => {
                        crate::defmt::error!(
                            "Failed to parse chunked vcal data: {}",
                            crate::defmt::Debug2Format(&fail)
                        );
                        break;
                    }
                }
            }
        }

        if spill_buffer.is_empty() {
            // Copy the remaining unparsed bytes into the spill buffer
            if parsed_bytes < len {
                spill_buffer.extend_from_slice(&buf[parsed_bytes..]);
            }
        } else {
            spill_buffer.drain(..parsed_bytes);
        }

        // Consume all remaining bytes, it only fetches new data if we consumed everything that was previously fetched
        embedded_io_async::BufRead::consume(body_reader, len);
    }
    crate::defmt::info!(
        "Finished parsing calendar events, total events parsed: {:?}",
        events.len()
    );
    Ok(events)
}
