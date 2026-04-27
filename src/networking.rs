use alloc::string::String;
use alloc::string::ToString;
use core::fmt::Write;
use core::net::{SocketAddr, SocketAddrV4};

use embassy_net::Stack;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::TcpClient;
use embassy_net::udp::PacketMetadata;
use esp_backtrace as _;
use jiff::tz;
use jiff::tz::TimeZone;
use reqwless::client::{HttpClient, TlsConfig};
use reqwless::request::RequestBuilder;
use smoltcp::wire::DnsQueryType;
use static_cell::StaticCell;
pub use vcal_parser::calendars::CalendarData;

use crate::storage::CaldavCreds;

const UTC_OFFSET_HOURS: i8 = 2;
pub const USER_TIMEZONE: TimeZone = TimeZone::fixed(tz::offset(UTC_OFFSET_HOURS));
pub const CERT_STORE: &core::ffi::CStr = {
    // add missing null byte compile time
    let s = concat!(include_str!("../certs/cert-mix.pem"), "\0");

    match core::ffi::CStr::from_bytes_with_nul(s.as_bytes()) {
        Ok(c) => c,
        Err(_) => panic!("cert contains interior null bytes or is missing terminator"),
    }
};
/// This is a boolean value to whether the initial NTP sync occurred
#[esp_hal::ram(unstable(rtc_fast, persistent))]
pub static INITIAL_NTP_SYNC: portable_atomic::AtomicU8 = portable_atomic::AtomicU8::new(0);

pub(crate) static CLIENT_STATE: StaticCell<
    embassy_net::tcp::client::TcpClientState<1, 4096, 4096>,
> = StaticCell::new();
static RX_META: StaticCell<[PacketMetadata; 16]> = StaticCell::new();
static RX_BUFFER: StaticCell<[u8; 4096]> = StaticCell::new();
static TX_META: StaticCell<[PacketMetadata; 16]> = StaticCell::new();
static TX_BUFFER: StaticCell<[u8; 4096]> = StaticCell::new();

pub(crate) static REQ_BUFFER: StaticCell<[u8; 8192]> = StaticCell::new();

