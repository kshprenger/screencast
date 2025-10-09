mod state;

use eframe::egui;
use tokio::sync::mpsc;

pub(super) struct GUI {
    frame_rx: Option<mpsc::Receiver<super::video::Frame>>,
}

impl Default for GUI {
    fn default() -> Self {
        GUI { frame_rx: None }
    }
}

impl eframe::App for GUI {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::TopBottomPanel::bottom("buttons").show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.button("Show entire screen");
            })
        });
    }
}
