#[cfg(test)]
#[allow(dead_code)]
mod msg_sync {
    // Setup server-side RocksDB

    // create server buffer and populate initial events 1..=10
    use crate as _;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::time::sleep;
    use tonic::codegen::tokio_stream::wrappers::TcpListenerStream;
    use tte_msg_sync::{
        MsgBufWriter, MsgBuffer, MsgBufferConfig, OutputEvent, StorageConfig,
        transport::{GrpcClient, GrpcServer, MsgBufferBackend},
    };

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct SimpleEvent {
        id: u64,
        prev_id: u64,
        value: u64,
    }

    impl OutputEvent for SimpleEvent {
        fn id(&self) -> u64 {
            self.id
        }
        fn prev_id(&self) -> u64 {
            self.prev_id
        }
    }

    fn make_config(path: &str) -> MsgBufferConfig {
        MsgBufferConfig {
            storage: StorageConfig {
                db_path: path.to_string(),
                cf_name: "integration_test".to_string(),
                use_compression: false,
                write_buffer_size: 1024 * 1024,
            },
            component: "integration_test".to_string(),
        }
    }

    async fn start_server_with_buffer(
        buffer: Arc<MsgBuffer<SimpleEvent>>,
    ) -> (
        String,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let backend = MsgBufferBackend::new(buffer);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:6100")
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let svc = GrpcServer::new(Arc::new(backend)).into_service();

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(svc)
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        (format!("http://{}", addr), shutdown_tx, handle)
    }

    fn fill_events(start: u64, end: u64) -> Vec<SimpleEvent> {
        (start..=end)
            .map(|i| SimpleEvent {
                id: i,
                prev_id: i - 1,
                value: i,
            })
            .collect()
    }

    // Expectation: Keep Sync after crash recovery.
    // Test flow:
    //  1. Output 1..=10 to server buffer
    //  2. Client fetches 1..=10
    //  3. Server crash and restart
    //  4. Output 11..=20 to server buffer
    //  5. Client fetches 11..=20
    //  6. Client crash and restart
    #[tokio::test]
    async fn test_msg_sync_with_restarts() {
        // Setup server-side RocksDB
        let tmp_dir_srv = TempDir::new().unwrap();
        let srv_path = tmp_dir_srv.path().to_str().unwrap();

        // create server buffer and populate initial events 1..=10
        let mut srv_buf = MsgBuffer::new(make_config(srv_path)).unwrap();
        let initial = fill_events(1, 10);
        srv_buf.append(initial.clone()).await.unwrap();

        // start gRPC server exposing srv_buf
        let srv_arc = Arc::new(srv_buf);
        let (uri, shutdown_tx, server_handle) = start_server_with_buffer(srv_arc.clone()).await;

        // Setup client-side RocksDB (empty)
        let tmp_dir_cli = TempDir::new().unwrap();
        let cli_path = tmp_dir_cli.path().to_str().unwrap();
        let mut cli_buf = MsgBuffer::new(make_config(cli_path)).unwrap();

        // connect client and fetch first batch [1..=10]
        let mut client = GrpcClient::connect(uri.clone()).await.unwrap();
        let res = client
            .fetch("integration_test".to_string(), 1, 20)
            .await
            .unwrap();
        assert_eq!(res.start_id, 1);
        assert_eq!(res.end_id, 10);
        assert_eq!(res.events.len(), 10);

        // decode and append to client buffer
        let decoded: Vec<SimpleEvent> = res
            .events
            .into_iter()
            .map(|b| serde_json::from_slice(&b).unwrap())
            .collect();
        cli_buf.append(decoded.clone()).await.unwrap();

        // verify client has data
        for i in 1..=10u64 {
            let got = cli_buf.get(i).await.unwrap().unwrap();
            assert_eq!(got.value, i);
        }

        // Simulate server crash: request graceful shutdown and wait for task to finish
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
        drop(srv_arc);
        sleep(std::time::Duration::from_millis(50)).await;

        // Append more events to server RocksDB using a new buffer instance (simulating server restart/apply)
        let mut srv_buf2 = MsgBuffer::new(make_config(srv_path)).unwrap();
        let more = fill_events(11, 20);
        srv_buf2.append(more.clone()).await.unwrap();

        // restart gRPC server with new backend pointing to same RocksDB
        let (uri2, shutdown2_tx, server_handle2) =
            start_server_with_buffer(Arc::new(srv_buf2)).await;
        assert_eq!(uri, uri2);

        // client resumes and fetches [11..=20]
        let mut client2 = GrpcClient::connect(uri2.clone()).await.unwrap();
        let res2 = client2
            .fetch("integration_test".to_string(), 11, 20)
            .await
            .unwrap();
        assert_eq!(res2.start_id, 11);
        assert_eq!(res2.end_id, 20);
        assert_eq!(res2.events.len(), 10);

        let decoded2: Vec<SimpleEvent> = res2
            .events
            .into_iter()
            .map(|b| serde_json::from_slice(&b).unwrap())
            .collect();

        // Simulate client crash by dropping and recreating local MsgBuffer (same path)
        drop(cli_buf);
        let mut cli_buf_recover = MsgBuffer::new(make_config(cli_path)).unwrap();
        cli_buf_recover.append(decoded2.clone()).await.unwrap();
        for i in 1..=20u64 {
            let got = cli_buf_recover.get(i).await.unwrap().unwrap();
            assert_eq!(got.value, i);
        }

        // cleanup server
        let _ = shutdown2_tx.send(());
        let _ = server_handle2.await;
    }
}