#[derive(thiserror::Error, picoserve::response::ErrorWithStatusCode, Debug)]
#[status_code(INTERNAL_SERVER_ERROR)]
pub enum NetworkError {
    #[error("Failed to send request: {0}")]
    RequestError(#[from] reqwless::Error),
    #[error("Failed to read to String")]
    ReadError(#[from] core::str::Utf8Error),
    #[status_code(BAD_REQUEST)]
    #[error("Failed to parse as xml")]
    ParsingError,
    #[status_code(BAD_REQUEST)]
    #[error("Failed to parse URL")]
    WrongUrl,
    #[status_code(UNAUTHORIZED)]
    #[error("Invalid credentials")]
    InvalidCredentials,
}

#[derive(Copy, Clone, Default)]
struct NtpTimestamp {
    duration: jiff::SignedDuration,
}

impl sntpc::NtpTimestampGenerator for NtpTimestamp {
    fn init(&mut self) {
        let ticks = embassy_time::Instant::now().as_ticks();
        let micros = ticks * 1_000_000 / embassy_time::TICK_HZ;
        self.duration = jiff::SignedDuration::from_micros(micros as i64);
    }

    fn timestamp_sec(&self) -> u64 {
        self.duration.as_secs() as u64
    }

    fn timestamp_subsec_micros(&self) -> u32 {
        self.duration.subsec_micros() as u32
    }
}

pub async fn sync_time(
    prev_boot_count: u32,
    stack: Stack<'_>,
    rtc: &mut esp_hal::rtc_cntl::Rtc<'_>,
) {
    let need_initial_sync = INITIAL_NTP_SYNC.load(core::sync::atomic::Ordering::Relaxed) == 0;
    // The RTC clock drifts, so every 5th boot we resync it with the NTP time.
    if prev_boot_count.is_multiple_of(5) || need_initial_sync {
        crate::defmt::info!("Syncing RTC with NTP (boot {})", prev_boot_count + 1);
        let time = get_time(stack).await;
        // set_current_time_us expects microseconds
        rtc.set_current_time_us(
            (time.as_second() as u64 * 1_000_000) + (time.subsec_microsecond() as u64),
        );
        if need_initial_sync {
            INITIAL_NTP_SYNC.store(1, core::sync::atomic::Ordering::Relaxed);
        }
    }
}

pub async fn get_time(stack: Stack<'_>) -> jiff::Timestamp {
    use embassy_net::udp::UdpSocket;
    use sntpc::{NtpContext, get_time};

    let rx_meta = RX_META.init([PacketMetadata::EMPTY; 16]);
    #[allow(clippy::large_stack_frames, reason = "false positive")]
    let rx_buffer = RX_BUFFER.init_with(|| [0; 4096]);
    let tx_meta = TX_META.init([PacketMetadata::EMPTY; 16]);
    #[allow(clippy::large_stack_frames, reason = "false positive")]
    let tx_buffer = TX_BUFFER.init_with(|| [0; 4096]);

    let mut socket = UdpSocket::new(stack, rx_meta, rx_buffer, tx_meta, tx_buffer);
    socket.bind(123).unwrap();
    let socket = sntpc_net_embassy::UdpSocketWrapper::new(socket);

    let context = NtpContext::new(NtpTimestamp::default());

    let ip = match stack
        .dns_query("pool.ntp.org", DnsQueryType::A)
        .await
        .unwrap()
        .first()
        .unwrap()
    {
        embassy_net::IpAddress::Ipv4(ipv4_addr) => *ipv4_addr,
    };

    //todo error handling
    let result = get_time(SocketAddr::V4(SocketAddrV4::new(ip, 123)), &socket, context)
        .await
        .unwrap();
    let time = jiff::Timestamp::from_second(result.seconds as i64).unwrap();
    crate::defmt::info!("Current time: {:?}", crate::defmt::Debug2Format(&time));
    time
}

pub fn init_https_client<'a>(
    tcp_client: &'a TcpClient<'a, 1, 4096, 4096>,
    dns_socket: &'a DnsSocket<'a>,
    tls_reference: reqwless::TlsReference<'a>,
) -> HttpClient<'a, TcpClient<'a, 1, 4096, 4096>, DnsSocket<'a>> {
    let certs = reqwless::Certificate::new(reqwless::X509::PEM(CERT_STORE)).unwrap();
    let tls_config = TlsConfig::new(reqwless::TlsVersion::Tls1_3, certs, tls_reference);

    HttpClient::new_with_tls(tcp_client, dns_socket, tls_config)
}

