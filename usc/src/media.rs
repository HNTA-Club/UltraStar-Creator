use std::fs;
use std::path::{Path, PathBuf};

use lofty::prelude::{Accessor, TaggedFileExt};
use whatlang::Lang;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tags {
    pub artist: String,
    pub title: String,
    pub genre: String,
    pub year: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderMedia {
    pub images: Vec<String>,
    pub videos: Vec<String>,
    pub audios: Vec<String>,
    pub cover: Option<String>,
    pub background: Option<String>,
    pub video: Option<String>,
    pub instrumental: Option<String>,
    pub vocals: Option<String>,
}

pub fn read_tags(path: &Path) -> Tags {
    let mut tags = Tags::default();

    if let Ok(tagged) = lofty::read_from_path(path)
        && let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag())
    {
        tags.artist = tag.artist().map(|s| s.trim().to_string()).unwrap_or_default();
        tags.title = tag.title().map(|s| s.trim().to_string()).unwrap_or_default();
        tags.genre = tag.genre().map(|s| s.trim().to_string()).unwrap_or_default();
        tags.year = tag
            .year()
            .filter(|&y| y > 0)
            .map(|y| y.to_string())
            .unwrap_or_default();
    }

    if (tags.artist.is_empty() || tags.title.is_empty())
        && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        && let Some((left, right)) = stem.split_once('-')
    {
        if tags.artist.is_empty() {
            tags.artist = left.trim().to_string();
        }
        if tags.title.is_empty() {
            tags.title = right.trim().to_string();
        }
    }

    tags
}

pub fn detect_language(text: &str) -> Option<&'static str> {
    if crate::romaji::contains_japanese(text) {
        return Some("Japanese");
    }
    let info = whatlang::detect(text)?;
    match info.lang() {
        Lang::Ara => Some("Arabic"),
        Lang::Cmn => Some("Chinese"),
        Lang::Hrv => Some("Croatian"),
        Lang::Ces => Some("Czech"),
        Lang::Dan => Some("Danish"),
        Lang::Nld => Some("Dutch"),
        Lang::Eng => Some("English"),
        Lang::Fin => Some("Finnish"),
        Lang::Fra => Some("French"),
        Lang::Deu => Some("German"),
        Lang::Hin => Some("Hindi"),
        Lang::Ita => Some("Italian"),
        Lang::Jpn => Some("Japanese"),
        Lang::Kor => Some("Korean"),
        Lang::Lat => Some("Latin"),
        Lang::Nob => Some("Norwegian"),
        Lang::Pol => Some("Polish"),
        Lang::Por => Some("Portuguese"),
        Lang::Ron => Some("Romanian"),
        Lang::Rus => Some("Russian"),
        Lang::Slk => Some("Slovak"),
        Lang::Slv => Some("Slowenian"),
        Lang::Spa => Some("Spanish"),
        Lang::Swe => Some("Swedish"),
        Lang::Tur => Some("Turkish"),
        _ => None,
    }
}

pub fn scan_folder(audio_path: &Path) -> FolderMedia {
    let mut media = FolderMedia::default();
    let Some(dir) = audio_path.parent() else {
        return media;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return media;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some(ext) = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
        else {
            continue;
        };

        if is_image_ext(&ext) {
            media.images.push(name);
        } else if is_video_ext(&ext) {
            media.videos.push(name.clone());
            media.audios.push(name);
        } else if is_audio_only_ext(&ext) {
            media.audios.push(name);
        }
    }

    sort_ci(&mut media.images);
    sort_ci(&mut media.videos);
    sort_ci(&mut media.audios);

    media.cover = pick_cover(&media.images);
    media.background = pick_background(&media.images);
    media.video = find_ci(&media.videos, &["_muted", "[vid]"])
        .or_else(|| media.videos.first().cloned());

    let main_name = audio_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    media.instrumental = pick_audio_match(
        &media.audios,
        main_name,
        &["instrumental", "karaoke", "[inst]", "inst.", "_inst", "-inst"],
    );
    media.vocals = pick_audio_match(
        &media.audios,
        main_name,
        &["vocals", "acapella", "accapella", "vocal", "vox", "[voc]"],
    );

    media
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSlot {
    Cover,
    Background,
}

impl ImageSlot {
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Cover => "[CO]",
            Self::Background => "[BG]",
        }
    }
}

pub fn is_near_square(width: u32, height: u32) -> bool {
    if width == 0 || height == 0 {
        return false;
    }
    let ratio = width as f64 / height as f64;
    (0.85..=1.18).contains(&ratio)
}

