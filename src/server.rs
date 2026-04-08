use picoserve::AppBuilder;

use crate::storage;

const INDEX_HTML_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/index.html.gz"));
const DISPLAY_HTML_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/config.html.gz"));

pub const WEB_TASK_POOL_SIZE: usize = 2;

#[cfg(target_arch = "xtensa")]
pub use xtensa::*;

#[cfg(target_arch = "xtensa")]
static REQ_MUTEX: static_cell::StaticCell<
    embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::NoopRawMutex,
        &'static mut [u8; 8192],
    >,
> = static_cell::StaticCell::new();

pub struct AppProps {
    #[cfg(target_arch = "xtensa")]
    pub flash_storage: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::NoopRawMutex,
        storage::FlashStorage<'static>,
    >,
    #[cfg(target_arch = "xtensa")]
    pub tls_mutex: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::NoopRawMutex,
        &'static mut mbedtls_rs::Tls<'static>,
    >,
    #[cfg(target_arch = "xtensa")]
    pub dns_socket: &'static embassy_net::dns::DnsSocket<'static>,
    #[cfg(target_arch = "xtensa")]
    pub tcp_client: &'static embassy_net::tcp::client::TcpClient<'static, 1, 4096, 4096>,
}

impl AppBuilder for AppProps {
    type PathRouter = impl picoserve::routing::PathRouter;

    fn build_app(self) -> picoserve::Router<Self::PathRouter> {
        #[cfg(target_arch = "xtensa")]
        let flash = self.flash_storage;
        #[cfg(target_arch = "xtensa")]
        let tls_mutex = self.tls_mutex;
        #[cfg(target_arch = "xtensa")]
        let dns_socket = self.dns_socket;
        #[cfg(target_arch = "xtensa")]
        let tcp_client = self.tcp_client;

        // Reuse existing REQ_BUFFER
        #[cfg(target_arch = "xtensa")]
        let req_buffer = crate::networking::REQ_BUFFER.init([0u8; 8192]);
        #[cfg(target_arch = "xtensa")]
        let req_buffer_mutex = &*REQ_MUTEX.init(embassy_sync::mutex::Mutex::new(req_buffer));

        picoserve::Router::new()
            .route("/", picoserve::routing::get(move || config_page_handler()))
            .route(
                "/api/config/wifi",
                picoserve::routing::post(
                    move |picoserve::extract::Json(resp_wifi): picoserve::extract::Json<
                        storage::WifiCreds,
                    >| async move {
                        log::info!("Received config change request: {:?}", resp_wifi);

                        #[cfg(target_arch = "xtensa")]
                        let mut nvs = storage::read_config(flash).await.unwrap_or_default();
                        #[cfg(not(target_arch = "xtensa"))]
                        let mut nvs = storage::read_config().await.unwrap_or_default();

                        nvs.wifi = Some(resp_wifi);

                        #[cfg(target_arch = "xtensa")]
                        storage::write_config(flash, nvs).await;
                        #[cfg(not(target_arch = "xtensa"))]
                        storage::write_config(nvs).await;
                    },
                ),
            )
            .route(
                "/api/config/caldav",
                picoserve::routing::post(
                    move |req: picoserve::extract::Json<storage::CaldavCreds>| async move {
                        #[cfg(target_arch = "xtensa")]
                        return save_caldav_handler(flash, req).await;
                        #[cfg(not(target_arch = "xtensa"))]
                        return save_caldav_handler(req).await;
                    },
                ),
            )
            .route(
                "/api/config/display",
                picoserve::routing::post(
                    move |picoserve::extract::Json(resp_caldav): picoserve::extract::Json<
                        storage::DisplayConfig,
                    >| async move {
                        log::info!("Received config change request: {:?}", resp_caldav);

                        #[cfg(target_arch = "xtensa")]
                        let mut nvs = storage::read_config(flash).await.unwrap_or_default();
                        #[cfg(not(target_arch = "xtensa"))]
                        let mut nvs = storage::read_config().await.unwrap_or_default();

                        nvs.display = Some(resp_caldav);

                        #[cfg(target_arch = "xtensa")]
                        storage::write_config(flash, nvs).await;
                        #[cfg(not(target_arch = "xtensa"))]
                        storage::write_config(nvs).await;
                    },
                ),
            )
            .route(
                "/display_config",
                picoserve::routing::get(move || display_config_page_handler()),
            )
            .route(
                "/api/config/caldav/endpoint",
                picoserve::routing::post(move |body| async move {
                    #[cfg(target_arch = "xtensa")]
                    return fetch_domain_endpoint(
                        tls_mutex,
                        dns_socket,
                        tcp_client,
                        body,
                        req_buffer_mutex,
                    )
                    .await;
                    #[cfg(not(target_arch = "xtensa"))]
                    return fetch_domain_endpoint(body).await;
                }),
            )
    }
}

