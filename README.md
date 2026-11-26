# UltraStar-Creator

[![Rust CI](https://github.com/HNTA-Club/UltraStar-Creator/actions/workflows/rust.yml/badge.svg)](https://github.com/HNTA-Club/UltraStar-Creator/actions/workflows/rust.yml)
[![License](https://img.shields.io/badge/license-GPLv2-blue.svg)](LICENSE)

![UltraStar-Creator Logo](setup/UltraStar-Creator128.png)

### 1. About
UltraStar-Creator (`usc`) is a free and open-source karaoke song creation tool written in Rust with `egui` / `eframe`, running natively on Windows, Linux, and macOS. Beyond classic real-time syllable tapping and duet support, this fork includes local AI stem separation (HTDemucs), Japanese & Hepburn Romaji mora splitting, drag-and-drop `[CO].jpg` / `[BG].jpg` artwork conversion, and optional external media fetching via `yt-dlp`.

### 2. Releases & Versioning
This repository is an enhanced distribution of UltraStar-Creator maintained by **HNTA-Club**.

#### Versioning Scheme
Releases follow [Semantic Versioning](https://semver.org/) extended with a fork identifier:
```text
v<major>.<minor>.<patch>-takai.<revision>
```
Example: **`v2.0.0-takai.1`**
- **Major Version (`2.0.0`)**: Marks the native Rust + `egui` rewrite generation following the `1.x` C++/Qt releases.
- **Fork Suffix (`-takai.<revision>`)**: Identifies releases specific to this distribution. This ensures version tags never collide with upstream tags and clearly indicates the build origin to end users.
- **Automated CI/CD**: Pushing a `v*` version tag triggers GitHub Actions to build, test, verify, and package standalone portable archives across Windows, Linux, and macOS (`.zip` / `.tar.gz` / `.app`) and draft a release on [GitHub Releases](https://github.com/HNTA-Club/UltraStar-Creator/releases).

### 3. Controls & Hotkeys

| Key / Shortcut | Action | Description |
| :--- | :--- | :--- |
| <kbd>Space</kbd> / <kbd>V</kbd> | **Tap Syllable / Start** | Hold down for the duration of the syllable. Also starts/resumes playback. |
| <kbd>X</kbd> | **Undo Syllable** | Erases the last tapped syllable and rewinds audio by 1.0s. |
| <kbd>Z</kbd> | **Undo Row (Live)** | Erases all words on the current row and rewinds audio by 2.0s to re-take in rhythm. |
| <kbd>Ctrl+Z</kbd> | **Undo Row & Pause** | Erases all words on the current row, rewinds by 2.0s, and pauses playback. |
| <kbd>Ctrl+P</kbd> | **Play / Pause** | Toggles audio playback. |
| <kbd>S</kbd> | **Stop** | Stops playback and finalizes tapping. |
| <kbd>Ctrl+R</kbd> | **Reset Song** | Resets tapping progress and rewinds to the beginning (when stopped). |
| <kbd>Ctrl+S</kbd> | **Save** | Opens the save dialog to write the UltraStar `.txt` file. |
| <kbd>F5</kbd> | **Refresh Media** | Rescans the directory to update cover, background, video, and audio files. |
| <kbd>Ctrl+Q</kbd> | **Exit** | Quits the application. |

### 4. Command-Line Parameters
- `usc <audio-file>`: Launches UltraStar-Creator with `<audio-file>` preloaded.
- `usc --check-bass`: Verifies that `BASS` and `BASS_FX` native libraries load cleanly and prints their versions.

### 5. Build and Run

#### Prerequisites
1. **Rust stable toolchain** (`cargo` + `clippy`, e.g. via [rustup](https://rustup.rs/)).
2. **BASS & BASS_FX shared libraries** placed in `lib/<windows|linux|macos>/` (or next to the compiled executable):
   - Windows: `bass.dll`, `bass_fx.dll`
   - Linux: `libbass.so`, `libbass_fx.so`
   - macOS: `libbass.dylib`, `libbass_fx.dylib`
3. **Linux system development packages** (Ubuntu / Debian example):
   ```bash
   sudo apt-get install -y pkg-config libssl-dev libxkbcommon-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libgl1-mesa-dev
   ```

#### Test, Lint & Build
From the repository root:
```bash
cd usc
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```
On Windows, if `ort-sys` needs target build subdirectories before the very first build, ensure `usc/target/{debug,release}/build/{examples,deps}` exist.

To run locally in development mode (automatically picks up `lib/<os>/` and `syllabification/*.txt`):
```bash
cargo run --release
```

### 6. Third-Party Components & Licenses

- **[BASS Audio Library & BASS_FX](https://www.un4seen.com/bass.html)** — Proprietary license, **free for non-commercial use only** (Un4seen Developments Ltd.). Handles audio decoding, playback, BPM detection, and pitch-preserved tempo control.
- **[HTDemucs](https://github.com/facebookresearch/demucs) (via [stem-splitter-core](https://github.com/gentij/stem-splitter-core) & ONNX Runtime)** — MIT License (Demucs) / MIT or Apache-2.0 (`stem-splitter-core`). Runs locally to separate audio into Vocals and Instrumental stems; model weights are downloaded on first use and cached locally.
- **[FFmpeg / FFprobe](https://ffmpeg.org)** — LGPL v2.1+ / GPL v2+ (depending on build). Used for audio extraction, muted `.mp4` video encoding, and peak-limited instrumental MP3 output. Not bundled in release archives; resolved from `PATH` or downloaded on demand via the in-app **Check / Download Tools** button.
- **[yt-dlp](https://github.com/yt-dlp/yt-dlp)** — The Unlicense (Public Domain). Optional external media fetcher supporting 1,000+ web platforms and direct media URLs. Not bundled in release archives; resolved from `PATH` or downloaded on demand via **Check / Download Tools**. Only use it with media you own, have permission to download, or that is freely licensed — users are responsible for respecting copyright and applicable site Terms of Service.

### 7. Contribute
Feel free to fork this project, modify it to your heart's content, and open pull requests to this repository for additional features, improvements, or clean-ups.
