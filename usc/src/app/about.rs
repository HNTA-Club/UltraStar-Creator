use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AboutTopic {
    App,
    Bass,
    HtDemucs,
    Ffmpeg,
    YtDlp,
}

pub fn format_bass_version(v: u32) -> String {
    let major = (v >> 24) & 0xFF;
    let minor = (v >> 16) & 0xFF;
    let patch = (v >> 8) & 0xFF;
    let rev = v & 0xFF;
    format!("{major}.{minor}.{patch}.{rev}")
}

pub fn show_menu(ui: &mut egui::Ui, active: &mut Option<AboutTopic>) {
    ui.menu_button("About", |ui| {
        for (topic, label) in [
            (AboutTopic::App, "About UltraStar Creator"),
            (AboutTopic::Bass, "About BASS"),
            (AboutTopic::HtDemucs, "About HTDemucs"),
            (AboutTopic::Ffmpeg, "About FFmpeg"),
            (AboutTopic::YtDlp, "About yt-dlp"),
        ] {
            if topic == AboutTopic::Bass {
                ui.separator();
            }
            if ui.button(label).clicked() {
                *active = Some(topic);
                ui.close();
            }
        }
    });
}

pub fn show_modal(
    ctx: &egui::Context,
    active: &mut Option<AboutTopic>,
    bass_versions: Option<(u32, u32)>,
    ffmpeg_installed: bool,
    ytdlp_installed: bool,
) {
    let Some(topic) = *active else {
        return;
    };

    let title = match topic {
        AboutTopic::App => "About UltraStar Creator",
        AboutTopic::Bass => "About BASS",
        AboutTopic::HtDemucs => "About HTDemucs",
        AboutTopic::Ffmpeg => "About FFmpeg",
        AboutTopic::YtDlp => "About yt-dlp",
    };

    let mut open = true;
    let mut ok_clicked = false;
    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(440.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            match topic {
                AboutTopic::App => show_app_about(ui),
                AboutTopic::Bass => show_bass_about(ui, bass_versions),
                AboutTopic::HtDemucs => show_htdemucs_about(ui),
                AboutTopic::Ffmpeg => show_ffmpeg_about(ui, ffmpeg_installed),
                AboutTopic::YtDlp => show_ytdlp_about(ui, ytdlp_installed),
            }
            ui.add_space(10.0);
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("OK").clicked() {
                    ok_clicked = true;
                }
            });
        });

    if !open || ok_clicked {
        *active = None;
    }
}

fn show_app_about(ui: &mut egui::Ui) {
    ui.heading("UltraStar Creator");
    ui.label(format!("Version: {}", env!("CARGO_PKG_VERSION")));
    ui.add_space(6.0);
    ui.label(
        "Free and open-source karaoke song creation tool for UltraStar Deluxe, rewritten in Rust & egui with built-in Japanese mora romanization and HTDemucs stem splitting.",
    );
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Credits").strong());
    ui.label("Original Authors: Markus Böhning, Marcel Taeumel");
    ui.label("Syllabification Basis: Klafhor & the UltraStar-Deluxe community");
    ui.label("Maintainer (HNTA-Club Fork): takai / HNTA-Club");
    ui.add_space(6.0);
    ui.label("License: GNU General Public License v2 (GPLv2)");
    ui.horizontal_wrapped(|ui| {
        ui.hyperlink_to(
            "HNTA-Club Fork",
            "https://github.com/HNTA-Club/UltraStar-Creator",
        );
        ui.label("•");
        ui.hyperlink_to(
            "Upstream UltraStar-Creator",
            "https://github.com/UltraStar-Deluxe/UltraStar-Creator",
        );
    });
}

fn show_bass_about(ui: &mut egui::Ui, bass_versions: Option<(u32, u32)>) {
    ui.label(egui::RichText::new("BASS Audio Library & BASS_FX Extension").strong());
    ui.add_space(4.0);
    ui.label(
        "Provides cross-platform audio decoding, low-latency playback, BPM detection, and tempo control without pitch shift.",
    );
    ui.add_space(6.0);
    match bass_versions {
        Some((bass, fx)) => {
            ui.label(format!("BASS Version: {}", format_bass_version(bass)));
            ui.label(format!("BASS_FX Version: {}", format_bass_version(fx)));
        }
        None => {
            ui.label("Status: Not loaded");
        }
    }
    ui.add_space(6.0);
    ui.label("License: Proprietary — free for non-commercial use only (Un4seen Developments Ltd.).");
    ui.hyperlink_to("Un4seen Developments — BASS", "https://www.un4seen.com/bass.html");
}

fn show_htdemucs_about(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("HTDemucs (Hybrid Transformer Demucs)").strong());
    ui.add_space(4.0);
    ui.label(
        "Locally separates audio into Vocals and Instrumental stems using Meta Research's HTDemucs model executed via stem-splitter-core and ONNX Runtime.",
    );
    ui.add_space(6.0);
    ui.label("Model weights are fetched on first separation run and cached locally.");
    ui.label("License: MIT License (Demucs & stem-splitter-core)");
    ui.horizontal_wrapped(|ui| {
        ui.hyperlink_to("Demucs (Meta Research)", "https://github.com/facebookresearch/demucs");
        ui.label("•");
        ui.hyperlink_to(
            "stem-splitter-core",
            "https://github.com/stem-splitter/stem-splitter-core",
        );
    });
}

fn show_ffmpeg_about(ui: &mut egui::Ui, installed: bool) {
    ui.label(egui::RichText::new("FFmpeg & FFprobe").strong());
    ui.add_space(4.0);
    ui.label(
        "Handles audio extraction, muted H.264/MP4 video transcoding, and peak-limited instrumental MP3 encoding.",
    );
    ui.add_space(6.0);
    ui.label(if installed {
        "Status: Ready (found on PATH or in ./bin/)"
    } else {
        "Status: Not installed — downloadable on demand from Prepare Media / Split Stems."
    });
    ui.label("Not bundled in release archives; fetched on demand into ./bin/ if missing.");
    ui.label("License: LGPL v2.1+ / GPL v2+ (depending on build configuration)");
    ui.hyperlink_to("FFmpeg Homepage", "https://ffmpeg.org");
}

fn show_ytdlp_about(ui: &mut egui::Ui, installed: bool) {
    ui.label(egui::RichText::new("yt-dlp").strong());
    ui.add_space(4.0);
    ui.label(
        "Optional external command-line media fetcher supporting over 1,000 web platforms and direct media URLs.",
    );
    ui.add_space(6.0);
    ui.label(if installed {
        "Status: Ready (found on PATH or in ./bin/)"
    } else {
        "Status: Not installed — downloadable on demand from Prepare Media / Split Stems."
    });
    ui.label("Not bundled in release archives; fetched on demand into ./bin/ if missing.");
    ui.label("License: The Unlicense (Public Domain)");
    ui.add_space(4.0);
    ui.colored_label(
        egui::Color32::from_rgb(217, 119, 6),
        "Only use with media you own, have permission to download, or that is freely licensed. Users are responsible for complying with applicable copyright laws and site Terms of Service.",
    );
    ui.hyperlink_to("yt-dlp on GitHub", "https://github.com/yt-dlp/yt-dlp");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bass_version_unpacks_nibbles() {
        assert_eq!(format_bass_version(0x0204_1100), "2.4.17.0");
        assert_eq!(format_bass_version(0x0204_0C06), "2.4.12.6");
    }
}