pub fn save_image_as_slot(
    bytes: &[u8],
    song_dir: &Path,
    base_name: &str,
    slot: ImageSlot,
) -> Result<(String, u32, u32), String> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| format!("Failed to decode dropped image: {e}"))?;
    let w = img.width();
    let h = img.height();
    let clean_base = crate::stems::sanitize_filename_part(base_name);
    let stem = if clean_base.is_empty() {
        "song"
    } else {
        clean_base.as_str()
    };
    let filename = format!("{stem} {}.jpg", slot.suffix());
    let dest = song_dir.join(&filename);
    img.to_rgb8()
        .save_with_format(&dest, image::ImageFormat::Jpeg)
        .map_err(|e| format!("Failed to save {}: {e}", dest.display()))?;
    Ok((filename, w, h))
}

pub fn dictionaries_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        if let Some(parent) = exe_dir.parent() {
            candidates.push(parent.join("Resources"));
        }
        candidates.push(exe_dir.to_path_buf());
        candidates.push(exe_dir.join("syllabification"));
    }
    candidates.push(PathBuf::from("syllabification"));

    candidates.into_iter().find(|dir| has_txt_dictionary(dir))
}

fn has_txt_dictionary(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let p = e.path();
        p.is_file()
            && p.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"))
    })
}

fn sort_ci(list: &mut [String]) {
    list.sort_by(|a, b| {
        a.to_ascii_lowercase()
            .cmp(&b.to_ascii_lowercase())
            .then_with(|| a.cmp(b))
    });
}

fn is_image_ext(ext: &str) -> bool {
    matches!(ext, "jpg" | "jpeg" | "png")
}

fn is_audio_only_ext(ext: &str) -> bool {
    matches!(ext, "m4a" | "mp3" | "ogg")
}

fn is_video_ext(ext: &str) -> bool {
    matches!(
        ext,
        "avi"
            | "divx"
            | "flv"
            | "m2v"
            | "m4v"
            | "mkv"
            | "mov"
            | "mp4"
            | "mpeg"
            | "mpg"
            | "ogm"
            | "ts"
            | "vob"
            | "webm"
            | "wmv"
    )
}

fn pick_cover(images: &[String]) -> Option<String> {
    if let Some(found) = find_ci(images, &["[co]", "cover"]) {
        return Some(found);
    }
    if images.len() == 1 {
        images.first().cloned()
    } else if images.len() > 1 {
        images.get(1).cloned()
    } else {
        None
    }
}

fn pick_background(images: &[String]) -> Option<String> {
    find_ci(images, &["[bg]", "background", "back"]).or_else(|| images.first().cloned())
}

