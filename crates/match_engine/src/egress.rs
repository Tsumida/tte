use crate::types::{CmdWrapper, MatchCmdOutput, MatchResultSender};
use tonic::async_trait;
use tte_rlr::{AppTypeConfig, RaftTypeConfig};
use tte_sequencer::raft::EgressController;

pub struct AllowAllEgressController {
    sender: MatchResultSender,
}

impl AllowAllEgressController {
    pub fn new(sender: MatchResultSender) -> Self {
        Self { sender }
    }
}

#[async_trait]
impl<R> EgressController<R> for AllowAllEgressController
where
    R: TryFrom<<AppTypeConfig as RaftTypeConfig>::R> + Send + Sync,
{
    async fn handle_response(
        &self,
        rsp: <AppTypeConfig as RaftTypeConfig>::R,
    ) -> Result<(), anyhow::Error> {
        let ob_output: CmdWrapper<MatchCmdOutput> = rsp.try_into()?;
        if let MatchCmdOutput::MatchResult(match_result) = ob_output.inner {
            let _ = self.sender.send(match_result).await?; // todo: handle error
        }
        Ok(())
    }
}
