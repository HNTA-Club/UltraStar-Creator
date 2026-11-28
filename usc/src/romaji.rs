use std::borrow::Cow;
use std::sync::OnceLock;

use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RomajiSystem {
    Hepburn,
    Kunrei,
    Nippon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LongVowelMode {
    Ascii,
    Double,
    Macron,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HatsuonMode {
    Attach,
    Standalone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatinCase {
    Original,
    Uppercase,
}

#[derive(Debug, Clone)]
pub struct RomajiConfig {
    pub split_char: char,
    pub system: RomajiSystem,
    pub long_vowels: LongVowelMode,
    pub strict_mora: bool,
    pub merge_diphthongs: bool,
    pub hatsuon: HatsuonMode,
    pub latin_case: LatinCase,
    pub capitalize_first_word: bool,
}

impl Default for RomajiConfig {
    fn default() -> Self {
        Self {
            split_char: '•',
            system: RomajiSystem::Hepburn,
            long_vowels: LongVowelMode::Ascii,
            strict_mora: false,
            merge_diphthongs: true,
            hatsuon: HatsuonMode::Attach,
            latin_case: LatinCase::Original,
            capitalize_first_word: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JapaneseWord {
    pub surface: String,
    pub reading: String,
    pub pos: String,
    pub is_particle: bool,
}

pub const fn is_japanese_char(c: char) -> bool {
    matches!(
        c,
        '\u{3040}'..='\u{309F}'
            | '\u{30A0}'..='\u{30FF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{31F0}'..='\u{31FF}'
    )
}

pub fn contains_japanese(text: &str) -> bool {
    text.chars().any(is_japanese_char)
}

static SEGMENTER: OnceLock<Segmenter> = OnceLock::new();

fn get_segmenter() -> &'static Segmenter {
    SEGMENTER.get_or_init(|| {
        let dictionary =
            load_dictionary("embedded://ipadic").expect("embedded IPADIC dictionary must load");
        Segmenter::new(Mode::Normal, dictionary, None)
    })
}

fn extract_token_reading(surface: &str, pos1: &str, raw_reading: Option<&str>) -> String {
    if pos1 == "助詞" {
        match surface {
            "は" => return "ワ".to_owned(),
            "へ" => return "エ".to_owned(),
            "を" => return "オ".to_owned(),
            _ => {}
        }
    }
    match surface {
        "こんにちは" => return "コンニチワ".to_owned(),
        "こんばんは" => return "コンバンワ".to_owned(),
        _ => {}
    }

    if let Some(r) = raw_reading
        && !r.is_empty()
        && r != "*"
        && r != "UNK"
    {
        return r.to_owned();
    }

    surface.to_owned()
}

pub fn analyze_japanese_text(text: &str) -> Vec<JapaneseWord> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let segmenter = get_segmenter();
    let Ok(mut tokens) = segmenter.segment(Cow::Borrowed(text)) else {
        return Vec::new();
    };
    if tokens.is_empty() {
        return Vec::new();
    }

    struct RawTok {
        surface: String,
        pos1: String,
        pos2: String,
        reading: String,
    }

    let mut raw_tokens = Vec::with_capacity(tokens.len());
    for tok in &mut tokens {
        let surface = tok.surface.to_string();
        let details = tok.details();
        let pos1 = details.first().copied().unwrap_or("UNK").to_owned();
        let pos2 = details.get(1).copied().unwrap_or("*").to_owned();
        let reading_field = details.get(7).copied();
        let reading = extract_token_reading(&surface, &pos1, reading_field);
        raw_tokens.push(RawTok {
            surface,
            pos1,
            pos2,
            reading,
        });
    }

    let mut words = Vec::new();
    let mut i = 0;
    let n = raw_tokens.len();

    while i < n {
        let tok = &raw_tokens[i];
        if tok.pos1 == "助詞" {
            words.push(JapaneseWord {
                surface: tok.surface.clone(),
                reading: tok.reading.clone(),
                pos: tok.pos1.clone(),
                is_particle: true,
            });
            i += 1;
            continue;
        }

        let mut current_surface = tok.surface.clone();
        let mut current_reading = tok.reading.clone();
        let current_pos = tok.pos1.clone();

        let mut j = i + 1;
        while j < n {
            let next_tok = &raw_tokens[j];
            let next_pos = next_tok.pos1.as_str();
            let next_pos2 = next_tok.pos2.as_str();

            if next_pos == "助動詞" || next_pos == "接尾辞" || next_pos2 == "接尾" {
                current_surface.push_str(&next_tok.surface);
                current_reading.push_str(&next_tok.reading);
                j += 1;
            } else if next_pos == "助詞"
                && (next_tok.surface == "て" || next_tok.surface == "で")
                && (current_pos == "動詞"
                    || current_surface.ends_with('っ')
                    || current_surface.ends_with('ん'))
            {
                current_surface.push_str(&next_tok.surface);
                current_reading.push_str(&next_tok.reading);
                j += 1;

                if j < n
                    && raw_tokens[j].pos1 == "動詞"
                    && matches!(
                        raw_tokens[j].surface.as_str(),
                        "い" | "いる"
                            | "いた"
                            | "おる"
                            | "ある"
                            | "み"
                            | "みる"
                            | "みた"
                            | "しまっ"
                            | "しまう"
                    )
                {
                    current_surface.push_str(&raw_tokens[j].surface);
                    current_reading.push_str(&raw_tokens[j].reading);
                    j += 1;
                }
            } else {
                break;
            }
        }

        words.push(JapaneseWord {
            surface: current_surface,
            reading: current_reading,
            pos: current_pos,
            is_particle: false,
        });
        i = j;
    }

    words
}

fn hepburn_lookup(mora: &str) -> Option<&'static str> {
    Some(match mora {
        // Digraphs (Yoon)
        "きゃ" | "キャ" => "kya",
        "きゅ" | "キュ" => "kyu",
        "きょ" | "キョ" => "kyo",
        "しゃ" | "シャ" => "sha",
        "しゅ" | "シュ" => "shu",
        "しょ" | "ショ" => "sho",
        "ちゃ" | "チャ" => "cha",
        "ちゅ" | "チュ" => "chu",
        "ちょ" | "チョ" => "cho",
        "にゃ" | "ニャ" => "nya",
        "にゅ" | "ニュ" => "nyu",
        "にょ" | "ニョ" => "nyo",
        "ひゃ" | "ヒャ" => "hya",
        "ひゅ" | "ヒュ" => "hyu",
        "ひょ" | "ヒョ" => "hyo",
        "みゃ" | "ミャ" => "mya",
        "みゅ" | "ミュ" => "myu",
        "みょ" | "ミョ" => "myo",
        "りゃ" | "リャ" => "rya",
        "りゅ" | "リュ" => "ryu",
        "りょ" | "リョ" => "ryo",
        "ぎゃ" | "ギャ" => "gya",
        "ぎゅ" | "ギュ" => "gyu",
        "ぎょ" | "ギョ" => "gyo",
        "じゃ" | "ジャ" | "ぢゃ" | "ヂャ" => "ja",
        "じゅ" | "ジュ" | "ぢゅ" | "ヂュ" => "ju",
        "じょ" | "ジョ" | "ぢょ" | "ヂョ" => "jo",
        "びゃ" | "ビャ" => "bya",
        "びゅ" | "ビュ" => "byu",
        "びょ" | "ビョ" => "byo",
        "ぴゃ" | "ピャ" => "pya",
        "ぴゅ" | "ピュ" => "pyu",
        "ぴょ" | "ピョ" => "pyo",
        // Foreign Katakana combinations
        "ティ" => "ti",
        "ディ" => "di",
        "テュ" => "tyu",
        "デュ" => "dyu",
        "ファ" => "fa",
        "フィ" => "fi",
        "フェ" => "fe",
        "フォ" => "fo",
        "フュ" => "fyu",
        "ウィ" => "wi",
        "ウェ" => "we",
        "ウォ" => "wo",
        "シェ" => "she",
        "ジェ" => "je",
        "チェ" => "che",
        "ツァ" => "tsa",
        "ツィ" => "tsi",
        "ツェ" => "tse",
        "ツォ" => "tso",
        "クァ" => "kwa",
        "クィ" => "kwi",
        "クェ" => "kwe",
        "クォ" => "kwo",
        "グァ" => "gwa",
        "ヴァ" => "va",
        "ヴィ" => "vi",
        "ヴェ" => "ve",
        "ヴォ" => "vo",
        "イェ" => "ye",
        // Base Vowels
        "あ" | "ア" | "ぁ" | "ァ" => "a",
        "い" | "イ" | "ぃ" | "ィ" => "i",
        "う" | "ウ" | "ぅ" | "ゥ" => "u",
        "え" | "エ" | "ぇ" | "ェ" => "e",
        "お" | "オ" | "ぉ" | "ォ" | "を" | "ヲ" => "o",
        // K
        "か" | "カ" => "ka",
        "き" | "キ" => "ki",
        "く" | "ク" => "ku",
        "け" | "ケ" => "ke",
        "こ" | "コ" => "ko",
        // S
        "さ" | "サ" => "sa",
        "し" | "シ" => "shi",
        "す" | "ス" => "su",
        "せ" | "セ" => "se",
        "そ" | "ソ" => "so",
        // T
        "た" | "タ" => "ta",
        "ち" | "チ" => "chi",
        "つ" | "ツ" => "tsu",
        "て" | "テ" => "te",
        "と" | "ト" => "to",
        // N
        "な" | "ナ" => "na",
        "に" | "ニ" => "ni",
        "ぬ" | "ヌ" => "nu",
        "ね" | "ネ" => "ne",
        "の" | "ノ" => "no",
        // H
        "は" | "ハ" => "ha",
        "ひ" | "ヒ" => "hi",
        "ふ" | "フ" => "fu",
        "へ" | "ヘ" => "he",
        "ほ" | "ホ" => "ho",
        // M
        "ま" | "マ" => "ma",
        "み" | "ミ" => "mi",
        "む" | "ム" => "mu",
        "め" | "メ" => "me",
        "も" | "モ" => "mo",
        // Y
        "や" | "ヤ" | "ゃ" | "ャ" => "ya",
        "ゆ" | "ユ" | "ゅ" | "ュ" => "yu",
        "よ" | "ヨ" | "ょ" | "ョ" => "yo",
        // R
        "ら" | "ラ" => "ra",
        "り" | "リ" => "ri",
        "る" | "ル" => "ru",
        "れ" | "レ" => "re",
        "ろ" | "ロ" => "ro",
        // W
        "わ" | "ワ" | "ゎ" | "ヮ" => "wa",
        "ゐ" | "ヰ" => "wi",
        "ゑ" | "ヱ" => "we",
        // Moraic N
        "ん" | "ン" => "n",
        // G
        "が" | "ガ" => "ga",
        "ぎ" | "ギ" => "gi",
        "ぐ" | "グ" => "gu",
        "げ" | "ゲ" => "ge",
        "ご" | "ゴ" => "go",
        // Z
        "ざ" | "ザ" => "za",
        "じ" | "ジ" | "ぢ" | "ヂ" => "ji",
        "ず" | "ズ" | "づ" | "ヅ" => "zu",
        "ぜ" | "ゼ" => "ze",
        "ぞ" | "ゾ" => "zo",
        // D
        "だ" | "ダ" => "da",
        "で" | "デ" => "de",
        "ど" | "ド" => "do",
        // B
        "ば" | "バ" => "ba",
        "び" | "ビ" => "bi",
        "ぶ" | "ブ" => "bu",
        "べ" | "ベ" => "be",
        "ぼ" | "ボ" => "bo",
        // P
        "ぱ" | "パ" => "pa",
        "ぴ" | "ピ" => "pi",
        "ぷ" | "プ" => "pu",
        "ぺ" | "ペ" => "pe",
        "ぽ" | "ポ" => "po",
        // V
        "ゔ" | "ヴ" => "vu",
        _ => return None,
    })
}

fn kunrei_override(mora: &str) -> Option<&'static str> {
    Some(match mora {
        "し" | "シ" => "si",
        "ち" | "チ" => "ti",
        "つ" | "ツ" => "tu",
        "ふ" | "フ" => "hu",
        "じ" | "ジ" | "ぢ" | "ヂ" => "zi",
        "づ" | "ヅ" => "zu",
        "しゃ" | "シャ" => "sya",
        "しゅ" | "シュ" => "syu",
        "しょ" | "ショ" => "syo",
        "ちゃ" | "チャ" => "tya",
        "ちゅ" | "チュ" => "tyu",
        "ちょ" | "チョ" => "tyo",
        "じゃ" | "ジャ" | "ぢゃ" | "ヂャ" => "zya",
        "じゅ" | "ジュ" | "ぢゅ" | "ヂュ" => "zyu",
        "じょ" | "ジョ" | "ぢょ" | "ヂョ" => "zyo",
        _ => return None,
    })
}

fn nippon_override(mora: &str) -> Option<&'static str> {
    Some(match mora {
        "ぢ" | "ヂ" => "di",
        "づ" | "ヅ" => "du",
        "ぢゃ" | "ヂャ" => "dya",
        "ぢゅ" | "ヂュ" => "dyu",
        "ぢょ" | "ヂョ" => "dyo",
        "を" | "ヲ" => "wo",
        _ => return kunrei_override(mora),
    })
}

pub fn get_mora_romaji(mora: &str, system: RomajiSystem) -> String {
    let found = match system {
        RomajiSystem::Hepburn => hepburn_lookup(mora),
        RomajiSystem::Kunrei => kunrei_override(mora).or_else(|| hepburn_lookup(mora)),
        RomajiSystem::Nippon => nippon_override(mora).or_else(|| hepburn_lookup(mora)),
    };
    found.unwrap_or(mora).to_owned()
}

fn is_known_digraph(two_chars: &str) -> bool {
    two_chars.chars().count() == 2 && hepburn_lookup(two_chars).is_some()
}

fn base_vowel(c: char) -> Option<char> {
    match c.to_lowercase().next()? {
        'a' | 'ā' => Some('a'),
        'i' | 'ī' => Some('i'),
        'u' | 'ū' => Some('u'),
        'e' | 'ē' => Some('e'),
        'o' | 'ō' => Some('o'),
        _ => None,
    }
}

const fn macron_for_vowel(v: char) -> char {
    match v {
        'a' => 'ā',
        'i' => 'ī',
        'u' => 'ū',
        'e' => 'ē',
        'o' => 'ō',
        other => other,
    }
}

fn terminal_vowel(romaji: &str) -> Option<char> {
    for ch in romaji.chars().rev() {
        if let Some(v) = base_vowel(ch) {
            return Some(v);
        }
    }
    None
}

pub fn elongate_vowel(prev_romaji: &str, long_vowels: LongVowelMode) -> String {
    if prev_romaji.is_empty() {
        return String::new();
    }

    let mut last_vowel_byte_range = None;
    let mut vowel_char = 'a';
    for (idx, ch) in prev_romaji.char_indices().rev() {
        if let Some(v) = base_vowel(ch) {
            last_vowel_byte_range = Some(idx..idx + ch.len_utf8());
            vowel_char = v;
            break;
        }
    }

    let Some(range) = last_vowel_byte_range else {
        return prev_romaji.to_owned();
    };

    match long_vowels {
        LongVowelMode::Macron => {
            let mut out = String::with_capacity(prev_romaji.len() + 2);
            out.push_str(&prev_romaji[..range.start]);
            out.push(macron_for_vowel(vowel_char));
            out.push_str(&prev_romaji[range.end..]);
            out
        }
        LongVowelMode::Ascii | LongVowelMode::Double => {
            let mut out = prev_romaji.to_owned();
            out.push(vowel_char);
            out
        }
    }
}

pub fn split_kana_to_morae(kana_text: &str, cfg: &RomajiConfig) -> Vec<String> {
    if kana_text.is_empty() {
        return Vec::new();
    }

    let chars: Vec<char> = kana_text.chars().collect();
    let mut raw_units: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if i + 2 <= chars.len() {
            let two: String = chars[i..i + 2].iter().collect();
            if is_known_digraph(&two) {
                raw_units.push(two);
                i += 2;
                continue;
            }
        }
        raw_units.push(chars[i].to_string());
        i += 1;
    }

    let mut morae: Vec<String> = Vec::new();
    let mut u_idx = 0;
    let num_units = raw_units.len();

    while u_idx < num_units {
        let unit = raw_units[u_idx].as_str();

        // Sokuon (っ / ッ)
        if unit == "っ" || unit == "ッ" {
            if u_idx + 1 < num_units {
                let next_unit = raw_units[u_idx + 1].as_str();
                let next_romaji = get_mora_romaji(next_unit, cfg.system);
                u_idx += 2;

                let geminated =
                    if next_romaji.starts_with("ch") && cfg.system == RomajiSystem::Hepburn {
                        format!("t{next_romaji}")
                    } else if let Some(first) = next_romaji.chars().next() {
                        format!("{first}{next_romaji}")
                    } else {
                        next_romaji
                    };
                morae.push(geminated);
            } else {
                if let Some(last) = morae.last_mut() {
                    last.push('\'');
                } else {
                    morae.push("'".to_owned());
                }
                u_idx += 1;
            }
            continue;
        }

        // Choonpu (ー)
        if unit == "ー" {
            if cfg.strict_mora {
                let prev_vowel = morae
                    .last()
                    .and_then(|s| terminal_vowel(s))
                    .unwrap_or('a');
                morae.push(prev_vowel.to_string());
            } else if let Some(last) = morae.last_mut() {
                *last = elongate_vowel(last, cfg.long_vowels);
            } else {
                morae.push("-".to_owned());
            }
            u_idx += 1;
            continue;
        }

        // Hatsuon (ん / ン)
        if unit == "ん" || unit == "ン" {
            let mut needs_apostrophe = false;
            if u_idx + 1 < num_units {
                let peek_romaji = get_mora_romaji(&raw_units[u_idx + 1], cfg.system);
                if let Some(first) = peek_romaji.chars().next()
                    && matches!(first, 'a' | 'e' | 'i' | 'o' | 'u' | 'y')
                {
                    needs_apostrophe = true;
                }
            }
            let suffix = if needs_apostrophe { "n'" } else { "n" };

            if cfg.hatsuon == HatsuonMode::Attach
                && let Some(last) = morae.last_mut()
                && !last.ends_with('\'')
            {
                last.push_str(suffix);
            } else {
                morae.push(suffix.to_owned());
            }
            u_idx += 1;
            continue;
        }

        let romaji = get_mora_romaji(unit, cfg.system);

        if matches!(cfg.long_vowels, LongVowelMode::Macron | LongVowelMode::Double)
            && let Some(last) = morae.last_mut()
            && let Some(prev_vowel) = terminal_vowel(last)
            && ((prev_vowel == 'o' && matches!(unit, "う" | "ウ" | "お" | "オ"))
                || (prev_vowel == 'u' && matches!(unit, "う" | "ウ"))
                || (prev_vowel == 'e' && matches!(unit, "え" | "エ"))
                || (prev_vowel == 'a' && matches!(unit, "あ" | "ア"))
                || (prev_vowel == 'i' && matches!(unit, "い" | "イ")))
        {
            *last = elongate_vowel(last, cfg.long_vowels);
            u_idx += 1;
            continue;
        }

        morae.push(romaji);
        u_idx += 1;
    }

    if cfg.merge_diphthongs && !cfg.strict_mora && morae.len() > 1 {
        morae = merge_mora_diphthongs(morae);
    }

    morae
}