pub async fn calendar_data_req(
    client: &mut HttpClient<'_, TcpClient<'_, 1, 4096, 4096>, DnsSocket<'_>>,
    date: &jiff::Zoned,
    req_buffer: &mut [u8; 8192],
    creds: &CaldavCreds,
    calendar_ids: &[String],
) -> alloc::vec::Vec<vcal_parser::vevent::VEventData> {
    crate::defmt::info!(
        "Making calendar request for date: {}",
        crate::defmt::Debug2Format(&date)
    );
    let mut start_display_hour = date.hour();
    if !crate::display::limit_to_today() {
        start_display_hour =
            start_display_hour.clamp(0, 24 - crate::display::get_display_hours() as i8);
    }

    let start_zoned = date
        .datetime()
        .date()
        .to_zoned(date.time_zone().clone())
        .unwrap()
        .checked_add(jiff::Span::new().hours(start_display_hour as i32))
        .unwrap();
    let end_zoned = start_zoned
        .checked_add(jiff::Span::new().hours(crate::display::get_display_hours() as i32))
        .unwrap();

    let start_utc = start_zoned.with_time_zone(TimeZone::UTC).datetime();
    let end_utc = end_zoned.with_time_zone(TimeZone::UTC).datetime();

    let mut start_fmt = heapless::String::<16>::new();
    let _ = write!(
        start_fmt,
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        start_utc.year(),
        start_utc.month(),
        start_utc.day(),
        start_utc.hour(),
        start_utc.minute(),
        start_utc.second()
    );

    let mut end_fmt = heapless::String::<16>::new();
    let _ = write!(
        end_fmt,
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        end_utc.year(),
        end_utc.month(),
        end_utc.day(),
        end_utc.hour(),
        end_utc.minute(),
        end_utc.second()
    );

    let body: heapless::String<554> = heapless::format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
    <d:prop>
        <d:getetag/>
        <c:calendar-data>
            <c:expand start="{}" end="{}"/>
        </c:calendar-data>
    </d:prop>
    <c:filter>
        <c:comp-filter name="VCALENDAR">
            <c:comp-filter name="VEVENT">
                <c:time-range start="{}" end="{}"/>
            </c:comp-filter>
        </c:comp-filter>
    </c:filter>
</c:calendar-query>"#,
        start_fmt,
        end_fmt,
        start_fmt,
        end_fmt
    )
    .unwrap();

    // todo remove from prod
    let url = creds.url.as_str();
    let username = creds.username.as_str();
    let password = creds.password.as_str();

    let url = fluent_uri::Uri::parse(url);
    let url = match url {
        Ok(u) => u,
        Err(e) => {
            crate::defmt::error!("Failed to parse URL: {}", crate::defmt::Debug2Format(&e));
            crate::BootType::set(crate::BootType::Config);
            crate::wifi::stop_wifi_and_reset().await;
        }
    };

    let origin: heapless::String<{ crate::server::MAX_ORIGIN_LEN }> = heapless::format!(
        "{}://{}",
        url.scheme().as_str(),
        url.authority().unwrap().as_str()
    )
    .unwrap();

    let mut all_cals = alloc::vec::Vec::new();
    for cal_id in calendar_ids {
        let path: heapless::String<{ crate::server::MAX_PATH_LEN }> =
            heapless::format!("{}calendars/{}{}", url.path().as_str(), username, cal_id).unwrap();

        crate::defmt::info!("url path: {}", url.path().as_str());
        crate::defmt::info!(
            "username: {}, calendar id: {}",
            username,
            crate::defmt::Display2Format(cal_id)
        );
        crate::defmt::debug!("request path: {}", path);

        let vec = req(
            client,
            &origin,
            &path,
            username,
            password,
            body.as_bytes(),
            req_buffer,
        )
        .await;
        all_cals.extend(vec);
    }

    all_cals
}

async fn req(
    client: &mut HttpClient<'_, TcpClient<'_, 1, 4096, 4096>, DnsSocket<'_>>,
    origin: &str,
    path: &str,
    username: &str,
    password: &str,
    body: &[u8],
    req_buffer: &mut [u8; 8192],
) -> alloc::vec::Vec<vcal_parser::vevent::VEventData> {
    let mut request = client
        .request(reqwless::request::Method::REPORT, origin)
        .await
        .unwrap()
        .basic_auth(username, password)
        .path(path)
        .headers(&[("Content-Type", "text/xml; charset=utf-8"), ("Depth", "1")])
        .body(body);

    let response = request.send(req_buffer).await.unwrap();
    crate::defmt::debug!("Response status: {:?}", response.status);

    let mut reader = response.body().reader();
    let cal = crate::parsing::parse_body_cal(&mut reader).await.unwrap();
    crate::defmt::info!(
        "Parsed calendar data: {:?}",
        crate::defmt::Debug2Format(&cal)
    );
    cal
}