async fn fetch_domain_endpoint(
    #[cfg(target_arch = "xtensa")] tls_mutex: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::NoopRawMutex,
        &'static mut mbedtls_rs::Tls<'static>,
    >,
    #[cfg(target_arch = "xtensa")] dns_socket: &'static embassy_net::dns::DnsSocket<'static>,
    #[cfg(target_arch = "xtensa")] tcp_client: &'static embassy_net::tcp::client::TcpClient<
        'static,
        1,
        4096,
        4096,
    >,
    body: alloc::string::String,
    #[cfg(target_arch = "xtensa")] req_buffer_mutex: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::NoopRawMutex,
        &'static mut [u8; 8192],
    >,
) -> Result<picoserve::response::json::Json<serde_json::Value>, picoserve::response::StatusCode> {
    #[cfg(target_arch = "xtensa")]
    {
        let mut buf_guard = req_buffer_mutex.lock().await;

        let tls = tls_mutex.lock().await;
        let tls_reference = tls.reference();

        let mut client =
            crate::networking::init_https_client(tcp_client, dns_socket, tls_reference);

        let endpoint =
            crate::networking::fetch_domain_endpoint(&mut client, &body, &mut *buf_guard).await;
        match endpoint {
            Some(url) => Ok(picoserve::response::json::Json(
                serde_json::json!({ "endpoint": url }),
            )),
            None => Err(picoserve::response::StatusCode::BAD_REQUEST),
        }
    }
}

async fn config_page_handler() -> impl picoserve::response::IntoResponse {
    (
        [
            ("Content-Type", "text/html; charset=utf-8"),
            ("Content-Encoding", "gzip"),
            ("Content-Length", env!("INDEX_HTML_GZ_LEN")),
        ],
        INDEX_HTML_GZ,
    )
}

async fn display_config_page_handler() -> impl picoserve::response::IntoResponse {
    (
        [
            ("Content-Type", "text/html; charset=utf-8"),
            ("Content-Encoding", "gzip"),
            ("Content-Length", env!("DISPLAY_HTML_GZ_LEN")),
        ],
        DISPLAY_HTML_GZ,
    )
}

async fn save_caldav_handler(
    #[cfg(target_arch = "xtensa")] flash: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::NoopRawMutex,
        storage::FlashStorage<'static>,
    >,
    picoserve::extract::Json(resp_caldav): picoserve::extract::Json<storage::CaldavCreds>,
) -> impl picoserve::response::IntoResponse {
    log::info!("Received config change request: {:?}", resp_caldav);

    let url = fluent_uri::Uri::parse(resp_caldav.url.as_str());
    match url {
        Ok(res) => log::info!("Parsed URL: {}", res.as_str()),
        Err(err) => {
            log::error!("Failed to parse URL: {}", err);
            return picoserve::response::StatusCode::BAD_REQUEST;
        }
    };

    #[cfg(target_arch = "xtensa")]
    let mut nvs = storage::read_config(flash).await.unwrap_or_default();
    #[cfg(not(target_arch = "xtensa"))]
    let mut nvs = storage::read_config().await.unwrap_or_default();

    nvs.caldav = Some(resp_caldav);

    #[cfg(target_arch = "xtensa")]
    storage::write_config(flash, nvs).await;
    #[cfg(not(target_arch = "xtensa"))]
    storage::write_config(nvs).await;
    return picoserve::response::StatusCode::OK;
}

#[derive(serde::Serialize, PartialEq, Clone, Copy)]
#[serde(tag = "status")]
pub enum NetworkStatus {
    AccessPoint, // The device is running an access point
    Network,     // The device is connected to a Wi-Fi network
}

#[cfg(target_arch = "xtensa")]
mod xtensa {
    use super::*;
    use static_cell::StaticCell;

    static CONFIG: picoserve::Config = picoserve::Config::const_default().keep_connection_alive();

    static TCP_RX_BUFFERS: [StaticCell<[u8; 1024]>; WEB_TASK_POOL_SIZE] =
        [const { StaticCell::new() }; WEB_TASK_POOL_SIZE];
    static TCP_TX_BUFFERS: [StaticCell<[u8; 1024]>; WEB_TASK_POOL_SIZE] =
        [const { StaticCell::new() }; WEB_TASK_POOL_SIZE];
    static HTTP_BUFFERS: [StaticCell<[u8; 2048]>; WEB_TASK_POOL_SIZE] =
        [const { StaticCell::new() }; WEB_TASK_POOL_SIZE];

    #[cfg_attr(target_arch = "xtensa", embassy_executor::task(pool_size = WEB_TASK_POOL_SIZE))]
    pub async fn web_task(
        task_id: usize,
        stack: embassy_net::Stack<'static>,
        app: &'static picoserve::AppRouter<AppProps>,
    ) -> ! {
        let port = 80;
        let tcp_rx_buffer = TCP_RX_BUFFERS[task_id].init([0; 1024]);
        let tcp_tx_buffer = TCP_TX_BUFFERS[task_id].init([0; 1024]);
        let http_buffer = HTTP_BUFFERS[task_id].init([0; 2048]);

        picoserve::Server::new(app, &CONFIG, http_buffer)
            .listen_and_serve(task_id, stack, port, tcp_rx_buffer, tcp_tx_buffer)
            .await
            .into_never()
    }
}