fn merge_mora_diphthongs(morae: Vec<String>) -> Vec<String> {
    if morae.len() <= 1 {
        return morae;
    }
    let mut merged = Vec::with_capacity(morae.len());
    let mut m_idx = 0;
    while m_idx < morae.len() {
        let current = &morae[m_idx];
        if m_idx + 1 < morae.len()
            && !(current.ends_with('\'') || current.ends_with('n') || current.ends_with('N'))
        {
            let nxt = morae[m_idx + 1].as_str();
            if let Some(last_char) = current.chars().last().map(|c| c.to_ascii_lowercase()) {
                if nxt.eq_ignore_ascii_case("i")
                    && matches!(last_char, 'a' | 'e' | 'o' | 'u' | 'i')
                {
                    merged.push(format!("{current}{nxt}"));
                    m_idx += 2;
                    continue;
                }
                if nxt.eq_ignore_ascii_case("u") && matches!(last_char, 'o' | 'u' | 'a') {
                    merged.push(format!("{current}{nxt}"));
                    m_idx += 2;
                    continue;
                }
                if (nxt.eq_ignore_ascii_case("a")
                    || nxt.eq_ignore_ascii_case("e")
                    || nxt.eq_ignore_ascii_case("o"))
                    && nxt.to_ascii_lowercase().starts_with(last_char)
                {
                    merged.push(format!("{current}{nxt}"));
                    m_idx += 2;
                    continue;
                }
            }
        }
        merged.push(current.clone());
        m_idx += 1;
    }
    merged
}

