use core::fmt::Write;
use core::net::{SocketAddr, SocketAddrV4};
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::TcpClient;
use embassy_net::Stack;
use static_cell::StaticCell;

use mbedtls_rs::Tls;

use esp_println::println;

use esp_backtrace as _;
use reqwless::client::{HttpClient, TlsConfig};
use reqwless::request::RequestBuilder;
use reqwless::{Certificate, TlsReference};
use smoltcp::wire::DnsQueryType;

const NEXTCLOUD_CERT: &core::ffi::CStr = {
    // add missing null byte compile time
    let s = concat!(include_str!("../cert.pem"), "\0");

    match core::ffi::CStr::from_bytes_with_nul(s.as_bytes()) {
        Ok(c) => c,
        Err(_) => panic!("cert contains interior null bytes or is missing terminator"),
    }
};
const CALENDAR_ID: &str = "szakdoga-teszt";
const TOTAL_VCAL_BUFFER: usize = crate::MAX_DAILY_EVENTS * crate::MAX_VCALENDAR_BYTES;

static CLIENT_STATE: StaticCell<embassy_net::tcp::client::TcpClientState<1, 4096, 4096>> =
    StaticCell::new();

#[derive(Copy, Clone, Default)]
struct NtpTimestamp {
    duration: time::Duration,
}

impl sntpc::NtpTimestampGenerator for NtpTimestamp {
    fn init(&mut self) {
        let ticks = embassy_time::Instant::now().as_ticks();
        let micros = ticks * 1_000_000 / embassy_time::TICK_HZ;
        self.duration = time::Duration::microseconds(micros as i64);
    }

    fn timestamp_sec(&self) -> u64 {
        self.duration.whole_seconds() as u64
    }

    fn timestamp_subsec_micros(&self) -> u32 {
        self.duration.subsec_microseconds() as u32
    }
}

pub async fn get_time(stack: Stack<'_>) -> time::UtcDateTime {
    use embassy_net::udp::UdpSocket;
    use sntpc::{get_time, NtpContext};

    let mut rx_meta = [embassy_net::udp::PacketMetadata::EMPTY; 16];
    let mut rx_buffer = [0; 4096];
    let mut tx_meta = [embassy_net::udp::PacketMetadata::EMPTY; 16];
    let mut tx_buffer = [0; 4096];

    // Within an Embassy async context
    let mut socket = UdpSocket::new(
        stack,
        &mut rx_meta,
        &mut rx_buffer,
        &mut tx_meta,
        &mut tx_buffer,
    );
    socket.bind(123).unwrap();
    let socket = sntpc_net_embassy::UdpSocketWrapper::new(socket);

    let context = NtpContext::new(NtpTimestamp::default());

    let ip = match stack
        .dns_query("pool.ntp.org", DnsQueryType::A)
        .await
        .unwrap()
        .get(0)
        .unwrap()
    {
        embassy_net::IpAddress::Ipv4(ipv4_addr) => ipv4_addr.clone(),
    };

    //todo error handling
    let result = get_time(SocketAddr::V4(SocketAddrV4::new(ip, 123)), &socket, context)
        .await
        .unwrap();
    let time = time::UtcDateTime::from_unix_timestamp(result.seconds.into()).unwrap();
    println!("Current time: {:?}", time);
    time
}

pub async fn network_req<'t>(
    stack: Stack<'_>,
    tls_reference: TlsReference<'_>,
    date: time::Date,
) -> heapless::String<TOTAL_VCAL_BUFFER> {
    let mut fmt_date = heapless::String::<8>::new();

    let _ = write!(
        fmt_date,
        "{}{:02}{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    );

    let tcp_client = TcpClient::new(
        stack,
        CLIENT_STATE.init(embassy_net::tcp::client::TcpClientState::new()),
    );
    let dns_socket = DnsSocket::new(stack);

    let certs = Certificate::new(reqwless::X509::PEM(NEXTCLOUD_CERT)).unwrap();
    let tls_config = TlsConfig::new(reqwless::TlsVersion::Tls1_3, certs, tls_reference);

    let mut client = HttpClient::new_with_tls(&tcp_client, &dns_socket, tls_config);

    let mut req_buffer = [0; 8192];

    let body: heapless::String<553> = heapless::format!(
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
                <c:time-range start="{}T000000Z" end="{}T235959"/>
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

    let origin = option_env!("ORIGIN").unwrap();
    let username = option_env!("CALDAV_USER").unwrap();
    let password = option_env!("CALDAV_PASS").unwrap();
    // 64 long uid + max 64 long username
    let path: heapless::String<128> =
        heapless::format!("/remote.php/dav/calendars/{}/{}/", username, CALENDAR_ID).unwrap();

    let mut request = client
        .request(reqwless::request::Method::REPORT, &origin)
        .await
        .unwrap()
        .basic_auth(username, password)
        .path(&path)
        .headers(&[("Content-Type", "text/xml; charset=utf-8"), ("Depth", "1")])
        .body(body.as_bytes());

    let response = request.send(&mut req_buffer).await.unwrap();
    println!("Response status: {:?}", response.status);

    let res = response.body().read_to_end().await.unwrap();

    let res = match str::from_utf8(&res) {
        Ok(v) => v,
        Err(_) => {
            println!("Response body (hex): {:02x?}", res);
            todo!()
        }
    };
    heapless::String::try_from(res).unwrap()
}
