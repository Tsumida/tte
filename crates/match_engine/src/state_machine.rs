use crate::{
    orderbook::OrderBook,
    types::{CmdWrapper, MatchCmd, MatchCmdOutput},
};
use tte_rlr::AppStateMachine;

impl TryFrom<&tte_rlr::AppStateMachineInput> for CmdWrapper<MatchCmd> {
    type Error = anyhow::Error;

    fn try_from(value: &tte_rlr::AppStateMachineInput) -> Result<Self, Self::Error> {
        let cmd_wrapper: CmdWrapper<MatchCmd> = serde_json::from_slice(&value.0)?;
        Ok(cmd_wrapper)
    }
}

impl TryInto<tte_rlr::AppStateMachineOutput> for CmdWrapper<MatchCmdOutput> {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<tte_rlr::AppStateMachineOutput, Self::Error> {
        let data = serde_json::to_vec(self.inner())?;
        Ok(tte_rlr::AppStateMachineOutput(data))
    }
}

impl TryFrom<tte_rlr::AppStateMachineOutput> for CmdWrapper<MatchCmdOutput> {
    type Error = anyhow::Error;

    fn try_from(value: tte_rlr::AppStateMachineOutput) -> Result<Self, Self::Error> {
        let cmd_wrapper: CmdWrapper<MatchCmdOutput> = serde_json::from_slice(&value.0)?;
        Ok(cmd_wrapper)
    }
}

impl AppStateMachine for OrderBook {
    type Input = CmdWrapper<MatchCmd>;
    type Output = CmdWrapper<MatchCmdOutput>;

    fn apply(&mut self, req: Self::Input) -> anyhow::Result<Self::Output> {
        let cmd_wrapper = req;
        self.update_seq_id(cmd_wrapper.seq_id);
        Ok(match cmd_wrapper.inner {
            MatchCmd::MatchReq(req) => {
                let mut output = CmdWrapper {
                    inner: MatchCmdOutput::NoOp,
                    seq_id: cmd_wrapper.seq_id,
                    prev_seq_id: cmd_wrapper.prev_seq_id,
                    ts: cmd_wrapper.ts,
                };
                let result = self.handle_match_req(req);
                output.inner = MatchCmdOutput::MatchResult(result);
                output
            }
            MatchCmd::MatchAdminCmd(admin_cmd) => match admin_cmd.admin_action {
                // x if x == oms::AdminAction::TakeSnapshot as i32 => {
                //     // info!(
                //     //     "TakeSnapshot admin command received, current seq_id: {}",
                //     //     cmd_wrapper.seq_id
                //     // );

                //     // match self.take_snapshot() {
                //     //     Ok(snapshot) => {
                //     //         self.persist_snapshot_json(snapshot).await;
                //     //     }
                //     //     Err(e) => {
                //     //         error!("TakeSnapshot failed: {}", e);
                //     //     }
                //     // }
                // }
                _ => CmdWrapper {
                    inner: MatchCmdOutput::NoOp,
                    seq_id: cmd_wrapper.seq_id,
                    prev_seq_id: cmd_wrapper.prev_seq_id,
                    ts: cmd_wrapper.ts,
                },
            },
        })
    }

    fn from_snapshot(data: Vec<u8>) -> Result<Self, anyhow::Error> {
        Ok(serde_json::from_slice(&data)?)
    }

    fn take_snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}