const fn lower_romaji_char(c: char) -> char {
    match c {
        'Ā' => 'ā',
        'Ī' => 'ī',
        'Ū' => 'ū',
        'Ē' => 'ē',
        'Ō' => 'ō',
        other => other.to_ascii_lowercase(),
    }
}

const fn is_romaji_letter(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, 'ā' | 'ī' | 'ū' | 'ē' | 'ō' | 'Ā' | 'Ī' | 'Ū' | 'Ē' | 'Ō')
}

const fn base_vowel_preserving_case(c: char) -> Option<char> {
    match c {
        'ā' => Some('a'),
        'ī' => Some('i'),
        'ū' => Some('u'),
        'ē' => Some('e'),
        'ō' => Some('o'),
        'Ā' => Some('A'),
        'Ī' => Some('I'),
        'Ū' => Some('U'),
        'Ē' => Some('E'),
        'Ō' => Some('O'),
        _ => None,
    }
}

const fn is_lower_vowel(c: char) -> bool {
    matches!(
        c,
        'a' | 'i' | 'u' | 'e' | 'o' | 'ā' | 'ī' | 'ū' | 'ē' | 'ō'
    )
}

fn match_hepburn_onset(slice: &[char]) -> Option<usize> {
    let matches_str = |pat: &str| -> bool {
        let len = pat.len();
        slice.len() > len
            && is_lower_vowel(slice[len])
            && slice[..len].iter().copied().eq(pat.chars())
    };

    // 4-char consonant+yoon? Look at 3-char sokuon onsets first, then 2-char, then 1-char
    for pat in ["tch", "cch", "ssh", "tts"] {
        if matches_str(pat) {
            if pat == "tts" && !matches!(slice[3], 'u' | 'ū') {
                continue;
            }
            return Some(3);
        }
    }

    // Doubled consonant + yoon or doubled single consonant (strict Hepburn)
    if slice.len() >= 3 && slice[0] == slice[1] && slice[0] != 'n' {
        let doubled = slice[0];
        // Doubled consonant before yoon digraph: e.g. kkya, ppyo, ggyu, jja? (jj only valid as jja/jji/jju/jjo? Hepburn usually has j + vowel)
        if slice.len() > 3
            && slice[2] == 'y'
            && is_lower_vowel(slice[3])
            && matches!(slice[3], 'a' | 'u' | 'o')
            && matches!(doubled, 'k' | 'g' | 'p' | 'b' | 'm' | 'r')
        {
            return Some(3);
        }
        if is_lower_vowel(slice[2]) {
            let v = slice[2];
            let valid = match doubled {
                'k' | 'g' | 'p' | 'b' => true,
                's' => !matches!(v, 'i' | 'ī'),
                't' => !matches!(v, 'i' | 'ī' | 'u' | 'ū'),
                'd' => matches!(v, 'a' | 'e' | 'o' | 'ā' | 'ē' | 'ō'),
                'z' => !matches!(v, 'i' | 'ī'),
                'j' => matches!(v, 'a' | 'i' | 'u' | 'e' | 'o' | 'ā' | 'ī' | 'ū' | 'ē' | 'ō'),
                'f' => matches!(v, 'a' | 'i' | 'u' | 'e' | 'o' | 'ā' | 'ī' | 'ū' | 'ē' | 'ō'),
                _ => false,
            };
            if valid {
                return Some(2);
            }
        }
    }

    // Standard Hepburn 2-char onsets (followed by vowel)
    if slice.len() >= 3 && is_lower_vowel(slice[2]) {
        let c0 = slice[0];
        let c1 = slice[1];
        let v = slice[2];
        let valid2 = match (c0, c1) {
            ('s', 'h') | ('c', 'h') => true,
            ('t', 's') => matches!(v, 'u' | 'ū' | 'a' | 'i' | 'e' | 'o'),
            ('k', 'y')
            | ('g', 'y')
            | ('n', 'y')
            | ('h', 'y')
            | ('b', 'y')
            | ('p', 'y')
            | ('m', 'y')
            | ('r', 'y') => matches!(v, 'a' | 'u' | 'o' | 'ā' | 'ū' | 'ō'),
            ('f', 'y') => matches!(v, 'u' | 'ū'),
            ('k', 'w') => matches!(v, 'a' | 'i' | 'e' | 'o'),
            ('g', 'w') => matches!(v, 'a'),
            _ => false,
        };
        if valid2 {
            return Some(2);
        }
    }

    // Standard Hepburn 1-char onsets (strictly excludes si/ti/tu/hu/zi, c without h, l, q, v, x)
    if slice.len() >= 2 && is_lower_vowel(slice[1]) {
        let c0 = slice[0];
        let v = slice[1];
        let valid1 = match c0 {
            'k' | 'g' | 'n' | 'h' | 'b' | 'p' | 'm' | 'r' | 'j' | 'f' => true,
            's' => !matches!(v, 'i' | 'ī'),
            't' => !matches!(v, 'i' | 'ī' | 'u' | 'ū'),
            'd' => matches!(v, 'a' | 'e' | 'o' | 'ā' | 'ē' | 'ō'),
            'z' => !matches!(v, 'i' | 'ī'),
            'y' => matches!(v, 'a' | 'u' | 'e' | 'o' | 'ā' | 'ū' | 'ē' | 'ō'),
            'w' => matches!(v, 'a' | 'i' | 'e' | 'o' | 'ā' | 'ē' | 'ō'),
            _ => false,
        };
        if valid1 {
            return Some(1);
        }
    }

    None
}

