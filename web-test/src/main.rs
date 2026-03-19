#![feature(impl_trait_in_assoc_type)]
use std::net::SocketAddr;

#[path = "../../src/storage.rs"]
pub mod storage;

#[path = "../../src/server.rs"]
pub mod server;

use picoserve::AppBuilder;
use server::AppProps;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let port = 8000;
    log::info!("Starting web-test server on http://localhost:{}", port);

    let props = AppProps {};

    let app = picoserve::make_static!(
        picoserve::Router<<AppProps as AppBuilder>::PathRouter>,
        props.build_app()
    );
    let config = picoserve::make_static!(picoserve::Config, picoserve::Config::const_default());

    let app: &'static _ = &*app;
    let config: &'static _ = &*config;

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await?;

    tokio::task::LocalSet::new()
        .run_until(async {
            loop {
                let (stream, remote_address) = listener.accept().await.unwrap();
                println!("Connection from {}", remote_address);

                tokio::task::spawn_local(async move {
                    let mut buffer = [0; 2048];

                    match picoserve::Server::new_tokio(app, config, &mut buffer)
                        .serve(stream)
                        .await
                    {
                        Ok(_) => {}
                        Err(err) => println!("Error serving connection: {:?}", err),
                    }
                });
            }
        })
        .await;

    Ok(())
}
