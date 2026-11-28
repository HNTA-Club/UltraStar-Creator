use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, SystemTime};

use stem_splitter_core::{
    SplitResult,
    core::{
        audio::{WavWriter, create_wav_writer, read_audio, sample_to_i16},
        engine,
    },
    ensure_model, set_download_progress_callback,
};

pub const CANCELLED_MSG: &str = "Cancelled by user.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceMode {
    File,
    Url,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoPreset {
    Compact,
    Balanced,
    High,
    Source,
}

impl VideoPreset {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact (720p, CRF 24)",
            Self::Balanced => "Balanced (720p, CRF 22)",
            Self::High => "High (1080p, CRF 20)",
            Self::Source => "Source (Original res, CRF 18)",
        }
    }

    pub const fn crf(self) -> u8 {
        match self {
            Self::Compact => 24,
            Self::Balanced => 22,
            Self::High => 20,
            Self::Source => 18,
        }
    }

    pub const fn max_height(self) -> Option<u32> {
        match self {
            Self::Compact | Self::Balanced => Some(720),
            Self::High => Some(1080),
            Self::Source => None,
        }
    }

    pub const fn x264_preset(self) -> &'static str {
        match self {
            Self::Compact | Self::Balanced | Self::Source => "slow",
            Self::High => "medium",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputeDevice {
    Auto,
    Cpu,
    DirectML,
    CoreML,
}

impl ComputeDevice {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (Best available)",
            Self::Cpu => "CPU",
            Self::DirectML => "DirectML (Windows GPU)",
            Self::CoreML => "CoreML (Apple Silicon)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedOutputs {
    pub mp3: PathBuf,
    pub muted_mp4: Option<PathBuf>,
    pub vocals: PathBuf,
    pub instrumental: PathBuf,
    pub artist: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct StemsJob {
    pub source_input: String,
    pub output_dir: PathBuf,
    pub artist: String,
    pub title: String,
    pub preset: VideoPreset,
    pub device: ComputeDevice,
}

#[derive(Debug, Clone)]
pub enum StemsEvent {
    Log(String),
    Progress { fraction: f32, status: String },
    SetupFinished(Result<(), String>),
    Finished(Result<DerivedOutputs, String>),
}


#[expect(clippy::missing_const_for_fn)]
pub fn configure_no_window(cmd: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = cmd;
    }
}

fn check_cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err(CANCELLED_MSG.to_owned())
    } else {
        Ok(())
    }
}

fn run_command_cancellable(
    cmd: &mut Command,
    cancel: &AtomicBool,
    step_label: &str,
) -> Result<(), String> {
    check_cancelled(cancel)?;
    configure_no_window(cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{step_label}: failed to spawn process: {e}"))?;

    // Drain stderr in a helper thread so the child never blocks on a full pipe buffer
    let mut stderr_pipe = child.stderr.take();
    let stderr_handle = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(ref mut pipe) = stderr_pipe {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });

    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stderr_handle.join();
            return Err(CANCELLED_MSG.to_owned());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let stderr = stderr_handle.join().unwrap_or_default();

                if status.success() {
                    return Ok(());
                }

                let summary = stderr
                    .lines()
                    .rev()
                    .take(4)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join(" | ");

                let message = if summary.is_empty() {
                    "non-zero exit code".to_string()
                } else {
                    summary
                };

                return Err(format!("{step_label} failed: {message}"));
            }

            Ok(None) => thread::sleep(Duration::from_millis(50)),

            Err(error) => {
                let _ = child.kill();
                let _ = stderr_handle.join();

                return Err(format!(
                    "{step_label}: error waiting on process: {error}"
                ));
            }
        }
    }
}

pub fn binary_filename(name: &str) -> String {
    if cfg!(target_os = "windows") && !name.to_ascii_lowercase().ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

pub fn local_bin_dir() -> PathBuf {
    if let Ok(exe) = env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        return exe_dir.join("bin");
    }
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("bin")
}

pub fn find_binary(name: &str) -> Option<PathBuf> {
    let bin_name = binary_filename(name);

    if let Ok(exe) = env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        let candidate_bin = exe_dir.join("bin").join(&bin_name);
        if candidate_bin.is_file() {
            return Some(candidate_bin);
        }
        let candidate_root = exe_dir.join(&bin_name);
        if candidate_root.is_file() {
            return Some(candidate_root);
        }
    }

    if let Ok(cwd) = env::current_dir() {
        let local_bin = cwd.join("bin").join(&bin_name);
        if local_bin.is_file() {
            return Some(local_bin);
        }
    }

    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join(&bin_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

pub fn get_ffmpeg_path() -> Option<PathBuf> {
    find_binary("ffmpeg")
}

pub fn get_ffprobe_path() -> Option<PathBuf> {
    find_binary("ffprobe")
}

pub fn get_ytdlp_path() -> Option<PathBuf> {
    find_binary("yt-dlp")
}

pub fn normalize_url(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Some(trimmed.to_owned())
    } else if lower.starts_with("www.") {
        Some(format!("https://{trimmed}"))
    } else {
        None
    }
}

pub fn normalize_url_input(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    }
}

pub fn is_url(input: &str) -> bool {
    normalize_url(input).is_some()
}