pub fn split_romaji_word_to_morae(word: &str, cfg: &RomajiConfig) -> Option<Vec<String>> {
    if word.is_empty() || word.contains('•') || word.contains('+') {
        return None;
    }

    let orig_chars: Vec<char> = word.chars().collect();
    if orig_chars
        .iter()
        .any(|&c| !is_romaji_letter(c) && c != '\'')
    {
        return None;
    }

    // Must contain at least one letter
    if !orig_chars.iter().any(|&c| is_romaji_letter(c)) {
        return None;
    }

    let lower_chars: Vec<char> = orig_chars.iter().copied().map(lower_romaji_char).collect();
    let n = lower_chars.len();
    let mut i = 0;
    let mut morae: Vec<String> = Vec::new();

    let push_mora_slice = |morae: &mut Vec<String>, start: usize, end: usize| {
        let last_orig = orig_chars[end - 1];
        if cfg.strict_mora
            && let Some(expanded_v) = base_vowel_preserving_case(last_orig)
        {
            let mut base: String = orig_chars[start..end - 1].iter().collect();
            base.push(expanded_v);
            morae.push(base);
            morae.push(expanded_v.to_string());
            return;
        }
        morae.push(orig_chars[start..end].iter().collect());
    };

    while i < n {
        // Trailing glottal stop apostrophe (e.g. "a'", "matte'")
        if lower_chars[i] == '\'' {
            if i + 1 == n && !morae.is_empty() {
                if let Some(last) = morae.last_mut() {
                    last.push('\'');
                }
                i += 1;
                continue;
            }
            return None;
        }

        // Moraic n': "n'" before vowel or 'y' (or end of word)
        if lower_chars[i] == 'n'
            && i + 1 < n
            && lower_chars[i + 1] == '\''
            && (i + 2 == n || is_lower_vowel(lower_chars[i + 2]) || lower_chars[i + 2] == 'y')
        {
            let slice_str: String = orig_chars[i..i + 2].iter().collect();
            if cfg.hatsuon == HatsuonMode::Attach
                && let Some(last) = morae.last_mut()
                && !last.ends_with('\'')
            {
                last.push_str(&slice_str);
            } else {
                morae.push(slice_str);
            }
            i += 2;
            continue;
        }

        // Moraic n before consonant or end of word
        if lower_chars[i] == 'n'
            && (i + 1 == n || (!is_lower_vowel(lower_chars[i + 1]) && lower_chars[i + 1] != 'y'))
        {
            let n_char: String = orig_chars[i..i + 1].iter().collect();
            if cfg.hatsuon == HatsuonMode::Attach
                && let Some(last) = morae.last_mut()
                && !last.ends_with('\'')
            {
                last.push_str(&n_char);
            } else {
                morae.push(n_char);
            }
            i += 1;
            continue;
        }

        // Consonant onset + vowel
        if let Some(onset_len) = match_hepburn_onset(&lower_chars[i..]) {
            let end = i + onset_len + 1;
            push_mora_slice(&mut morae, i, end);
            i = end;
            continue;
        }

        // Bare vowel mora
        if is_lower_vowel(lower_chars[i]) {
            push_mora_slice(&mut morae, i, i + 1);
            i += 1;
            continue;
        }

        return None;
    }

    if cfg.merge_diphthongs && !cfg.strict_mora && morae.len() > 1 {
        morae = merge_mora_diphthongs(morae);
    }

    Some(morae)
}

