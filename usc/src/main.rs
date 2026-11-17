#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let arg = std::env::args().nth(1);
    if arg.as_deref() == Some("--check-bass") {
        match usc::audio::BassLibs::load() {
            Ok(libs) => {
                let (bass, fx) = libs.versions();
                println!("BASS 0x{bass:08x} BASS_FX 0x{fx:08x}");
                return Ok(());
            }
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    }

    let initial_audio = arg.map(PathBuf::from).filter(|p| p.is_file());
    let options = eframe::NativeOptions {
        vsync: false,
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1150.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "UltraStar Creator",
        options,
        Box::new(move |cc| Ok(Box::new(usc::app::UscApp::new(cc, initial_audio)))),
    )
}
