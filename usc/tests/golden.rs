use usc::lyrics::{Role, Syllable, clean, syllables};
use usc::song::{Header, render};
use usc::tapping::{Filter, Session};

fn parse_filter(name: &str) -> Filter {
    match name {
        "All" => Filter::All,
        "P1AndBoth" => Filter::P1AndBoth,
        "P2AndBoth" => Filter::P2AndBoth,
        "P1Only" => Filter::P1Only,
        "P2Only" => Filter::P2Only,
        "BothOnly" => Filter::BothOnly,
        other => panic!("unknown filter: {other}"),
    }
}

fn run(lyrics_raw: &str, duet: bool, script: &str) -> String {
    let header = Header {
        title: "T".into(),
        artist: "A".into(),
        mp3: "a.mp3".into(),
        bpm: 300.0,
        duet: duet.then(|| ("Ann".into(), "Bob".into())),
        ..Default::default()
    };
    let syls = syllables(&clean(lyrics_raw), duet);
    let mut session = Session::new(syls.clone(), Filter::All, vec![]);

    for raw_line in script.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "undo" {
            session.undo();
        } else if line == "undo_row" {
            session.undo_row();
        } else if let Some(filter_name) = line.strip_prefix("pass ") {
            let earlier = session.notes();
            session = Session::new(syls.clone(), parse_filter(filter_name.trim()), earlier);
        } else {
            let mut parts = line.split_whitespace();
            let press_ms: f64 = parts.next().expect("press ms").parse().unwrap();
            let release_ms: f64 = parts.next().expect("release ms").parse().unwrap();
            session.press(press_ms);
            session.release(release_ms);
        }
    }

    render(&header, &syls, &session.notes())
}

const SOLO: &str = "#TITLE:T\n#ARTIST:A\n#MP3:a.mp3\n#BPM:300\n#GAP:1000\n\
: 0 4 0 Hel\n: 5 3 0 lo \n: 10 8 0 world\n- 24\n: 30 4 0 Good\n: 36 4 0 bye\nE\n";

#[test]
fn solo_two_lines() {
    assert_eq!(
        run(
            "Hel•lo world\nGood•bye",
            false,
            "1000 1200\n1250 1400\n1500 1900\n2500 2700\n2800 3000"
        ),
        SOLO
    );
}

#[test]
fn undo_and_undo_row_leave_no_trace() {
    assert_eq!(
        run(
            "Hel•lo world\nGood•bye",
            false,
            "1000 1200\n1250 1400\nundo\n1260 1420\n1500 1900\n2500 2700\nundo_row\n2510 2710\n2800 3000"
        ),
        SOLO
    );
}

#[test]
fn duet_two_passes() {
    assert_eq!(
        run(
            "[P1] x\n[P2] y\nz",
            true,
            "pass P1AndBoth\n1000 1100\n3000 3100\npass P2Only\n2000 2100"
        ),
        "#TITLE:T\n#ARTIST:A\n#MP3:a.mp3\n#BPM:300\n#GAP:1000\n\
#DUETSINGERP1:Ann\n#P1:Ann\n#DUETSINGERP2:Bob\n#P2:Bob\n\
P 1\n: 0 2 0 x\n- 21\n: 40 2 0 z\nP 2\n: 20 2 0 y\n- 31\n: 40 2 0 z\nE\n"
    );
}

#[test]
fn collision_after_line_end() {
    assert!(
        run("a\nb", false, "1000 1480\n1490 1590")
            .ends_with(": 0 9 0 a\n- 9\n: 10 2 0 b\nE\n")
    );
}

#[test]
fn random_sessions_always_valid() {
    let bpms = [120.0, 287.5, 300.0, 600.0, 1500.0];
    let roles = [Role::P1, Role::P2, Role::Both];

    for seed in 1u64..=1000 {
        let mut state = seed;
        let mut rng = || -> u64 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            state >> 33
        };

        let count = 1 + (rng() % 40) as usize;
        let duet = rng().is_multiple_of(2);
        let bpm = bpms[(rng() as usize) % bpms.len()];

        let syls: Vec<Syllable> = (0..count)
            .map(|i| Syllable {
                text: format!("s{i}"),
                role: if duet {
                    roles[(rng() as usize) % roles.len()]
                } else {
                    Role::Solo
                },
                line_end: rng().is_multiple_of(4),
            })
            .collect();

        let mut session = Session::new(syls.clone(), Filter::All, vec![]);
        let mut t = (rng() % 3000) as f64;
        while session.current().is_some() {
            let roll = rng() % 20;
            if roll < 2 {
                let seek = session.undo();
                t = seek + (rng() % 1501) as f64;
            } else if roll == 2 {
                let seek = session.undo_row();
                t = seek + (rng() % 1501) as f64;
            } else {
                session.press(t);
                let dur = (rng() % 601) as f64;
                session.release(t + dur);
                t += dur + (rng() % 401) as f64;
            }
        }

        let header = Header {
            title: "T".into(),
            artist: "A".into(),
            mp3: "a.mp3".into(),
            bpm,
            duet: duet.then(|| ("Ann".into(), "Bob".into())),
            ..Default::default()
        };
        let out = render(&header, &syls, &session.notes());
        assert!(out.ends_with("E\n"), "seed {seed}: missing E\\n");

        let mut prev_note: Option<(i64, i64)> = None;
        let mut pending_break: Option<i64> = None;

        for line in out.lines() {
            if line.starts_with('#') || line == "E" {
                continue;
            }
            if line == "P 1" || line == "P 2" {
                assert!(pending_break.is_none(), "seed {seed}: trailing break before {line}");
                prev_note = None;
                continue;
            }
            if let Some(rest) = line.strip_prefix("- ") {
                let b: i64 = rest.parse().unwrap_or_else(|_| panic!("seed {seed}: bad break {line}"));
                assert!(pending_break.is_none(), "seed {seed}: consecutive breaks");
                let (ps, pl) = prev_note.unwrap_or_else(|| panic!("seed {seed}: leading break"));
                assert!(b >= ps + pl, "seed {seed}: break {b} < prevEnd {}", ps + pl);
                pending_break = Some(b);
                continue;
            }
            if let Some(rest) = line.strip_prefix(": ") {
                let mut parts = rest.split_whitespace();
                let start: i64 = parts.next().unwrap().parse().unwrap();
                let len: i64 = parts.next().unwrap().parse().unwrap();
                assert!(start >= 0, "seed {seed}: negative start {start}");
                assert!(len >= 1, "seed {seed}: len < 1 ({len})");
                if let Some((ps, pl)) = prev_note {
                    assert!(start > ps, "seed {seed}: non-increasing starts {ps} -> {start}");
                    assert!(ps + pl <= start, "seed {seed}: overlap {} > {start}", ps + pl);
                }
                if let Some(b) = pending_break.take() {
                    assert!(b < start, "seed {seed}: break {b} >= nextStart {start}");
                }
                prev_note = Some((start, len));
            }
        }
        assert!(pending_break.is_none(), "seed {seed}: trailing break before E");
    }
}