pub fn is_video_extension(path: &Path) -> Option<bool> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "mp3" | "wav" | "flac" | "ogg" | "opus" | "m4a" | "aac" | "alac" | "wma" | "aiff"
        | "aif" => Some(false),
        "mp4" | "webm" | "mkv" | "avi" | "mov" | "flv" | "wmv" => Some(true),
        _ => None,
    }
}

pub fn has_video_stream(media_path: &Path) -> bool {
    if !media_path.is_file() {
        return false;
    }

    if  is_video_extension(media_path) == Some(false) {
        return false;
    }

    if let Some(ffprobe) = get_ffprobe_path() {
        let mut cmd = Command::new(ffprobe);
        configure_no_window(&mut cmd);
        cmd.args([
            "-v",
            "error",
            "-select_streams",
            "v",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "csv=p=0",
        ])
        .arg(media_path);
        if let Ok(out) = cmd.output()
            && out.status.success()
        {
            let stdout = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
            return stdout.contains("video");
        }
    }
    is_video_extension(media_path).unwrap_or(true)
}

pub fn sanitize_filename_part(s: &str) -> String {
    let cleaned: String = s
        .trim()
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ if c.is_control() => ' ',
            _ => c,
        })
        .collect();
    cleaned.trim_matches(|c: char| c == ' ' || c == '.').to_owned()
}

pub fn resolve_base_name(artist: &str, title: &str, fallback_stem: &str) -> String {
    let a = sanitize_filename_part(artist);
    let t = sanitize_filename_part(title);
    let base = match (a.is_empty(), t.is_empty()) {
        (false, false) => format!("{a} - {t}"),
        (true, false) => t,
        (false, true) => {
            let fb = sanitize_filename_part(fallback_stem);
            if fb.is_empty() { a } else { format!("{a} - {fb}") }
        }
        (true, true) => sanitize_filename_part(fallback_stem),
    };
    if base.is_empty() {
        "song".to_owned()
    } else {
        base
    }
}

pub fn resolve_song_output_dir(output_dir: &Path, base_name: &str) -> PathBuf {
    if output_dir
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == base_name)
    {
        output_dir.to_path_buf()
    } else {
        output_dir.join(base_name)
    }
}

pub fn derive_output_paths(output_dir: &Path, base_name: &str, has_video: bool) -> DerivedOutputs {
    DerivedOutputs {
        mp3: output_dir.join(format!("{base_name}.mp3")),
        muted_mp4: has_video.then(|| output_dir.join(format!("{base_name}_muted.mp4"))),
        vocals: output_dir.join(format!("{base_name} [Vocals].mp3")),
        instrumental: output_dir.join(format!("{base_name} [Instrumental].mp3")),
        artist: String::new(),
        title: String::new(),
    }
}

pub fn build_ytdlp_args(
    ffmpeg_bin: Option<&Path>,
    url: &str,
    output_dir: &Path,
) -> Vec<String> {
    let target_pattern = output_dir.join("%(title)s.%(ext)s");
    let mut args = vec![
        "--no-playlist".to_owned(),
        "-f".to_owned(),
        "bv*+ba/b".to_owned(),
        "--merge-output-format".to_owned(),
        "mp4".to_owned(),
    ];
    if let Some(ff) = ffmpeg_bin
        && let Some(parent) = ff.parent()
    {
        args.push("--ffmpeg-location".to_owned());
        args.push(parent.to_string_lossy().into_owned());
    }
    args.extend([
        "-o".to_owned(),
        target_pattern.to_string_lossy().into_owned(),
        url.trim().to_owned(),
    ]);
    args
}

pub fn download_url_media(
    url: &str,
    output_dir: &Path,
    cancel: &AtomicBool,
    tx: &Option<Sender<StemsEvent>>,
) -> Result<PathBuf, String> {
    check_cancelled(cancel)?;
    let ytdlp_exe = get_ytdlp_path().ok_or_else(|| {
        "URL downloading requires yt-dlp, which was not found.\nClick 'Check / Download Tools' to install portable FFmpeg & yt-dlp automatically.".to_owned()
    })?;

    fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create download dir: {e}"))?;

    let before_files: HashSet<PathBuf> = fs::read_dir(output_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();

    send_log(tx, format!("[yt-dlp] Downloading media from URL: {url}"));
    send_progress(tx, 0.03, "Downloading video via yt-dlp…");

    let ffmpeg_bin = get_ffmpeg_path();
    let args = build_ytdlp_args(ffmpeg_bin.as_deref(), url, output_dir);
    let mut cmd = Command::new(&ytdlp_exe);
    cmd.args(&args);
    run_command_cancellable(&mut cmd, cancel, "yt-dlp download")?;

    let after_files: Vec<PathBuf> = fs::read_dir(output_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();

    let is_media_candidate = |p: &Path| -> bool {
        let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
            return false;
        };
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "mp4" | "mkv" | "webm" | "m4a" | "mp3" | "opus" | "wav"
        ) && !p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .ends_with("_muted.mp4")
    };

    let mut candidates: Vec<PathBuf> = after_files
        .iter()
        .filter(|p| !before_files.contains(*p) && is_media_candidate(p))
        .cloned()
        .collect();

    if candidates.is_empty() {
        candidates = after_files
            .into_iter()
            .filter(|p| is_media_candidate(p))
            .collect();
    }

    candidates.sort_by_key(|p| {
        p.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    });

    let chosen = candidates.pop().ok_or_else(|| {
        "yt-dlp completed but did not produce a recognized media file.".to_owned()
    })?;

    send_log(
        tx,
        format!(
            "[yt-dlp] Downloaded: {}",
            chosen
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("media")
        ),
    );
    Ok(chosen)
}

