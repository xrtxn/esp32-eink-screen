use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;

use esp_storage::FlashStorage;
use picoserve::AppBuilder;
use static_cell::StaticCell;

use crate::storage;

const INDEX_HTML_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/index.html.gz"));

static CONFIG: picoserve::Config = picoserve::Config::const_default().keep_connection_alive();

pub const WEB_TASK_POOL_SIZE: usize = 2;

static TCP_RX_BUFFERS: [StaticCell<[u8; 1024]>; WEB_TASK_POOL_SIZE] =
    [const { StaticCell::new() }; WEB_TASK_POOL_SIZE];
static TCP_TX_BUFFERS: [StaticCell<[u8; 1024]>; WEB_TASK_POOL_SIZE] =
    [const { StaticCell::new() }; WEB_TASK_POOL_SIZE];
static HTTP_BUFFERS: [StaticCell<[u8; 2048]>; WEB_TASK_POOL_SIZE] =
    [const { StaticCell::new() }; WEB_TASK_POOL_SIZE];

pub(crate) struct AppProps {
    pub flash_storage: &'static Mutex<NoopRawMutex, FlashStorage<'static>>,
}

impl AppBuilder for AppProps {
    type PathRouter = impl picoserve::routing::PathRouter;

    fn build_app(self) -> picoserve::Router<Self::PathRouter> {
        let flash = self.flash_storage;

        picoserve::Router::new()
            .route(
                "/",
                picoserve::routing::get(move || config_page_handler(flash)),
            )
            .route(
                "/api/config/wifi",
                picoserve::routing::post(
                    move |picoserve::extract::Json(resp_wifi): picoserve::extract::Json<
                        storage::WifiCreds,
                    >| async move {
                        log::info!("Received config change request: {:?}", resp_wifi);
                        let mut nvs = storage::read_config(flash).await.unwrap_or_default();
                        nvs.wifi = Some(resp_wifi);
                        storage::write_config(flash, nvs).await;
                    },
                ),
            )
    }
}

#[embassy_executor::task(pool_size = WEB_TASK_POOL_SIZE)]
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

async fn config_page_handler(
    _flash: &'static Mutex<NoopRawMutex, FlashStorage<'static>>,
) -> impl picoserve::response::IntoResponse {
    (
        [
            ("Content-Type", "text/html; charset=utf-8"),
            ("Content-Encoding", "gzip"),
            ("Content-Length", env!("INDEX_HTML_GZ_LEN")),
        ],
        INDEX_HTML_GZ,
    )
}
