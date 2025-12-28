// #![cfg(feature = "openraft")]

use crate::types::AppTypeConfig;
// use crate::rpc::{RequestPayload, ResponsePayload, RpcHandler};
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
struct RlrNetwork {}

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