pub fn resolve_input_media(
    source_input: &str,
    temp_dir: &Path,
    cancel: &AtomicBool,
    tx: &Option<Sender<StemsEvent>>,
) -> Result<PathBuf, String> {
    check_cancelled(cancel)?;
    let trimmed = source_input.trim();
    if trimmed.is_empty() {
        return Err("Source input is empty.".to_owned());
    }
    if let Some(url) = normalize_url(trimmed) {
        return download_url_media(&url, temp_dir, cancel, tx);
    }
    let path = PathBuf::from(trimmed);
    if path.is_file() {
        return Ok(path);
    }
    Err(format!("Source media file not found: {trimmed}"))
}

pub fn build_ffmpeg_muted_video_args(
    source_video: &Path,
    output_mp4: &Path,
    preset: VideoPreset,
) -> Vec<String> {
    let mut args = vec![
        "-y".to_owned(),
        "-i".to_owned(),
        source_video.to_string_lossy().into_owned(),
        "-an".to_owned(),
    ];
    if let Some(h) = preset.max_height() {
        args.push("-vf".to_owned());
        args.push(format!("scale=-2:min({h}\\,ih)"));
    }
    args.extend([
        "-c:v".to_owned(),
        "libx264".to_owned(),
        "-crf".to_owned(),
        preset.crf().to_string(),
        "-preset".to_owned(),
        preset.x264_preset().to_owned(),
        "-pix_fmt".to_owned(),
        "yuv420p".to_owned(),
        output_mp4.to_string_lossy().into_owned(),
    ]);
    args
}

pub fn build_ffmpeg_pcm_wav_args(source_media: &Path, output_wav: &Path) -> Vec<String> {
    vec![
        "-y".to_owned(),
        "-i".to_owned(),
        source_media.to_string_lossy().into_owned(),
        "-vn".to_owned(),
        "-ar".to_owned(),
        "44100".to_owned(),
        "-ac".to_owned(),
        "2".to_owned(),
        "-c:a".to_owned(),
        "pcm_s16le".to_owned(),
        output_wav.to_string_lossy().into_owned(),
    ]
}

pub fn build_ffmpeg_mp3_args(source_media: &Path, output_mp3: &Path) -> Vec<String> {
    vec![
        "-y".to_owned(),
        "-i".to_owned(),
        source_media.to_string_lossy().into_owned(),
        "-vn".to_owned(),
        "-codec:a".to_owned(),
        "libmp3lame".to_owned(),
        "-q:a".to_owned(),
        "0".to_owned(),
        output_mp3.to_string_lossy().into_owned(),
    ]
}

pub fn build_ffmpeg_inst_mix_args(component_stems: &[&Path], output_mp3: &Path) -> Vec<String> {
    let mut args = vec!["-y".to_owned()];
    let mut filter_inputs = String::new();
    for (idx, stem) in component_stems.iter().enumerate() {
        args.push("-i".to_owned());
        args.push(stem.to_string_lossy().into_owned());
        filter_inputs.push_str(&format!("[{idx}:a]"));
    }
    let filter_str = format!(
        "{filter_inputs}amix=inputs={}:normalize=0,alimiter=limit=0.99:attack=5:release=50",
        component_stems.len()
    );
    args.extend([
        "-filter_complex".to_owned(),
        filter_str,
        "-codec:a".to_owned(),
        "libmp3lame".to_owned(),
        "-q:a".to_owned(),
        "0".to_owned(),
        output_mp3.to_string_lossy().into_owned(),
    ]);
    args
}

fn run_ffmpeg(
    ffmpeg_bin: &Path,
    args: &[String],
    cancel: &AtomicBool,
    step_label: &str,
) -> Result<(), String> {
    let mut cmd = Command::new(ffmpeg_bin);
    cmd.args(args);
    run_command_cancellable(&mut cmd, cancel, step_label)
}

fn send_log(tx: &Option<Sender<StemsEvent>>, msg: impl Into<String>) {
    if let Some(tx) = tx {
        let _ = tx.send(StemsEvent::Log(msg.into()));
    }
}

fn send_progress(tx: &Option<Sender<StemsEvent>>, fraction: f32, status: impl Into<String>) {
    if let Some(tx) = tx {
        let _ = tx.send(StemsEvent::Progress {
            fraction: fraction.clamp(0.0, 1.0),
            status: status.into(),
        });
    }
}

fn fill_stereo_window(
    samples: &[f32],
    channels: u16,
    start_frame: usize,
    left_raw: &mut [f32],
    right_raw: &mut [f32],
) {
    let ch = usize::from(channels.max(1));
    for i in 0..left_raw.len() {
        let frame = start_frame + i;
        let base = frame * ch;
        if base >= samples.len() {
            left_raw[i] = 0.0;
            right_raw[i] = 0.0;
            continue;
        }
        let left = samples[base];
        let right = if ch == 1 {
            left
        } else {
            samples.get(base + 1).copied().unwrap_or(left)
        };
        left_raw[i] = left;
        right_raw[i] = right;
    }
}

