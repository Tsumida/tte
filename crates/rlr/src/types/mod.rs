use crate::pbcode::raft as pb;

// #[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub type AppStateMachineInput = pb::AppStateMachineInput;

impl std::fmt::Display for AppStateMachineInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AppEntry({} bytes)", self.data.len())
    }
}

impl TryFrom<&[u8]> for AppStateMachineInput {
    type Error = anyhow::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Ok(AppStateMachineInput {
            data: value.to_vec(),
        })
    }
}

impl TryFrom<&AppStateMachineInput> for AppStateMachineInput {
    type Error = anyhow::Error;

    fn try_from(value: &AppStateMachineInput) -> Result<Self, Self::Error> {
        Ok(AppStateMachineInput {
            data: value.data.clone(),
        })
    }
}

pub type AppStateMachineOutput = pb::AppStateMachineOutput;

impl TryFrom<&[u8]> for AppStateMachineOutput {
    type Error = anyhow::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Ok(AppStateMachineOutput {
            data: value.to_vec(),
        })
    }
}

impl std::fmt::Display for AppStateMachineOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AppResponse({} bytes)", self.data.len())
    }
}

pub type AppNodeId = u64;

// note: 目前还不能用AppInput<T>, 不然可以节省一次反序列化
openraft::declare_raft_types!(
    pub AppTypeConfig:
        D = AppStateMachineInput,
        R = AppStateMachineOutput,
        LeaderId = pb::LeaderId,
        Vote = pb::Vote,
        Entry = pb::Entry,
        NodeId = AppNodeId,
        Node = pb::Node,
        SnapshotData = Vec<u8>,
);
