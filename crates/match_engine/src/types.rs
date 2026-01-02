use getset::Getters;
use tte_core::pbcode::oms;
use tte_rlr::AppStateMachineInput;
use tte_sequencer::api::SequenceEntry;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MatchCmd {
    MatchReq(oms::BatchMatchRequest),
    MatchAdminCmd(oms::MatchAdminCmd),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MatchCmdOutput {
    NoOp,
    MatchResult(oms::BatchMatchResult),
    Snapshot(Vec<u8>),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Getters)]
pub struct CmdWrapper<T: Send + Sync + 'static> {
    #[get = "pub"]
    pub(crate) inner: T,
    pub(crate) seq_id: u64,
    pub(crate) prev_seq_id: u64,
    pub(crate) ts: u64,
}

impl Into<AppStateMachineInput> for CmdWrapper<MatchCmd> {
    fn into(self) -> AppStateMachineInput {
        AppStateMachineInput(serde_json::to_vec(&self).expect("serialize MatchCmd"))
    }
}

impl<T> CmdWrapper<T>
where
    T: Send + Sync + 'static,
{
    pub fn new(inner: T) -> Self {
        CmdWrapper {
            inner,
            seq_id: 0,
            prev_seq_id: 0,
            ts: 0,
        }
    }
}

impl<T> SequenceEntry for CmdWrapper<T>
where
    T: Send + Sync + 'static,
{
    fn set_seq_id(&mut self, seq_id: u64, prev_seq_id: u64) {
        self.seq_id = seq_id;
        self.prev_seq_id = prev_seq_id;
    }

    fn set_ts(&mut self, ts: u64) {
        self.ts = ts;
    }
}

pub(crate) type SequencerSender = tokio::sync::mpsc::Sender<CmdWrapper<MatchCmd>>;
pub(crate) type SequencerReceiver = tokio::sync::mpsc::Receiver<CmdWrapper<MatchCmd>>;
pub(crate) type MatchResultSender = tokio::sync::mpsc::Sender<CmdWrapper<oms::BatchMatchResult>>;
pub(crate) type MatchResultReceiver =
    tokio::sync::mpsc::Receiver<CmdWrapper<oms::BatchMatchResult>>;
