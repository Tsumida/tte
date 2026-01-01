use std::sync::atomic::AtomicU64;

use tokio::sync::mpsc;

use crate::api::SequenceEntry;

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
    T: Send + Sync + 'static + SequenceEntry,
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

    fn get_seq_id(&self) -> u64 {
        self.seq_id.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub async fn run(mut self) {
        tracing::info!("sequencer up");
        while let Some(mut cmd) = self.submit_recv.recv().await {
            let prev_seq_id = self.get_seq_id();
            let seq_id = self.advance_seq_id();
            cmd.set_seq_id(seq_id, prev_seq_id);
            cmd.set_ts(tte_core::time::now_us());
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
