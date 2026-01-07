#[cfg(test)]
mod test {
    use std::net::SocketAddr;

    use tte_sequencer::raft::RaftSequencerConfig;

    #[test]
    fn test_config_from_env() {
        temp_env::with_vars(
            vec![
                ("RAFT_NODE_ID", Some("1")),
                (
                    "RAFT_NODES",
                    Some("1@127.0.0.1:6001,2@127.0.0.1:6002,3@127.0.0.1:6003"),
                ),
                ("RAFT_DB_PATH", Some("/data/me_BTCUSDT_1/db")),
                ("RAFT_SNAPSHOT_PATH", Some("/data/me_BTCUSDT_1/snapshots")),
            ],
            || {
                let node_id_str = std::env::var("RAFT_NODE_ID").unwrap();
                let nodes_str_raw = std::env::var("RAFT_NODES").unwrap();
                let db_path_str = std::env::var("RAFT_DB_PATH").unwrap();
                let snapshot_path_str = std::env::var("RAFT_SNAPSHOT_PATH").unwrap();
                let nodes_str: Vec<(String, SocketAddr)> = nodes_str_raw
                    .split(",")
                    .map(|s| {
                        let parts: Vec<&str> = s.splitn(2, '@').collect();
                        assert!(parts.len() == 2, "Invalid RAFT_NODES format");
                        let id = parts[0].to_string();
                        let addr = parts[1]
                            .parse::<SocketAddr>()
                            .expect("Invalid socket address");
                        (id, addr)
                    })
                    .collect();

                let config = RaftSequencerConfig::from_env(
                    node_id_str,
                    nodes_str,
                    db_path_str,
                    snapshot_path_str,
                )
                .unwrap();
                assert_eq!(*config.node_id(), 1);
                assert_eq!(config.nodes().len(), 3);
                assert_eq!(config.db_path(), "/data/me_BTCUSDT_1/db");
                assert_eq!(config.snapshot_path(), "/data/me_BTCUSDT_1/snapshots");
            },
        );
    }
}
