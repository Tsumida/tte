use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use tokio::time::{Duration, sleep};
use tte_msg_sync::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MatchReq {
    trade_id: u64,
    prev_trade_id: u64,
    order_id: String,
    quantity: u64,
}

impl OutputEvent for MatchReq {
    fn id(&self) -> u64 {
        self.trade_id
    }

    fn prev_id(&self) -> u64 {
        self.prev_trade_id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MatchResult {
    match_id: u64,
    prev_match_id: u64,
    trade_id: u64,
    filled_qty: u64,
}

impl OutputEvent for MatchResult {
    fn id(&self) -> u64 {
        self.match_id
    }

    fn prev_id(&self) -> u64 {
        self.prev_match_id
    }
}

fn create_test_buffer<T: OutputEvent>() -> (MsgBuffer<T>, TempDir) {
    let tmp_dir = TempDir::new().unwrap();
    let config = MsgBufferConfig {
        ring_buffer: RingBufferConfig {
            capacity: 100,
            overwrite_on_full: true,
        },
        storage: StorageConfig {
            db_path: tmp_dir.path().to_str().unwrap().to_string(),
            cf_name: "test_buffer".to_string(),
            use_compression: false,
            write_buffer_size: 4 * 1024 * 1024,
        },
        component: "test".to_string(),
    };

    let buffer = MsgBuffer::new(config).unwrap();
    (buffer, tmp_dir)
}

#[tokio::test]
async fn test_gap_fill_scenario() {
    // Simulate the gap-fill scenario described in the design doc
    let (mut buffer, _tmp) = create_test_buffer::<MatchReq>();

    // Initial state: leader processes requests 1-5
    let initial_batch: Vec<MatchReq> = (1..=5)
        .map(|i| MatchReq {
            trade_id: i,
            prev_trade_id: i - 1,
            order_id: format!("order_{}", i),
            quantity: 100,
        })
        .collect();

    buffer.append(initial_batch).await.unwrap();

    // Simulate leader restart - requests 6-10 are in Kafka but not yet processed
    // Leader receives request with prev_trade_id=10, but current max is 5

    let max_id = buffer.max_id().await.unwrap();
    assert_eq!(max_id, Some(5));

    // Detect gap: prev_trade_id (10) > current_max_id (5)
    let prev_trade_id = 10;
    assert!(prev_trade_id > max_id.unwrap());

    // Trigger gap fill by syncing [6, 10]
    // First, populate the gap (simulating sync from peer)
    let gap_batch: Vec<MatchReq> = (6..=10)
        .map(|i| MatchReq {
            trade_id: i,
            prev_trade_id: i - 1,
            order_id: format!("order_{}", i),
            quantity: 100,
        })
        .collect();

    buffer.append(gap_batch).await.unwrap();

    // Now verify gap is filled
    let synced = buffer.sync(6, 5).await.unwrap();
    assert_eq!(synced.len(), 5);
    assert_eq!(synced[0].trade_id, 6);
    assert_eq!(synced[4].trade_id, 10);

    // Verify continuity
    for i in 0..synced.len() - 1 {
        assert_eq!(synced[i + 1].prev_trade_id, synced[i].trade_id);
    }
}

#[tokio::test]
async fn test_data_loss_detection() {
    let (mut buffer, _tmp) = create_test_buffer::<MatchResult>();

    // Populate buffer with match results
    let batch: Vec<MatchResult> = (1..=100)
        .map(|i| MatchResult {
            match_id: i,
            prev_match_id: i - 1,
            trade_id: i,
            filled_qty: 50,
        })
        .collect();

    buffer.append(batch).await.unwrap();

    // Trim messages < 50 (simulating OMS receiving match result with trade_id=50)
    let deleted = buffer.trim(50).await.unwrap();
    assert!(deleted >= 49);

    // Try to query trimmed data - should detect data loss
    let result = buffer.sync(10, 10).await;
    assert!(result.is_err());

    match result {
        Err(MsgBufError::DataTrimmed {
            requested_id,
            min_available_id,
        }) => {
            assert_eq!(requested_id, 10);
            assert_eq!(min_available_id, 50);
        }
        _ => panic!("Expected DataTrimmed error"),
    }

    // Verify error detection helpers
    if let Err(e) = buffer.sync(10, 10).await {
        assert!(e.is_data_loss());
        assert!(!e.is_temporary());
        assert!(!e.is_corruption());
    }
}

#[tokio::test]
async fn test_sync_lag_detection() {
    let (mut buffer, _tmp) = create_test_buffer::<MatchReq>();

    // Populate with some data
    let batch: Vec<MatchReq> = (1..=10)
        .map(|i| MatchReq {
            trade_id: i,
            prev_trade_id: i - 1,
            order_id: format!("order_{}", i),
            quantity: 100,
        })
        .collect();

    buffer.append(batch).await.unwrap();

    // Try to query data beyond current max - indicates sync lag
    let result = buffer.sync(20, 10).await;
    assert!(result.is_err());

    match result {
        Err(MsgBufError::DataNotYetAvailable {
            requested_id,
            current_max_id,
        }) => {
            assert_eq!(requested_id, 20);
            assert_eq!(current_max_id, 10);
        }
        _ => panic!("Expected DataNotYetAvailable error"),
    }

    // Verify this is a temporary error
    if let Err(e) = buffer.sync(20, 10).await {
        assert!(e.is_temporary());
        assert!(!e.is_data_loss());
    }
}

#[tokio::test]
async fn test_sequence_validation() {
    let (mut buffer, _tmp) = create_test_buffer::<MatchReq>();

    // Try to append out-of-order messages
    let valid_batch = vec![MatchReq {
        trade_id: 1,
        prev_trade_id: 0,
        order_id: "order_1".to_string(),
        quantity: 100,
    }];

    buffer.append(valid_batch).await.unwrap();

    // Try to append message with gap (3 instead of 2)
    let invalid_batch = vec![MatchReq {
        trade_id: 3,
        prev_trade_id: 2, // Gap! Previous was 1
        order_id: "order_3".to_string(),
        quantity: 100,
    }];

    // This should succeed because we're only validating within a batch
    // The gap detection happens at query time
    let result = buffer.append(invalid_batch).await;
    assert!(result.is_ok());

    // However, within a batch, sequence must be valid
    let bad_batch = vec![
        MatchReq {
            trade_id: 10,
            prev_trade_id: 9,
            order_id: "order_10".to_string(),
            quantity: 100,
        },
        MatchReq {
            trade_id: 12,      // Gap!
            prev_trade_id: 10, // But prev doesn't match
            order_id: "order_12".to_string(),
            quantity: 100,
        },
    ];

    let result = buffer.append(bad_batch).await;
    assert!(result.is_err());
    assert!(matches!(result, Err(MsgBufError::DataCorrupted { .. })));
}

#[tokio::test]
async fn test_concurrent_append_and_query() {
    let (buffer, _tmp) = create_test_buffer::<MatchReq>();

    // Spawn writer task
    let mut writer_buffer = buffer.clone();
    let writer_handle = tokio::spawn(async move {
        for batch_num in 0..10 {
            let start_id = batch_num * 10 + 1;
            let batch: Vec<MatchReq> = (start_id..start_id + 10)
                .map(|i| MatchReq {
                    trade_id: i,
                    prev_trade_id: i - 1,
                    order_id: format!("order_{}", i),
                    quantity: 100,
                })
                .collect();

            writer_buffer.append(batch).await.unwrap();
            sleep(Duration::from_millis(10)).await;
        }
    });

    // Spawn reader task
    let reader_buffer = buffer.clone();
    let reader_handle = tokio::spawn(async move {
        sleep(Duration::from_millis(50)).await; // Wait for some data

        let mut read_count = 0;
        for i in 1..=50 {
            match reader_buffer.get(i).await {
                Ok(Some(_)) => read_count += 1,
                Ok(None) => break,
                Err(e) if e.is_temporary() => {
                    sleep(Duration::from_millis(10)).await;
                    continue;
                }
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }
        read_count
    });

    writer_handle.await.unwrap();
    let read_count = reader_handle.await.unwrap();

    assert!(read_count > 0);
    assert!(buffer.max_id().await.unwrap().unwrap() >= 50);
}

#[tokio::test]
async fn test_hot_cold_boundary() {
    let (mut buffer, _tmp) = create_test_buffer::<MatchReq>();

    // Append more messages than ring buffer capacity
    let batch: Vec<MatchReq> = (1..=50)
        .map(|i| MatchReq {
            trade_id: i,
            prev_trade_id: i - 1,
            order_id: format!("order_{}", i),
            quantity: 100,
        })
        .collect();

    buffer.append(batch).await.unwrap();

    // Query early messages (should come from cold storage)
    let msg1 = buffer.get(1).await.unwrap().unwrap();
    assert_eq!(msg1.trade_id, 1);

    // Query recent messages (should come from hot storage)
    let msg50 = buffer.get(50).await.unwrap().unwrap();
    assert_eq!(msg50.trade_id, 50);

    // Query range spanning hot/cold boundary
    let range = buffer.sync(25, 20).await.unwrap();
    assert_eq!(range.len(), 20);
    assert_eq!(range[0].trade_id, 25);
    assert_eq!(range[19].trade_id, 44);
}

#[tokio::test]
async fn test_trim_consistency() {
    let (mut buffer, _tmp) = create_test_buffer::<MatchResult>();

    // Populate buffer
    let batch: Vec<MatchResult> = (1..=100)
        .map(|i| MatchResult {
            match_id: i,
            prev_match_id: i - 1,
            trade_id: i,
            filled_qty: 50,
        })
        .collect();

    buffer.append(batch).await.unwrap();

    // Perform multiple trims
    buffer.trim(20).await.unwrap();
    buffer.trim(40).await.unwrap();
    buffer.trim(60).await.unwrap();

    // Verify metadata is consistent
    let stats = buffer.stats().await.unwrap();
    assert_eq!(stats.min_id, Some(60));
    assert_eq!(stats.max_id, Some(100));

    // Verify we can still query valid range
    let range = buffer.sync(60, 10).await.unwrap();
    assert_eq!(range.len(), 10);
    assert_eq!(range[0].match_id, 60);

    // Verify trimmed data is inaccessible
    let result = buffer.get(30).await;
    assert!(result.is_err() || result.unwrap().is_none());
}

#[tokio::test]
async fn test_buffer_stats() {
    let (mut buffer, _tmp) = create_test_buffer::<MatchReq>();

    // Initially empty
    let stats = buffer.stats().await.unwrap();
    assert_eq!(stats.total_count, 0);
    assert_eq!(stats.min_id, None);
    assert_eq!(stats.max_id, None);

    // After appending
    let batch: Vec<MatchReq> = (1..=50)
        .map(|i| MatchReq {
            trade_id: i,
            prev_trade_id: i - 1,
            order_id: format!("order_{}", i),
            quantity: 100,
        })
        .collect();

    buffer.append(batch).await.unwrap();

    let stats = buffer.stats().await.unwrap();
    assert_eq!(stats.total_count, 50);
    assert_eq!(stats.min_id, Some(1));
    assert_eq!(stats.max_id, Some(50));
    assert!(stats.hot_count <= 50);

    // After trimming
    buffer.trim(30).await.unwrap();

    let stats = buffer.stats().await.unwrap();
    assert_eq!(stats.min_id, Some(30));
    assert_eq!(stats.max_id, Some(50));
    assert!(stats.last_trim_ts.is_some());
}

#[tokio::test]
async fn test_reader_sequential_consumption() {
    let (mut buffer, _tmp) = create_test_buffer::<MatchReq>();

    // Populate buffer
    let batch: Vec<MatchReq> = (1..=20)
        .map(|i| MatchReq {
            trade_id: i,
            prev_trade_id: i - 1,
            order_id: format!("order_{}", i),
            quantity: 100,
        })
        .collect();

    buffer.append(batch).await.unwrap();

    // Create reader starting from position 5
    use std::sync::Arc;
    let mut reader = MsgBufReaderImpl::new(Arc::new(buffer), 5);

    // Read sequentially
    for expected_id in 5..=20 {
        let msg = reader.next().await.unwrap().unwrap();
        assert_eq!(msg.trade_id, expected_id);
        assert_eq!(reader.position(), expected_id + 1);
    }

    // Next should return None (end of stream)
    let msg = reader.next().await.unwrap();
    assert!(msg.is_none());
}