fn split_file_cancellable(
    input_path: &Path,
    output_dir: &Path,
    cancel: &AtomicBool,
    tx: &Option<Sender<StemsEvent>>,
) -> Result<SplitResult, String> {
    check_cancelled(cancel)?;
    send_log(tx, "[HTDemucs] Resolving & verifying model…");
    let handle =
        ensure_model("htdemucs_ort_v1", None).map_err(|e| format!("Model resolve failed: {e}"))?;

    check_cancelled(cancel)?;
    send_log(tx, "[HTDemucs] Preloading ONNX Runtime session…");
    engine::preload(&handle).map_err(|e| format!("Engine preload failed: {e}"))?;

    let mf = engine::manifest();
    if mf.sample_rate != 44100 {
        return Err("Expected 44.1 kHz HTDemucs model".to_owned());
    }

    check_cancelled(cancel)?;
    let audio =
        read_audio(input_path).map_err(|e| format!("Failed to read extracted PCM WAV: {e}"))?;
    let channels = usize::from(audio.channels.max(1));
    let n = audio.samples.len() / channels;
    if n == 0 {
        return Err("Extracted audio is empty".to_owned());
    }

    let win = mf.window;
    let hop = mf.hop;
    if !(win > 0 && hop > 0 && hop <= win) {
        return Err("Invalid window/hop in HTDemucs manifest".to_owned());
    }

    let names = if mf.stems.is_empty() {
        vec![
            "vocals".to_owned(),
            "drums".to_owned(),
            "bass".to_owned(),
            "other".to_owned(),
        ]
    } else {
        mf.stems.clone()
    };

    let stem_base = output_dir.join("raw_stem");
    let vocals_out = format!("{}_vocals.wav", stem_base.to_string_lossy());
    let drums_out = format!("{}_drums.wav", stem_base.to_string_lossy());
    let bass_out = format!("{}_bass.wav", stem_base.to_string_lossy());
    let other_out = format!("{}_other.wav", stem_base.to_string_lossy());

    struct StemOut {
        idx: usize,
        writer: WavWriter,
    }

    let mut name_idx: HashMap<String, usize> = HashMap::new();
    for (i, name) in names.iter().enumerate() {
        name_idx.insert(name.to_ascii_lowercase(), i);
    }

    let mut left_raw = vec![0.0_f32; win];
    let mut right_raw = vec![0.0_f32; win];
    let mut stem_outputs: Vec<StemOut> = Vec::new();

    let mut pos = 0usize;
    let mut chunk_done = 0usize;
    let total_chunks = if n <= hop { 1 } else { (n - 1) / hop + 1 };
    let mut first_chunk = true;

    while pos < n {
        check_cancelled(cancel)?;
        fill_stereo_window(
            &audio.samples,
            audio.channels,
            pos,
            &mut left_raw,
            &mut right_raw,
        );

        let out = engine::run_window_demucs(&left_raw, &right_raw)
            .map_err(|e| format!("HTDemucs inference failed: {e}"))?;
        let (stems_count, _, t_out) = (out.shape()[0], out.shape()[1], out.shape()[2]);

        if first_chunk {
            let get_idx = |key: &str, fallback: usize| -> usize {
                name_idx
                    .get(key)
                    .copied()
                    .unwrap_or_else(|| fallback.min(stems_count.saturating_sub(1)))
            };
            stem_outputs = vec![
                StemOut {
                    idx: get_idx("vocals", 0),
                    writer: create_wav_writer(&vocals_out, mf.sample_rate, 2)
                        .map_err(|e| e.to_string())?,
                },
                StemOut {
                    idx: get_idx("drums", 1),
                    writer: create_wav_writer(&drums_out, mf.sample_rate, 2)
                        .map_err(|e| e.to_string())?,
                },
                StemOut {
                    idx: get_idx("bass", 2),
                    writer: create_wav_writer(&bass_out, mf.sample_rate, 2)
                        .map_err(|e| e.to_string())?,
                },
                StemOut {
                    idx: get_idx("other", 3),
                    writer: create_wav_writer(&other_out, mf.sample_rate, 2)
                        .map_err(|e| e.to_string())?,
                },
            ];
            first_chunk = false;
        }

        let copy_len = hop.min(t_out).min(n - pos);
        for s_out in &mut stem_outputs {
            for i in 0..copy_len {
                s_out
                    .writer
                    .write_sample(sample_to_i16(out[(s_out.idx, 0, i)]))
                    .map_err(|e| e.to_string())?;
                s_out
                    .writer
                    .write_sample(sample_to_i16(out[(s_out.idx, 1, i)]))
                    .map_err(|e| e.to_string())?;
            }
        }

        chunk_done += 1;
        let pct = chunk_done as f32 / total_chunks as f32 * 100.0;
        let frac = (pct / 100.0).mul_add(0.55, 0.25);
        send_progress(
            tx,
            frac,
            format!("Separating stems: chunk {chunk_done}/{total_chunks} ({pct:.0}%)"),
        );

        if pos + hop >= n {
            break;
        }
        pos += hop;
    }

    check_cancelled(cancel)?;
    for s_out in stem_outputs {
        s_out.writer.finalize().map_err(|e| e.to_string())?;
    }

    Ok(SplitResult {
        vocals_path: vocals_out,
        drums_path: drums_out,
        bass_path: bass_out,
        other_path: other_out,
    })
}

