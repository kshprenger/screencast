use eframe::egui;

pub(super) struct GUI {}

impl Default for GUI {
    fn default() -> Self {
        GUI {}
    }
}

impl eframe::App for GUI {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::TopBottomPanel::bottom("buttons").show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.button("Mic");
                ui.button("Camera");
                ui.button("Show entire screen");
            })
        });
    }
}
