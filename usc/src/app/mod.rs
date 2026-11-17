pub mod header;
pub mod lyrics;
pub mod tap;
pub mod waveform;

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use eframe::egui;

use crate::app::waveform::WaveformConfig;
use crate::audio::{AudioEngine, WaveformPeaks};
use crate::lyrics::{Role, clean, syllabify, syllables};
use crate::media::{FolderMedia, detect_language, dictionaries_dir, read_tags, scan_folder};
use crate::romaji::{RomajiConfig, contains_japanese, convert_text};
use crate::song::{Header, render};
use crate::stems::{
    ComputeDevice, SourceMode, StemsEvent, StemsJob, VideoPreset, get_ffmpeg_path, get_ytdlp_path,
    process_media, run_tools_setup,
};
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
pub enum MonitorTrack {
    Mix,
    Vocals,
    Instrumental,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinishModal {
    DuetPass { tapped: usize, total: usize },
    SongComplete,
}

struct StemsModalState {
    open: bool,
    source_mode: SourceMode,
    file_path: String,
    url_input: String,
    output_dir: PathBuf,
    preset: VideoPreset,
    device: ComputeDevice,
    running: bool,
    progress: f32,
    status_line: String,
    logs: Vec<String>,
    rx: Option<Receiver<StemsEvent>>,
}

impl Default for StemsModalState {
    fn default() -> Self {
        let default_out = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("output");
        Self {
            open: false,
            source_mode: SourceMode::File,
            file_path: String::new(),
            url_input: String::new(),
            output_dir: default_out,
            preset: VideoPreset::Compact,
            device: ComputeDevice::Auto,
            running: false,
            progress: 0.0,
            status_line: "Ready.".into(),
            logs: Vec::new(),
            rx: None,
        }
    }
}

pub struct UscApp {
    audio: Result<AudioEngine, String>,
    audio_path: Option<PathBuf>,
    monitor_track: MonitorTrack,
    duration_s: f64,
    header: Header,
    is_duet: bool,
    singer_p1: String,
    singer_p2: String,
    folder_media: FolderMedia,
    lyrics_text: String,
    lyrics_cursor: (usize, usize),
    pending_cursor: Option<(usize, usize)>,
    romaji_cfg: RomajiConfig,
    waveform_cfg: WaveformConfig,
    waveform_peaks: Option<WaveformPeaks>,
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
    clear_focus: bool,
    status: String,
    error_banner: Option<String>,
    finish_modal: Option<FinishModal>,
    stems_modal: StemsModalState,
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
            monitor_track: MonitorTrack::Mix,
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
            romaji_cfg: RomajiConfig::default(),
            waveform_cfg: WaveformConfig::default(),
            waveform_peaks: None,
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
            clear_focus: false,
            status: "Open or drop an audio/video file to begin.".into(),
            error_banner,
            finish_modal: None,
            stems_modal: StemsModalState::default(),
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
        self.monitor_track = MonitorTrack::Mix;
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
        self.refresh_waveform_peaks();
        self.refresh_output();
    }

    fn refresh_waveform_peaks(&mut self) {
        let Some(base_path) = &self.audio_path else {
            self.waveform_peaks = None;
            return;
        };
        let Ok(engine) = &self.audio else {
            self.waveform_peaks = None;
            return;
        };

        // Prefer the isolated [Vocals] stem as the amplitude "source of truth" when present
        let (target_path, label) = if let Some(dir) = base_path.parent()
            && !self.header.vocals.trim().is_empty()
        {
            let voc_path = dir.join(self.header.vocals.trim());
            if voc_path.is_file() {
                (voc_path, "Vocals")
            } else {
                (base_path.clone(), "Mix")
            }
        } else {
            (base_path.clone(), "Mix")
        };

        self.waveform_peaks = engine.extract_peaks(&target_path, 20.0, label).ok();
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
        self.refresh_waveform_peaks();
        self.refresh_output();
        self.status = "Media folder refreshed.".into();
    }

    fn invalidate_session(&mut self) {
        self.session = None;
        self.kept_notes.clear();
        if self.state == PlaybackState::Stopped {
            self.state = PlaybackState::Initialized;
        }
        self.refresh_output();
    }

    fn handle_new_lyrics(&mut self, raw: String) {
        if let Some(lang) = detect_language(&raw) {
            self.header.language = lang.to_string();
        }
        let cleaned = clean(&raw);
        self.lyrics_text = cleaned;
        self.syllabify_current();
        self.invalidate_session();
    }

