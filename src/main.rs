mod app;
mod data;
mod firewall;
mod map;
mod sdr;
mod settings;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1220.0, 760.0]),
        ..Default::default()
    };

    eframe::run_native(
        "CS2 Server Chooser",
        options,
        Box::new(|cc| Ok(Box::new(app::ServerChooserApp::new(cc)))),
    )
}
