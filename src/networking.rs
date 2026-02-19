use core::fmt::Write;
use core::net::{SocketAddr, SocketAddrV4};
use embassy_net::Stack;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::TcpClient;
use embassy_net::udp::PacketMetadata;
use esp_backtrace as _;
use reqwless::client::{HttpClient, TlsConfig};
use reqwless::request::RequestBuilder;
use reqwless::{Certificate, TlsReference};
use smoltcp::wire::DnsQueryType;
use static_cell::StaticCell;

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

pub(crate) static CLIENT_STATE: StaticCell<
    embassy_net::tcp::client::TcpClientState<1, 4096, 4096>,
> = StaticCell::new();
static RX_META: StaticCell<[PacketMetadata; 16]> = StaticCell::new();
static RX_BUFFER: StaticCell<[u8; 4096]> = StaticCell::new();
static TX_META: StaticCell<[PacketMetadata; 16]> = StaticCell::new();
static TX_BUFFER: StaticCell<[u8; 4096]> = StaticCell::new();

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
    let time = time::UtcDateTime::from_unix_timestamp(result.seconds.into()).unwrap();
    log::info!("Current time: {:?}", time);
    time
}

pub async fn network_req(
    stack: Stack<'_>,
    tcp_client: &TcpClient<'_, 1, 4096, 4096>,
    tls_reference: TlsReference<'_>,
    date: time::Date,
    cal_xml_buf: &mut heapless::String<TOTAL_VCAL_BUFFER>,
    req_buffer: &mut [u8; 8192],
) {
    let mut fmt_date = heapless::String::<8>::new();

    let _ = write!(
        fmt_date,
        "{}{:02}{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    );

    let dns_socket = DnsSocket::new(stack);

    let certs = Certificate::new(reqwless::X509::PEM(NEXTCLOUD_CERT)).unwrap();
    let tls_config = TlsConfig::new(reqwless::TlsVersion::Tls1_3, certs, tls_reference);

    let mut client = HttpClient::new_with_tls(tcp_client, &dns_socket, tls_config);

    // todo get date and time based on user timezone, caldav only accepts utc time
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

    let origin = env!("ORIGIN");
    let username = env!("CALDAV_USER");
    let password = env!("CALDAV_PASS");
    // 64 long uid + max 64 long username
    let path: heapless::String<128> =
        heapless::format!("/remote.php/dav/calendars/{}/{}/", username, CALENDAR_ID).unwrap();

    let mut request = client
        .request(reqwless::request::Method::REPORT, origin)
        .await
        .unwrap()
        .basic_auth(username, password)
        .path(&path)
        .headers(&[("Content-Type", "text/xml; charset=utf-8"), ("Depth", "1")])
        .body(body.as_bytes());

    let response = request.send(req_buffer).await.unwrap();
    log::debug!("Response status: {:?}", response.status);

    let res = response.body().read_to_end().await.unwrap();

    let res = match str::from_utf8(res) {
        Ok(v) => v,
        Err(_) => {
            log::error!("Response body (hex): {:02x?}", res);
            todo!()
        }
    };
    cal_xml_buf.clear();
    cal_xml_buf
        .push_str(res)
        .expect("Response too large for calendar buffer");
}
