use eframe::egui;

use crate::media::FolderMedia;
use crate::song::Header;

pub const SONGS_LANGUAGES: &[&str] = &[
    "",
    "Arabic",
    "Chinese",
    "Croatian",
    "Czech",
    "Danish",
    "Dutch",
    "English",
    "Finnish",
    "French",
    "German",
    "Hindi",
    "Italian",
    "Japanese",
    "Korean",
    "Latin",
    "Norwegian",
    "Polish",
    "Portuguese",
    "Portuguese (Brazil)",
    "Romanian",
    "Russian",
    "Slovak",
    "Slowenian",
    "Spanish",
    "Spanish (Latino)",
    "Swedish",
    "Turkish",
];

pub const SONGS_GENRES: &[&str] = &[
    "",
    "A capella",
    "Acid",
    "Acid Jazz",
    "Acid Punk",
    "Acoustic",
    "AlternRock",
    "Alternative",
    "Ambient",
    "Avantgarde",
    "Ballad",
    "Bass",
    "Bebob",
    "Big Band",
    "Bluegrass",
    "Blues",
    "Booty Bass",
    "Cabaret",
    "Celtic",
    "Chamber Music",
    "Chanson",
    "Chorus",
    "Christian Rap",
    "Classic Rock",
    "Classical",
    "Club",
    "Comedy",
    "Country",
    "Cult",
    "Dance",
    "Dance Hall",
    "Darkwave",
    "Death Metal",
    "Disco",
    "Dream",
    "Duet",
    "Easy Listening",
    "Electronic",
    "Ethnic",
    "Euro-House",
    "Euro-Techno",
    "Eurodance",
    "Fast Fusion",
    "Folk",
    "Folk-Rock",
    "Folklore",
    "Freestyle",
    "Funk",
    "Fusion",
    "Game",
    "Gangsta",
    "Gospel",
    "Gothic",
    "Gothic Rock",
    "Grunge",
    "Hard Rock",
    "Hip-Hop",
    "House",
    "Humour",
    "Industrial",
    "Instrumental",
    "Instrumental Pop",
    "Instrumental Rock",
    "Jazz",
    "Jazz+Funk",
    "Jungle",
    "Latin",
    "Lo-Fi",
    "Meditative",
    "Metal",
    "Musical",
    "National Folk",
    "Native American",
    "New Age",
    "New Wave",
    "Noise",
    "Oldies",
    "Opera",
    "Other",
    "Polka",
    "Pop",
    "Pop-Folk",
    "Pop/Funk",
    "Porn Groove",
    "Power Ballad",
    "Pranks",
    "Primus",
    "Progressive Rock",
    "Psychadelic",
    "Psychedelic Rock",
    "Punk",
    "Punk Rock",
    "R&B",
    "Rap",
    "Rave",
    "Reggae",
    "Retro",
    "Revival",
    "Rhythmic Soul",
    "Rock",
    "Rock & Roll",
    "Samba",
    "Satire",
    "Schlager",
    "Showtunes",
    "Ska",
    "Slow Jam",
    "Slow Rock",
    "Sonata",
    "Soul",
    "Sound Clip",
    "Soundtrack",
    "Southern Rock",
    "Space",
    "Speech",
    "Swing",
    "Symphonic Rock",
    "Symphony",
    "Tango",
    "Techno",
    "Techno-Industrial",
    "Top",
    "Trailer",
    "Trance",
    "Tribal",
    "Trip-Hop",
    "Vocal",
];

