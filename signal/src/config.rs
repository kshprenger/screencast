use axum::http::Method;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::FmtSubscriber;

#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
pub(super) struct Args {
    #[arg(short, long, default_value_t = 35080)]
    pub(super) port: u16,
}

pub(super) fn setup_logging() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

pub(super) fn setup_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Method::GET)
        .allow_headers(Any)
}
