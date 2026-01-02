#[cfg(test)]
mod me {
    use tracing::info;

    use tte_core::pbcode::oms::{self};
    use tte_core::types::TradePair;
    use tte_infra::config::AppConfig;
    use tte_infra::kafka::{ConsumerConfig, ProducerConfig};
    use tte_me::egress::AllowAllEgressController;
    use tte_me::orderbook::{self, OrderBook};
    use tte_me::service::MatchEngineService;
    use tte_me::types::{CmdWrapper, MatchCmd, MatchCmdOutput};
    use tte_sequencer::raft::{RaftSequencerBuilder, RaftSequencerConfig};

    use std::path::Path;

    use openraft::StorageError;
    use openraft::testing::log::{StoreBuilder, Suite};
    use tempfile::TempDir;
    use tte_rlr::{AppStateMachineHandler, AppTypeConfig, RlrLogStore};

    async fn run_me(
        node_id: u64,
        trade_pair: TradePair,
        config: AppConfig,
        raft_config: RaftSequencerConfig,
        mut exit_signal: tokio::sync::broadcast::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _ = config.init_tracer().await?;
        config.print_args();

        let (_req_send, _match_req_consumer, _sequencer, _match_result_producer) =
            MatchEngineService::build_component(
                raft_config,
                trade_pair.clone(),
                orderbook::OrderBook::new(trade_pair.clone()),
                config.kafka_producers().clone(),
                config.kafka_consumers().clone(),
            )
            .await
            .expect("start match engine service");

        // block until exit
        exit_signal.recv().await?;
        info!("shutting down match engine for {}", node_id);
        Ok(())
    }

    fn test_config(db_path: &str) -> (AppConfig, TradePair, RaftSequencerConfig) {
        let base = "BTC";
        let quote = "USDT";
        let mut config = AppConfig::dev();
        config
            .with_kafka_producer(
                &format!("match_result_{}{}", base, quote),
                ProducerConfig {
                    trade_pair: TradePair::new(base, quote),
                    bootstrap_servers: "kafka-dev:9092".to_string(),
                    topic: format!("match_result_{}{}", base, quote),
                    acks: -1, // "all"
                    message_timeout_ms: 5000,
                },
            )
            .with_kafka_consumer(
                &format!("match_req_{}{}", base, quote),
                ConsumerConfig {
                    trade_pair: TradePair::new(base, quote),
                    bootstrap_servers: "kafka-dev:9092".to_string(),
                    topics: vec![format!("match_req_{}{}", base, quote)],
                    group_id: "oms_match_result".to_string(),
                    auto_offset_reset: "earliest".to_string(), // auto
                },
            );
        let raft_config = RaftSequencerConfig::test(db_path); // todo: from AppConfig
        (config, TradePair::new(base, quote), raft_config)
    }

    struct TestSuiteBuilder {}

    impl
        StoreBuilder<
            AppTypeConfig,
            RlrLogStore<AppTypeConfig>,
            AppStateMachineHandler<OrderBook>,
            TempDir,
        > for TestSuiteBuilder
    {
        async fn build(
            &self,
        ) -> Result<
            (
                TempDir,
                RlrLogStore<AppTypeConfig>,
                AppStateMachineHandler<OrderBook>,
            ),
            StorageError<AppTypeConfig>,
        > {
            // create a temp dir in WORKING_DIR/tmp
            let dir = Path::new("tmp");
            if !dir.exists() {
                tokio::fs::create_dir_all(dir).await.unwrap();
            }
            let td = TempDir::new_in(dir).unwrap();
            let (_, trade_pair, raft_config) = test_config(td.path().to_str().unwrap());
            let batch_size = 32;
            let (match_result_sender, _) =
                tokio::sync::mpsc::channel::<oms::BatchMatchResult>(batch_size);
            let (_, req_recv) = tokio::sync::mpsc::channel::<CmdWrapper<MatchCmd>>(256);
            let (_, _, rlr_store, state_machine) = RaftSequencerBuilder::<
                OrderBook,
                CmdWrapper<MatchCmd>,
                CmdWrapper<MatchCmdOutput>,
                AllowAllEgressController,
            >::new()
            .with_node_id(*raft_config.node_id())
            .with_db_path(td.path().to_path_buf())
            .with_nodes(raft_config.nodes().clone())
            .with_request_receiver(req_recv)
            .with_state_machine(OrderBook::new(trade_pair.clone()))
            .with_egress(AllowAllEgressController::new(match_result_sender))
            .build_components()
            .await
            .expect("build sequencer");

            Ok((td, rlr_store, state_machine))
        }
    }

    #[tokio::test]
    pub async fn test_me_store_correctness() -> Result<(), Box<dyn std::error::Error>> {
        tracing_subscriber::fmt()
            .with_file(true)
            .with_line_number(true)
            .with_env_filter("info")
            .with_thread_ids(true)
            .init();

        let builder = TestSuiteBuilder {};
        Suite::test_all(builder).await.unwrap();
        Ok(())
    }

    #[tokio::test]
    #[ignore = "todo"]
    pub async fn test_me_cluster() -> Result<(), Box<dyn std::error::Error>> {
        // Steps:
        // 1. Start 3 ME nodes forming a cluster using RaftSequencerBuilder::build()), blocking until peers all set up
        // 2. Wait for leader election
        // 3. Send orders to the leader like PlaceOrder in client.rs, like:
        //      OMS:Bid,1003,CLI_1003_00002,LIMIT,BTC_USDT,79990.00,0.5,GTK,1,false
        //      OMS:Ask,1002,CLI_1002_00003,LIMIT,BTC_USDT,80010.00,1.5,GTK,1,false
        // 4. Record snapshot  and then shutdown all nodes.
        // 5. Restart all nodes from snapshot, verify state correctness.

        // Requirements: using RaftSequencerBuilder::build() to start each ME node.

        Ok(())
    }
}
