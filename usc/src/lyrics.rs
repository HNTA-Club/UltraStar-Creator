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
