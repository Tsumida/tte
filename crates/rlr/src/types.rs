use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppEntry(pub Vec<u8>);

impl std::fmt::Display for AppEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AppEntry({} bytes)", self.0.len())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppResponse(pub Vec<u8>);
pub type AppNodeId = u64;

openraft::declare_raft_types!(
    pub AppTypeConfig:
        D = AppEntry,
        R = AppResponse,
        // Use standard raft leader-id semantics so committed leader-id is just `term`,
        // which matches the protobuf types used for network transport in this crate.
        LeaderId = openraft::impls::leader_id_std::LeaderId<Self>,
        NodeId = AppNodeId,
        Node = openraft::impls::BasicNode,
);
