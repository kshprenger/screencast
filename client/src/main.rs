mod gui;

use fs_extra::dir;
use xcap::Monitor;

fn normalized(filename: String) -> String {
    filename.replace(['|', '\\', ':', '/'], "")
}

fn main() -> eframe::Result {
    // let monitors = Monitor::all().unwrap();

    // dir::create_all("target/monitors", true).unwrap();

    // for monitor in monitors {
    //     let image = monitor.capture_image().unwrap();

    //     image
    //         .save(format!(
    //             "target/monitors/monitor-{}.png",
    //             normalized(monitor.name().unwrap())
    //         ))
    //         .unwrap();
    // }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1080.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Screencast",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::<gui::GUI>::default())
        }),
    )
}
