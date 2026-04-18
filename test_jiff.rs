fn main() {
    let z = jiff::Zoned::now();
    let start_zoned = z.datetime().date().to_zoned(z.time_zone().clone()).unwrap();
    let end_zoned = start_zoned.checked_add(jiff::Span::new().days(1)).unwrap();
    
    let start_utc = start_zoned.with_time_zone(jiff::tz::TimeZone::UTC).datetime();
    let end_utc = end_zoned.with_time_zone(jiff::tz::TimeZone::UTC).datetime();
    
    println!("{:04}{:02}{:02}T{:02}{:02}{:02}Z", 
        start_utc.year(), start_utc.month(), start_utc.day(),
        start_utc.hour(), start_utc.minute(), start_utc.second()
    );
    println!("{:04}{:02}{:02}T{:02}{:02}{:02}Z", 
        end_utc.year(), end_utc.month(), end_utc.day(),
        end_utc.hour(), end_utc.minute(), end_utc.second()
    );
}
