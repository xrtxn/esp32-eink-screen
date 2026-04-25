#[cfg(target_arch = "xtensa")]
use alloc::string::String;
#[cfg(target_arch = "xtensa")]
use alloc::vec::Vec;
#[cfg(not(target_arch = "xtensa"))]
use std::string::String;

use picoserve::AppBuilder;
use vcal_parser::calendars::CalendarData;

use crate::storage;

const INDEX_HTML_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/credentials.html.gz"));
const DISPLAY_HTML_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/calendar-config.html.gz"));

pub const WEB_TASK_POOL_SIZE: usize = 2;

pub const MAX_ORIGIN_LEN: usize = 128;
pub const MAX_PATH_LEN: usize = 255;
pub const MAX_URL_LEN: usize = MAX_ORIGIN_LEN + MAX_PATH_LEN;

#[cfg(target_arch = "xtensa")]
pub use xtensa::*;

#[derive(thiserror::Error, picoserve::response::ErrorWithStatusCode, Debug)]
#[status_code(INTERNAL_SERVER_ERROR)]
pub enum AppError {
    #[error("Network error: {0}")]
    Network(#[from] crate::networking::NetworkError),
    #[error("Storage error: {0}")]
    Storage(#[from] crate::storage::StorageError),
}

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
        #[allow(clippy::large_stack_frames, reason = "false positive")]
        let req_buffer = crate::networking::REQ_BUFFER.init_with(|| [0u8; 8192]);
        #[cfg(target_arch = "xtensa")]
        #[allow(clippy::large_stack_frames, reason = "false positive")]
        let req_buffer_mutex =
            &*REQ_MUTEX.init_with(|| embassy_sync::mutex::Mutex::new(req_buffer));

        picoserve::Router::new()
            .route("/", picoserve::routing::get(config_page_handler))
            .route(
                "/api/config/wifi",
                picoserve::routing::post(
                    move |picoserve::extract::Json(resp_wifi): picoserve::extract::Json<
                        storage::WifiCreds,
                    >| async move {
                        crate::defmt::info!("Received config change request: {:?}", resp_wifi);

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
                        #[cfg(target_arch = "xtensa")]
                        let mut nvs = storage::read_config(flash).await.unwrap_or_default();
                        #[cfg(not(target_arch = "xtensa"))]
                        let mut nvs = storage::read_config().await.unwrap_or_default();

                        #[cfg(target_arch = "xtensa")]
                        crate::display::DISPLAY_HOURS.store(
                            resp_caldav.displayed_hours,
                            core::sync::atomic::Ordering::Relaxed,
                        );
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
                picoserve::routing::get(display_config_page_handler),
            )
            .route(
                "/api/config/caldav/endpoint",
                picoserve::routing::post(
                    move |picoserve::extract::Json(body): picoserve::extract::Json<
                        EndpointRequest,
                    >| async move {
                        #[cfg(target_arch = "xtensa")]
                        return fetch_domain_endpoint(
                            tls_mutex,
                            dns_socket,
                            tcp_client,
                            &body.url,
                            req_buffer_mutex,
                        )
                        .await;
                        #[cfg(not(target_arch = "xtensa"))]
                        return fetch_domain_endpoint(&body.url).await;
                    },
                ),
            )
            .route(
                "/api/config/caldav/credentials",
                picoserve::routing::post(
                    move |picoserve::extract::Json(credentials): picoserve::extract::Json<
                        storage::CaldavCreds,
                    >| async move {
                        #[cfg(target_arch = "xtensa")]
                        {
                            return check_caldav_credentials(
                                tls_mutex,
                                dns_socket,
                                tcp_client,
                                req_buffer_mutex,
                                &credentials,
                            )
                            .await
                            .map_err(AppError::Network);
                        }
                        #[cfg(not(target_arch = "xtensa"))]
                        return check_caldav_credentials(&credentials)
                            .await
                            .map_err(AppError::Network);
                    },
                ),
            )
            // TODO: fix logic in case of bad config
            .route(
                "/api/config/caldav/calendars",
                picoserve::routing::get(move || async move {
                    #[cfg(target_arch = "xtensa")]
                    {
                        let nvs = storage::read_config(flash)
                            .await
                            .ok_or(AppError::Storage(storage::StorageError::ReadError))?
                            .caldav
                            .ok_or(AppError::Storage(storage::StorageError::ReadError))?;

                        return fetch_calendars(
                            tls_mutex,
                            dns_socket,
                            tcp_client,
                            &nvs.url,
                            req_buffer_mutex,
                            &nvs,
                        )
                        .await
                        .map_err(AppError::Network);
                    }
                    #[cfg(not(target_arch = "xtensa"))]
                    return fetch_calendars().await.map_err(AppError::Network);
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
    #[cfg(target_arch = "xtensa")] body: &str,
    #[cfg(not(target_arch = "xtensa"))] body: &str,
    #[cfg(target_arch = "xtensa")] req_buffer_mutex: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::NoopRawMutex,
        &'static mut [u8; 8192],
    >,
) -> Result<picoserve::response::json::Json<EndpointResponse>, picoserve::response::StatusCode> {
    #[cfg(target_arch = "xtensa")]
    {
        let mut buf_guard = req_buffer_mutex.lock().await;

        let tls = tls_mutex.lock().await;
        let tls_reference = tls.reference();

        let mut client =
            crate::networking::init_https_client(tcp_client, dns_socket, tls_reference);

        let endpoint =
            crate::networking::fetch_domain_endpoint(&mut client, body, *buf_guard).await;
        match endpoint {
            Some(url) => Ok(picoserve::response::json::Json(EndpointResponse {
                endpoint: url,
            })),
            None => Err(picoserve::response::StatusCode::BAD_REQUEST),
        }
    }
    #[cfg(not(target_arch = "xtensa"))]
    {
        let _ = body;
        let resp: heapless::String<{ MAX_URL_LEN }> =
            heapless::String::try_from("https://example.com/caldav").unwrap();
        Ok(picoserve::response::json::Json(EndpointResponse {
            endpoint: resp,
        }))
    }
}

async fn fetch_calendars(
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
    #[cfg(target_arch = "xtensa")] body: &str,
    #[cfg(target_arch = "xtensa")] req_buffer_mutex: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::NoopRawMutex,
        &'static mut [u8; 8192],
    >,
    #[cfg(target_arch = "xtensa")] credentials: &storage::CaldavCreds,
) -> Result<picoserve::response::json::Json<Vec<CalendarData>>, crate::networking::NetworkError> {
    #[cfg(target_arch = "xtensa")]
    {
        let mut buf_guard = req_buffer_mutex.lock().await;

        let tls = tls_mutex.lock().await;
        let tls_reference = tls.reference();

        let mut client =
            crate::networking::init_https_client(tcp_client, dns_socket, tls_reference);

        let res =
            crate::networking::fetch_principal_url(&mut client, body, credentials, *buf_guard)
                .await
                .ok_or(crate::networking::NetworkError::ParsingError)?;
        let home = crate::networking::fetch_calendar_home_set(
            &mut client,
            body,
            &res,
            credentials,
            *buf_guard,
        )
        .await
        .ok_or(crate::networking::NetworkError::ParsingError)?;
        let calendars =
            crate::networking::fetch_calendars(&mut client, body, &home, credentials, *buf_guard)
                .await;
        Ok(picoserve::response::json::Json(calendars))
    }
    #[cfg(not(target_arch = "xtensa"))]
    {
        std::thread::sleep(std::time::Duration::from_secs(2));
        Ok(picoserve::response::json::Json(vec![CalendarData::new(
            Some("https://example.com/caldav/calendars/test/".to_string()),
            Some("Test Calendar".to_string()),
        )]))
    }
}

async fn check_caldav_credentials(
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
    #[cfg(target_arch = "xtensa")] req_buffer_mutex: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::NoopRawMutex,
        &'static mut [u8; 8192],
    >,
    #[cfg(target_arch = "xtensa")] credentials: &storage::CaldavCreds,
    #[cfg(not(target_arch = "xtensa"))] credentials: &storage::CaldavCreds,
) -> Result<picoserve::response::StatusCode, crate::networking::NetworkError> {
    #[cfg(target_arch = "xtensa")]
    {
        let mut buf_guard = req_buffer_mutex.lock().await;

        let tls = tls_mutex.lock().await;
        let tls_reference = tls.reference();

        let mut client =
            crate::networking::init_https_client(tcp_client, dns_socket, tls_reference);

        let url_str = credentials.url.as_str();
        let uri = fluent_uri::Uri::parse(url_str)
            .map_err(|_| crate::networking::NetworkError::WrongUrl)?;

        let scheme = uri.scheme().as_str();
        let authority = uri.authority().map(|a| a.as_str()).unwrap_or("");
        let origin = alloc::format!("{}://{}", scheme, authority);
        let path = uri.path().as_str();

        crate::networking::check_credentials(
            &mut client,
            &origin,
            if path.is_empty() { "/" } else { path },
            credentials,
            *buf_guard,
        )
        .await?;

        Ok(picoserve::response::StatusCode::OK)
    }
    #[cfg(not(target_arch = "xtensa"))]
    {
        let _ = credentials;
        Ok(picoserve::response::StatusCode::OK)
    }
}

#[derive(serde::Deserialize)]
struct EndpointRequest {
    url: String,
}

#[derive(serde::Serialize)]
struct EndpointResponse {
    endpoint: heapless::String<{ MAX_URL_LEN }>,
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
    crate::defmt::info!("Received config change request: {:?}", resp_caldav);

    let url = fluent_uri::Uri::parse(resp_caldav.url.as_str());
    match url {
        Ok(res) => crate::defmt::info!("Parsed URL: {}", res.as_str()),
        Err(err) => {
            crate::defmt::error!("Failed to parse URL: {}", crate::defmt::Debug2Format(&err));
            return picoserve::response::StatusCode::BAD_REQUEST;
        }
    }

    #[cfg(target_arch = "xtensa")]
    let mut nvs = storage::read_config(flash).await.unwrap_or_default();
    #[cfg(not(target_arch = "xtensa"))]
    let mut nvs = storage::read_config().await.unwrap_or_default();

    nvs.caldav = Some(resp_caldav);

    #[cfg(target_arch = "xtensa")]
    storage::write_config(flash, nvs).await;
    #[cfg(not(target_arch = "xtensa"))]
    storage::write_config(nvs).await;
    picoserve::response::StatusCode::OK
}

#[derive(serde::Serialize, PartialEq, Clone, Copy)]
#[serde(tag = "status")]
pub enum NetworkStatus {
    AccessPoint, // The device is running an access point
    Network,     // The device is connected to a Wi-Fi network
}

#[cfg(target_arch = "xtensa")]
mod xtensa {
    use embassy_time::Duration;
    use static_cell::StaticCell;

    use super::{AppProps, WEB_TASK_POOL_SIZE};

    static CONFIG: picoserve::Config = picoserve::Config::new(picoserve::Timeouts {
        start_read_request: Duration::from_secs(5),
        persistent_start_read_request: Duration::from_secs(1),
        read_request: Duration::from_secs(3),
        write: Duration::from_secs(5),
    })
    .close_connection_after_response();

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
        #[allow(clippy::large_stack_frames, reason = "false positive")]
        let tcp_rx_buffer = TCP_RX_BUFFERS[task_id].init_with(|| [0; 1024]);
        #[allow(clippy::large_stack_frames, reason = "false positive")]
        let tcp_tx_buffer = TCP_TX_BUFFERS[task_id].init_with(|| [0; 1024]);
        #[allow(clippy::large_stack_frames, reason = "false positive")]
        let http_buffer = HTTP_BUFFERS[task_id].init_with(|| [0; 2048]);

        picoserve::Server::new(app, &CONFIG, http_buffer)
            .listen_and_serve(task_id, stack, port, tcp_rx_buffer, tcp_tx_buffer)
            .await
            .into_never()
    }
}