#[cfg(not(target_os = "macos"))]
fn copy_found_binary_recursive(dir: &Path, file_name: &str, dest: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if copy_found_binary_recursive(&path, file_name, dest) {
                return true;
            }
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(file_name))
            && fs::copy(&path, dest).is_ok()
        {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(dest, fs::Permissions::from_mode(0o755));
            }
            return true;
        }
    }
    false
}

#[cfg(target_os = "macos")]
pub fn download_and_extract_ffmpeg(
    target_bin_dir: &Path,
    cancel: &AtomicBool,
    tx: &Option<Sender<StemsEvent>>,
) -> Result<(), String> {
    let _ = (target_bin_dir, cancel, tx);
    Err("On macOS, please install FFmpeg via Homebrew: `brew install ffmpeg`".to_owned())
}

#[cfg(not(target_os = "macos"))]
pub fn download_and_extract_ffmpeg(
    target_bin_dir: &Path,
    cancel: &AtomicBool,
    tx: &Option<Sender<StemsEvent>>,
) -> Result<(), String> {
    check_cancelled(cancel)?;
    fs::create_dir_all(target_bin_dir)
        .map_err(|e| format!("Failed to create bin dir {}: {e}", target_bin_dir.display()))?;

    let url = if cfg!(target_os = "windows") {
        "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
    } else {
        "https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz"
    };

    send_log(tx, format!("Downloading official FFmpeg static build from {url}…"));
    send_progress(tx, 0.15, "Downloading FFmpeg archive…");

    let resp = reqwest::blocking::get(url)
        .map_err(|e| format!("Failed to download FFmpeg: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("FFmpeg download returned HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .map_err(|e| format!("Failed to read FFmpeg archive bytes: {e}"))?;
    check_cancelled(cancel)?;

    let temp_extract = env::temp_dir().join(format!("usc_ffmpeg_setup_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_extract);
    fs::create_dir_all(&temp_extract)
        .map_err(|e| format!("Failed to create temp extract dir: {e}"))?;

    let archive_name = if cfg!(target_os = "windows") {
        "ffmpeg.zip"
    } else {
        "ffmpeg.tar.xz"
    };
    let archive_path = temp_extract.join(archive_name);
    fs::write(&archive_path, &bytes)
        .map_err(|e| format!("Failed to write temporary FFmpeg archive: {e}"))?;

    send_log(tx, "Extracting ffmpeg and ffprobe binaries…");
    send_progress(tx, 0.50, "Extracting FFmpeg & FFprobe…");

    let mut cmd = Command::new("tar");
    cmd.arg("-xf")
        .arg(&archive_path)
        .arg("-C")
        .arg(&temp_extract);
    if let Err(e) = run_command_cancellable(&mut cmd, cancel, "FFmpeg archive extraction") {
        let _ = fs::remove_dir_all(&temp_extract);
        return Err(e);
    }

    let ffmpeg_name = binary_filename("ffmpeg");
    let ffprobe_name = binary_filename("ffprobe");
    let ok_ff = copy_found_binary_recursive(
        &temp_extract,
        &ffmpeg_name,
        &target_bin_dir.join(&ffmpeg_name),
    );
    let ok_fp = copy_found_binary_recursive(
        &temp_extract,
        &ffprobe_name,
        &target_bin_dir.join(&ffprobe_name),
    );
    let _ = fs::remove_dir_all(&temp_extract);

    if !ok_ff || !ok_fp {
        return Err("Extracted archive did not contain ffmpeg and ffprobe.".to_owned());
    }

    send_log(
        tx,
        format!(
            "FFmpeg & FFprobe installed to {}",
            target_bin_dir.display()
        ),
    );
    Ok(())
}

pub fn download_ytdlp(
    target_bin_dir: &Path,
    cancel: &AtomicBool,
    tx: &Option<Sender<StemsEvent>>,
) -> Result<(), String> {
    check_cancelled(cancel)?;
    fs::create_dir_all(target_bin_dir)
        .map_err(|e| format!("Failed to create bin dir {}: {e}", target_bin_dir.display()))?;

    let url = if cfg!(target_os = "windows") {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos"
    } else {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp"
    };

    let dest = target_bin_dir.join(binary_filename("yt-dlp"));
    send_log(tx, format!("Downloading standalone yt-dlp from {url}…"));
    send_progress(tx, 0.75, "Downloading yt-dlp…");

    let resp = reqwest::blocking::get(url)
        .map_err(|e| format!("Failed to download yt-dlp: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("yt-dlp download returned HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .map_err(|e| format!("Failed to read yt-dlp binary bytes: {e}"))?;
    check_cancelled(cancel)?;
    fs::write(&dest, &bytes)
        .map_err(|e| format!("Failed to write {}: {e}", dest.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o755));
    }

    send_log(tx, format!("yt-dlp installed to {}", dest.display()));
    Ok(())
}

pub fn run_tools_setup(
    force: bool,
    cancel: Arc<AtomicBool>,
    tx: Option<Sender<StemsEvent>>,
) -> Result<(), String> {
    let ffmpeg_found = get_ffmpeg_path();
    let ffprobe_found = get_ffprobe_path();
    let ytdlp_found = get_ytdlp_path();

    send_log(&tx, "--- Dependency Status ---");
    send_log(
        &tx,
        format!(
            "FFmpeg:  {}",
            ffmpeg_found
                .as_ref()
                .map(|p| format!("Found at {}", p.display()))
                .unwrap_or_else(|| "NOT FOUND".to_owned())
        ),
    );
    send_log(
        &tx,
        format!(
            "FFprobe: {}",
            ffprobe_found
                .as_ref()
                .map(|p| format!("Found at {}", p.display()))
                .unwrap_or_else(|| "NOT FOUND".to_owned())
        ),
    );
    send_log(
        &tx,
        format!(
            "yt-dlp:  {}",
            ytdlp_found
                .as_ref()
                .map(|p| format!("Found at {}", p.display()))
                .unwrap_or_else(|| "NOT FOUND".to_owned())
        ),
    );
    send_log(&tx, "-------------------------");

    let need_ffmpeg = ffmpeg_found.is_none() || ffprobe_found.is_none() || force;
    let need_ytdlp = ytdlp_found.is_none() || force;

    if !need_ffmpeg && !need_ytdlp {
        send_progress(&tx, 1.0, "All tools (FFmpeg, FFprobe, yt-dlp) are already installed!");
        send_log(&tx, "All tools are already present and ready.");
        return Ok(());
    }

    let bin_dir = local_bin_dir();
    if need_ffmpeg {
        download_and_extract_ffmpeg(&bin_dir, &cancel, &tx)?;
    }
    if need_ytdlp {
        download_ytdlp(&bin_dir, &cancel, &tx)?;
    }

    send_progress(&tx, 1.0, "Tools setup complete! FFmpeg & yt-dlp are ready.");
    send_log(&tx, "Tools setup finished: FFmpeg & yt-dlp are ready.");
    Ok(())
}

fn configure_device_env(device: ComputeDevice) {
    // SAFETY: Called before starting ORT session creation on the worker thread.
    unsafe {
        match device {
            ComputeDevice::Auto => {
                env::remove_var("STEMMER_FORCE_CPU");
                env::remove_var("STEMMER_EP_FORCE");
            }
            ComputeDevice::Cpu => {
                env::set_var("STEMMER_FORCE_CPU", "1");
                env::set_var("STEMMER_EP_FORCE", "cpu");
            }
            ComputeDevice::DirectML => {
                env::remove_var("STEMMER_FORCE_CPU");
                env::set_var("STEMMER_EP_FORCE", "directml");
            }
            ComputeDevice::CoreML => {
                env::remove_var("STEMMER_FORCE_CPU");
                env::set_var("STEMMER_EP_FORCE", "coreml");
            }
        }
    }
}

fn move_or_copy(src: &Path, dst: &Path) -> Result<(), String> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    fs::copy(src, dst)
        .map(|_| ())
        .map_err(|e| format!("Failed to write {}: {e}", dst.display()))
}

pub fn process_media(
    job: &StemsJob,
    cancel: Arc<AtomicBool>,
    tx: Option<Sender<StemsEvent>>,
) -> Result<DerivedOutputs, String> {
    let ffmpeg_bin = get_ffmpeg_path().ok_or_else(|| {
        "FFmpeg was not found on your system (checked ./bin and PATH). Click 'Check / Download Tools' to install FFmpeg & yt-dlp automatically.".to_owned()
    })?;

    fs::create_dir_all(&job.output_dir)
        .map_err(|e| format!("Failed to create output directory: {e}"))?;

    let temp_dir = env::temp_dir().join(format!("usc_stems_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp dir: {e}"))?;

    let result = (|| -> Result<DerivedOutputs, String> {
        send_progress(&tx, 0.02, "Preparing media pipeline…");
        let media_file_buf = resolve_input_media(&job.source_input, &temp_dir, &cancel, &tx)?;
        let media_file = media_file_buf.as_path();

        let fallback_stem = media_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("song");
        let base_name_buf = resolve_base_name(&job.artist, &job.title, fallback_stem);
        let base_name = base_name_buf.as_str();
        let is_video = has_video_stream(media_file);

        // Stage outputs in temp_dir first so cancelling mid-run leaves output_dir untouched
        let song_dir = resolve_song_output_dir(&job.output_dir, base_name);
        let staged = derive_output_paths(&temp_dir, base_name, is_video);
        let mut final_derived = derive_output_paths(&song_dir, base_name, is_video);
        final_derived.artist = job.artist.trim().to_owned();
        final_derived.title = job.title.trim().to_owned();

        send_log(
            &tx,
            format!(
                "Processing media: {} ({})",
                media_file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("input"),
                if is_video { "video+audio" } else { "audio-only" }
            ),
        );

        std::thread::scope(|s| -> Result<(), String> {
            let tx_vid = tx.clone();
            let ffmpeg_vid = ffmpeg_bin.clone();
            let staged_mp4 = staged.muted_mp4.clone();
            let preset = job.preset;
            let cancel_vid = cancel.clone();

            let vid_handle = s.spawn(move || -> Result<(), String> {
                let Some(out_mp4) = staged_mp4 else {
                    send_log(&tx_vid, "[Video] Audio-only input; skipping muted video.");
                    return Ok(());
                };
                send_log(
                    &tx_vid,
                    format!(
                        "[Video] Encoding muted background video: {} ({})",
                        out_mp4.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                        preset.label()
                    ),
                );
                let args = build_ffmpeg_muted_video_args(media_file, &out_mp4, preset);
                run_ffmpeg(&ffmpeg_vid, &args, &cancel_vid, "Muted video encoding")?;
                send_log(
                    &tx_vid,
                    format!(
                        "[Video] Muted video ready: {}",
                        out_mp4.file_name().and_then(|n| n.to_str()).unwrap_or("")
                    ),
                );
                Ok(())
            });

            let tx_aud = tx.clone();
            let ffmpeg_aud = ffmpeg_bin.clone();
            let staged_aud = staged.clone();
            let temp_aud = temp_dir.clone();
            let device = job.device;
            let cancel_aud = cancel.clone();

            let aud_handle = s.spawn(move || -> Result<(), String> {
                send_log(
                    &tx_aud,
                    format!(
                        "[Audio] Encoding master MP3 (-q:a 0): {}",
                        staged_aud.mp3.file_name().and_then(|n| n.to_str()).unwrap_or("")
                    ),
                );
                send_progress(&tx_aud, 0.05, "Encoding master MP3…");
                let mp3_args = build_ffmpeg_mp3_args(media_file, &staged_aud.mp3);
                run_ffmpeg(&ffmpeg_aud, &mp3_args, &cancel_aud, "Master MP3 encoding")?;

                let pcm_wav = temp_aud.join(format!("{base_name}_pcm44k.wav"));
                send_log(&tx_aud, "[Audio] Extracting 44.1 kHz stereo PCM for HTDemucs…");
                send_progress(&tx_aud, 0.10, "Extracting 44.1 kHz stereo PCM…");
                let wav_args = build_ffmpeg_pcm_wav_args(media_file, &pcm_wav);
                run_ffmpeg(&ffmpeg_aud, &wav_args, &cancel_aud, "PCM WAV extraction")?;

                configure_device_env(device);

                let tx_dl = tx_aud.clone();
                set_download_progress_callback(move |downloaded, total| {
                    if total > 0 {
                        let pct = downloaded as f32 / total as f32;
                        send_progress(
                            &tx_dl,
                            pct.mul_add(0.15, 0.10),
                            format!(
                                "Downloading HTDemucs model ({:.1} / {:.1} MB)…",
                                downloaded as f64 / 1_048_576.0,
                                total as f64 / 1_048_576.0
                            ),
                        );
                    }
                });

                let stems_dir = temp_aud.join("stems");
                fs::create_dir_all(&stems_dir)
                    .map_err(|e| format!("Failed to create stems dir: {e}"))?;

                send_log(
                    &tx_aud,
                    format!(
                        "[Audio] Running HTDemucs separation (device: {})…",
                        device.label()
                    ),
                );
                let split_res =
                    split_file_cancellable(&pcm_wav, &stems_dir, &cancel_aud, &tx_aud)?;

                send_progress(&tx_aud, 0.90, "Encoding Vocals & Instrumental MP3s…");
                send_log(
                    &tx_aud,
                    "[Audio] Encoding [Vocals] and peak-limited [Instrumental] MP3s…",
                );

                let vocals_wav = Path::new(&split_res.vocals_path);
                let drums_wav = Path::new(&split_res.drums_path);
                let bass_wav = Path::new(&split_res.bass_path);
                let other_wav = Path::new(&split_res.other_path);

                let voc_args = build_ffmpeg_mp3_args(vocals_wav, &staged_aud.vocals);
                run_ffmpeg(&ffmpeg_aud, &voc_args, &cancel_aud, "Vocals MP3 encoding")?;

                let inst_args = build_ffmpeg_inst_mix_args(
                    &[drums_wav, bass_wav, other_wav],
                    &staged_aud.instrumental,
                );
                run_ffmpeg(
                    &ffmpeg_aud,
                    &inst_args,
                    &cancel_aud,
                    "Instrumental mix & MP3 encoding",
                )?;

                send_log(
                    &tx_aud,
                    format!(
                        "[Audio] Stems ready: {}, {}",
                        staged_aud
                            .vocals
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(""),
                        staged_aud
                            .instrumental
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                    ),
                );
                Ok(())
            });

            let aud_res = aud_handle
                .join()
                .map_err(|_| "Audio worker thread panicked".to_owned())?;
            if aud_res.is_err() {
                // Signal the video worker to abort immediately too if audio failed/cancelled
                cancel.store(true, Ordering::Relaxed);
            }
            let vid_res = vid_handle
                .join()
                .map_err(|_| "Video worker thread panicked".to_owned())?;

            aud_res?;
            vid_res?;
            Ok(())
        })?;

        check_cancelled(&cancel)?;
        fs::create_dir_all(&song_dir)
            .map_err(|e| format!("Failed to create song output folder {}: {e}", song_dir.display()))?;
        move_or_copy(&staged.mp3, &final_derived.mp3)?;
        if let (Some(src_mp4), Some(dst_mp4)) = (&staged.muted_mp4, &final_derived.muted_mp4) {
            move_or_copy(src_mp4, dst_mp4)?;
        }
        move_or_copy(&staged.vocals, &final_derived.vocals)?;
        move_or_copy(&staged.instrumental, &final_derived.instrumental)?;

        send_progress(&tx, 1.0, "Media & stem preparation complete!");
        send_log(&tx, "Media & stem preparation complete!");
        Ok(final_derived)
    })();

    let _ = fs::remove_dir_all(&temp_dir);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_paths_for_video_and_audio_only() {
        let dir = Path::new("/tmp/out");
        let with_vid = derive_output_paths(dir, "MySong", true);
        assert_eq!(with_vid.mp3, dir.join("MySong.mp3"));
        assert_eq!(with_vid.muted_mp4, Some(dir.join("MySong_muted.mp4")));
        assert_eq!(with_vid.vocals, dir.join("MySong [Vocals].mp3"));
        assert_eq!(
            with_vid.instrumental,
            dir.join("MySong [Instrumental].mp3")
        );

        let audio_only = derive_output_paths(dir, "MySong", false);
        assert_eq!(audio_only.muted_mp4, None);
    }

    #[test]
    fn video_extension_and_url_detection() {
        assert_eq!(is_video_extension(Path::new("clip.mp4")), Some(true));
        assert_eq!(is_video_extension(Path::new("clip.MKV")), Some(true));
        assert_eq!(is_video_extension(Path::new("track.flac")), Some(false));
        assert_eq!(is_video_extension(Path::new("track.MP3")), Some(false));

        assert!(is_url("https://example.com/watch?v=sample123"));
        assert!(is_url("  HTTP://example.com/video.mp4 "));
        assert!(is_url("www.example.com/watch?v=sample123"));
        assert_eq!(
            normalize_url(" www.example.com/watch?v=1 ").as_deref(),
            Some("https://www.example.com/watch?v=1")
        );
        assert_eq!(
            normalize_url_input("example.com/track"),
            "https://example.com/track"
        );
        assert_eq!(
            normalize_url_input("http://example.com/track"),
            "http://example.com/track"
        );
        assert!(!is_url("C:\\Music\\song.mp4"));
        assert!(!is_url("clip.mp4"));
    }

    #[test]
    fn ytdlp_args_include_ffmpeg_location_and_mp4_merge() {
        let ff = Path::new("/opt/usc/bin/ffmpeg");
        let out = Path::new("/tmp/dl");
        let args = build_ytdlp_args(Some(ff), "https://example.com/media/abc", out);
        assert!(args.contains(&"--no-playlist".to_owned()));
        assert!(args.contains(&"bv*+ba/b".to_owned()));
        assert!(args.contains(&"mp4".to_owned()));
        assert!(args.windows(2).any(|w| w == ["--ffmpeg-location", "/opt/usc/bin"]));
        assert_eq!(args.last().map(String::as_str), Some("https://example.com/media/abc"));
    }

    #[test]
    fn resolve_base_name_uses_artist_and_title_and_sanitizes() {
        assert_eq!(
            resolve_base_name("YOASOBI", "Idol", "yt_12345"),
            "YOASOBI - Idol"
        );
        assert_eq!(
            resolve_base_name("A/B", "Song: *Test*?", "raw"),
            "A_B - Song_ _Test__"
        );
        assert_eq!(resolve_base_name("", "Only Title", "raw"), "Only Title");
        assert_eq!(resolve_base_name("", "", "yt [Official MV]"), "yt [Official MV]");
    }

    #[test]
    fn resolve_song_output_dir_creates_subfolder_without_double_nesting() {
        let base = Path::new("/music/output");
        assert_eq!(
            resolve_song_output_dir(base, "YOASOBI - Idol"),
            Path::new("/music/output/YOASOBI - Idol")
        );
        let already = Path::new("/music/output/YOASOBI - Idol");
        assert_eq!(
            resolve_song_output_dir(already, "YOASOBI - Idol"),
            Path::new("/music/output/YOASOBI - Idol")
        );
    }

    #[test]
    fn cancel_flag_aborts_immediately() {
        let flag = AtomicBool::new(true);
        assert_eq!(check_cancelled(&flag), Err(CANCELLED_MSG.to_owned()));
    }

    #[test]
    fn ffmpeg_command_builders_match_vid2stems_spec() {
        let vid_args = build_ffmpeg_muted_video_args(
            Path::new("in.mp4"),
            Path::new("out_muted.mp4"),
            VideoPreset::Compact,
        );
        assert!(vid_args.contains(&"-an".to_owned()));
        assert!(vid_args.contains(&"scale=-2:min(720\\,ih)".to_owned()));
        assert!(vid_args.contains(&"24".to_owned()));
        assert!(vid_args.contains(&"yuv420p".to_owned()));

        let src_args = build_ffmpeg_muted_video_args(
            Path::new("in.mp4"),
            Path::new("out_muted.mp4"),
            VideoPreset::Source,
        );
        assert!(!src_args.contains(&"-vf".to_owned()));
        assert!(src_args.contains(&"18".to_owned()));

        let pcm_args = build_ffmpeg_pcm_wav_args(Path::new("in.mp4"), Path::new("pcm.wav"));
        assert!(pcm_args.windows(2).any(|w| w == ["-ar", "44100"]));
        assert!(pcm_args.windows(2).any(|w| w == ["-c:a", "pcm_s16le"]));

        let mp3_args = build_ffmpeg_mp3_args(Path::new("pcm.wav"), Path::new("out.mp3"));
        assert!(mp3_args.windows(2).any(|w| w == ["-codec:a", "libmp3lame"]));
        assert!(mp3_args.windows(2).any(|w| w == ["-q:a", "0"]));

        let mix_args = build_ffmpeg_inst_mix_args(
            &[
                Path::new("drums.wav"),
                Path::new("bass.wav"),
                Path::new("other.wav"),
            ],
            Path::new("inst.mp3"),
        );
        assert!(mix_args.iter().any(|a| a.contains(
            "[0:a][1:a][2:a]amix=inputs=3:normalize=0,alimiter=limit=0.99:attack=5:release=50"
        )));
    }
}