#[derive(Default)]
pub struct HeaderActions {
    pub browse_audio: bool,
    pub prepare_stems: bool,
    pub refresh_media: bool,
    pub changed: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    header: &mut Header,
    is_duet: &mut bool,
    singer_p1: &mut String,
    singer_p2: &mut String,
    media: &FolderMedia,
    has_audio: bool,
    locked: bool,
) -> HeaderActions {
    let mut actions = HeaderActions::default();

    ui.heading("Song Header");
    ui.add_space(4.0);

    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(!locked, egui::Button::new("Browse Audio..."))
            .clicked()
        {
            actions.browse_audio = true;
        }
        if ui
            .add_enabled(!locked, egui::Button::new("Prepare Media / Stems…"))
            .on_hover_text("Split video/audio into Muted MP4, Vocals, and Instrumental stems (HTDemucs)")
            .clicked()
        {
            actions.prepare_stems = true;
        }
        if ui
            .add_enabled(has_audio && !locked, egui::Button::new("Refresh (F5)"))
            .clicked()
        {
            actions.refresh_media = true;
        }
    });
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
        .id_salt("header_scroll")
        .show(ui, |ui| {
            ui.add_enabled_ui(has_audio && !locked, |ui| {
                egui::Grid::new("header_grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        actions.changed |= text_row(ui, "Title:", &mut header.title);
                        actions.changed |= text_row(ui, "Artist:", &mut header.artist);

                        ui.label("Duet:");
                        if ui.checkbox(is_duet, "Duet song").changed() {
                            actions.changed = true;
                        }
                        ui.end_row();

                        if *is_duet {
                            actions.changed |= text_row(ui, "Singer P1:", singer_p1);
                            actions.changed |= text_row(ui, "Singer P2:", singer_p2);
                        }

                        actions.changed |=
                            combo_preset_row(ui, "Language:", &mut header.language, SONGS_LANGUAGES);
                        actions.changed |= text_row(ui, "Edition:", &mut header.edition);
                        actions.changed |=
                            combo_preset_row(ui, "Genre:", &mut header.genre, SONGS_GENRES);
                        actions.changed |= text_row(ui, "Year:", &mut header.year);
                        actions.changed |= text_row(ui, "Creator:", &mut header.creator);

                        ui.label("MP3:");
                        ui.add(egui::TextEdit::singleline(&mut header.mp3).interactive(false));
                        ui.end_row();

                        actions.changed |=
                            media_combo_row(ui, "Instrumental:", &mut header.instrumental, &media.audios);
                        actions.changed |=
                            media_combo_row(ui, "Vocals:", &mut header.vocals, &media.audios);
                        actions.changed |=
                            media_combo_row(ui, "Cover:", &mut header.cover, &media.images);
                        actions.changed |=
                            media_combo_row(ui, "Background:", &mut header.background, &media.images);
                        actions.changed |=
                            media_combo_row(ui, "Video:", &mut header.video, &media.videos);

                        ui.label("BPM:");
                        if ui
                            .add(
                                egui::DragValue::new(&mut header.bpm)
                                    .range(1.0..=1500.0)
                                    .speed(0.5)
                                    .max_decimals(1),
                            )
                            .changed()
                        {
                            actions.changed = true;
                        }
                        ui.end_row();
                    });
            });
        });

    actions
}

fn text_row(ui: &mut egui::Ui, label: &str, value: &mut String) -> bool {
    ui.label(label);
    let changed = ui
        .add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY))
        .changed();
    ui.end_row();
    changed
}

fn combo_preset_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    presets: &[&str],
) -> bool {
    ui.label(label);
    let mut changed = false;
    let display = if value.is_empty() {
        "(none)"
    } else {
        value.as_str()
    };
    egui::ComboBox::from_id_salt(label)
        .selected_text(display)
        .width(160.0)
        .show_ui(ui, |ui| {
            if !value.is_empty() && !presets.contains(&value.as_str()) {
                let custom = value.clone();
                ui.selectable_value(value, custom.clone(), &custom);
            }
            for &item in presets {
                let text = if item.is_empty() { "(none)" } else { item };
                if ui.selectable_value(value, item.to_string(), text).changed() {
                    changed = true;
                }
            }
        });
    ui.end_row();
    changed
}

fn media_combo_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    items: &[String],
) -> bool {
    ui.label(label);
    let mut changed = false;
    let display = if value.is_empty() {
        "(none)"
    } else {
        value.as_str()
    };
    egui::ComboBox::from_id_salt(label)
        .selected_text(display)
        .width(160.0)
        .show_ui(ui, |ui| {
            if ui.selectable_value(value, String::new(), "(none)").changed() {
                changed = true;
            }
            for item in items {
                if ui.selectable_value(value, item.clone(), item).changed() {
                    changed = true;
                }
            }
        });
    ui.end_row();
    changed
}