    fn active_audio_file(&self) -> Option<PathBuf> {
        let base_path = self.audio_path.as_ref()?;
        let dir = base_path.parent()?;
        match self.monitor_track {
            MonitorTrack::Mix => Some(base_path.clone()),
            MonitorTrack::Vocals => {
                let v = self.header.vocals.trim();
                if !v.is_empty() {
                    let p = dir.join(v);
                    if p.is_file() {
                        return Some(p);
                    }
                }
                Some(base_path.clone())
            }
            MonitorTrack::Instrumental => {
                let i = self.header.instrumental.trim();
                if !i.is_empty() {
                    let p = dir.join(i);
                    if p.is_file() {
                        return Some(p);
                    }
                }
                Some(base_path.clone())
            }
        }
    }

    fn switch_monitor_track(&mut self, new_track: MonitorTrack) {
        if self.monitor_track == new_track {
            return;
        }
        self.monitor_track = new_track;
        let Some(target_path) = self.active_audio_file() else {
            return;
        };

        let is_tapping = matches!(self.state, PlaybackState::Playing | PlaybackState::Paused);
        let is_preview = matches!(
            self.preview_state,
            PreviewState::Playing | PreviewState::Paused
        );
        if !is_tapping && !is_preview {
            return;
        }

        let Ok(engine) = &mut self.audio else {
            return;
        };
        let pos_ms = engine.position_ms();
        let was_playing = engine.is_playing();
        if let Err(err) = engine.open(&target_path) {
            self.error_banner = Some(err);
            return;
        }
        let tempo = if is_tapping {
            (self.speed_pct - 100) as f32
        } else {
            0.0
        };
        let _ = engine.set_tempo(tempo);
        engine.seek_ms(pos_ms);
        if was_playing && let Err(err) = engine.resume() {
            self.error_banner = Some(err);
        }
        if is_tapping {
            self.clear_focus = true;
        }
    }

    fn syllabify_current(&mut self) {
        let mut did_japanese = false;
        if contains_japanese(&self.lyrics_text) {
            self.lyrics_text = convert_text(&self.lyrics_text, &self.romaji_cfg);
            if self.header.language.trim().is_empty() {
                self.header.language = "Japanese".into();
            }
            did_japanese = true;
        }

        if self.header.language.trim().is_empty() {
            if let Some(lang) = detect_language(&self.lyrics_text) {
                self.header.language = lang.to_string();
            } else if !did_japanese {
                self.status = "Select a song Language first to syllabify.".into();
                return;
            }
        }

        if let Some(dir) = dictionaries_dir() {
            let dict_lang = if did_japanese && self.header.language.trim() == "Japanese" {
                "English".to_owned()
            } else {
                self.header.language.trim().to_owned()
            };
            let dict_path = dir.join(format!("{dict_lang}.txt"));
            if let Ok(dict) = std::fs::read_to_string(&dict_path) {
                self.lyrics_text = syllabify(&self.lyrics_text, &dict);
                self.invalidate_session();
                if did_japanese {
                    self.status =
                        format!("Romanized Japanese morae & syllabified ({dict_lang}.txt).");
                } else {
                    self.status = format!("Syllabified using {dict_lang}.txt.");
                }
                return;
            }
        }

        if did_japanese {
            self.invalidate_session();
            self.status = "Romanized & mora-split Japanese lyrics.".into();
        } else {
            self.status = format!(
                "No syllabification dictionary for '{}'.",
                self.header.language.trim()
            );
        }
    }

