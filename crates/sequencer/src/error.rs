#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SequencerErr(i32);

impl SequencerErr {
    pub fn new(code: i32) -> Self {
        SequencerErr(code)
    }
}

impl std::fmt::Display for SequencerErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SequencerErr(code={})", self.0)
    }
}
