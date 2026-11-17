fn main() {
    if std::env::args().nth(1).as_deref() == Some("--check-bass") {
        match usc::audio::BassLibs::load() {
            Ok(libs) => {
                let (bass, fx) = libs.versions();
                println!("BASS 0x{bass:08x} BASS_FX 0x{fx:08x}");
            }
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    }
}
