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

pub fn is_japanese_char(c: char) -> bool {
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

fn macron_for_vowel(v: char) -> char {
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
                continue;
            } else {
                if let Some(last) = morae.last_mut() {
                    last.push('\'');
                } else {
                    morae.push("'".to_owned());
                }
                u_idx += 1;
                continue;
            }
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
        let mut merged = Vec::with_capacity(morae.len());
        let mut m_idx = 0;
        while m_idx < morae.len() {
            let current = &morae[m_idx];
            if m_idx + 1 < morae.len() && !(current.ends_with('\'') || current.ends_with('n')) {
                let nxt = morae[m_idx + 1].as_str();
                if let Some(last_char) = current.chars().last().and_then(|c| c.to_lowercase().next())
                {
                    if nxt == "i" && matches!(last_char, 'a' | 'e' | 'o' | 'u' | 'i') {
                        merged.push(format!("{current}i"));
                        m_idx += 2;
                        continue;
                    }
                    if nxt == "u" && matches!(last_char, 'o' | 'u' | 'a') {
                        merged.push(format!("{current}u"));
                        m_idx += 2;
                        continue;
                    }
                    if matches!(nxt, "a" | "e" | "o") && nxt.starts_with(last_char) {
                        merged.push(format!("{current}{nxt}"));
                        m_idx += 2;
                        continue;
                    }
                }
            }
            merged.push(current.clone());
            m_idx += 1;
        }
        morae = merged;
    }

    morae
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
        } else if c.is_ascii_alphanumeric() {
            let start = i;
            i += 1;
            while i < n {
                if chars[i].is_ascii_alphanumeric() {
                    i += 1;
                } else if chars[i] == '\'' && i + 1 < n && chars[i + 1].is_ascii_alphanumeric() {
                    i += 2;
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
}
