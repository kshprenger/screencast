use std::sync::Arc;

use clap::Parser;
use tracing_subscriber::FmtSubscriber;

mod capture;
mod cli;
mod gui;
mod network;

fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .with_line_number(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let args = cli::Cli::parse();

    let webrtc = network::WebrtcNetwork::new_shared(args.address, args.port);
    let webrtc_gui = Arc::clone(&webrtc);

    let async_rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap(),
    );

    let async_rt_gui = Arc::clone(&async_rt);

    std::thread::spawn(move || {
        async_rt.block_on(webrtc.join());
    });

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

            Ok(Box::new(gui::GUI::new(webrtc_gui, async_rt_gui)))
        }),
    )
    .unwrap();
}
