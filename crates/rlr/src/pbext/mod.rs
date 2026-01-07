use crate::AppTypeConfig;
use crate::pbcode::raft::LeaderId;
use crate::pbcode::raft::snapshot_req::Payload;
// 为Pb Msg实现各种From/Into
use crate::pbcode::raft as pb;
use openraft::LogId;
use openraft::Membership;
use openraft::entry::RaftEntry;
use openraft::entry::RaftPayload;
use openraft::raft::AppendEntriesRequest;
use openraft::raft::AppendEntriesResponse;
use openraft::raft::ClientWriteResponse;
use openraft::raft::StreamAppendError;
use openraft::raft::StreamAppendResult;
use openraft::raft::VoteRequest;
use openraft::raft::VoteResponse;
use openraft::type_config::alias::LogIdOf;

use openraft::entry::EntryPayload;
use openraft::vote::LeaderIdCompare;
use openraft::vote::RaftLeaderId;
use openraft::vote::RaftVote;
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt;

impl From<AppendEntriesRequest<AppTypeConfig>> for pb::AppendEntriesReq {
    fn from(ae_req: AppendEntriesRequest<AppTypeConfig>) -> Self {
        pb::AppendEntriesReq {
            vote: Some(ae_req.vote),
            prev_log_id: ae_req.prev_log_id.map(|log_id| log_id.into()),
            entries: ae_req.entries,
            leader_commit: ae_req.leader_commit.map(|log_id| log_id.into()),
        }
    }
}

impl TryFrom<pb::AppendEntriesReq> for AppendEntriesRequest<AppTypeConfig> {
    type Error = anyhow::Error;

    fn try_from(proto: pb::AppendEntriesReq) -> Result<Self, Self::Error> {
        Ok(AppendEntriesRequest {
            vote: proto
                .vote
                .ok_or_else(|| anyhow::anyhow!("Missing vote in AppendEntriesReq"))?,
            prev_log_id: proto
                .prev_log_id
                .map(|log_id| log_id.try_into())
                .transpose()?,
            entries: proto.entries,
            leader_commit: proto
                .leader_commit
                .map(|log_id| log_id.try_into())
                .transpose()?,
        })
    }
}

impl TryFrom<pb::AppendEntriesRsp> for AppendEntriesResponse<AppTypeConfig> {
    type Error = anyhow::Error;

    fn try_from(proto: pb::AppendEntriesRsp) -> Result<Self, Self::Error> {
        if let Some(higher) = proto.rejected_by {
            return Ok(AppendEntriesResponse::HigherVote(higher));
        }
        if proto.conflict {
            return Ok(AppendEntriesResponse::Conflict);
        }
        if let Some(log_id) = proto.last_log_id {
            Ok(AppendEntriesResponse::PartialSuccess(Some(
                log_id.try_into()?,
            )))
        } else {
            Ok(AppendEntriesResponse::Success)
        }
    }
}

impl From<AppendEntriesResponse<AppTypeConfig>> for pb::AppendEntriesRsp {
    fn from(ae_resp: AppendEntriesResponse<AppTypeConfig>) -> Self {
        match ae_resp {
            AppendEntriesResponse::Success => pb::AppendEntriesRsp {
                rejected_by: None,
                conflict: false,
                last_log_id: None,
            },
            AppendEntriesResponse::PartialSuccess(Some(log_id)) => pb::AppendEntriesRsp {
                rejected_by: None,
                conflict: false,
                last_log_id: Some(log_id.into()),
            },
            AppendEntriesResponse::PartialSuccess(None) => pb::AppendEntriesRsp {
                rejected_by: None,
                conflict: false,
                last_log_id: None,
            },
            AppendEntriesResponse::Conflict => pb::AppendEntriesRsp {
                rejected_by: None,
                conflict: true,
                last_log_id: None,
            },
            AppendEntriesResponse::HigherVote(vote) => pb::AppendEntriesRsp {
                rejected_by: Some(vote),
                conflict: false,
                last_log_id: None,
            },
        }
    }
}

