use crate::types::MatchResultSender;
use tonic::async_trait;
use tte_core::pbcode::oms::{self, MatchResult};
use tte_rlr::{AppTypeConfig, RaftTypeConfig};
use tte_sequencer::raft::EgressController;

pub(crate) struct NoOpEgressController {}

#[async_trait]
impl EgressController for NoOpEgressController {
    // drop all
    async fn handle_response(
        &self,
        _: <AppTypeConfig as RaftTypeConfig>::R,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

pub(crate) struct AllowAllEgressController {
    sender: MatchResultSender,
}

impl AllowAllEgressController {
    pub fn new(sender: MatchResultSender) -> Self {
        Self { sender }
    }
}

#[async_trait]
impl EgressController for AllowAllEgressController {
    async fn handle_response(
        &self,
        rsp: <AppTypeConfig as RaftTypeConfig>::R,
    ) -> Result<(), anyhow::Error> {
        // let match_result: MatchResult = resp.into();
        // self.sender.send(match_result).await
        // Ok(())

        todo!()
    }
}
