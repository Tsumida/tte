#![allow(dead_code)]
use crate::error::SequencerErr;

pub trait SequenceEntry {
    fn set_seq_id(&mut self, seq_id: u64, prev_seq_id: u64);

    // 微秒时间戳
    fn set_ts(&mut self, ts: u64);
}

pub struct SequenceResult<T: Send + Sync + 'static> {
    pub payload: T,
    pub err: Option<SequencerErr>,
}

impl<T: Send + Sync + 'static> SequenceResult<T> {
    pub fn new(payload: T, err: Option<SequencerErr>) -> Self {
        SequenceResult { payload, err }
    }
}
