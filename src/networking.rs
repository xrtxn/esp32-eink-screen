use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use core::fmt::Write;
use core::net::{SocketAddr, SocketAddrV4};
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::TcpClient;
use embassy_net::Stack;
use static_cell::StaticCell;
use embassy_time::{Duration, Timer};
use esp_hal::peripherals::{RSA, SHA};

use esp_mbedtls::Tls;

use esp_println::println;

use esp_backtrace as _;
use reqwless::client::{HttpClient, TlsConfig};
use reqwless::request::RequestBuilder;
use reqwless::{Certificates, X509};
use smoltcp::wire::DnsQueryType;

// add missing null byte
const NEXTCLOUD_CERT: &[u8] = concat!(include_str!("../cert.pem"), "\0").as_bytes();

static CLIENT_STATE: StaticCell<embassy_net::tcp::client::TcpClientState<1, 4096, 4096>> =
    StaticCell::new();

#[derive(Copy, Clone, Default)]
struct Timestamp {
    duration: time::Duration,
}

impl sntpc::NtpTimestampGenerator for Timestamp {
    fn init(&mut self) {
        self.duration = time::Duration::new(0, 0);
    }

    fn timestamp_sec(&self) -> u64 {
        self.duration.as_seconds_f32() as u64
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
    let context = NtpContext::new(Timestamp::default());

    let ip = match stack
        .dns_query("pool.ntp.org", DnsQueryType::A)
        .await
        .unwrap()
        .get(0)
        .unwrap()
    {
        embassy_net::IpAddress::Ipv4(ipv4_addr) => ipv4_addr.clone(),
    };

    let result = get_time(SocketAddr::V4(SocketAddrV4::new(ip, 123)), &socket, context)
        .await
        .unwrap();
    let time = time::UtcDateTime::from_unix_timestamp(result.seconds.into()).unwrap();
    println!("Current time: {:?}", time);
    time
}

pub async fn network_req(
    stack: Stack<'_>,
    rsa_peripherial: RSA<'_>,
    sha_peripherial: SHA<'_>,
    date: time::Date,
) -> String {
    Timer::after(Duration::from_millis(1_000)).await;
    let mut fmt_date = heapless::String::<8>::new();

    // We format directly into the stack-allocated string
    // This avoids all dynamic memory allocation
    let _ = write!(
        fmt_date,
        "{}{:02}{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    );

    let tcp_client =
        TcpClient::new(stack, CLIENT_STATE.init(embassy_net::tcp::client::TcpClientState::new()));
    let dns_socket = DnsSocket::new(stack);

    let tls = Tls::new(sha_peripherial)
        .unwrap()
        .with_hardware_rsa(rsa_peripherial);
    let mut certs = Certificates::new();
    certs.ca_chain = Some(X509::pem(NEXTCLOUD_CERT).unwrap());
    let tls_config = TlsConfig::new(reqwless::TlsVersion::Tls1_3, certs, tls.reference());

    let mut client = HttpClient::new_with_tls(&tcp_client, &dns_socket, tls_config);

    let mut req_buffer = [0; 4096];

    let creds = include_str!("../passwd.txt");

    let origin = creds.split('\n').nth(2).unwrap();

    let body = format!(
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
        fmt_date, fmt_date, fmt_date, fmt_date
    );

    let username = creds.split('\n').nth(3).unwrap();
    let password = creds.split('\n').nth(4).unwrap();
    let path = format!(
        "/remote.php/dav/calendars/{}/e4a2c806-b52b-43a3-828b-d97ec82f698b/",
        username
    );

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
    res.to_owned()
}