const ENGLISH_ONLY_SKIP: &[&str] = &[
    "are", "awake", "bake", "bare", "base", "before", "bite", "bode", "bone", "bore", "cage",
    "code", "cope", "cube", "damage", "daze", "done", "dope", "dose", "dude", "dune", "erase",
    "fade", "fake", "fame", "fare", "fate", "fine", "fire", "fuse", "game", "gate", "gaze",
    "gone", "here", "heroine", "hide", "hire", "home", "hope", "hose", "huge", "image", "jade",
    "joke", "june", "kite", "maze", "mere", "mirage", "mode", "mope", "more", "muse", "nine",
    "node", "none", "nope", "nose", "note", "nude", "one", "page", "parade", "poke", "pope",
    "pose", "pure", "rage", "rate", "ride", "rode", "rope", "rose", "rude", "rune", "sage",
    "same", "some", "sure", "tone", "tore", "tube", "tune", "use", "wade", "wage", "wide",
    "wine", "wire", "woke", "wore", "zone",
];

const AMBIGUOUS_EN_JP: &[&str] = &[
    "an", "hate", "in", "made", "make", "mate", "mine", "name", "on", "shine", "take", "wake",
];

fn is_in_word_list(word: &str, list: &[&str]) -> bool {
    let lower = word.to_ascii_lowercase();
    list.binary_search(&lower.as_str()).is_ok()
}

pub fn split_morae(text: &str, cfg: &RomajiConfig) -> Vec<Vec<String>> {
    let words = analyze_japanese_text(text);
    let mut out = Vec::new();
    for w in words {
        let morae = split_kana_to_morae(&w.reading, cfg);
        if !morae.is_empty() {
            out.push(morae);
        }
    }
    out
}

