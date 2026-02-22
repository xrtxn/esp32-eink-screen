use core::cell::RefCell;

use askama::Template as _;
use esp_storage::FlashStorage;
use picoserve::AppBuilder;
use static_cell::StaticCell;

use crate::storage;

static CONFIG: picoserve::Config = picoserve::Config::const_default().keep_connection_alive();

pub const WEB_TASK_POOL_SIZE: usize = 2;

static TCP_RX_BUFFERS: [StaticCell<[u8; 1024]>; WEB_TASK_POOL_SIZE] =
    [const { StaticCell::new() }; WEB_TASK_POOL_SIZE];
static TCP_TX_BUFFERS: [StaticCell<[u8; 1024]>; WEB_TASK_POOL_SIZE] =
    [const { StaticCell::new() }; WEB_TASK_POOL_SIZE];
static HTTP_BUFFERS: [StaticCell<[u8; 2048]>; WEB_TASK_POOL_SIZE] =
    [const { StaticCell::new() }; WEB_TASK_POOL_SIZE];

pub(crate) struct AppProps {
    pub flash_storage: &'static RefCell<FlashStorage<'static>>,
}

impl AppBuilder for AppProps {
    type PathRouter = impl picoserve::routing::PathRouter;

    fn build_app(self) -> picoserve::Router<Self::PathRouter> {
        // &'static RefCell<...> is Copy, so the closure is AsyncFn (not just AsyncFnMut)
        let flash = self.flash_storage;

        picoserve::Router::new()
            .route(
                "/",
                picoserve::routing::get(move || config_page_handler(flash)),
            )
            .route(
                "/api/change_config",
                picoserve::routing::post(async || {
                    log::info!("Received config change request");
                }),
            )
            .nest_service(
                "/static",
                const {
                    picoserve::response::Directory {
                        files: &[(
                            "pico.min.css",
                            picoserve::response::File::with_content_type_and_headers(
                                "text/css; charset=utf-8",
                                include_bytes!("../static/pico.min.css.gz"),
                                &[("Content-Encoding", "gzip")],
                            ),
                        )],
                        ..picoserve::response::Directory::DEFAULT
                    }
                },
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
    flash: &'static RefCell<FlashStorage<'static>>,
) -> impl picoserve::response::IntoResponse {
    let cfg = storage::read_config(flash).await.unwrap_or_default();

    // Render the template into an allocated String
    let rendered_html: alloc::string::String = cfg.render().unwrap();

    (
        [("Content-Type", "text/html; charset=utf-8")],
        rendered_html,
    )
}
