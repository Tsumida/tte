// #![cfg(feature = "openraft")]

use crate::pbcode::raft::{VoteReq, VoteRsp};
use crate::types::AppTypeConfig;
use openraft::Vote;
use openraft::{
    OptionalSend, Snapshot,
    error::{RPCError, ReplicationClosed, StreamingError},
    network::{RPCOption, RaftNetwork, RaftNetworkFactory},
    raft::{
        AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest,
        InstallSnapshotResponse, SnapshotResponse, VoteRequest, VoteResponse,
    },
    type_config::alias::VoteOf,
};
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::time::Duration;
struct RlrNetwork {
    // peers:
}

impl openraft::network::v2::RaftNetworkV2<AppTypeConfig> for RlrNetwork {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<AppTypeConfig>,
        option: RPCOption,
    ) -> Result<AppendEntriesResponse<AppTypeConfig>, RPCError<AppTypeConfig>> {
        todo!()
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<AppTypeConfig>,
        option: RPCOption,
    ) -> Result<VoteResponse<AppTypeConfig>, RPCError<AppTypeConfig>> {
        // let req = VoteReq {
        //     term: rpc.vote.term,
        //     candidate_id: rpc.vote.candidate_id,
        //     last_log_index: rpc.vote.last_log_index,
        //     last_log_term: rpc.vote.last_log_term,
        // };
        // let
        todo!()
    }

    async fn full_snapshot(
        &mut self,
        vote: VoteOf<AppTypeConfig>,
        snapshot: Snapshot<AppTypeConfig>,
        cancel: impl Future<Output = ReplicationClosed> + OptionalSend + 'static,
        option: RPCOption,
    ) -> Result<SnapshotResponse<AppTypeConfig>, StreamingError<AppTypeConfig>> {
        todo!()
    }
}