    fn toggle_play_pause(&mut self) {
        let Some(audio_path) = self.active_audio_file() else {
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
                self.clear_focus = true;
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
                self.clear_focus = true;
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
        let Some(audio_path) = self.active_audio_file() else {
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
        if self.state == PlaybackState::Playing {
            self.toggle_play_pause();
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
        self.invalidate_session();
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

    fn open_stems_modal(&mut self, initial_source: Option<PathBuf>) {
        self.stems_modal.open = true;
        if let Some(src) = initial_source {
            if let Some(parent) = src.parent() {
                self.stems_modal.output_dir = parent.to_path_buf();
            }
            self.stems_modal.source_mode = SourceMode::File;
            self.stems_modal.file_path = src.to_string_lossy().into_owned();
        } else if self.stems_modal.file_path.is_empty()
            && let Some(dir) = &self.default_dir
        {
            self.stems_modal.output_dir = dir.clone();
        }
    }

    fn start_tools_setup(&mut self) {
        if self.stems_modal.running {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.stems_modal.running = true;
        self.stems_modal.progress = 0.05;
        self.stems_modal.status_line = "Checking & downloading FFmpeg / yt-dlp into ./bin…".into();
        self.stems_modal.logs.clear();
        self.stems_modal.rx = Some(rx);

        std::thread::spawn(move || {
            let tx_done = tx.clone();
            let res = run_tools_setup(false, Some(tx));
            let _ = tx_done.send(StemsEvent::SetupFinished(res));
        });
    }

    fn start_stems_processing(&mut self) {
        if self.stems_modal.running {
            return;
        }
        let source_input = match self.stems_modal.source_mode {
            SourceMode::File => self.stems_modal.file_path.trim().to_owned(),
            SourceMode::Url => self.stems_modal.url_input.trim().to_owned(),
        };
        if source_input.is_empty() {
            self.stems_modal.status_line = match self.stems_modal.source_mode {
                SourceMode::File => "Please select a local video or audio file first.".into(),
                SourceMode::Url => "Please enter a YouTube or media URL first.".into(),
            };
            return;
        }
        let job = StemsJob {
            source_input,
            output_dir: self.stems_modal.output_dir.clone(),
            preset: self.stems_modal.preset,
            device: self.stems_modal.device,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.stems_modal.running = true;
        self.stems_modal.progress = 0.01;
        self.stems_modal.status_line = "Starting parallel media & HTDemucs pipeline…".into();
        self.stems_modal.logs.clear();
        self.stems_modal.rx = Some(rx);

        std::thread::spawn(move || {
            let tx_done = tx.clone();
            let res = process_media(&job, Some(tx));
            let _ = tx_done.send(StemsEvent::Finished(res));
        });
    }

    fn poll_stems_worker(&mut self, ctx: &egui::Context) {
        let mut finished_result = None;
        let mut setup_result = None;
        if let Some(rx) = &self.stems_modal.rx {
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    StemsEvent::Log(msg) => {
                        self.stems_modal.logs.push(msg);
                    }
                    StemsEvent::Progress { fraction, status } => {
                        self.stems_modal.progress = fraction;
                        self.stems_modal.status_line = status;
                    }
                    StemsEvent::SetupFinished(res) => {
                        setup_result = Some(res);
                    }
                    StemsEvent::Finished(res) => {
                        finished_result = Some(res);
                    }
                }
            }
        }
        if self.stems_modal.running {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
        if let Some(res) = setup_result {
            self.stems_modal.running = false;
            self.stems_modal.rx = None;
            match res {
                Ok(()) => {
                    self.stems_modal.progress = 1.0;
                    self.stems_modal.status_line =
                        "Tools ready! FFmpeg & yt-dlp are available.".into();
                }
                Err(err) => {
                    self.stems_modal.status_line = format!("Setup failed: {err}");
                    self.stems_modal.logs.push(format!("ERROR: {err}"));
                }
            }
        }
        if let Some(res) = finished_result {
            self.stems_modal.running = false;
            self.stems_modal.rx = None;
            match res {
                Ok(derived) => {
                    self.stems_modal.progress = 1.0;
                    self.stems_modal.status_line =
                        "Complete! Loaded separated stems & muted video into project.".into();
                    self.stems_modal.open = false;
                    self.load_audio(derived.mp3);
                    self.status =
                        "Media & stems prepared! Toggle [Mix | Vocals | Inst] in the right pane while tapping."
                            .into();
                }
                Err(err) => {
                    self.stems_modal.status_line = format!("Failed: {err}");
                    self.stems_modal.logs.push(format!("ERROR: {err}"));
                }
            }
        }
    }

    fn show_stems_modal(&mut self, ctx: &egui::Context) {
        if !self.stems_modal.open {
            return;
        }
        let mut open = self.stems_modal.open;
        let mut start_clicked = false;
        let mut setup_clicked = false;
        let running = self.stems_modal.running;
        let ffmpeg_ok = get_ffmpeg_path().is_some();
        let ytdlp_ok = get_ytdlp_path().is_some();

        egui::Window::new("Prepare Media & Split Stems (vid2stems / HTDemucs)")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(580.0)
            .show(ctx, |ui| {
                ui.label(
                    "Splits local media or YouTube/web URLs into Master MP3, Muted Video (.mp4), Isolated Vocals, and Peak-Limited Instrumental using HTDemucs.",
                );
                ui.add_space(6.0);

                ui.add_enabled_ui(!running, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Source Mode:").strong());
                        ui.radio_value(
                            &mut self.stems_modal.source_mode,
                            SourceMode::File,
                            "Local Media (Video / Audio)",
                        );
                        ui.radio_value(
                            &mut self.stems_modal.source_mode,
                            SourceMode::Url,
                            "YouTube / Web URL (yt-dlp)",
                        );
                    });
                    ui.add_space(4.0);

                    egui::Grid::new("stems_modal_grid")
                        .num_columns(3)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            match self.stems_modal.source_mode {
                                SourceMode::File => {
                                    ui.label("File Path:");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.stems_modal.file_path)
                                            .hint_text(
                                                "Select video (.mp4/.mkv/.webm) or audio (.flac/.wav/.mp3)",
                                            )
                                            .desired_width(340.0),
                                    );
                                    if ui.button("Browse…").clicked() {
                                        let mut dlg = rfd::FileDialog::new().add_filter(
                                            "Media files",
                                            &[
                                                "mp4", "mkv", "webm", "mov", "avi", "flac", "wav",
                                                "mp3", "ogg", "m4a", "opus",
                                            ],
                                        );
                                        if let Some(dir) = &self.default_dir {
                                            dlg = dlg.set_directory(dir);
                                        }
                                        if let Some(p) = dlg.pick_file() {
                                            if let Some(parent) = p.parent() {
                                                self.stems_modal.output_dir = parent.to_path_buf();
                                                self.default_dir = Some(parent.to_path_buf());
                                            }
                                            self.stems_modal.file_path =
                                                p.to_string_lossy().into_owned();
                                        }
                                    }
                                    ui.end_row();
                                }
                                SourceMode::Url => {
                                    ui.label("Media URL:");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.stems_modal.url_input)
                                            .hint_text("https://www.youtube.com/watch?v=...")
                                            .desired_width(340.0),
                                    );
                                    if ui.button("Paste").clicked()
                                        && let Ok(mut cb) = arboard::Clipboard::new()
                                        && let Ok(text) = cb.get_text()
                                    {
                                        self.stems_modal.url_input = text.trim().to_owned();
                                    }
                                    ui.end_row();
                                }
                            }

                            ui.label("Output Folder:");
                            let mut out_str =
                                self.stems_modal.output_dir.to_string_lossy().into_owned();
                            if ui
                                .add(egui::TextEdit::singleline(&mut out_str).desired_width(340.0))
                                .changed()
                            {
                                self.stems_modal.output_dir = PathBuf::from(out_str);
                            }
                            if ui.button("Browse…").clicked() {
                                let mut dlg = rfd::FileDialog::new();
                                if self.stems_modal.output_dir.is_dir() {
                                    dlg = dlg.set_directory(&self.stems_modal.output_dir);
                                }
                                if let Some(folder) = dlg.pick_folder() {
                                    self.stems_modal.output_dir = folder;
                                }
                            }
                            ui.end_row();

                            ui.label("Video Preset:");
                            egui::ComboBox::from_id_salt("stems_video_preset")
                                .selected_text(self.stems_modal.preset.label())
                                .width(230.0)
                                .show_ui(ui, |ui| {
                                    for p in [
                                        VideoPreset::Compact,
                                        VideoPreset::Balanced,
                                        VideoPreset::High,
                                        VideoPreset::Source,
                                    ] {
                                        ui.selectable_value(
                                            &mut self.stems_modal.preset,
                                            p,
                                            p.label(),
                                        );
                                    }
                                });
                            ui.label("");
                            ui.end_row();

                            ui.label("Device:");
                            egui::ComboBox::from_id_salt("stems_compute_device")
                                .selected_text(self.stems_modal.device.label())
                                .width(230.0)
                                .show_ui(ui, |ui| {
                                    for d in [
                                        ComputeDevice::Auto,
                                        ComputeDevice::Cpu,
                                        ComputeDevice::DirectML,
                                        ComputeDevice::CoreML,
                                    ] {
                                        ui.selectable_value(
                                            &mut self.stems_modal.device,
                                            d,
                                            d.label(),
                                        );
                                    }
                                });
                            ui.label("");
                            ui.end_row();
                        });
                });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let ok_color = egui::Color32::from_rgb(22, 163, 74);
                    let miss_color = egui::Color32::from_rgb(220, 38, 38);
                    ui.colored_label(
                        if ffmpeg_ok { ok_color } else { miss_color },
                        if ffmpeg_ok {
                            "● FFmpeg: Ready"
                        } else {
                            "○ FFmpeg: Missing"
                        },
                    );
                    ui.add_space(8.0);
                    ui.colored_label(
                        if ytdlp_ok { ok_color } else { miss_color },
                        if ytdlp_ok {
                            "● yt-dlp: Ready"
                        } else {
                            "○ yt-dlp: Missing"
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !running,
                                egui::Button::new("Check / Download Tools (FFmpeg + yt-dlp)"),
                            )
                            .on_hover_text(
                                "Automatically download portable FFmpeg, FFprobe, and yt-dlp into ./bin/",
                            )
                            .clicked()
                        {
                            setup_clicked = true;
                        }
                    });
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let has_source = match self.stems_modal.source_mode {
                        SourceMode::File => !self.stems_modal.file_path.trim().is_empty(),
                        SourceMode::Url => !self.stems_modal.url_input.trim().is_empty(),
                    };
                    let tools_ready = ffmpeg_ok
                        && (self.stems_modal.source_mode != SourceMode::Url || ytdlp_ok);
                    let can_start = !running && tools_ready && has_source;
                    if ui
                        .add_enabled(can_start, egui::Button::new("Start Processing"))
                        .clicked()
                    {
                        start_clicked = true;
                    }
                    ui.label(&self.stems_modal.status_line);
                });

                ui.add_space(4.0);
                ui.add(
                    egui::ProgressBar::new(self.stems_modal.progress)
                        .show_percentage()
                        .animate(running),
                );

                if !self.stems_modal.logs.is_empty() {
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical()
                        .id_salt("stems_log_scroll")
                        .max_height(150.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &self.stems_modal.logs {
                                ui.monospace(line);
                            }
                        });
                }
            });

        if !running {
            self.stems_modal.open = open;
        }
        if setup_clicked {
            self.start_tools_setup();
        } else if start_clicked {
            self.start_stems_processing();
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
                } else if matches!(
                    ext.as_str(),
                    "mp4" | "mkv" | "webm" | "mov" | "avi" | "flac" | "wav"
                ) {
                    self.open_stems_modal(Some(path));
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

        self.poll_stems_worker(ctx);

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

        if self.clear_focus {
            self.clear_focus = false;
            if let Some(focused) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(focused));
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
                        .add_enabled(
                            !locked,
                            egui::Button::new("Prepare Media / Split Stems (HTDemucs)..."),
                        )
                        .clicked()
                    {
                        ui.close();
                        self.open_stems_modal(None);
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
                let prev_duet = self.is_duet;
                let prev_vocals = self.header.vocals.clone();
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
                if act.prepare_stems {
                    self.open_stems_modal(None);
                }
                if act.refresh_media {
                    self.refresh_media();
                }
                if self.header.vocals != prev_vocals {
                    self.refresh_waveform_peaks();
                }
                if self.is_duet != prev_duet {
                    self.invalidate_session();
                } else if act.changed {
                    self.refresh_output();
                }
            });

        egui::SidePanel::right("tap_panel")
            .default_width(360.0)
            .min_width(300.0)
            .show(ctx, |ui| {
                let tap_pos_s = if locked { pos_s } else { 0.0 };
                let has_vocals = !self.header.vocals.trim().is_empty();
                let has_instrumental = !self.header.instrumental.trim().is_empty();
                let act = tap::show(
                    ui,
                    self.state,
                    self.audio.is_ok(),
                    self.audio_path.is_some(),
                    has_vocals,
                    has_instrumental,
                    self.monitor_track,
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
                if let Some(new_mon) = act.monitor_changed {
                    self.switch_monitor_track(new_mon);
                }
                if let Some(new_filter) = act.filter_changed {
                    self.filter = new_filter;
                    if self.state == PlaybackState::Stopped {
                        self.state = PlaybackState::Initialized;
                    }
                    let syls = syllables(&clean(&self.lyrics_text), self.is_duet);
                    if !syls.is_empty() {
                        self.session = Some(Session::new(syls, new_filter, self.kept_notes.clone()));
                    }
                    self.refresh_output();
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
                pos_s,
                self.duration_s,
                self.session.as_ref(),
                &mut self.romaji_cfg,
                &mut self.waveform_cfg,
                self.waveform_peaks.as_ref(),
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
            } else if act.text_changed {
                self.invalidate_session();
            }
        });

        self.show_stems_modal(ctx);

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
                            self.state = PlaybackState::Initialized;
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
                            self.state = PlaybackState::Initialized;
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
