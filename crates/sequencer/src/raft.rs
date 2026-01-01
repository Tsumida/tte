use std::{
    collections::HashMap,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::api::SequenceEntry;
use futures::TryStreamExt;
use tokio::sync::oneshot;
use tte_rlr::{
    AppNodeId, AppStateMachine, AppStateMachineHandler, AppStateMachineInput, AppTypeConfig,
    BasicNode, Raft, RaftStateMachine, Rlr, new_rlr,
};

// todo: Input, Output用不同类型
pub struct RaftSequencer<S: AppStateMachine, E: Into<AppStateMachineInput> + SequenceEntry + Clone>
{
    // 采用log_id作为seq_id
    seq_id: AtomicU64,
    // 可靠日志复制 + 业务状态机, todo: 依赖注入 -> Rlr<S>
    raft: Rlr,
    req_recv: tokio::sync::mpsc::Receiver<E>,
    _phantom: std::marker::PhantomData<S>,
}

impl<S, E> RaftSequencer<S, E>
where
    S: AppStateMachine,
    E: Into<AppStateMachineInput> + SequenceEntry + Clone,
{
    pub async fn new(
        node_id: AppNodeId,
        db_path: &Path,
        nodes: &HashMap<AppNodeId, BasicNode>,
    ) -> Result<(Self, tokio::sync::mpsc::Sender<E>), anyhow::Error> {
        let (req_send, req_recv) = tokio::sync::mpsc::channel::<E>(256);
        let rlr = new_rlr::<S>(node_id, db_path, nodes).await?;

        // wait for re-install snapshot
        let seq_id = Self::load_last_seq_id(&rlr).await?;

        Ok((
            RaftSequencer {
                seq_id: AtomicU64::new(seq_id), // todo
                raft: rlr,
                req_recv,
                _phantom: std::marker::PhantomData,
            },
            req_send,
        ))
    }

    async fn load_last_seq_id(rlr: &Raft<AppTypeConfig>) -> Result<u64, anyhow::Error> {
        let (s, r) = oneshot::channel::<u64>();
        rlr.with_state_machine(|sm: &mut AppStateMachineHandler<S>| {
            Box::pin(async {
                let (last_applied_log_id, _) = sm.applied_state().await.unwrap();
                s.send(last_applied_log_id.map_or(0, |id| id.index))
                    .unwrap();
            })
        })
        .await??;
        let seq_id = r.await.unwrap();
        Ok(seq_id)
    }

    fn advance_seq_id(&self, last_applied_log_id: u64) -> u64 {
        self.seq_id.fetch_max(last_applied_log_id, Ordering::SeqCst)
    }

    pub async fn run(mut self) {
        let batch_size = 256;
        let mut batch: Vec<E> = Vec::with_capacity(batch_size);

        loop {
            while batch.len() < batch_size {
                if let Ok(entry) = self.req_recv.try_recv() {
                    batch.push(entry);
                } else if batch.is_empty() {
                    // If empty, do a blocking wait
                    if let Some(entry) = self.req_recv.recv().await {
                        batch.push(entry);
                    } else {
                        tracing::error!("RaftSequencer: request channel closed");
                        return; // Channel closed
                    }
                } else {
                    break;
                }
            }
            if batch.is_empty() {
                continue;
            }
            // todo: seq_id应该要维护在statemachine里
            // propose分配seq_id, 但未持久化, 只有commit后通过append_entries更新seq_id. 不能让日志形成空洞, 不然后续
            let mut current_seq = self.seq_id.load(Ordering::SeqCst);
            let inputs: Vec<AppStateMachineInput> = batch
                .iter()
                .map(|entry| {
                    let next_seq = current_seq + 1;
                    let mut req = entry.clone();
                    req.set_seq_id(next_seq, current_seq);
                    current_seq = next_seq;
                    req.into()
                })
                .collect();

            if let Err(fatal_err) = self.batch_propose(inputs).await {
                panic!("RaftSequencer fatal error: {:?}", fatal_err);
            }

            batch.clear();
        }
    }

    async fn batch_propose(&self, inputs: Vec<AppStateMachineInput>) -> Result<(), anyhow::Error> {
        // ? 返回都是Fatal, 无法处理只能停机
        let mut result_stream = self.raft.client_write_many(inputs).await?;
        while let Some(r) = result_stream.try_next().await? {
            match r {
                Ok(write_resp) => {
                    self.advance_seq_id(write_resp.log_id.index);
                    tracing::info!(
                        "RaftSequencer propose success, rsp={:?}",
                        write_resp.response,
                    );
                }
                Err(err) => {
                    // 无法propose
                    tracing::warn!("RaftSequencer isn't leader: {:?}", err);
                }
            }
        }
        Ok(())
    }
}
