use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::api::SequenceEntry;
use futures::TryStreamExt;
use getset::Getters;
use tokio::sync::oneshot;
use tonic::async_trait;
use tte_rlr::{
    AppNodeId, AppStateMachine, AppStateMachineHandler, AppStateMachineInput, AppTypeConfig,
    BasicNode, RaftStateMachine, RaftTypeConfig, Rlr, RlrBuilder, RlrLogStore, RlrNetworkFactory,
};

#[derive(Clone, Debug, Getters)]
pub struct RaftSequencerConfig {
    #[getset(get = "pub")]
    db_path: String,
    #[getset(get = "pub")]
    snapshot_path: String,
    #[getset(get = "pub")]
    node_id: AppNodeId,
    #[getset(get = "pub")]
    nodes: HashMap<AppNodeId, BasicNode>,
}

impl RaftSequencerConfig {
    // 从环境变量加载配置
    //  RAFT_NODE_ID=1
    //  RAFT_NODES="1@127.0.0.1:7001,2@127.0.0.1:7002,3@127.0.0.1:7003"
    //  RAFT_DB_PATH="./data/raft_node_1"
    pub fn from_env() -> Result<Self, anyhow::Error> {
        let node_id: AppNodeId = std::env::var("RAFT_NODE_ID")?.parse()?;

        let nodes_str = std::env::var("RAFT_NODES")?;
        let mut nodes = HashMap::new();
        for node_pair in nodes_str.split(',') {
            let mut parts = node_pair.splitn(2, '@');
            let id: AppNodeId = parts
                .next()
                .expect("Invalid RAFT_NODES format")
                .parse()
                .expect("Invalid node ID in RAFT_NODES");
            let addr = parts.next().expect("Invalid RAFT_NODES format").to_string();
            nodes.insert(id, BasicNode::new(addr));
        }

        let db_path = std::env::var("RAFT_DB_PATH")?;
        let snapshot_path = std::env::var("RAFT_SNAPSHOT_PATH").unwrap_or_else(|_| {
            let mut path = db_path.clone();
            path.push_str("./snapshots"); // 一般在 bin/snapshots
            path
        });

        Ok(RaftSequencerConfig {
            db_path,
            node_id,
            nodes,
            snapshot_path,
        })
    }

    pub fn test(db_path: &str, snapshot_path: &str) -> Self {
        let node_id: AppNodeId = 1;

        let mut nodes = HashMap::new();
        nodes.insert(1, BasicNode::new("127.0.0.1:7001".to_string()));
        nodes.insert(2, BasicNode::new("127.0.0.1:7002".to_string()));
        nodes.insert(3, BasicNode::new("127.0.0.1:7003".to_string()));
        Self {
            db_path: db_path.to_string(),
            node_id,
            nodes,
            snapshot_path: snapshot_path.to_string(),
        }
    }
}

pub struct RaftSequencerBuilder<S, D, R, E>
where
    S: AppStateMachine,
    D: Into<<AppTypeConfig as RaftTypeConfig>::D> + SequenceEntry + Clone,
    R: TryFrom<<AppTypeConfig as RaftTypeConfig>::R> + Send + Sync,
    E: EgressController<R>,
{
    node_id: Option<<AppTypeConfig as RaftTypeConfig>::NodeId>,
    db_path: Option<PathBuf>,
    snapshot_path: Option<PathBuf>,
    nodes: HashMap<<AppTypeConfig as RaftTypeConfig>::NodeId, BasicNode>,
    req_recv: Option<tokio::sync::mpsc::Receiver<D>>,
    egress: Option<E>,
    state_machine: Option<S>,
    _r: std::marker::PhantomData<R>,
}