impl From<StreamAppendResult<AppTypeConfig>> for pb::AppendEntriesRsp {
    fn from(result: StreamAppendResult<AppTypeConfig>) -> Self {
        match result {
            Ok(Some(log_id)) => pb::AppendEntriesRsp {
                rejected_by: None,
                conflict: false,
                last_log_id: Some(log_id.into()),
            },
            Ok(None) => pb::AppendEntriesRsp {
                rejected_by: None,
                conflict: false,
                last_log_id: None,
            },
            Err(StreamAppendError::Conflict(log_id)) => pb::AppendEntriesRsp {
                rejected_by: None,
                conflict: true,
                last_log_id: Some(log_id.into()),
            },
            Err(StreamAppendError::HigherVote(vote)) => pb::AppendEntriesRsp {
                rejected_by: Some(vote),
                conflict: false,
                last_log_id: None,
            },
        }
    }
}

impl fmt::Display for pb::Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Entry{{term={},index={}}}", self.term, self.index)
    }
}

impl RaftPayload<AppTypeConfig> for pb::Entry {
    fn get_membership(&self) -> Option<Membership<AppTypeConfig>> {
        self.membership.clone().and_then(|m| m.try_into().ok())
    }
}

impl RaftEntry<AppTypeConfig> for pb::Entry {
    fn new(log_id: LogIdOf<AppTypeConfig>, payload: EntryPayload<AppTypeConfig>) -> Self {
        let mut membership = None;
        match payload {
            EntryPayload::Blank => Self {
                term: log_id.leader_id,
                index: log_id.index,
                entry_type: pb::EntryType::Blank as u32,
                data: vec![],
                membership,
            },
            EntryPayload::Normal(data) => Self {
                term: log_id.leader_id,
                index: log_id.index,
                entry_type: pb::EntryType::Normal as u32,
                data: data.data,
                membership,
            },
            EntryPayload::Membership(m) => {
                membership = Some(m.into());
                Self {
                    term: log_id.leader_id,
                    index: log_id.index,
                    entry_type: pb::EntryType::Membership as u32,
                    data: vec![],
                    membership,
                }
            }
        }
    }

    fn log_id_parts(&self) -> (&u64, u64) {
        (&self.term, self.index)
    }

    fn set_log_id(&mut self, new: LogIdOf<AppTypeConfig>) {
        self.term = new.leader_id;
        self.index = new.index;
    }
}

/// Implements PartialOrd for LeaderId to enforce the standard Raft behavior of at most one leader
/// per term.
///
/// In standard Raft, each term can have at most one leader. This is enforced by making leader IDs
/// with the same term incomparable (returning None), unless they refer to the same node.
///
/// This differs from the [`PartialOrd`] default implementation which would allow multiple leaders
/// in the same term by comparing node IDs.
impl PartialOrd for pb::LeaderId {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        LeaderIdCompare::std(self, other)
    }
}

impl PartialEq<u64> for pb::LeaderId {
    fn eq(&self, _other: &u64) -> bool {
        false
    }
}

impl PartialOrd<u64> for pb::LeaderId {
    fn partial_cmp(&self, other: &u64) -> Option<Ordering> {
        self.term.partial_cmp(other)
    }
}

impl fmt::Display for pb::LeaderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T{}-N{}", self.term, self.node_id)
    }
}

impl RaftLeaderId<AppTypeConfig> for pb::LeaderId {
    type Committed = u64;

    fn new(term: u64, node_id: u64) -> Self {
        Self { term, node_id }
    }

    fn term(&self) -> u64 {
        self.term
    }

    fn node_id(&self) -> &u64 {
        &self.node_id
    }

    fn to_committed(&self) -> Self::Committed {
        self.term
    }
}

impl From<LogId<AppTypeConfig>> for pb::LogId {
    fn from(log_id: LogId<AppTypeConfig>) -> Self {
        pb::LogId {
            term: log_id.leader_id, // note: leader_id is term here
            index: log_id.index(),
        }
    }
}

impl From<pb::LogId> for LogId<AppTypeConfig> {
    fn from(proto_log_id: pb::LogId) -> Self {
        LogId::new(proto_log_id.term, proto_log_id.index)
    }
}

impl TryFrom<pb::Membership> for Membership<AppTypeConfig> {
    type Error = openraft::error::MembershipError<AppTypeConfig>;
    fn try_from(value: pb::Membership) -> Result<Self, Self::Error> {
        let mut configs = vec![];
        for c in value.configs {
            let config: BTreeSet<u64> = c.node_ids.keys().copied().collect();
            configs.push(config);
        }
        let nodes = value.nodes;
        Membership::new(configs, nodes)
    }
}

