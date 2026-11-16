#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Solo,
    P1,
    P2,
    Both,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Syllable {
    pub text: String,
    pub role: Role,
    pub line_end: bool,
}

pub fn clean(raw: &str) -> String {
    let normalized = raw
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace(['´', '`', '\''], "’");

    let mut out_lines: Vec<String> = Vec::new();
    for line in normalized.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut collapsed = String::with_capacity(trimmed.len());
        let mut prev_space = false;
        for ch in trimmed.chars() {
            if ch == ' ' {
                if !prev_space {
                    collapsed.push(' ');
                    prev_space = true;
                }
            } else {
                collapsed.push(ch);
                prev_space = false;
            }
        }
        out_lines.push(collapsed);
    }
    out_lines.join("\n")
}

pub fn syllables(text: &str, duet: bool) -> Vec<Syllable> {
    let mut out = Vec::new();

    for line in text.lines() {
        let (rest, detected) = strip_role_tag(line);
        if rest.trim().is_empty() {
            continue;
        }
        let role = if duet {
            detected.unwrap_or(Role::Both)
        } else {
            Role::Solo
        };

        let line_start = out.len();
        let mut chars = rest.chars().peekable();
        while let Some(&c) = chars.peek() {
            if is_syllable_sep(c) {
                chars.next();
            } else {
                break;
            }
        }

        while chars.peek().is_some() {
            let mut syl = String::new();
            while let Some(&c) = chars.peek() {
                if is_syllable_sep(c) {
                    break;
                }
                syl.push(c);
                chars.next();
            }
            while let Some(&c) = chars.peek() {
                if !is_syllable_sep(c) {
                    break;
                }
                if c != '•' && c != '+' {
                    syl.push(c);
                }
                chars.next();
            }
            out.push(Syllable {
                text: syl,
                role,
                line_end: false,
            });
        }

        if out.len() > line_start {
            out.last_mut().expect("just pushed").line_end = true;
        }
    }

    out
}

fn is_syllable_sep(c: char) -> bool {
    matches!(c, ' ' | '•' | '+' | '-')
}

fn strip_role_tag(line: &str) -> (&str, Option<Role>) {
    let trimmed = line.trim_start();
    let Some(after_open) = trimmed.strip_prefix('[') else {
        return (line, None);
    };
    let Some(close_idx) = after_open.find(']') else {
        return (line, None);
    };
    let tag = &after_open[..close_idx];
    let role = if tag.eq_ignore_ascii_case("p1") {
        Role::P1
    } else if tag.eq_ignore_ascii_case("p2") {
        Role::P2
    } else if tag.eq_ignore_ascii_case("both") || tag.eq_ignore_ascii_case("all") {
        Role::Both
    } else {
        return (line, None);
    };
    (after_open[close_idx + 1..].trim_start(), Some(role))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_normalises() {
        assert_eq!(
            clean("  Hello   world \r\n\r\n\r\nIt's `me´  \n"),
            "Hello world\nIt’s ’me’"
        );
        assert_eq!(clean(""), "");
    }

    #[test]
    fn splits_on_separators_keeping_space_and_hyphen() {
        let v = syllables("Hel•lo world\nwell-known It+s", false);
        let texts: Vec<_> = v.iter().map(|s| (s.text.as_str(), s.line_end)).collect();
        assert_eq!(
            texts,
            [
                ("Hel", false),
                ("lo ", false),
                ("world", true),
                ("well-", false),
                ("known ", false),
                ("It", false),
                ("s", true),
            ]
        );
        assert!(v.iter().all(|s| s.role == Role::Solo));
    }

    #[test]
    fn duet_roles_and_tags() {
        let v = syllables("[P1] a\n[p2]b\nc [P1] d\n[All] e", true);
        let got: Vec<_> = v.iter().map(|s| (s.text.as_str(), s.role)).collect();
        assert_eq!(
            got,
            [
                ("a", Role::P1),
                ("b", Role::P2),
                ("c ", Role::Both),
                ("[P1] ", Role::Both),
                ("d", Role::Both),
                ("e", Role::Both),
            ]
        );
    }

    #[test]
    fn tags_stripped_in_solo_mode() {
        let v = syllables("[Both] hey", false);
        assert_eq!((v[0].text.as_str(), v[0].role), ("hey", Role::Solo));
    }

    #[test]
    fn non_ascii_and_empty() {
        let v = syllables("Grü•ße ’n", false);
        let t: Vec<_> = v.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(t, ["Grü", "ße ", "’n"]);
        assert!(syllables("", false).is_empty());
        assert!(syllables("\n  \n", true).is_empty());
    }
}
