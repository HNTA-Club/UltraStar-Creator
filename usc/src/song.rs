use crate::lyrics::{Role, Syllable};
use crate::tapping::Note;

#[derive(Clone, Debug, Default)]
pub struct Header {
    pub title: String,
    pub artist: String,
    pub language: String,
    pub edition: String,
    pub genre: String,
    pub year: String,
    pub creator: String,
    pub mp3: String,
    pub instrumental: String,
    pub vocals: String,
    pub cover: String,
    pub background: String,
    pub video: String,
    pub bpm: f64,
    pub duet: Option<(String, String)>,
}

pub fn render(header: &Header, syllables: &[Syllable], notes: &[Note]) -> String {
    let mut lines: Vec<String> = Vec::new();

    let title = non_empty_or(&header.title, "Unknown Title");
    let artist = non_empty_or(&header.artist, "Unknown Artist");
    lines.push(format!("#TITLE:{title}"));
    lines.push(format!("#ARTIST:{artist}"));

    for (tag, val) in [
        ("LANGUAGE", &header.language),
        ("EDITION", &header.edition),
        ("GENRE", &header.genre),
        ("YEAR", &header.year),
        ("CREATOR", &header.creator),
        ("MP3", &header.mp3),
        ("INSTRUMENTAL", &header.instrumental),
        ("VOCALS", &header.vocals),
        ("COVER", &header.cover),
        ("BACKGROUND", &header.background),
        ("VIDEO", &header.video),
    ] {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            lines.push(format!("#{tag}:{trimmed}"));
        }
    }

    lines.push(format!("#BPM:{}", header.bpm));

    let gap_ms = if notes.is_empty() {
        0.0
    } else {
        let min_start = notes.iter().map(|n| n.start_ms).fold(f64::INFINITY, f64::min);
        let rounded = (min_start / 10.0).round() * 10.0;
        let gap = if rounded == 0.0 { 0.0 } else { rounded };
        lines.push(format!("#GAP:{gap:.0}"));
        gap
    };

    if let Some((p1, p2)) = &header.duet {
        let s1 = non_empty_or(p1, "Singer 1");
        let s2 = non_empty_or(p2, "Singer 2");
        lines.push(format!("#DUETSINGERP1:{s1}"));
        lines.push(format!("#P1:{s1}"));
        lines.push(format!("#DUETSINGERP2:{s2}"));
        lines.push(format!("#P2:{s2}"));

        lines.push("P 1".into());
        render_track(
            &mut lines,
            syllables,
            notes,
            gap_ms,
            header.bpm,
            |role| matches!(role, Role::P1 | Role::Both | Role::Solo),
        );

        lines.push("P 2".into());
        render_track(
            &mut lines,
            syllables,
            notes,
            gap_ms,
            header.bpm,
            |role| matches!(role, Role::P2 | Role::Both),
        );
    } else {
        render_track(&mut lines, syllables, notes, gap_ms, header.bpm, |_| true);
    }

    lines.push("E".into());
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let trimmed = value.trim();
    if trimmed.is_empty() { fallback } else { trimmed }
}