fn pick_audio_match(audios: &[String], main_name: &str, patterns: &[&str]) -> Option<String> {
    for name in audios {
        if name.eq_ignore_ascii_case(main_name) {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if patterns.iter().any(|p| lower.contains(p)) {
            return Some(name.clone());
        }
    }
    None
}

fn find_ci(items: &[String], patterns: &[&str]) -> Option<String> {
    for name in items {
        let lower = name.to_ascii_lowercase();
        if patterns.iter().any(|p| lower.contains(p)) {
            return Some(name.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn minimal_wav_bytes() -> Vec<u8> {
        let mut b = Vec::with_capacity(48);
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&44100u32.to_le_bytes());
        b.extend_from_slice(&88200u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(&[0u8; 4]);
        b
    }

    #[test]
    fn read_tags_filename_fallback_and_multi_hyphen() {
        let dir = TempDir::new().unwrap();
        let p1 = dir.path().join("Artist - Song - Remix.mp3");
        fs::write(&p1, b"").unwrap();
        assert_eq!(
            read_tags(&p1),
            Tags {
                artist: "Artist".into(),
                title: "Song - Remix".into(),
                genre: String::new(),
                year: String::new(),
            }
        );

        let p2 = dir.path().join("track01.mp3");
        fs::write(&p2, b"").unwrap();
        assert_eq!(read_tags(&p2), Tags::default());
    }

    #[test]
    fn read_tags_from_wav_with_lofty_tags() {
        use lofty::config::WriteOptions;
        use lofty::prelude::{Accessor, TagExt};
        use lofty::tag::{Tag, TagType};

        let dir = TempDir::new().unwrap();
        let wav = dir.path().join("ignored.wav");
        fs::write(&wav, minimal_wav_bytes()).unwrap();

        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_artist("Märchen".into());
        tag.set_title("Été".into());
        tag.set_genre("Pop".into());
        tag.set_year(2024);
        tag.save_to_path(&wav, WriteOptions::default()).unwrap();

        assert_eq!(
            read_tags(&wav),
            Tags {
                artist: "Märchen".into(),
                title: "Été".into(),
                genre: "Pop".into(),
                year: "2024".into(),
            }
        );
    }

    #[test]
    fn detect_language_maps_known_languages() {
        assert_eq!(
            detect_language("This is an English song about summer nights and dancing."),
            Some("English")
        );
        assert_eq!(
            detect_language("Das ist ein deutsches Lied über die Nacht und das Meer."),
            Some("German")
        );
        assert_eq!(
            detect_language("Ceci est une chanson française sur l'été et la musique."),
            Some("French")
        );
        assert_eq!(
            detect_language("Esta es una canción en español sobre el verano y el mar."),
            Some("Spanish")
        );
        assert_eq!(detect_language("太陽のもと 旅立とう"), Some("Japanese"));
        assert_eq!(detect_language(""), None);
    }

    #[test]
    fn scan_folder_prefers_muted_video_and_vid2stems_outputs() {
        let dir = TempDir::new().unwrap();
        let main_mp3 = dir.path().join("song.mp3");
        for name in [
            "song.mp3",
            "song.mp4",
            "song_muted.mp4",
            "song [Vocals].mp3",
            "song [Instrumental].mp3",
        ] {
            fs::write(dir.path().join(name), b"").unwrap();
        }
        let got = scan_folder(&main_mp3);
        assert_eq!(got.video.as_deref(), Some("song_muted.mp4"));
        assert_eq!(got.vocals.as_deref(), Some("song [Vocals].mp3"));
        assert_eq!(got.instrumental.as_deref(), Some("song [Instrumental].mp3"));
    }

    #[test]
    fn scan_folder_filters_and_auto_selects() {
        let dir = TempDir::new().unwrap();
        let main_mp3 = dir.path().join("main - song (instrumental).mp3");
        for name in [
            "main - song (instrumental).mp3",
            "song_inst.mp3",
            "song_vox.ogg",
            "Front [CO].jpg",
            "Bg_image.png",
            "clip.mp4",
            "notes.txt",
        ] {
            fs::write(dir.path().join(name), b"").unwrap();
        }

        let got = scan_folder(&main_mp3);
        assert_eq!(got.images, vec!["Bg_image.png", "Front [CO].jpg"]);
        assert_eq!(got.videos, vec!["clip.mp4"]);
        assert_eq!(
            got.audios,
            vec![
                "clip.mp4",
                "main - song (instrumental).mp3",
                "song_inst.mp3",
                "song_vox.ogg",
            ]
        );
        assert_eq!(got.cover.as_deref(), Some("Front [CO].jpg"));
        assert_eq!(got.background.as_deref(), Some("Bg_image.png"));
        assert_eq!(got.video.as_deref(), Some("clip.mp4"));
        assert_eq!(got.instrumental.as_deref(), Some("song_inst.mp3"));
        assert_eq!(got.vocals.as_deref(), Some("song_vox.ogg"));
    }

    #[test]
    fn scan_folder_image_fallback_without_tags() {
        let dir1 = TempDir::new().unwrap();
        let mp3_1 = dir1.path().join("song.mp3");
        fs::write(&mp3_1, b"").unwrap();
        fs::write(dir1.path().join("only.jpg"), b"").unwrap();
        let g1 = scan_folder(&mp3_1);
        assert_eq!(g1.cover.as_deref(), Some("only.jpg"));
        assert_eq!(g1.background.as_deref(), Some("only.jpg"));

        let dir2 = TempDir::new().unwrap();
        let mp3_2 = dir2.path().join("song.mp3");
        fs::write(&mp3_2, b"").unwrap();
        fs::write(dir2.path().join("a.jpg"), b"").unwrap();
        fs::write(dir2.path().join("b.jpg"), b"").unwrap();
        let g2 = scan_folder(&mp3_2);
        assert_eq!(g2.cover.as_deref(), Some("b.jpg"));
        assert_eq!(g2.background.as_deref(), Some("a.jpg"));
    }

    #[test]
    fn near_square_and_save_image_as_jpeg_slot() {
        assert!(is_near_square(640, 640));
        assert!(is_near_square(600, 540));
        assert!(!is_near_square(1280, 720));
        assert!(!is_near_square(0, 100));

        let dir = TempDir::new().unwrap();
        let img = image::RgbImage::from_pixel(4, 4, image::Rgb([120, 40, 200]));
        let mut png_bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .unwrap();

        let (name, w, h) =
            save_image_as_slot(&png_bytes, dir.path(), "YOASOBI - Idol", ImageSlot::Cover).unwrap();
        assert_eq!(name, "YOASOBI - Idol [CO].jpg");
        assert_eq!((w, h), (4, 4));
        assert!(dir.path().join(&name).is_file());
    }
}
