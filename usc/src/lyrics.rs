use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Solo,
    P1,
    P2,
    Both,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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

pub fn syllabify(text: &str, dictionary: &str) -> String {
    syllabify_with_sep(text, dictionary, '•')
}

pub fn syllabify_with_sep(text: &str, dictionary: &str, sep: char) -> String {
    let mut dict: HashMap<String, &str> = HashMap::new();
    for line in dictionary.lines() {
        let entry = line.trim();
        if entry.is_empty() {
            continue;
        }
        let key = entry.to_lowercase().replace('•', "");
        dict.insert(key, entry);
    }

    let mut out = String::with_capacity(text.len());
    let mut word = String::new();

    for ch in text.chars() {
        if is_word_delim(ch) {
            flush_word(&mut word, &dict, sep, &mut out);
            out.push(ch);
        } else {
            word.push(ch);
        }
    }
    flush_word(&mut word, &dict, sep, &mut out);
    out
}

const fn is_word_delim(c: char) -> bool {
    matches!(c, ' ' | '\n' | ',' | '!' | '.' | '?' | '(' | ')' | ';' | ':' | '-')
}

fn flush_word(word: &mut String, dict: &HashMap<String, &str>, sep: char, out: &mut String) {
    if word.is_empty() {
        return;
    }
    if let Some(&entry) = dict.get(&word.to_lowercase()) {
        let append_tail = |s: &str, buf: &mut String| {
            if sep == '•' {
                buf.push_str(s);
            } else {
                for ch in s.chars() {
                    buf.push(if ch == '•' { sep } else { ch });
                }
            }
        };
        let mut entry_chars = entry.chars();
        if word.chars().next().is_some_and(char::is_uppercase) {
            if let Some(first) = entry_chars.next() {
                out.extend(first.to_uppercase());
            }
            append_tail(entry_chars.as_str(), out);
        } else {
            append_tail(entry, out);
        }
    } else {
        out.push_str(word);
    }
    word.clear();
}

const fn is_syllable_sep(c: char) -> bool {
    matches!(c, ' ' | '•' | '+' | '-')
}

fn strip_role_tag(line: &str) -> (&str, Option<Role>) {
    let trimmed = line.trim_start();
    let Some(after_open) = trimmed.strip_prefix('[') else {
        return (line, None);
    };
    let Some((tag, rest)) = after_open.split_once(']') else {
        return (line, None);
    };
    let role = if tag.eq_ignore_ascii_case("p1") {
        Role::P1
    } else if tag.eq_ignore_ascii_case("p2") {
        Role::P2
    } else if tag.eq_ignore_ascii_case("both") || tag.eq_ignore_ascii_case("all") {
        Role::Both
    } else {
        return (line, None);
    };
    (rest.trim_start(), Some(role))
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

    #[test]
    fn syllabify_whole_words_only() {
        let dict = "ne•ver\r\nwhen•e•ver\n\n";
        assert_eq!(
            syllabify("Never whenever, ok. never-ever", dict),
            "Ne•ver when•e•ver, ok. ne•ver-ever"
        );
    }

    #[test]
    fn syllabify_keeps_capital_non_ascii() {
        assert_eq!(syllabify("Été straße", "é•té\nstra•ße"), "É•té stra•ße");
    }

    #[test]
    fn syllabify_leaves_unknown_and_presplit_words() {
        assert_eq!(syllabify("Hel•lo zzz\n", "hel•lo"), "Hel•lo zzz\n");
        assert_eq!(
            syllabify_with_sep("Never Hel+lo", "ne•ver\nhel•lo", '+'),
            "Ne+ver Hel+lo"
        );
    }
}
