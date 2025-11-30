#![allow(dead_code)]
use std::sync::atomic::AtomicU64;

use tokio::sync::mpsc;
// use tokio::sync::oneshot;
use tonic::async_trait;

use crate::sequencer::error::SequencerErr;

pub trait SequenceSetter {
    fn set_seq_id(&mut self, seq_id: u64, prev_seq_id: u64);
}

#[async_trait]
pub trait Sequencer<T: Send + Sync + 'static>: Send + Sync {
    fn get_seq_id(&self) -> u64;
    async fn append(&self, cmd: T) -> Result<SequenceResult<T>, SequencerErr>;
    async fn batch_append(&self, cmds: Vec<T>) -> Result<Vec<SequenceResult<T>>, SequencerErr>;
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

// todo: Input, Output用不同类型
pub struct DefaultSequencer<T: Send + Sync + 'static> {
    seq_id: AtomicU64,
    _phantom: std::marker::PhantomData<T>,
    submit_recv: mpsc::Receiver<T>,
    commit_send: mpsc::Sender<T>,
}

type SubmitSender<T> = tokio::sync::mpsc::Sender<T>;
type CommitReceiver<T> = tokio::sync::mpsc::Receiver<T>;

impl<T> DefaultSequencer<T>
where
    T: Send + Sync + 'static + SequenceSetter,
{
    pub fn new(
        init_seq_id: u64,
        channel_size: usize,
    ) -> (Self, SubmitSender<T>, CommitReceiver<T>) {
        let (submit_send, submit_recv) = mpsc::channel::<T>(channel_size);
        let (commit_send, commit_recv) = mpsc::channel::<T>(channel_size);

        let sequencer = DefaultSequencer {
            seq_id: AtomicU64::new(init_seq_id),
            _phantom: std::marker::PhantomData,
            submit_recv,
            commit_send,
        };

        (sequencer, submit_send, commit_recv)
    }

    fn advance_seq_id(&self) -> u64 {
        self.seq_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    }

    pub async fn run(mut self) {
        tracing::info!("sequencer up");
        while let Some(mut cmd) = self.submit_recv.recv().await {
            let prev_seq_id = self.get_seq_id();
            let seq_id = self.advance_seq_id();
            cmd.set_seq_id(seq_id, prev_seq_id);
            tracing::info!(
                "sequencer: received cmd, seq_id={}, prev_seq_id={}",
                seq_id,
                prev_seq_id
            );
            // todo: persist cmd
            _ = self.commit_send.send(cmd).await;
            tracing::info!("sequencer: cmd seq_id={} sent to commit", seq_id);
        }
        tracing::info!("sequencer down");
    }
}

#[async_trait]
impl<T> Sequencer<T> for DefaultSequencer<T>
where
    T: Send + Sync + 'static,
{
    fn get_seq_id(&self) -> u64 {
        self.seq_id.load(std::sync::atomic::Ordering::SeqCst)
    }

    async fn append(&self, cmd: T) -> Result<SequenceResult<T>, SequencerErr> {
        Ok(SequenceResult {
            payload: cmd,
            err: None,
        })
    }

    async fn batch_append(&self, cmds: Vec<T>) -> Result<Vec<SequenceResult<T>>, SequencerErr> {
        let results = cmds
            .into_iter()
            .map(|cmd| SequenceResult {
                payload: cmd,
                err: None,
            })
            .collect();
        Ok(results)
    }
}