fn render_track(
    lines: &mut Vec<String>,
    syllables: &[Syllable],
    notes: &[Note],
    gap_ms: f64,
    bpm: f64,
    include: impl Fn(Role) -> bool,
) {
    struct RenderNote<'a> {
        syllable_idx: usize,
        start: i64,
        len: i64,
        text: &'a str,
        line_end: bool,
    }

    let mut track: Vec<RenderNote<'_>> = notes
        .iter()
        .filter_map(|n| {
            let syl = syllables.get(n.syllable)?;
            if !include(syl.role) {
                return None;
            }
            let start = (((n.start_ms - gap_ms) * bpm) / 15000.0).round() as i64;
            let len = ((n.dur_ms * bpm) / 15000.0).round() as i64;
            Some(RenderNote {
                syllable_idx: n.syllable,
                start: start.max(0),
                len: len.max(1),
                text: &syl.text,
                line_end: syl.line_end,
            })
        })
        .collect();

    if track.is_empty() {
        return;
    }

    track.sort_by_key(|rn| rn.syllable_idx);

    for i in 1..track.len() {
        let need_break = track[i - 1].line_end;
        let min_start = track[i - 1].start + if need_break { 2 } else { 1 };
        if track[i].start < min_start {
            track[i].start = min_start;
        }
        let max_prev_end = track[i].start - if need_break { 1 } else { 0 };
        if track[i - 1].start + track[i - 1].len > max_prev_end {
            track[i - 1].len = (max_prev_end - track[i - 1].start).max(1);
        }
    }

    for i in 0..track.len() {
        let rn = &track[i];
        lines.push(format!(": {} {} 0 {}", rn.start, rn.len, rn.text));
        if rn.line_end && i + 1 < track.len() {
            let prev_end = rn.start + rn.len;
            let next_start = track[i + 1].start;
            let mid = prev_end + (next_start - prev_end) / 2;
            let break_beat = mid.clamp(prev_end, next_start);
            lines.push(format!("- {break_beat}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Role::*;

    fn h() -> Header {
        Header {
            title: "T".into(),
            artist: "A".into(),
            mp3: "a.mp3".into(),
            bpm: 300.0,
            ..Default::default()
        }
    }

    fn s(text: &str, role: Role, line_end: bool) -> Syllable {
        Syllable {
            text: text.into(),
            role,
            line_end,
        }
    }

    fn n(syllable: usize, start_ms: f64, dur_ms: f64) -> Note {
        Note {
            syllable,
            start_ms,
            dur_ms,
        }
    }

    #[test]
    fn solo_basic() {
        let syl = [s("Hel", Solo, false), s("lo ", Solo, false), s("world", Solo, true)];
        let notes = [n(0, 1000.0, 200.0), n(1, 1250.0, 150.0), n(2, 1500.0, 400.0)];
        assert_eq!(
            render(&h(), &syl, &notes),
            "#TITLE:T\n#ARTIST:A\n#MP3:a.mp3\n#BPM:300\n#GAP:1000\n: 0 4 0 Hel\n: 5 3 0 lo \n: 10 8 0 world\nE\n"
        );
    }

    #[test]
    fn linebreak_midpoint() {
        let syl = [s("a", Solo, true), s("b", Solo, true)];
        let out = render(&h(), &syl, &[n(0, 2000.0, 500.0), n(1, 3000.0, 100.0)]);
        assert!(out.ends_with("#GAP:2000\n: 0 10 0 a\n- 15\n: 20 2 0 b\nE\n"));
    }

    #[test]
    fn collision_trims_previous() {
        let syl = [s("a", Solo, true), s("b", Solo, true)];
        let out = render(&h(), &syl, &[n(0, 1000.0, 480.0), n(1, 1490.0, 100.0)]);
        assert!(out.ends_with(": 0 9 0 a\n- 9\n: 10 2 0 b\nE\n"));
    }

    #[test]
    fn same_beat_forced_apart() {
        let syl = [s("a", Solo, false), s("b", Solo, false)];
        let out = render(&h(), &syl, &[n(0, 1000.0, 20.0), n(1, 1010.0, 20.0)]);
        assert!(out.ends_with(": 0 1 0 a\n: 1 1 0 b\nE\n"));
    }

    #[test]
    fn bpm_1500_zero_length_taps() {
        let mut hd = h();
        hd.bpm = 1500.0;
        let syl = [s("a", Solo, false), s("b", Solo, false), s("c", Solo, false)];
        let out = render(&hd, &syl, &[n(0, 0.0, 0.0), n(1, 0.0, 0.0), n(2, 1.0, 0.0)]);
        assert!(out.ends_with("#BPM:1500\n#GAP:0\n: 0 1 0 a\n: 1 1 0 b\n: 2 1 0 c\nE\n"));
    }

    #[test]
    fn lyrics_order_not_time_order() {
        let syl = [s("a", Solo, false), s("b", Solo, false)];
        let out = render(&h(), &syl, &[n(1, 1000.0, 100.0), n(0, 2000.0, 100.0)]);
        assert!(out.ends_with("#GAP:1000\n: 20 1 0 a\n: 21 2 0 b\nE\n"));
    }

    #[test]
    fn header_defaults_and_bpm_decimal() {
        let hd = Header {
            bpm: 287.5,
            mp3: "a.mp3".into(),
            genre: "Pop".into(),
            ..Default::default()
        };
        assert_eq!(
            render(&hd, &[], &[]),
            "#TITLE:Unknown Title\n#ARTIST:Unknown Artist\n#GENRE:Pop\n#MP3:a.mp3\n#BPM:287.5\nE\n"
        );
    }

    #[test]
    fn duet_tracks() {
        let mut hd = h();
        hd.duet = Some(("Ann".into(), "Bob".into()));
        let syl = [s("x", P1, true), s("y", P2, true), s("z", Both, true)];
        let notes = [n(0, 1000.0, 100.0), n(1, 2000.0, 100.0), n(2, 3000.0, 100.0)];
        assert_eq!(
            render(&hd, &syl, &notes),
            "#TITLE:T\n#ARTIST:A\n#MP3:a.mp3\n#BPM:300\n#GAP:1000\n\
             #DUETSINGERP1:Ann\n#P1:Ann\n#DUETSINGERP2:Bob\n#P2:Bob\n\
             P 1\n: 0 2 0 x\n- 21\n: 40 2 0 z\nP 2\n: 20 2 0 y\n- 31\n: 40 2 0 z\nE\n"
        );
    }

    #[test]
    fn duet_empty_singer_names() {
        let mut hd = h();
        hd.duet = Some((String::new(), " ".into()));
        let out = render(&hd, &[], &[]);
        assert!(out.contains(
            "#DUETSINGERP1:Singer 1\n#P1:Singer 1\n#DUETSINGERP2:Singer 2\n#P2:Singer 2\nP 1\nP 2\nE\n"
        ));
    }
}