impl<S, D, R, E> RaftSequencerBuilder<S, D, R, E>
where
    S: AppStateMachine,
    D: Into<<AppTypeConfig as RaftTypeConfig>::D> + SequenceEntry + Clone,
    R: TryFrom<<AppTypeConfig as RaftTypeConfig>::R> + Send + Sync,
    E: EgressController<R>,
{
    pub fn new() -> Self {
        RaftSequencerBuilder {
            node_id: None,
            db_path: None,
            snapshot_path: None,
            nodes: HashMap::new(),
            req_recv: None,
            egress: None,
            state_machine: None,
            _r: std::marker::PhantomData,
        }
    }

    pub fn with_node_id(mut self, node_id: AppNodeId) -> Self {
        self.node_id = Some(node_id);
        self
    }

    pub fn with_db_path(mut self, db_path: PathBuf) -> Self {
        self.db_path = Some(db_path);
        self
    }

    pub fn with_snapshot_path(mut self, snapshot_path: PathBuf) -> Self {
        self.snapshot_path = Some(snapshot_path);
        self
    }

    pub fn with_nodes(
        mut self,
        nodes: HashMap<<AppTypeConfig as RaftTypeConfig>::NodeId, BasicNode>,
    ) -> Self {
        self.nodes = nodes;
        self
    }

    pub fn with_request_receiver(mut self, req_recv: tokio::sync::mpsc::Receiver<D>) -> Self {
        self.req_recv = Some(req_recv);
        self
    }

    pub fn with_egress(mut self, egress: E) -> Self {
        self.egress = Some(egress);
        self
    }

    pub fn with_state_machine(mut self, state_machine: S) -> Self {
        self.state_machine = Some(state_machine);
        self
    }

    pub async fn build_components(
        self,
    ) -> Result<
        (
            Arc<tte_rlr::Config>,
            RlrNetworkFactory,
            RlrLogStore<AppTypeConfig>,
            AppStateMachineHandler<S>,
        ),
        anyhow::Error,
    > {
        // let node_id = self.node_id.expect("node_id is required");
        let db_path = self.db_path.expect("db_path is required");
        let snapshot_dir = self.snapshot_path.expect("snapshot_path is required");
        let state_machine = self.state_machine.expect("state machine is required");
        RlrBuilder::new()
            .build_components_only::<S>(
                Path::new(&db_path),
                Path::new(&snapshot_dir),
                &self.nodes,
                state_machine,
            )
            .await
    }

    pub async fn build(self) -> Result<RaftSequencer<S, D, R, E>, anyhow::Error> {
        let node_id = self.node_id.expect("node_id is required");
        let db_path = self.db_path.expect("db_path is required");
        let snapshot_dir = self.snapshot_path.expect("snapshot_path is required");
        let req_recv = self.req_recv.expect("request receiver is required");
        let egress = self.egress.expect("egress controller is required");
        let state_machine = self.state_machine.expect("state machine is required");
        let rlr = RlrBuilder::new()
            .build::<S>(
                node_id,
                Path::new(&db_path),
                Path::new(&snapshot_dir),
                &self.nodes,
                state_machine,
            )
            .await?;

        Ok(RaftSequencer {
            seq_id: AtomicU64::new(0), // 后续自己初始化
            raft: rlr,
            req_recv,
            egress,
            _s: std::marker::PhantomData,
            _d: std::marker::PhantomData,
            _r: std::marker::PhantomData,
        })
    }
}

#[derive(Getters)]
pub struct RaftSequencer<
    S: AppStateMachine,
    D: Into<<AppTypeConfig as RaftTypeConfig>::D> + SequenceEntry + Clone,
    R: TryFrom<<AppTypeConfig as RaftTypeConfig>::R> + Send + Sync,
    E: EgressController<R>,
> {
    seq_id: AtomicU64, // 采用log_id作为seq_id
    #[getset(get = "pub")]
    raft: Rlr, // 可靠日志复制 + 自动更新业务状态机
    req_recv: tokio::sync::mpsc::Receiver<D>,
    egress: E,
    _s: std::marker::PhantomData<S>,
    _d: std::marker::PhantomData<D>,
    _r: std::marker::PhantomData<R>,
}

impl<S, D, R, E> RaftSequencer<S, D, R, E>
where
    S: AppStateMachine,
    D: Into<<AppTypeConfig as RaftTypeConfig>::D> + SequenceEntry + Clone,
    R: TryFrom<<AppTypeConfig as RaftTypeConfig>::R> + Send + Sync,
    E: EgressController<R>,
{
    pub async fn load_last_seq_id(&mut self) -> Result<u64, anyhow::Error> {
        let (s, r) = oneshot::channel::<u64>();
        self.raft
            .with_state_machine(|sm: &mut AppStateMachineHandler<S>| {
                Box::pin(async {
                    let (last_applied_log_id, _) = sm.applied_state().await.unwrap();
                    s.send(last_applied_log_id.map_or(0, |id| id.index))
                        .unwrap();
                })
            })
            .await??;
        let seq_id = r.await.unwrap();

        self.seq_id.store(seq_id, Ordering::SeqCst);
        Ok(seq_id)
    }

    fn advance_seq_id(&self, last_applied_log_id: u64) -> u64 {
        self.seq_id.fetch_max(last_applied_log_id, Ordering::SeqCst)
    }

    async fn read_batch(&mut self, batch: &mut Vec<D>, batch_size: usize) {
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
    }

    pub async fn run(mut self) {
        let batch_size = 32;
        let mut batch: Vec<D> = Vec::with_capacity(batch_size);
        loop {
            self.read_batch(&mut batch, batch_size).await;
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
                    // todo: 处理egress失败
                    self.egress
                        .handle_response(write_resp.response)
                        .await
                        .map_err(|e| {
                            tracing::error!("EgressController handle_response error: {}", e);
                            e
                        })?;
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

#[async_trait]
pub trait EgressController<R: TryFrom<<AppTypeConfig as RaftTypeConfig>::R> + Send + Sync> {
    async fn handle_response(
        &self,
        response: <AppTypeConfig as RaftTypeConfig>::R,
    ) -> Result<(), anyhow::Error>;
}
