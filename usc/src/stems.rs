use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::SystemTime;

use stem_splitter_core::{
    SplitOptions, SplitProgress, set_download_progress_callback, set_split_progress_callback,
    split_file,
};

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
    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact (720p, CRF 24)",
            Self::Balanced => "Balanced (720p, CRF 22)",
            Self::High => "High (1080p, CRF 20)",
            Self::Source => "Source (Original res, CRF 18)",
        }
    }

    pub fn crf(self) -> u8 {
        match self {
            Self::Compact => 24,
            Self::Balanced => 22,
            Self::High => 20,
            Self::Source => 18,
        }
    }

    pub fn max_height(self) -> Option<u32> {
        match self {
            Self::Compact | Self::Balanced => Some(720),
            Self::High => Some(1080),
            Self::Source => None,
        }
    }

    pub fn x264_preset(self) -> &'static str {
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
    pub fn label(self) -> &'static str {
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
}

#[derive(Debug, Clone)]
pub struct StemsJob {
    pub source_input: String,
    pub output_dir: PathBuf,
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

pub fn is_url(input: &str) -> bool {
    let trimmed = input.trim().to_ascii_lowercase();
    trimmed.starts_with("http://") || trimmed.starts_with("https://")
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
    if let Some(false) = is_video_extension(media_path) {
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

pub fn derive_output_paths(output_dir: &Path, base_name: &str, has_video: bool) -> DerivedOutputs {
    DerivedOutputs {
        mp3: output_dir.join(format!("{base_name}.mp3")),
        muted_mp4: has_video.then(|| output_dir.join(format!("{base_name}_muted.mp4"))),
        vocals: output_dir.join(format!("{base_name} [Vocals].mp3")),
        instrumental: output_dir.join(format!("{base_name} [Instrumental].mp3")),
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
    tx: &Option<Sender<StemsEvent>>,
) -> Result<PathBuf, String> {
    let ytdlp_exe = get_ytdlp_path().ok_or_else(|| {
        "URL downloading requires yt-dlp, which was not found.\nClick 'Download / Setup Tools' to install portable FFmpeg & yt-dlp automatically.".to_owned()
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
    configure_no_window(&mut cmd);
    cmd.args(&args);

    let out = cmd
        .output()
        .map_err(|e| format!("Failed to launch yt-dlp: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = stderr.trim();
        return Err(format!(
            "yt-dlp download failed: {}",
            if msg.is_empty() { "unknown error" } else { msg }
        ));
    }

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
    tx: &Option<Sender<StemsEvent>>,
) -> Result<PathBuf, String> {
    let trimmed = source_input.trim();
    if trimmed.is_empty() {
        return Err("Source input is empty.".to_owned());
    }
    if is_url(trimmed) {
        return download_url_media(trimmed, temp_dir, tx);
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

fn run_ffmpeg(ffmpeg_bin: &Path, args: &[String], step_label: &str) -> Result<(), String> {
    let mut cmd = Command::new(ffmpeg_bin);
    configure_no_window(&mut cmd);
    cmd.args(args);
    let out = cmd
        .output()
        .map_err(|e| format!("{step_label}: failed to execute ffmpeg: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(4).collect();
        let summary = tail.into_iter().rev().collect::<Vec<_>>().join(" | ");
        return Err(format!("{step_label} failed: {summary}"));
    }
    Ok(())
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

pub fn download_and_extract_ffmpeg(
    target_bin_dir: &Path,
    tx: &Option<Sender<StemsEvent>>,
) -> Result<(), String> {
    fs::create_dir_all(target_bin_dir)
        .map_err(|e| format!("Failed to create bin dir {}: {e}", target_bin_dir.display()))?;

    let url = if cfg!(target_os = "windows") {
        "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
    } else if cfg!(target_os = "linux") {
        "https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz"
    } else {
        return Err(
            "On macOS, please install FFmpeg via Homebrew: `brew install ffmpeg`".to_owned(),
        );
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
    configure_no_window(&mut cmd);
    cmd.arg("-xf")
        .arg(&archive_path)
        .arg("-C")
        .arg(&temp_extract);
    let out = cmd
        .output()
        .map_err(|e| format!("Failed to invoke system tar to extract FFmpeg: {e}"))?;
    if !out.status.success() {
        let _ = fs::remove_dir_all(&temp_extract);
        return Err(format!(
            "Archive extraction failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
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
    tx: &Option<Sender<StemsEvent>>,
) -> Result<(), String> {
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

pub fn run_tools_setup(force: bool, tx: Option<Sender<StemsEvent>>) -> Result<(), String> {
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
        download_and_extract_ffmpeg(&bin_dir, &tx)?;
    }
    if need_ytdlp {
        download_ytdlp(&bin_dir, &tx)?;
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

pub fn process_media(job: &StemsJob, tx: Option<Sender<StemsEvent>>) -> Result<DerivedOutputs, String> {
    let ffmpeg_bin = get_ffmpeg_path().ok_or_else(|| {
        "FFmpeg was not found on your system (checked ./bin and PATH). Click 'Download / Setup Tools' to install FFmpeg & yt-dlp automatically.".to_owned()
    })?;

    fs::create_dir_all(&job.output_dir)
        .map_err(|e| format!("Failed to create output directory: {e}"))?;

    let temp_dir = env::temp_dir().join(format!("usc_stems_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp dir: {e}"))?;

    let result = (|| -> Result<DerivedOutputs, String> {
        send_progress(&tx, 0.02, "Preparing media pipeline…");
        let media_file_buf = resolve_input_media(&job.source_input, &temp_dir, &tx)?;
        let media_file = media_file_buf.as_path();

        let base_name = media_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("song");
        let is_video = has_video_stream(media_file);
        let derived = derive_output_paths(&job.output_dir, base_name, is_video);

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

        std::thread::scope(|s| -> Result<DerivedOutputs, String> {
            let tx_vid = tx.clone();
            let ffmpeg_vid = ffmpeg_bin.clone();
            let derived_mp4 = derived.muted_mp4.clone();
            let preset = job.preset;

            let vid_handle = s.spawn(move || -> Result<(), String> {
                let Some(out_mp4) = derived_mp4 else {
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
                run_ffmpeg(&ffmpeg_vid, &args, "Muted video encoding")?;
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
            let derived_aud = derived.clone();
            let temp_aud = temp_dir.clone();
            let device = job.device;

            let aud_handle = s.spawn(move || -> Result<(), String> {
                send_log(
                    &tx_aud,
                    format!(
                        "[Audio] Encoding master MP3 (-q:a 0): {}",
                        derived_aud.mp3.file_name().and_then(|n| n.to_str()).unwrap_or("")
                    ),
                );
                send_progress(&tx_aud, 0.05, "Encoding master MP3…");
                let mp3_args = build_ffmpeg_mp3_args(media_file, &derived_aud.mp3);
                run_ffmpeg(&ffmpeg_aud, &mp3_args, "Master MP3 encoding")?;

                let pcm_wav = temp_aud.join(format!("{base_name}_pcm44k.wav"));
                send_log(&tx_aud, "[Audio] Extracting 44.1 kHz stereo PCM for HTDemucs…");
                send_progress(&tx_aud, 0.10, "Extracting 44.1 kHz stereo PCM…");
                let wav_args = build_ffmpeg_pcm_wav_args(media_file, &pcm_wav);
                run_ffmpeg(&ffmpeg_aud, &wav_args, "PCM WAV extraction")?;

                configure_device_env(device);

                let tx_dl = tx_aud.clone();
                set_download_progress_callback(move |downloaded, total| {
                    if total > 0 {
                        let pct = downloaded as f32 / total as f32;
                        send_progress(
                            &tx_dl,
                            0.10 + pct * 0.15,
                            format!(
                                "Downloading HTDemucs model ({:.1} / {:.1} MB)…",
                                downloaded as f64 / 1_048_576.0,
                                total as f64 / 1_048_576.0
                            ),
                        );
                    }
                });

                let tx_sp = tx_aud.clone();
                set_split_progress_callback(move |prog| match prog {
                    SplitProgress::Stage(stage) => {
                        send_log(&tx_sp, format!("[HTDemucs] Stage: {stage}"));
                    }
                    SplitProgress::Chunks {
                        done,
                        total,
                        percent,
                    } => {
                        let frac = 0.25 + (percent / 100.0) * 0.55;
                        send_progress(
                            &tx_sp,
                            frac,
                            format!("Separating stems: chunk {done}/{total} ({percent:.0}%)"),
                        );
                    }
                    SplitProgress::Writing {
                        stem,
                        done,
                        total,
                        ..
                    } => {
                        send_progress(
                            &tx_sp,
                            0.80 + (done as f32 / total.max(1) as f32) * 0.08,
                            format!("Writing raw stem: {stem} ({done}/{total})"),
                        );
                    }
                    SplitProgress::Finished => {
                        send_log(&tx_sp, "[HTDemucs] Raw stem separation finished.");
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
                let split_opts = SplitOptions {
                    output_dir: stems_dir.to_string_lossy().into_owned(),
                    model_name: "htdemucs_ort_v1".to_owned(),
                    manifest_url_override: None,
                };
                let split_res = split_file(&pcm_wav.to_string_lossy(), split_opts)
                    .map_err(|e| format!("HTDemucs separation failed: {e}"))?;

                send_progress(&tx_aud, 0.90, "Encoding Vocals & Instrumental MP3s…");
                send_log(
                    &tx_aud,
                    "[Audio] Encoding [Vocals] and peak-limited [Instrumental] MP3s…",
                );

                let vocals_wav = Path::new(&split_res.vocals_path);
                let drums_wav = Path::new(&split_res.drums_path);
                let bass_wav = Path::new(&split_res.bass_path);
                let other_wav = Path::new(&split_res.other_path);

                let voc_args = build_ffmpeg_mp3_args(vocals_wav, &derived_aud.vocals);
                run_ffmpeg(&ffmpeg_aud, &voc_args, "Vocals MP3 encoding")?;

                let inst_args = build_ffmpeg_inst_mix_args(
                    &[drums_wav, bass_wav, other_wav],
                    &derived_aud.instrumental,
                );
                run_ffmpeg(&ffmpeg_aud, &inst_args, "Instrumental mix & MP3 encoding")?;

                send_log(
                    &tx_aud,
                    format!(
                        "[Audio] Stems ready: {}, {}",
                        derived_aud
                            .vocals
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(""),
                        derived_aud
                            .instrumental
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                    ),
                );
                Ok(())
            });

            let vid_res = vid_handle
                .join()
                .map_err(|_| "Video worker thread panicked".to_owned())?;
            let aud_res = aud_handle
                .join()
                .map_err(|_| "Audio worker thread panicked".to_owned())?;

            vid_res?;
            aud_res?;

            send_progress(&tx, 1.0, "Media & stem preparation complete!");
            send_log(&tx, "Media & stem preparation complete!");
            Ok(derived)
        })
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

        assert!(is_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(is_url("  HTTP://example.com/video.mp4 "));
        assert!(!is_url("C:\\Music\\song.mp4"));
    }

    #[test]
    fn ytdlp_args_include_ffmpeg_location_and_mp4_merge() {
        let ff = Path::new("/opt/usc/bin/ffmpeg");
        let out = Path::new("/tmp/dl");
        let args = build_ytdlp_args(Some(ff), "https://youtu.be/abc", out);
        assert!(args.contains(&"--no-playlist".to_owned()));
        assert!(args.contains(&"bv*+ba/b".to_owned()));
        assert!(args.contains(&"mp4".to_owned()));
        assert!(args.windows(2).any(|w| w == ["--ffmpeg-location", "/opt/usc/bin"]));
        assert_eq!(args.last().map(String::as_str), Some("https://youtu.be/abc"));
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
