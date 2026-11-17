pub mod header;
pub mod lyrics;
pub mod tap;

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;

use crate::audio::AudioEngine;
use crate::lyrics::{Role, clean, syllabify, syllables};
use crate::media::{FolderMedia, detect_language, dictionaries_dir, read_tags, scan_folder};
use crate::song::{Header, render};
use crate::tapping::{Filter, Note, Session};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

impl ThemeMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    NoSong,
    Initialized,
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewState {
    Unavailable,
    Stopped,
    Playing,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinishModal {
    DuetPass { tapped: usize, total: usize },
    SongComplete,
}

pub struct UscApp {
    audio: Result<AudioEngine, String>,
    audio_path: Option<PathBuf>,
    duration_s: f64,
    header: Header,
    is_duet: bool,
    singer_p1: String,
    singer_p2: String,
    folder_media: FolderMedia,
    lyrics_text: String,
    lyrics_cursor: (usize, usize),
    pending_cursor: Option<(usize, usize)>,
    session: Option<Session>,
    kept_notes: Vec<Note>,
    filter: Filter,
    output_text: String,
    state: PlaybackState,
    preview_state: PreviewState,
    speed_pct: i32,
    input_font_size: f32,
    output_font_size: f32,
    theme: ThemeMode,
    default_dir: Option<PathBuf>,
    mouse_tapping: bool,
    status: String,
    error_banner: Option<String>,
    finish_modal: Option<FinishModal>,
}

impl UscApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial_audio: Option<PathBuf>) -> Self {
        let storage = cc.storage;
        let theme = storage
            .and_then(|s| s.get_string("theme"))
            .map_or(ThemeMode::System, |s| ThemeMode::from_str(&s));
        let creator = storage
            .and_then(|s| s.get_string("creator"))
            .unwrap_or_default();
        let default_dir = storage
            .and_then(|s| s.get_string("default_dir"))
            .map(PathBuf::from);
        let input_font_size = storage
            .and_then(|s| s.get_string("input_font_size"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(13.0_f32)
            .clamp(8.0, 24.0);
        let output_font_size = storage
            .and_then(|s| s.get_string("output_font_size"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(12.0_f32)
            .clamp(8.0, 24.0);
        let speed_pct = storage
            .and_then(|s| s.get_string("speed_pct"))
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(100)
            .clamp(50, 150);

        let audio = AudioEngine::new();
        let error_banner = audio.as_ref().err().cloned();

        let mut app = Self {
            audio,
            audio_path: None,
            duration_s: 0.0,
            header: Header {
                creator,
                bpm: 400.0,
                ..Default::default()
            },
            is_duet: false,
            singer_p1: String::new(),
            singer_p2: String::new(),
            folder_media: FolderMedia::default(),
            lyrics_text: String::new(),
            lyrics_cursor: (0, 0),
            pending_cursor: None,
            session: None,
            kept_notes: Vec::new(),
            filter: Filter::All,
            output_text: String::new(),
            state: PlaybackState::NoSong,
            preview_state: PreviewState::Unavailable,
            speed_pct,
            input_font_size,
            output_font_size,
            theme,
            default_dir,
            mouse_tapping: false,
            status: "Open or drop an audio file to begin.".into(),
            error_banner,
            finish_modal: None,
        };

        if let Some(path) = initial_audio {
            app.load_audio(path);
        }
        app
    }

    fn effective_header(&self) -> Header {
        let mut h = self.header.clone();
        h.duet = self
            .is_duet
            .then(|| (self.singer_p1.clone(), self.singer_p2.clone()));
        h
    }

    fn refresh_output(&mut self) {
        if self.audio_path.is_none() {
            self.output_text.clear();
            return;
        }
        let h = self.effective_header();
        if let Some(session) = &self.session {
            self.output_text = render(&h, session.syllables(), &session.notes());
        } else {
            self.output_text = render(&h, &[], &[]);
        }
    }

    fn load_audio(&mut self, path: PathBuf) {
        if !path.is_file() {
            self.status = format!("Audio file not found: {}", path.display());
            return;
        }
        if let Ok(engine) = &mut self.audio {
            engine.stop();
        }
        if let Some(parent) = path.parent() {
            self.default_dir = Some(parent.to_path_buf());
        }

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();

        let tags = read_tags(&path);
        let media = scan_folder(&path);

        let creator = self.header.creator.clone();
        let mut bpm = 400.0;
        let mut duration_s = 0.0;
        if let Ok(engine) = &self.audio {
            match engine.analyze(&path) {
                Ok(info) => {
                    bpm = info.bpm;
                    duration_s = info.duration_s;
                }
                Err(err) => {
                    self.error_banner = Some(err);
                }
            }
        }

        self.header = Header {
            title: tags.title,
            artist: tags.artist,
            genre: tags.genre,
            year: tags.year,
            creator,
            mp3: file_name,
            instrumental: media.instrumental.clone().unwrap_or_default(),
            vocals: media.vocals.clone().unwrap_or_default(),
            cover: media.cover.clone().unwrap_or_default(),
            background: media.background.clone().unwrap_or_default(),
            video: media.video.clone().unwrap_or_default(),
            bpm,
            ..Default::default()
        };
        self.folder_media = media;
        self.audio_path = Some(path);
        self.duration_s = duration_s;
        self.session = None;
        self.kept_notes.clear();
        self.state = PlaybackState::Initialized;
        self.preview_state = if self.audio.is_ok() {
            PreviewState::Stopped
        } else {
            PreviewState::Unavailable
        };
        self.finish_modal = None;
        self.status = "Audio loaded. Paste or open lyrics, then press Play (Ctrl+P).".into();
        self.refresh_output();
    }

    fn refresh_media(&mut self) {
        let Some(path) = &self.audio_path else {
            return;
        };
        let media = scan_folder(path);
        let keep_or = |current: &str, list: &[String], fallback: &Option<String>| -> String {
            if !current.is_empty() && list.iter().any(|item| item == current) {
                current.to_string()
            } else {
                fallback.clone().unwrap_or_default()
            }
        };
        self.header.cover = keep_or(&self.header.cover, &media.images, &media.cover);
        self.header.background =
            keep_or(&self.header.background, &media.images, &media.background);
        self.header.video = keep_or(&self.header.video, &media.videos, &media.video);
        self.header.instrumental = keep_or(
            &self.header.instrumental,
            &media.audios,
            &media.instrumental,
        );
        self.header.vocals = keep_or(&self.header.vocals, &media.audios, &media.vocals);
        self.folder_media = media;
        self.refresh_output();
        self.status = "Media folder refreshed.".into();
    }

    fn handle_new_lyrics(&mut self, raw: String) {
        if let Some(lang) = detect_language(&raw) {
            self.header.language = lang.to_string();
        }
        let cleaned = clean(&raw);
        self.lyrics_text = cleaned;
        self.syllabify_current();
        self.refresh_output();
    }

    fn syllabify_current(&mut self) {
        if self.header.language.trim().is_empty() {
            if let Some(lang) = detect_language(&self.lyrics_text) {
                self.header.language = lang.to_string();
            } else {
                self.status = "Select a song Language first to syllabify.".into();
                return;
            }
        }
        let Some(dir) = dictionaries_dir() else {
            self.status = "Syllabification dictionaries folder not found.".into();
            return;
        };
        let dict_path = dir.join(format!("{}.txt", self.header.language.trim()));
        match std::fs::read_to_string(&dict_path) {
            Ok(dict) => {
                self.lyrics_text = syllabify(&self.lyrics_text, &dict);
                self.status = format!("Syllabified using {}.txt.", self.header.language.trim());
            }
            Err(_) => {
                self.status = format!(
                    "No syllabification dictionary for '{}'.",
                    self.header.language.trim()
                );
            }
        }
    }

    fn toggle_play_pause(&mut self) {
        let Some(audio_path) = self.audio_path.clone() else {
            return;
        };
        match self.state {
            PlaybackState::Initialized => {
                let cleaned = clean(&self.lyrics_text);
                if cleaned.is_empty() {
                    self.status = "Enter song lyrics before tapping.".into();
                    return;
                }
                self.lyrics_text = cleaned.clone();
                let syls = syllables(&cleaned, self.is_duet);
                if syls.is_empty() {
                    self.status = "No syllables found in lyrics.".into();
                    return;
                }
                let session = Session::new(syls, self.filter, self.kept_notes.clone());
                if session.current().is_none() {
                    self.status = "No syllables match the selected singer filter.".into();
                    return;
                }

                let Ok(engine) = &mut self.audio else {
                    return;
                };
                if let Err(err) = engine.open(&audio_path) {
                    self.error_banner = Some(err);
                    return;
                }
                let _ = engine.set_tempo((self.speed_pct - 100) as f32);
                if let Err(err) = engine.play() {
                    self.error_banner = Some(err);
                    return;
                }

                self.session = Some(session);
                self.preview_state = PreviewState::Stopped;
                self.state = PlaybackState::Playing;
                self.finish_modal = None;
                self.refresh_output();
                self.status = "Tapping: hold Space or V for each syllable.".into();
            }
            PlaybackState::Playing => {
                if let Ok(engine) = &self.audio {
                    engine.pause();
                }
                self.mouse_tapping = false;
                self.state = PlaybackState::Paused;
                self.status = "Paused.".into();
            }
            PlaybackState::Paused => {
                if let Ok(engine) = &self.audio
                    && let Err(err) = engine.resume()
                {
                    self.error_banner = Some(err);
                    return;
                }
                self.state = PlaybackState::Playing;
                self.status = "Tapping resumed.".into();
            }
            PlaybackState::NoSong | PlaybackState::Stopped => {}
        }
    }

    fn stop_tapping(&mut self) {
        if !matches!(self.state, PlaybackState::Playing | PlaybackState::Paused) {
            return;
        }
        if let Ok(engine) = &mut self.audio {
            engine.stop();
        }
        self.mouse_tapping = false;
        if let Some(session) = &self.session {
            self.kept_notes = session.notes();
        }
        self.state = PlaybackState::Stopped;
        self.refresh_output();
        self.status = "Stopped.".into();
    }

    fn reset_tapping(&mut self) {
        if let Ok(engine) = &mut self.audio {
            engine.stop();
        }
        self.mouse_tapping = false;
        self.session = None;
        self.kept_notes.clear();
        self.finish_modal = None;
        if self.audio_path.is_some() {
            self.state = PlaybackState::Initialized;
            self.preview_state = if self.audio.is_ok() {
                PreviewState::Stopped
            } else {
                PreviewState::Unavailable
            };
        } else {
            self.state = PlaybackState::NoSong;
        }
        self.refresh_output();
        self.status = "Reset tapping progress.".into();
    }

    fn on_tap_press(&mut self) {
        if self.state != PlaybackState::Playing {
            return;
        }
        let Ok(engine) = &self.audio else {
            return;
        };
        let t_ms = engine.position_ms();
        if let Some(session) = &mut self.session {
            session.press(t_ms);
        }
    }

    fn on_tap_release(&mut self) {
        if self.state != PlaybackState::Playing {
            return;
        }
        let Ok(engine) = &self.audio else {
            return;
        };
        let t_ms = engine.position_ms();
        let finished = if let Some(session) = &mut self.session {
            session.release(t_ms)
        } else {
            false
        };
        self.refresh_output();

        if finished {
            let (total_tapped, total_syls) = self
                .session
                .as_ref()
                .map_or((0, 0), |s| (s.notes().len(), s.syllables().len()));
            self.stop_tapping();
            if self.is_duet && total_tapped < total_syls {
                self.finish_modal = Some(FinishModal::DuetPass {
                    tapped: total_tapped,
                    total: total_syls,
                });
            } else {
                self.finish_modal = Some(FinishModal::SongComplete);
            }
        }
    }

    fn on_undo(&mut self) {
        if !matches!(self.state, PlaybackState::Playing | PlaybackState::Paused) {
            return;
        }
        let Some(session) = &mut self.session else {
            return;
        };
        let seek_ms = session.undo();
        if let Ok(engine) = &self.audio {
            engine.seek_ms(seek_ms);
        }
        self.refresh_output();
    }

    fn on_undo_row(&mut self, pause_after: bool) {
        if !matches!(self.state, PlaybackState::Playing | PlaybackState::Paused) {
            return;
        }
        let Some(session) = &mut self.session else {
            return;
        };
        let seek_ms = session.undo_row();
        if let Ok(engine) = &self.audio {
            engine.seek_ms(seek_ms);
            if pause_after && self.state == PlaybackState::Playing {
                engine.pause();
                self.state = PlaybackState::Paused;
            }
        }
        self.refresh_output();
    }

    fn toggle_preview(&mut self) {
        if matches!(self.state, PlaybackState::Playing | PlaybackState::Paused) {
            return;
        }
        let Some(audio_path) = self.audio_path.clone() else {
            return;
        };
        let Ok(engine) = &mut self.audio else {
            return;
        };
        match self.preview_state {
            PreviewState::Stopped => {
                if let Err(err) = engine.open(&audio_path) {
                    self.error_banner = Some(err);
                    return;
                }
                let _ = engine.set_tempo(0.0);
                if let Err(err) = engine.play() {
                    self.error_banner = Some(err);
                    return;
                }
                self.preview_state = PreviewState::Playing;
            }
            PreviewState::Playing => {
                engine.pause();
                self.preview_state = PreviewState::Paused;
            }
            PreviewState::Paused => {
                if let Err(err) = engine.resume() {
                    self.error_banner = Some(err);
                    return;
                }
                self.preview_state = PreviewState::Playing;
            }
            PreviewState::Unavailable => {}
        }
    }

    fn open_audio_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new()
            .add_filter("Audio files", &["mp3", "ogg", "m4a"]);
        if let Some(dir) = &self.default_dir {
            dlg = dlg.set_directory(dir);
        }
        if let Some(path) = dlg.pick_file() {
            self.load_audio(path);
        }
    }

    fn open_lyrics_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new().add_filter("Text files", &["txt"]);
        if let Some(dir) = &self.default_dir {
            dlg = dlg.set_directory(dir);
        }
        if let Some(path) = dlg.pick_file() {
            self.load_lyrics_file(&path);
        }
    }

    fn load_lyrics_file(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                self.handle_new_lyrics(text);
            }
            Err(err) => {
                self.status = format!("Failed to read {}: {err}", path.display());
            }
        }
    }

    fn save_output_dialog(&mut self) {
        if self.audio_path.is_none() {
            return;
        }
        self.refresh_output();
        let artist = if self.header.artist.trim().is_empty() {
            "Unknown Artist"
        } else {
            self.header.artist.trim()
        };
        let title = if self.header.title.trim().is_empty() {
            "Unknown Title"
        } else {
            self.header.title.trim()
        };
        let suggested = sanitize_filename(&format!("{artist} - {title}.txt"));
        let mut dlg = rfd::FileDialog::new()
            .add_filter("UltraStar text", &["txt"])
            .set_file_name(&suggested);
        if let Some(dir) = self.audio_path.as_ref().and_then(|p| p.parent()) {
            dlg = dlg.set_directory(dir);
        }
        if let Some(path) = dlg.save_file() {
            match std::fs::write(&path, self.output_text.as_bytes()) {
                Ok(()) => self.status = format!("Saved {}", path.display()),
                Err(err) => self.status = format!("Save failed: {err}"),
            }
        }
    }

    fn apply_role_assignment(&mut self, role: Role) {
        if !self.is_duet || matches!(self.state, PlaybackState::Playing | PlaybackState::Paused) {
            return;
        }
        let (updated, sel) = lyrics::toggle_role_on_lines(&self.lyrics_text, self.lyrics_cursor, role);
        self.lyrics_text = updated;
        self.lyrics_cursor = sel;
        self.pending_cursor = Some(sel);
        self.refresh_output();
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let locked = matches!(self.state, PlaybackState::Playing | PlaybackState::Paused);
        let text_focused = ctx.wants_keyboard_input();

        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            let egui::Event::Key {
                key,
                pressed,
                repeat,
                modifiers,
                ..
            } = ev
            else {
                continue;
            };

            if pressed && !repeat {
                if key == egui::Key::Q && modifiers.ctrl && !modifiers.alt {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                if key == egui::Key::O && modifiers.ctrl && !modifiers.alt && !locked {
                    self.open_audio_dialog();
                    continue;
                }
                if key == egui::Key::S && modifiers.ctrl && !modifiers.alt {
                    self.save_output_dialog();
                    continue;
                }
                if key == egui::Key::F5 && !locked {
                    self.refresh_media();
                    continue;
                }
                if key == egui::Key::P && modifiers.ctrl && !modifiers.alt {
                    self.toggle_play_pause();
                    continue;
                }
                if key == egui::Key::R
                    && modifiers.ctrl
                    && !modifiers.alt
                    && matches!(
                        self.state,
                        PlaybackState::Initialized | PlaybackState::Paused | PlaybackState::Stopped
                    )
                {
                    self.reset_tapping();
                    continue;
                }
                if modifiers.alt && !modifiers.ctrl && !modifiers.shift && self.is_duet && !locked
                {
                    match key {
                        egui::Key::Num1 => self.apply_role_assignment(Role::P1),
                        egui::Key::Num2 => self.apply_role_assignment(Role::P2),
                        egui::Key::Num3 => self.apply_role_assignment(Role::Both),
                        _ => {}
                    }
                    continue;
                }
            }

            if text_focused {
                continue;
            }

            if pressed && !repeat {
                if key == egui::Key::Z && modifiers.ctrl && !modifiers.alt {
                    self.on_undo_row(true);
                    continue;
                }
                if modifiers.any() {
                    continue;
                }
                match key {
                    egui::Key::Z => self.on_undo_row(false),
                    egui::Key::X => self.on_undo(),
                    egui::Key::S => self.stop_tapping(),
                    egui::Key::Space | egui::Key::V => {
                        if self.state == PlaybackState::Playing {
                            self.on_tap_press();
                        } else if matches!(
                            self.state,
                            PlaybackState::Initialized | PlaybackState::Paused
                        ) {
                            self.toggle_play_pause();
                        }
                    }
                    _ => {}
                }
            } else if !pressed
                && !repeat
                && !modifiers.any()
                && matches!(key, egui::Key::Space | egui::Key::V)
                && self.state == PlaybackState::Playing
            {
                self.on_tap_release();
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        if matches!(self.state, PlaybackState::Playing | PlaybackState::Paused) {
            return;
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            if let Some(path) = file.path {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase)
                    .unwrap_or_default();
                if matches!(ext.as_str(), "mp3" | "ogg" | "m4a") {
                    self.load_audio(path);
                } else if ext == "txt" {
                    self.load_lyrics_file(&path);
                }
            }
        }
    }
}

