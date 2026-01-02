use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppStateMachineInput(pub Vec<u8>);

impl std::fmt::Display for AppStateMachineInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AppEntry({} bytes)", self.0.len())
    }
}

impl TryFrom<&[u8]> for AppStateMachineInput {
    type Error = anyhow::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Ok(AppStateMachineInput(value.to_vec()))
    }
}

impl TryFrom<&AppStateMachineInput> for AppStateMachineInput {
    type Error = anyhow::Error;

    fn try_from(value: &AppStateMachineInput) -> Result<Self, Self::Error> {
        Ok(AppStateMachineInput(value.0.clone()))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppStateMachineOutput(pub Vec<u8>);
pub type AppNodeId = u64;

// note: 目前还不能用AppInput<T>, 不然可以节省一次反序列化
openraft::declare_raft_types!(
    pub AppTypeConfig:
        D = AppStateMachineInput,
        R = AppStateMachineOutput,
        LeaderId = openraft::impls::leader_id_std::LeaderId<Self>,
        NodeId = AppNodeId,
        Node = openraft::impls::BasicNode,
);
