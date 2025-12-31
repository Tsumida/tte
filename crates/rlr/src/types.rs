use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppStateMachineInput(pub Vec<u8>);

impl std::fmt::Display for AppStateMachineInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AppEntry({} bytes)", self.0.len())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppStateMachineOutput(pub Vec<u8>);
pub type AppNodeId = u64;

openraft::declare_raft_types!(
    pub AppTypeConfig:
        D = AppStateMachineInput,
        R = AppStateMachineOutput,
        LeaderId = openraft::impls::leader_id_std::LeaderId<Self>,
        NodeId = AppNodeId,
        Node = openraft::impls::BasicNode,
);