// todo pass http client
pub(crate) async fn get_events(
    tls_ref: reqwless::TlsReference<'_>,
    dns_socket: &'_ DnsSocket<'_>,
    tcp: &TcpClient<'_, 1, 4096, 4096>,
    rtc: &mut esp_hal::rtc_cntl::Rtc<'_>,
    credentials: &CaldavCreds,
    calendar_ids: &[String],
) -> alloc::vec::Vec<vcal_parser::vevent::VEventData> {
    #[allow(clippy::large_stack_frames, reason = "false positive")]
    let req_buffer = REQ_BUFFER.init_with(|| [0u8; 8192]);

    let time_from_rtc =
        jiff::Timestamp::from_second(rtc.current_time_us() as i64 / 1_000_000).unwrap();
    let tzed = time_from_rtc.to_zoned(USER_TIMEZONE);

    let mut client = init_https_client(tcp, dns_socket, tls_ref);

    let mut resp = alloc::vec![];
    let mut success = false;
    for tries in 1..=3 {
        req_buffer.fill(0);
        let req = crate::networking::calendar_data_req(
            &mut client,
            &tzed,
            req_buffer,
            credentials,
            calendar_ids,
        );
        if let Ok(res) =
            embassy_time::with_timeout(embassy_time::Duration::from_secs(30), req).await
        {
            resp = res;
            success = true;
            break;
        };
        crate::defmt::warn!(
            "Failed to get calendar data on attempt {}, retrying...",
            tries
        );
    }

    if !success {
        crate::defmt::error!("Failed after 3 attempts, entering deep sleep");
        crate::hardware::go_to_deep_sleep(rtc);
    }
    resp
}

pub async fn fetch_domain_endpoint(
    client: &mut HttpClient<'_, TcpClient<'_, 1, 4096, 4096>, DnsSocket<'_>>,
    origin: &str,
    response_buf: &mut [u8; 8192],
) -> Option<heapless::String<{ crate::server::MAX_URL_LEN }>> {
    // no extra / at the end
    let path = "/.well-known/caldav";

    let mut request = match client
        .request(reqwless::request::Method::HEAD, origin)
        .await
    {
        Ok(req) => req.path(path),
        Err(e) => {
            crate::defmt::error!("Failed to create request: {:?}", e);
            return None;
        }
    };
    let response = match request.send(response_buf).await {
        Ok(res) => res,
        Err(e) => {
            crate::defmt::error!("Failed to send request: {:?}", e);
            return None;
        }
    };

    crate::defmt::info!("Response status: {:?}", response.status);

    let location: Option<heapless::String<{ crate::server::MAX_URL_LEN }>> = response
        .headers()
        .find(|(name, _)| name.eq_ignore_ascii_case("location"))
        .and_then(|(_, value)| core::str::from_utf8(value).ok())
        .and_then(|s| heapless::String::try_from(s).ok());

    crate::defmt::debug!("Response body: {:?}", location);
    location
}

pub(crate) async fn fetch_principal_url(
    client: &mut HttpClient<'_, TcpClient<'_, 1, 4096, 4096>, DnsSocket<'_>>,
    origin: &str,
    credentials: &CaldavCreds,
    response_buf: &mut [u8; 8192],
) -> Option<String> {
    const BODY: &str = r#"<d:propfind xmlns:d="DAV:">
      <d:prop>
        <d:current-user-principal />
      </d:prop>
    </d:propfind>"#;
    let username = credentials.username.as_str();
    let password = credentials.password.as_str();
    let url: &str = &credentials.url;

    let mut request = client
        .request(reqwless::request::Method::PROPFIND, origin)
        .await
        .unwrap()
        .basic_auth(username, password)
        .path(url)
        .headers(&[("Content-Type", "text/xml; charset=utf-8"), ("Depth", "1")])
        .body(BODY.as_bytes());

    let response = request.send(response_buf).await.unwrap();

    crate::defmt::info!("Response status: {:?}", response.status);
    let res = response.body().read_to_end().await.unwrap();

    let Ok(res) = str::from_utf8(res) else {
        crate::defmt::error!("Failed to parse response body as UTF-8");
        return None;
    };

    parse_principal_url(res)
}

const DAV_NS: &str = "DAV:";
const CALDAV_NS: &str = "urn:ietf:params:xml:ns:caldav";

pub fn parse_principal_url(xml: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok().unwrap();

    let res = doc
        .descendants()
        .find(|n| n.has_tag_name((DAV_NS, "current-user-principal")))?
        .children()
        .find(|n| n.has_tag_name((DAV_NS, "href")))?
        .text();
    res.map(|f| f.to_string())
}

