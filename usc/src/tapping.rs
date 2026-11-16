#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub syllable: usize,
    pub start_ms: f64,
    pub dur_ms: f64,
}