impl From<Membership<AppTypeConfig>> for pb::Membership {
    fn from(value: Membership<AppTypeConfig>) -> Self {
        let mut configs = vec![];
        for c in value.get_joint_config() {
            let mut node_ids = HashMap::new();
            for nid in c.iter() {
                node_ids.insert(*nid, ());
            }
            configs.push(pb::NodeIdSet { node_ids });
        }
        let nodes = value.nodes().map(|(nid, n)| (*nid, n.clone())).collect();
        pb::Membership { configs, nodes }
    }
}

impl pb::SnapshotReq {
    pub fn into_meta(self) -> Option<pb::SnapshotReqMeta> {
        let p = self.payload?;
        match p {
            Payload::Meta(meta) => Some(meta),
            Payload::Chunk(_) => None,
        }
    }

    pub fn into_data_chunk(self) -> Option<Vec<u8>> {
        let p = self.payload?;
        match p {
            Payload::Meta(_) => None,
            Payload::Chunk(chunk) => Some(chunk),
        }
    }
}

impl TryFrom<VoteRequest<AppTypeConfig>> for pb::VoteReq {
    type Error = anyhow::Error;

    fn try_from(vote_req: VoteRequest<AppTypeConfig>) -> Result<Self, Self::Error> {
        Ok(pb::VoteReq {
            vote: Some(vote_req.vote),
            last_log_id: vote_req
                .last_log_id
                .map(|log_id| log_id.try_into())
                .transpose()?,
        })
    }
}

impl TryFrom<VoteResponse<AppTypeConfig>> for pb::VoteRsp {
    type Error = anyhow::Error;

    fn try_from(vote_resp: VoteResponse<AppTypeConfig>) -> Result<Self, Self::Error> {
        Ok(pb::VoteRsp {
            vote: Some(vote_resp.vote),
            vote_granted: vote_resp.vote_granted,
            last_log_id: vote_resp
                .last_log_id
                .map(|log_id| log_id.try_into())
                .transpose()?,
        })
    }
}

impl TryFrom<pb::VoteReq> for VoteRequest<AppTypeConfig> {
    type Error = anyhow::Error;

    fn try_from(proto: pb::VoteReq) -> Result<Self, Self::Error> {
        Ok(VoteRequest {
            vote: proto
                .vote
                .ok_or_else(|| anyhow::anyhow!("Missing vote in VoteReq"))?,
            last_log_id: proto
                .last_log_id
                .map(|log_id| log_id.try_into())
                .transpose()?,
        })
    }
}

impl TryFrom<pb::VoteRsp> for VoteResponse<AppTypeConfig> {
    type Error = anyhow::Error;

    fn try_from(proto: pb::VoteRsp) -> Result<Self, Self::Error> {
        Ok(VoteResponse {
            vote: proto
                .vote
                .ok_or_else(|| anyhow::anyhow!("Missing vote in VoteRsp"))?,
            vote_granted: proto.vote_granted,
            last_log_id: proto
                .last_log_id
                .map(|log_id| log_id.try_into())
                .transpose()?,
        })
    }
}

impl RaftVote<AppTypeConfig> for pb::Vote {
    fn from_leader_id(leader_id: LeaderId, committed: bool) -> Self {
        pb::Vote {
            leader_id: Some(leader_id),
            committed,
        }
    }

    fn leader_id(&self) -> &LeaderId {
        self.leader_id.as_ref().expect("Vote must have a leader_id")
    }

    fn is_committed(&self) -> bool {
        self.committed
    }
}

impl fmt::Display for pb::Vote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "<{}:{}>",
            self.leader_id(),
            if self.is_committed() { "Q" } else { "-" }
        )
    }
}

impl TryFrom<ClientWriteResponse<AppTypeConfig>> for pb::MembershipRsp {
    type Error = anyhow::Error;

    fn try_from(cw_resp: ClientWriteResponse<AppTypeConfig>) -> Result<Self, Self::Error> {
        Ok(pb::MembershipRsp {
            log_id: Some(cw_resp.log_id.into()),
            membership: Some(
                cw_resp
                    .membership
                    .ok_or(anyhow::anyhow!("expect membership"))?
                    .into(),
            ),
        })
    }
}