pub(crate) async fn fetch_calendar_home_set(
    client: &mut HttpClient<'_, TcpClient<'_, 1, 4096, 4096>, DnsSocket<'_>>,
    origin: &str,
    path: &str,
    credentials: &CaldavCreds,
    response_buf: &mut [u8; 8192],
) -> Option<String> {
    const BODY: &str = r#"<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
        <d:prop>
          <c:calendar-home-set />
        </d:prop>
      </d:propfind>"#;
    let username = credentials.username.as_str();
    let password = credentials.password.as_str();

    let mut request = client
        .request(reqwless::request::Method::PROPFIND, origin)
        .await
        .unwrap()
        .basic_auth(username, password)
        .path(path)
        .headers(&[("Content-Type", "text/xml; charset=utf-8"), ("Depth", "1")])
        .body(BODY.as_bytes());

    let response = request.send(response_buf).await.unwrap();

    crate::defmt::info!("Response status: {:?}", response.status);
    let res = response.body().read_to_end().await.unwrap();

    let res = match str::from_utf8(res) {
        Ok(v) => v,
        Err(_) => {
            crate::defmt::error!("Failed to parse response body as UTF-8");
            return None;
        }
    };
    let res = get_calendar_home_set(res);
    crate::defmt::info!("Calendar home set: {}", crate::defmt::Debug2Format(&res));
    res
}

fn get_calendar_home_set(xml: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;

    let res = doc
        .descendants()
        .find(|n| n.has_tag_name((CALDAV_NS, "calendar-home-set")))?
        .children()
        .find(|n| n.has_tag_name((DAV_NS, "href")))?
        .text()
        .map(str::to_string);
    res.map(|f| f.to_string())
}

pub(crate) async fn fetch_calendars(
    client: &mut HttpClient<'_, TcpClient<'_, 1, 4096, 4096>, DnsSocket<'_>>,
    origin: &str,
    path: &str,
    credentials: &CaldavCreds,
    response_buf: &mut [u8; 8192],
) -> alloc::vec::Vec<CalendarData> {
    const BODY: &str = r#"<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
        <d:prop>
          <d:displayname />
          <d:resourcetype />
          <c:supported-calendar-component-set />
        </d:prop>
      </d:propfind>"#;
    let username = credentials.username.as_str();
    let password = credentials.password.as_str();

    let mut request = client
        .request(reqwless::request::Method::PROPFIND, origin)
        .await
        .unwrap()
        .basic_auth(username, password)
        .path(path)
        .headers(&[("Content-Type", "text/xml; charset=utf-8"), ("Depth", "1")])
        .body(BODY.as_bytes());

    let response = request.send(response_buf).await.unwrap();

    crate::defmt::info!("Response status: {:?}", response.status);
    let mut reader = response.body().reader();
    let calendars = crate::parsing::parse_body(&mut reader).await.unwrap();
    crate::defmt::info!("Calendars: {:?}", crate::defmt::Debug2Format(&calendars));

    calendars
}

pub(crate) async fn check_credentials(
    client: &mut HttpClient<'_, TcpClient<'_, 1, 4096, 4096>, DnsSocket<'_>>,
    origin: &str,
    path: &str,
    credentials: &CaldavCreds,
    response_buf: &mut [u8; 8192],
) -> Result<(), NetworkError> {
    let username = credentials.username.as_str();
    let password = credentials.password.as_str();

    let mut request = client
        .request(reqwless::request::Method::PROPFIND, origin)
        .await?
        .basic_auth(username, password)
        .path(path)
        .headers(&[("Content-Type", "text/xml; charset=utf-8"), ("Depth", "0")]);

    let response = request.send(response_buf).await?;

    crate::defmt::info!("Response status: {:?}", response.status);

    if !response.status.is_successful() {
        return Err(NetworkError::InvalidCredentials);
    }

    Ok(())
}
