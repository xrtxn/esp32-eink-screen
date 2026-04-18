use alloc::string::String;
use alloc::string::ToString;
use core::fmt::Write;
use core::net::{SocketAddr, SocketAddrV4};
use embassy_net::Stack;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::TcpClient;
use embassy_net::udp::PacketMetadata;
use esp_backtrace as _;
use reqwless::client::{HttpClient, TlsConfig};
use reqwless::request::RequestBuilder;
use smoltcp::wire::DnsQueryType;
use static_cell::StaticCell;
pub use vcal_parser::calendars::CalendarData;

use crate::storage::CaldavCreds;

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

pub static REQ_BUFFER: StaticCell<[u8; 8192]> = StaticCell::new();

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
        defmt::info!("Syncing RTC with NTP (boot {})", prev_boot_count + 1);
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
    let rx_buffer = RX_BUFFER.init([0; 4096]);
    let tx_meta = TX_META.init([PacketMetadata::EMPTY; 16]);
    let tx_buffer = TX_BUFFER.init([0; 4096]);

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
    defmt::info!("Current time: {:?}", defmt::Debug2Format(&time));
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
    date: jiff::civil::Date,
    req_buffer: &mut [u8; 8192],
    creds: &CaldavCreds,
    calendar_ids: &[String],
) -> alloc::vec::Vec<vcal_parser::vevent::VEventData> {
    defmt::info!(
        "Making calendar request for date: {}",
        defmt::Debug2Format(&date)
    );
    let mut fmt_date = heapless::String::<8>::new();

    let _ = write!(
        fmt_date,
        "{}{:02}{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    );

    // todo get date and time based on user timezone, caldav only accepts utc time
    // also limit to a few hours
    let body: heapless::String<554> = heapless::format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
    <d:prop>
        <d:getetag/>
        <c:calendar-data>
            <c:expand start="{}T000000Z" end="{}T235959Z"/>
        </c:calendar-data>
    </d:prop>
    <c:filter>
        <c:comp-filter name="VCALENDAR">
            <c:comp-filter name="VEVENT">
                <c:time-range start="{}T000000Z" end="{}T235959Z"/>
            </c:comp-filter>
        </c:comp-filter>
    </c:filter>
</c:calendar-query>"#,
        fmt_date,
        fmt_date,
        fmt_date,
        fmt_date
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
            defmt::error!("Failed to parse URL: {}", defmt::Debug2Format(&e));
            crate::BootType::set(crate::BootType::Config);
            esp_hal::system::software_reset();
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

        defmt::info!("url path: {}", url.path().as_str());
        defmt::info!(
            "username: {}, calendar id: {}",
            username,
            defmt::Display2Format(cal_id)
        );
        defmt::debug!("request path: {}", path);

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
        .request(reqwless::request::Method::REPORT, &origin)
        .await
        .unwrap()
        .basic_auth(username, password)
        .path(&path)
        .headers(&[("Content-Type", "text/xml; charset=utf-8"), ("Depth", "1")])
        .body(body);

    let response = request.send(req_buffer).await.unwrap();
    defmt::debug!("Response status: {:?}", response.status);

    let mut reader = response.body().reader();
    let cal = crate::parsing::parse_body_cal(&mut reader).await.unwrap();
    defmt::info!("Parsed calendar data: {:?}", defmt::Debug2Format(&cal));
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
    let req_buffer = REQ_BUFFER.init([0u8; 8192]);

    let time_from_rtc =
        jiff::Timestamp::from_second(rtc.current_time_us() as i64 / 1_000_000).unwrap();

    let mut client = init_https_client(tcp, dns_socket, tls_ref);

    let mut resp = alloc::vec![];
    let mut success = false;
    for tries in 1..=3 {
        req_buffer.fill(0);
        let req = crate::networking::calendar_data_req(
            &mut client,
            time_from_rtc.to_zoned(jiff::tz::TimeZone::UTC).date(),
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
        defmt::warn!(
            "Failed to get calendar data on attempt {}, retrying...",
            tries
        );
    }

    if !success {
        defmt::error!("Failed after 3 attempts, entering deep sleep");
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
            defmt::error!("Failed to create request: {:?}", e);
            return None;
        }
    };
    let response = match request.send(response_buf).await {
        Ok(res) => res,
        Err(e) => {
            defmt::error!("Failed to send request: {:?}", e);
            return None;
        }
    };

    defmt::info!("Response status: {:?}", response.status);

    let location: Option<heapless::String<{ crate::server::MAX_URL_LEN }>> = response
        .headers()
        .find(|(name, _)| name.eq_ignore_ascii_case("location"))
        .and_then(|(_, value)| core::str::from_utf8(value).ok())
        .and_then(|s| heapless::String::try_from(s).ok());

    defmt::debug!("Response body: {:?}", location);
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

    defmt::info!("Response status: {:?}", response.status);
    let res = response.body().read_to_end().await.unwrap();

    let Ok(res) = str::from_utf8(res) else {
        defmt::error!("Failed to parse response body as UTF-8");
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

    defmt::info!("Response status: {:?}", response.status);
    let res = response.body().read_to_end().await.unwrap();

    let res = match str::from_utf8(res) {
        Ok(v) => v,
        Err(_) => {
            defmt::error!("Failed to parse response body as UTF-8");
            return None;
        }
    };
    let res = get_calendar_home_set(res);
    defmt::info!("Calendar home set: {}", defmt::Debug2Format(&res));
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

    defmt::info!("Response status: {:?}", response.status);
    let mut reader = response.body().reader();
    let calendars = crate::parsing::parse_body(&mut reader).await.unwrap();
    defmt::info!("Calendars: {:?}", defmt::Debug2Format(&calendars));

    calendars
}