fn normalize_fullwidth_punct(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '！' => out.push('!'),
            '？' => out.push('?'),
            '（' => out.push('('),
            '）' => out.push(')'),
            '［' => out.push('['),
            '］' => out.push(']'),
            '、' => out.push_str(", "),
            '。' => out.push_str(". "),
            '：' => out.push(':'),
            '；' => out.push(';'),
            '〜' | '～' => out.push('~'),
            '\u{3000}' => out.push(' '),
            other => out.push(other),
        }
    }
    out
}

#[derive(Debug, PartialEq, Eq)]
enum LineToken {
    Japanese(String),
    AsciiWord(String),
    Whitespace(String),
    Punct(String),
}

const fn is_latin_token_char(c: char) -> bool {
    is_romaji_letter(c) || c.is_ascii_digit()
}

fn tokenize_mixed_line(s: &str) -> Vec<LineToken> {
    let chars: Vec<char> = s.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    let n = chars.len();

    while i < n {
        let c = chars[i];
        if is_japanese_char(c) {
            let start = i;
            i += 1;
            while i < n && is_japanese_char(chars[i]) {
                i += 1;
            }
            tokens.push(LineToken::Japanese(chars[start..i].iter().collect()));
        } else if is_latin_token_char(c) {
            let start = i;
            i += 1;
            while i < n {
                if is_latin_token_char(chars[i]) {
                    i += 1;
                } else if matches!(chars[i], '•' | '+')
                    && i + 1 < n
                    && is_latin_token_char(chars[i + 1])
                {
                    i += 2;
                } else if chars[i] == '\''
                    && ((i + 1 < n && is_latin_token_char(chars[i + 1]))
                        || i + 1 == n
                        || !is_latin_token_char(chars[i + 1]))
                {
                    // Include internal apostrophes (gen'in / don't) and trailing glottal stops (matte')
                    if i + 1 < n && is_latin_token_char(chars[i + 1]) {
                        i += 2;
                    } else if i + 1 == n || !chars[i + 1].is_alphabetic() {
                        // Only attach trailing '\'' if next char isn't part of a closing quote around punctuation?
                        // Wait: for "a'" followed by space or end, attaching '\'' keeps "matte'" together.
                        if i > start && !chars[start..i].contains(&'\'') {
                            i += 1;
                        }
                        break;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            tokens.push(LineToken::AsciiWord(chars[start..i].iter().collect()));
        } else if c.is_whitespace() {
            let start = i;
            i += 1;
            while i < n && chars[i].is_whitespace() {
                i += 1;
            }
            tokens.push(LineToken::Whitespace(chars[start..i].iter().collect()));
        } else {
            tokens.push(LineToken::Punct(c.to_string()));
            i += 1;
        }
    }

    tokens
}

pub fn convert_line(line: &str, cfg: &RomajiConfig) -> String {
    if line.trim().is_empty() {
        return String::new();
    }

    // Preserve leading [P1] / [P2] / [Both] duet tags untouched
    let (duet_prefix, body) = split_duet_tag(line);

    let normalized = normalize_fullwidth_punct(body);
    let tokens = tokenize_mixed_line(&normalized);
    let split_sep = cfg.split_char.to_string();

    let mut output_parts = Vec::with_capacity(tokens.len());
    for tok in tokens {
        match tok {
            LineToken::Japanese(seg) => {
                let jp_words = analyze_japanese_text(&seg);
                let mut jp_parts = Vec::new();
                for w in jp_words {
                    let morae = split_kana_to_morae(&w.reading, cfg);
                    let word_str = morae.join(&split_sep);
                    if !word_str.is_empty() {
                        jp_parts.push(word_str);
                    }
                }
                output_parts.push(jp_parts.join(" "));
            }
            LineToken::AsciiWord(w) => {
                if cfg.latin_case == LatinCase::Uppercase {
                    output_parts.push(w.to_uppercase());
                } else {
                    output_parts.push(w);
                }
            }
            LineToken::Whitespace(_) => {
                output_parts.push(" ".to_owned());
            }
            LineToken::Punct(p) => {
                output_parts.push(p);
            }
        }
    }

    let raw = output_parts.concat();
    let mut collapsed = String::with_capacity(raw.len());
    let mut prev_space = false;
    for ch in raw.trim().chars() {
        if ch == ' ' || ch == '\t' {
            if !prev_space {
                collapsed.push(' ');
                prev_space = true;
            }
        } else {
            // Remove space immediately before closing/terminal punctuation ? ! , . ) ]
            if prev_space && matches!(ch, '?' | '!' | ',' | '.' | ')' | ']') {
                collapsed.pop();
            }
            // Prevent space right after opening parenthesis ( or [
            if !collapsed.is_empty() && matches!(collapsed.chars().last(), Some('(' | '[')) && ch == ' ' {
                continue;
            }
            collapsed.push(ch);
            prev_space = matches!(ch, '(' | '[');
        }
    }

    if cfg.capitalize_first_word && !collapsed.is_empty() {
        let mut capitalized = String::with_capacity(collapsed.len());
        let mut done = false;
        for ch in collapsed.chars() {
            if !done && ch.is_alphabetic() {
                for up in ch.to_uppercase() {
                    capitalized.push(up);
                }
                done = true;
            } else {
                capitalized.push(ch);
            }
        }
        collapsed = capitalized;
    }

    if duet_prefix.is_empty() {
        collapsed
    } else {
        format!("{duet_prefix}{collapsed}")
    }
}

fn split_duet_tag(line: &str) -> (&str, &str) {
    let trimmed = line.trim_start();
    for tag in ["[P1]", "[p1]", "[P2]", "[p2]", "[Both]", "[both]", "[BOTH]"] {
        if let Some(rest) = trimmed.strip_prefix(tag) {
            let prefix_len = line.len() - rest.trim_start().len();
            return (&line[..prefix_len], rest.trim_start());
        }
    }
    ("", line)
}

pub fn convert_text(text: &str, cfg: &RomajiConfig) -> String {
    let mut out_lines = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            out_lines.push(String::new());
        } else if contains_japanese(line) {
            out_lines.push(convert_line(line, cfg));
        } else {
            out_lines.push(line.to_owned());
        }
    }
    let mut res = out_lines.join("\n");
    if text.ends_with('\n') {
        res.push('\n');
    }
    res
}

fn line_has_english_context(tokens: &[LineToken], cfg: &RomajiConfig) -> bool {
    tokens.iter().any(|tok| {
        let LineToken::AsciiWord(w) = tok else {
            return false;
        };
        if w.contains('•') || w.contains('+') {
            let clean: String = w.chars().filter(|&c| c != '•' && c != '+').collect();
            if is_in_word_list(&clean, ENGLISH_ONLY_SKIP) {
                return true;
            }
            match split_romaji_word_to_morae(&clean, cfg) {
                None => return true,
                Some(morae) => {
                    let expected = morae.join(&cfg.split_char.to_string());
                    // e.g. "Be•fore" (from English.txt) != "Be•fo•re" (Hepburn)
                    if !w.eq_ignore_ascii_case(&expected) {
                        return true;
                    }
                    return false;
                }
            }
        }
        is_in_word_list(w, ENGLISH_ONLY_SKIP) || split_romaji_word_to_morae(w, cfg).is_none()
    })
}

fn split_latin_word_preserving_case(
    w: &str,
    cfg: &RomajiConfig,
    has_english_ctx: bool,
) -> Option<String> {
    if is_in_word_list(w, ENGLISH_ONLY_SKIP) {
        return None;
    }
    if has_english_ctx && is_in_word_list(w, AMBIGUOUS_EN_JP) {
        return None;
    }
    let morae = split_romaji_word_to_morae(w, cfg)?;
    let sep = cfg.split_char.to_string();
    Some(morae.join(&sep))
}

fn convert_romaji_line_preserving_case(line: &str, cfg: &RomajiConfig) -> String {
    let (duet_prefix, body) = split_duet_tag(line);
    let tokens = tokenize_mixed_line(body);
    let has_english_ctx = line_has_english_context(&tokens, cfg);

    let mut out = String::with_capacity(line.len() + 16);
    out.push_str(duet_prefix);
    for tok in &tokens {
        match tok {
            LineToken::AsciiWord(w) => {
                if let Some(split) = split_latin_word_preserving_case(w, cfg, has_english_ctx) {
                    out.push_str(&split);
                } else {
                    out.push_str(w);
                }
            }
            LineToken::Japanese(s) | LineToken::Whitespace(s) | LineToken::Punct(s) => {
                out.push_str(s);
            }
        }
    }
    out
}

pub fn convert_text_with_romaji(text: &str, cfg: &RomajiConfig) -> String {
    let mut out_lines = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            out_lines.push(String::new());
        } else if contains_japanese(line) {
            let converted = convert_line(line, cfg);
            out_lines.push(convert_romaji_line_preserving_case(&converted, cfg));
        } else {
            out_lines.push(convert_romaji_line_preserving_case(line, cfg));
        }
    }
    let mut res = out_lines.join("\n");
    if text.ends_with('\n') {
        res.push('\n');
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_kana() {
        let cfg_raw = RomajiConfig {
            merge_diphthongs: false,
            hatsuon: HatsuonMode::Standalone,
            ..Default::default()
        };
        assert_eq!(
            split_kana_to_morae("たいよう", &cfg_raw),
            vec!["ta", "i", "yo", "u"]
        );

        let cfg_default = RomajiConfig::default();
        assert_eq!(
            split_kana_to_morae("さようなら", &cfg_default),
            vec!["sa", "you", "na", "ra"]
        );

        let cfg_no_merge = RomajiConfig {
            merge_diphthongs: false,
            ..Default::default()
        };
        assert_eq!(
            split_kana_to_morae("さようなら", &cfg_no_merge),
            vec!["sa", "yo", "u", "na", "ra"]
        );
    }

    #[test]
    fn sokuon_gemination_and_trailing() {
        let cfg = RomajiConfig::default();
        assert_eq!(split_kana_to_morae("きって", &cfg), vec!["ki", "tte"]);
        assert_eq!(split_kana_to_morae("ちょっと", &cfg), vec!["cho", "tto"]);
        assert_eq!(
            split_kana_to_morae("いっちょう", &cfg),
            vec!["i", "tchou"]
        );
        assert_eq!(split_kana_to_morae("まっち", &cfg), vec!["ma", "tchi"]);
        assert_eq!(split_kana_to_morae("あっ", &cfg), vec!["a'"]);
        assert_eq!(split_kana_to_morae("まってっ", &cfg), vec!["ma", "tte'"]);
    }

    #[test]
    fn hatsuon_apostrophe_attach_and_standalone() {
        let attach = RomajiConfig::default();
        assert_eq!(split_kana_to_morae("しんぶん", &attach), vec!["shin", "bun"]);
        assert_eq!(split_kana_to_morae("げんいん", &attach), vec!["gen'", "in"]);
        assert_eq!(split_kana_to_morae("こんや", &attach), vec!["kon'", "ya"]);

        let standalone = RomajiConfig {
            hatsuon: HatsuonMode::Standalone,
            ..Default::default()
        };
        assert_eq!(
            split_kana_to_morae("しんぶん", &standalone),
            vec!["shi", "n", "bu", "n"]
        );
        assert_eq!(
            split_kana_to_morae("げんいん", &standalone),
            vec!["ge", "n'", "i", "n"]
        );
        assert_eq!(
            split_kana_to_morae("こんや", &standalone),
            vec!["ko", "n'", "ya"]
        );
    }

    #[test]
    fn choonpu_and_long_vowels() {
        let cfg = RomajiConfig::default();
        assert_eq!(split_kana_to_morae("すたー", &cfg), vec!["su", "taa"]);
        assert_eq!(split_kana_to_morae("こーひー", &cfg), vec!["koo", "hii"]);
        assert_eq!(split_kana_to_morae("とうきょう", &cfg), vec!["tou", "kyou"]);

        let strict = RomajiConfig {
            strict_mora: true,
            ..Default::default()
        };
        assert_eq!(
            split_kana_to_morae("こーひー", &strict),
            vec!["ko", "o", "hi", "i"]
        );

        let double = RomajiConfig {
            long_vowels: LongVowelMode::Double,
            ..Default::default()
        };
        assert_eq!(
            split_kana_to_morae("とうきょう", &double),
            vec!["too", "kyoo"]
        );

        let macron = RomajiConfig {
            long_vowels: LongVowelMode::Macron,
            ..Default::default()
        };
        assert_eq!(
            split_kana_to_morae("とうきょう", &macron),
            vec!["tō", "kyō"]
        );
    }

    #[test]
    fn morphological_analysis_and_particles() {
        let words = analyze_japanese_text("食べない");
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].surface, "食べない");
        assert!(words[0].reading.contains("タベナイ"));

        let words2 = analyze_japanese_text("私は君が好き");
        let surfaces: Vec<&str> = words2.iter().map(|w| w.surface.as_str()).collect();
        assert_eq!(surfaces, vec!["私", "は", "君", "が", "好き"]);
        assert_eq!(words2[1].reading, "ワ");
        assert!(words2[1].is_particle);
        assert_eq!(words2[3].reading, "ガ");
        assert!(words2[3].is_particle);

        let words3 = analyze_japanese_text("探していたよ");
        assert!(words3.len() <= 2);
    }

    #[test]
    fn full_converter_and_song_sample() {
        let cfg = RomajiConfig::default();
        assert_eq!(
            convert_line("こんにちは世界", &cfg),
            "Kon•ni•chi•wa se•kai"
        );

        let cfg_standalone = RomajiConfig {
            hatsuon: HatsuonMode::Standalone,
            capitalize_first_word: false,
            ..Default::default()
        };
        assert_eq!(
            convert_line("こんにちは世界", &cfg_standalone),
            "ko•n•ni•chi•wa se•kai"
        );

        let cfg_plus = RomajiConfig {
            split_char: '+',
            ..Default::default()
        };
        assert_eq!(convert_line("ちょっと待って", &cfg_plus), "Cho+tto ma+tte");

        assert_eq!(
            split_morae("ちょっと待って", &cfg),
            vec![vec!["cho", "tto"], vec!["ma", "tte"]]
        );

        let sample = "太陽のもと 旅立とう\n\n寝ぼけ眼で jump up いつものサングラス\n今でも信じてんだ ooh yeah (ooh yeah)\n";
        let converted = convert_text(sample, &cfg);
        let lines: Vec<&str> = converted.split('\n').collect();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "Tai•you no mo•to ta•bi•da•tou");
        assert_eq!(lines[1], "");
        assert!(lines[2].contains("jump up"));
        assert!(lines[2].contains("san•gu•ra•su"));
        assert!(lines[3].contains("ooh yeah (ooh yeah)"));
    }

    #[test]
    fn romaji_splitter_matches_japanese_script_across_configs() {
        let cases = [
            ("日本語", "nihongo"),
            ("さようなら", "sayounara"),
            ("ちょっと待って", "chotto matte"),
            ("しんぶん", "shinbun"),
            ("原因", "gen'in"),
            ("こんや", "kon'ya"),
            ("マッチ", "matchi"),
            ("いっしょ", "issho"),
            ("みっつ", "mittsu"),
            ("東京", "toukyou"),
        ];

        let configs = [
            RomajiConfig {
                capitalize_first_word: false,
                ..Default::default()
            },
            RomajiConfig {
                hatsuon: HatsuonMode::Standalone,
                capitalize_first_word: false,
                ..Default::default()
            },
            RomajiConfig {
                merge_diphthongs: false,
                capitalize_first_word: false,
                ..Default::default()
            },
            RomajiConfig {
                split_char: '+',
                capitalize_first_word: false,
                ..Default::default()
            },
        ];

        for cfg in &configs {
            for (jp, roma) in cases {
                assert_eq!(
                    convert_text_with_romaji(roma, cfg),
                    convert_text_with_romaji(jp, cfg),
                    "mismatch for {jp} / {roma} with {cfg:?}"
                );
            }
        }

        // Strict mora expands macron vowels while non-strict preserves them
        let strict = RomajiConfig {
            strict_mora: true,
            ..Default::default()
        };
        assert_eq!(convert_text_with_romaji("tōkyō", &strict), "to•o•kyo•o");
        let default_cfg = RomajiConfig::default();
        assert_eq!(convert_text_with_romaji("tōkyō", &default_cfg), "tō•kyō");
        assert_eq!(convert_text_with_romaji("toukyou", &default_cfg), "tou•kyou");
        assert_eq!(convert_text_with_romaji("konnichiwa", &default_cfg), "kon•ni•chi•wa");
        assert_eq!(convert_text_with_romaji("tenmaku", &default_cfg), "ten•ma•ku");
        assert_eq!(
            split_romaji_word_to_morae("matchi", &default_cfg),
            Some(split_kana_to_morae("まっち", &default_cfg))
        );
        assert_eq!(
            split_romaji_word_to_morae("toukyou", &default_cfg),
            Some(split_kana_to_morae("とうきょう", &default_cfg))
        );
    }

    #[test]
    fn romaji_splitter_preserves_capitalization_and_protects_english() {
        let cfg = RomajiConfig::default();

        // 100% original casing preserved
        assert_eq!(convert_text_with_romaji("Nihongo", &cfg), "Ni•hon•go");
        assert_eq!(convert_text_with_romaji("NIHONGO", &cfg), "NI•HON•GO");
        assert_eq!(
            convert_text_with_romaji("Kimi no Na wa", &cfg),
            "Ki•mi no Na wa"
        );
        assert_eq!(convert_text_with_romaji("nihongo", &cfg), "ni•hon•go");

        // Sprinkled English words stay untouched (strict Hepburn rejects + skip list + ambiguous context)
        assert_eq!(
            convert_text_with_romaji("Odoridashite jump up now", &cfg),
            "O•do•ri•da•shi•te jump up now"
        );
        assert_eq!(
            convert_text_with_romaji("Love and heart don't stop stella night baby", &cfg),
            "Love and heart don't stop stella night baby"
        );
        assert_eq!(
            convert_text_with_romaji("Time by my side give me live fire home one more", &cfg),
            "Time by my side give me live fire home one more"
        );

        // Context-sensitive bilingual words
        assert_eq!(
            convert_text_with_romaji("Kimi no te made", &cfg),
            "Ki•mi no te ma•de"
        );
        assert_eq!(
            convert_text_with_romaji("I'll make it shine", &cfg),
            "I'll make it shine"
        );

        // Pre-split English.txt words remain untouched and whole output is idempotent
        let mixed = "[P1] Be•fore ki•mi to some•one a•wake, sayounara\n[P2] ちょっと matte!";
        let once = convert_text_with_romaji(mixed, &cfg);
        assert_eq!(
            once,
            "[P1] Be•fore ki•mi to some•one a•wake, sa•you•na•ra\n[P2] Cho•tto ma•tte!"
        );
        assert_eq!(convert_text_with_romaji(&once, &cfg), once);
    }
}
