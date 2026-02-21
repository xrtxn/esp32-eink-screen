use picoserve::routing::get_service;
use picoserve::AppBuilder;
use static_cell::StaticCell;

static CONFIG: picoserve::Config = picoserve::Config::const_default().keep_connection_alive();

pub const WEB_TASK_POOL_SIZE: usize = 2;

static TCP_RX_BUFFERS: [StaticCell<[u8; 1024]>; WEB_TASK_POOL_SIZE] =
    [const { StaticCell::new() }; WEB_TASK_POOL_SIZE];
static TCP_TX_BUFFERS: [StaticCell<[u8; 1024]>; WEB_TASK_POOL_SIZE] =
    [const { StaticCell::new() }; WEB_TASK_POOL_SIZE];
static HTTP_BUFFERS: [StaticCell<[u8; 2048]>; WEB_TASK_POOL_SIZE] =
    [const { StaticCell::new() }; WEB_TASK_POOL_SIZE];

pub struct AppProps;

impl AppBuilder for AppProps {
    type PathRouter = impl picoserve::routing::PathRouter;

    fn build_app(self) -> picoserve::Router<Self::PathRouter> {
        picoserve::Router::new()
            .route(
                "/",
                get_service(picoserve::response::File::html(include_str!(
                    "../static/index.html"
                ))),
            )
            .nest_service(
                "/static",
                const {
                    picoserve::response::Directory {
                        files: &[(
                            "htmx.min.js",
                            picoserve::response::File::with_content_type_and_headers(
                                "application/javascript; charset=utf-8",
                                include_bytes!("../static/htmx.min.js.gz"),
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