impl eframe::App for UscApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("theme", self.theme.as_str().into());
        storage.set_string("creator", self.header.creator.clone());
        if let Some(dir) = &self.default_dir {
            storage.set_string("default_dir", dir.to_string_lossy().into_owned());
        }
        storage.set_string("input_font_size", self.input_font_size.to_string());
        storage.set_string("output_font_size", self.output_font_size.to_string());
        storage.set_string("speed_pct", self.speed_pct.to_string());
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        match self.theme {
            ThemeMode::System => {}
            ThemeMode::Light => ctx.set_visuals(egui::Visuals::light()),
            ThemeMode::Dark => ctx.set_visuals(egui::Visuals::dark()),
        }

        // Poll stream playback state
        let mut pos_s = 0.0;
        if let Ok(engine) = &self.audio {
            if self.state == PlaybackState::Playing {
                if engine.is_playing() {
                    pos_s = engine.position_ms() / 1000.0;
                    ctx.request_repaint_after(Duration::from_millis(30));
                } else {
                    self.stop_tapping();
                }
            } else if self.state == PlaybackState::Paused {
                pos_s = engine.position_ms() / 1000.0;
            } else if self.preview_state == PreviewState::Playing {
                if engine.is_playing() {
                    pos_s = engine.position_ms() / 1000.0;
                    ctx.request_repaint_after(Duration::from_millis(30));
                } else {
                    self.preview_state = PreviewState::Stopped;
                }
            } else if self.preview_state == PreviewState::Paused {
                pos_s = engine.position_ms() / 1000.0;
            }
        }

        self.handle_dropped_files(ctx);
        self.handle_shortcuts(ctx);

        let locked = matches!(self.state, PlaybackState::Playing | PlaybackState::Paused);

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .add_enabled(!locked, egui::Button::new("Open Audio... (Ctrl+O)"))
                        .clicked()
                    {
                        ui.close();
                        self.open_audio_dialog();
                    }
                    if ui
                        .add_enabled(!locked, egui::Button::new("Open Lyrics..."))
                        .clicked()
                    {
                        ui.close();
                        self.open_lyrics_dialog();
                    }
                    if ui
                        .add_enabled(
                            self.audio_path.is_some(),
                            egui::Button::new("Save UltraStar File... (Ctrl+S)"),
                        )
                        .clicked()
                    {
                        ui.close();
                        self.save_output_dialog();
                    }
                    if ui
                        .add_enabled(
                            self.audio_path.is_some() && !locked,
                            egui::Button::new("Refresh Media (F5)"),
                        )
                        .clicked()
                    {
                        ui.close();
                        self.refresh_media();
                    }
                    ui.separator();
                    if ui.button("Quit (Ctrl+Q)").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Options", |ui| {
                    ui.menu_button("Theme", |ui| {
                        for (mode, label) in [
                            (ThemeMode::System, "Follow System"),
                            (ThemeMode::Light, "Light"),
                            (ThemeMode::Dark, "Dark"),
                        ] {
                            if ui.selectable_value(&mut self.theme, mode, label).clicked() {
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    ui.label("Playback Speed:");
                    if ui
                        .add(egui::Slider::new(&mut self.speed_pct, 50..=150).suffix("%"))
                        .changed()
                        && let Ok(engine) = &self.audio
                        && locked
                    {
                        let _ = engine.set_tempo((self.speed_pct - 100) as f32);
                    }
                    if ui.button("Reset Speed (100%)").clicked() {
                        self.speed_pct = 100;
                        if let Ok(engine) = &self.audio
                            && locked
                        {
                            let _ = engine.set_tempo(0.0);
                        }
                    }
                });
            });
        });

        if let Some(err) = self.error_banner.clone() {
            egui::TopBottomPanel::top("error_banner").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(220, 38, 38), format!("Audio error: {err}"));
                    if ui.button("Dismiss").clicked() {
                        self.error_banner = None;
                    }
                });
            });
        }

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                if let Some(session) = &self.session {
                    let (done, total) = session.progress();
                    let frac = if total > 0 {
                        done as f32 / total as f32
                    } else {
                        0.0
                    };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(
                            egui::ProgressBar::new(frac)
                                .desired_width(180.0)
                                .text(format!("{done}/{total}")),
                        );
                    });
                }
            });
        });

        egui::SidePanel::left("header_panel")
            .default_width(290.0)
            .min_width(240.0)
            .show(ctx, |ui| {
                let act = header::show(
                    ui,
                    &mut self.header,
                    &mut self.is_duet,
                    &mut self.singer_p1,
                    &mut self.singer_p2,
                    &self.folder_media,
                    self.audio_path.is_some(),
                    locked,
                );
                if act.browse_audio {
                    self.open_audio_dialog();
                }
                if act.refresh_media {
                    self.refresh_media();
                }
                if act.changed {
                    self.refresh_output();
                }
            });

        egui::SidePanel::right("tap_panel")
            .default_width(360.0)
            .min_width(300.0)
            .show(ctx, |ui| {
                let tap_pos_s = if locked { pos_s } else { 0.0 };
                let act = tap::show(
                    ui,
                    self.state,
                    self.audio.is_ok(),
                    self.audio_path.is_some(),
                    self.is_duet,
                    tap_pos_s,
                    self.duration_s,
                    &mut self.speed_pct,
                    self.filter,
                    self.session.as_ref(),
                    &mut self.mouse_tapping,
                    &self.output_text,
                    &mut self.output_font_size,
                );
                if act.toggle_play {
                    self.toggle_play_pause();
                }
                if act.stop {
                    self.stop_tapping();
                }
                if act.reset {
                    self.reset_tapping();
                }
                if let Some(s) = act.seek_s
                    && let Ok(engine) = &self.audio
                {
                    engine.seek_ms(s * 1000.0);
                }
                if act.speed_changed
                    && let Ok(engine) = &self.audio
                    && locked
                {
                    let _ = engine.set_tempo((self.speed_pct - 100) as f32);
                }
                if let Some(new_filter) = act.filter_changed {
                    self.filter = new_filter;
                    if self.state == PlaybackState::Stopped {
                        self.state = PlaybackState::Initialized;
                    }
                }
                if act.tap_press {
                    self.on_tap_press();
                }
                if act.tap_release {
                    self.on_tap_release();
                }
                if act.undo {
                    self.on_undo();
                }
                if act.undo_row {
                    self.on_undo_row(false);
                }
                if act.save {
                    self.save_output_dialog();
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let prev_pos_s = if !locked { pos_s } else { 0.0 };
            let act = lyrics::show(
                ui,
                &mut self.lyrics_text,
                &mut self.input_font_size,
                &mut self.lyrics_cursor,
                &mut self.pending_cursor,
                self.is_duet,
                locked,
                self.audio_path.is_some(),
                self.audio.is_ok(),
                self.preview_state,
                prev_pos_s,
                self.duration_s,
            );
            if act.open_file {
                self.open_lyrics_dialog();
            }
            if act.paste
                && let Ok(mut cb) = arboard::Clipboard::new()
                && let Ok(text) = cb.get_text()
            {
                self.handle_new_lyrics(text);
            }
            if act.syllabify {
                self.syllabify_current();
            }
            if act.preview_toggle {
                self.toggle_preview();
            }
            if let Some(s) = act.preview_seek_s
                && let Ok(engine) = &self.audio
            {
                engine.seek_ms(s * 1000.0);
            }
            if let Some(role) = act.assign_role {
                self.apply_role_assignment(role);
            }
        });

        if let Some(modal) = self.finish_modal {
            egui::Window::new(match modal {
                FinishModal::DuetPass { .. } => "Pass Finished",
                FinishModal::SongComplete => "Tapping Finished",
            })
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| match modal {
                FinishModal::DuetPass { tapped, total } => {
                    ui.label(format!(
                        "Finished tapping this pass ({tapped}/{total} syllables tapped overall).\nSwitch singer filter to continue tapping another singer, or save."
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Continue With Another Singer").clicked() {
                            self.state = PlaybackState::Initialized;
                            self.finish_modal = None;
                        }
                        if ui.button("Save UltraStar File").clicked() {
                            self.finish_modal = None;
                            self.save_output_dialog();
                        }
                    });
                }
                FinishModal::SongComplete => {
                    ui.label("Congratulations! You successfully tapped this song.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Try Again (Reset)").clicked() {
                            self.finish_modal = None;
                            self.reset_tapping();
                        }
                        if ui.button("Save UltraStar File").clicked() {
                            self.finish_modal = None;
                            self.save_output_dialog();
                        }
                    });
                }
            });
        }
    }
}

pub fn is_play_enabled(state: PlaybackState) -> bool {
    matches!(
        state,
        PlaybackState::Initialized | PlaybackState::Playing | PlaybackState::Paused
    )
}

pub fn format_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "0:00".into();
    }
    let total = seconds.floor() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_formats_and_clamps() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(5.9), "0:05");
        assert_eq!(format_time(125.9), "2:05");
        assert_eq!(format_time(-3.0), "0:00");
        assert_eq!(format_time(f64::NAN), "0:00");
    }

    #[test]
    fn sanitize_filename_replaces_illegal_chars() {
        assert_eq!(
            sanitize_filename("A/B: C*D? \"E\"<F>|G\\H.txt"),
            "A_B_ C_D_ _E__F__G_H.txt"
        );
    }

    #[test]
    fn play_disabled_when_stopped() {
        assert!(!is_play_enabled(PlaybackState::NoSong));
        assert!(!is_play_enabled(PlaybackState::Stopped));
        assert!(is_play_enabled(PlaybackState::Initialized));
        assert!(is_play_enabled(PlaybackState::Playing));
        assert!(is_play_enabled(PlaybackState::Paused));
    }
}
