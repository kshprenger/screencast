use std::sync::Arc;

use clap::Parser;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::FmtSubscriber;

mod cli;
mod gui;
mod transport;
mod video;

fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let args = cli::Cli::parse();

    let webrtc = transport::WebrtcTransport::new_shared(args.address, args.port);
    let webrtc_gui = Arc::clone(&webrtc);

    let rt = tokio::runtime::Builder::new_multi_thread().build().unwrap();
    rt.block_on(webrtc.join_peer_network(CancellationToken::new()));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1080.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Screencast",
        options,
        Box::new(|cc| {
            // This gives us image support
            egui_extras::install_image_loaders(&cc.egui_ctx);

            Ok(Box::new(gui::GUI::new(webrtc_gui)))
        }),
    )
    .unwrap();
}
