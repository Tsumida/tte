# Message Synchronization Module (msg_sync)

A high-performance, distributed message buffer system designed for leader-to-leader synchronization in financial trading systems. Supports gap-filling, data loss detection, and provides comprehensive observability.

## Architecture

The message buffer uses a two-tier storage architecture:

1. **Hot Path (RingBuffer)**: In-memory cache for recent messages, optimized for low-latency queries
2. **Cold Path (RocksDB)**: Persistent storage for durability and historical data access

```
┌─────────────────────────────────────┐
│         MsgBuffer                   │
│  ┌─────────────┐  ┌──────────────┐ │
│  │ RingBuffer  │  │  RocksDB     │ │
│  │  (Hot)      │  │  (Cold)      │ │
│  │  10K msgs   │  │  All msgs    │ │
│  └─────────────┘  └──────────────┘ │
└─────────────────────────────────────┘
```

## Key Features

### 1. Gap-Fill Synchronization

When a leader detects discontinuous data flow (e.g., after restart), it can synchronize missing messages:

```rust
use tte_msg_sync::*;

// Detect gap: prev_trade_id (100) > current_max_id (95)
let max_id = buffer.max_id().await?;
if prev_trade_id > max_id.unwrap() {
    // Fill gap by syncing from peer
    let gap_messages = buffer.sync(max_id.unwrap() + 1, limit).await?;
    // Process gap messages...
}
```

### 2. Data Loss Detection

The system actively detects and alerts on data loss scenarios:

```rust
// Attempt to query trimmed data
match buffer.sync(old_id, limit).await {
    Err(MsgBufError::DataTrimmed { requested_id, min_available_id }) => {
        // ALERT: Data has been permanently lost!
        alert_data_loss(requested_id, min_available_id);
    }
    Err(MsgBufError::DataNotYetAvailable { requested_id, current_max_id }) => {
        // Temporary lag - data may arrive later
        retry_with_backoff();
    }
    Ok(messages) => {
        // Success
    }
}
```

### 3. Sequence Validation

Messages are validated to ensure monotonic ID ordering:

```rust
// Each message must have id > prev_id
impl OutputEvent for MatchReq {
    fn id(&self) -> u64 {
        self.trade_id
    }

    fn prev_id(&self) -> u64 {
        self.prev_trade_id
    }
}
```

### 4. Comprehensive Metrics

OpenTelemetry metrics for monitoring:

- **Sync Lag**: `msg_buffer.sync.lag` - Measures replication lag
- **Query Latency**: `msg_buffer.query.duration` - Separate for hot/cold storage
- **Buffer Size**: `msg_buffer.size`, `msg_buffer.hot_size`
- **Trim Operations**: `msg_buffer.trim.total`, `msg_buffer.trim.duration`
- **Data Loss**: `msg_buffer.data_loss.detected` - Critical alert metric

## Usage Examples

### Basic Setup for Match Engine

```rust
use tte_msg_sync::*;
use tte_core::pbcode::oms;

// Define your event type
impl OutputEvent for oms::TradeCmd {
    fn id(&self) -> u64 {
        self.trade_id
    }

    fn prev_id(&self) -> u64 {
        self.prev_trade_id
    }
}

// Create buffer configuration
let config = MsgBufferConfig {
    ring_buffer: RingBufferConfig {
        capacity: 10_000, // Keep 10k recent messages in memory
        overwrite_on_full: true,
    },
    storage: StorageConfig {
        db_path: "/data/match_engine/msg_buffer".to_string(),
        cf_name: "match_req_buffer".to_string(),
        use_compression: true,
        write_buffer_size: 64 * 1024 * 1024, // 64MB
    },
    component: "match_engine".to_string(),
};

// Initialize with metrics
let meter = opentelemetry::global::meter("match_engine");
let metrics = Arc::new(MsgBufMetrics::new(&meter, "match_engine"));
let mut buffer = MsgBuffer::with_metrics(config, metrics)?;

// Append batch from Raft output
let batch: Vec<oms::TradeCmd> = /* ... */;
buffer.append(batch).await?;
```

### OMS Setup with Match Result Processing

```rust
use tte_msg_sync::*;

impl OutputEvent for oms::MatchResult {
    fn id(&self) -> u64 {
        self.match_id
    }

    fn prev_id(&self) -> u64 {
        self.prev_match_id
    }
}

let mut oms_buffer = MsgBuffer::new(MsgBufferConfig {
    component: "oms".to_string(),
    storage: StorageConfig {
        db_path: "/data/oms/msg_buffer".to_string(),
        cf_name: "match_result_buffer".to_string(),
        ..Default::default()
    },
    ..Default::default()
})?;

// Process match results
for result in match_results {
    // Update OMS state...
    
    // Trim buffer based on trade_id
    if result.trade_id % 1000 == 0 {
        // Keep last 10k results
        let trim_before = result.trade_id.saturating_sub(10_000);
        oms_buffer.trim(trim_before).await?;
    }
}
```

### Leader Restart and Gap-Fill

```rust
// After leader restart
async fn handle_leader_startup(
    buffer: &MsgBuffer<oms::TradeCmd>,
    kafka_consumer: &KafkaConsumer,
) -> Result<()> {
    // Get latest message from Kafka
    let latest_msg = kafka_consumer.poll().await?;
    let kafka_prev_id = latest_msg.prev_trade_id;
    
    // Get current max ID from buffer
    let current_max = buffer.max_id().await?.unwrap_or(0);
    
    // Detect gap
    if kafka_prev_id > current_max {
        info!(
            kafka_prev_id = kafka_prev_id,
            current_max = current_max,
            gap_size = kafka_prev_id - current_max,
            "Gap detected, triggering sync"
        );
        
        // Sync gap from peer leader
        let gap_messages = sync_from_peer(current_max + 1, kafka_prev_id).await?;
        buffer.append(gap_messages).await?;
        
        info!("Gap filled successfully");
    }
    
    Ok(())
}

// Sync from peer using gRPC
async fn sync_from_peer(start_id: u64, end_id: u64) -> Result<Vec<oms::TradeCmd>> {
    let mut client = MsgSyncClient::connect("http://peer:8080").await?;
    
    let request = SyncRequest {
        start_id,
        limit: (end_id - start_id + 1) as usize,
    };
    
    let response = client.sync(request).await?;
    Ok(response.into_inner().messages)
}
```

### Sequential Reader Pattern

```rust
// Create a reader for sequential consumption
let reader = MsgBufReaderImpl::new(
    Arc::new(buffer),
    start_position,
);

// Process messages sequentially
loop {
    match reader.next().await {
        Ok(Some(msg)) => {
            process_message(msg).await?;
        }
        Ok(None) => {
            // End of stream
            break;
        }
        Err(e) if e.is_temporary() => {
            // Wait and retry
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }
        Err(e) => {
            error!(error = ?e, "Fatal read error");
            return Err(e);
        }
    }
}
```

## Consistency Model

### Raft-Driven Trim

Trim operations MUST be driven by Raft logs to ensure cluster consistency:

```rust
// In Raft apply callback
async fn apply_raft_command(cmd: RaftCommand) {
    match cmd {
        RaftCommand::TrimBuffer { before_id } => {
            // All replicas trim at the same point
            buffer.trim(before_id).await?;
        }
        RaftCommand::TradeCmd(trade_cmd) => {
            // Append to buffer after Raft commit
            buffer.append(vec![trade_cmd]).await?;
        }
    }
}
```

### ME and OMS Coordination

- **Match Engine**: Uses `trade_id` as buffer ID, receives MatchReq from OMS
- **OMS**: Uses `match_id` as buffer ID, receives MatchResult from ME
- **Trim Coordination**: When OMS processes MatchResult with `trade_id` X, it can safely trim ME's buffer where `id < X`

```rust
// In OMS apply thread
async fn handle_match_result(
    oms: &mut OMS,
    result: oms::MatchResult,
    me_buffer: &MsgBuffer<oms::TradeCmd>,
) -> Result<()> {
    // Process match result
    oms.handle_fill(result.trade_id, &result.records)?;
    
    // Trim ME buffer - safe because OMS has processed up to trade_id
    let trim_point = result.trade_id.saturating_sub(BUFFER_RETAIN_SIZE);
    me_buffer.trim(trim_point).await?;
    
    Ok(())
}
```

## Monitoring and Alerting

### Critical Alerts

1. **Data Loss Detection**
   - Metric: `msg_buffer.data_loss.detected`
   - Trigger: Any non-zero value
   - Action: Immediate investigation, check trim policies

2. **High Sync Lag**
   - Metric: `msg_buffer.sync.lag`
   - Trigger: `lag > 1000` IDs
   - Action: Check network, check peer health

3. **Storage Errors**
   - Metric: `msg_buffer.append.errors`, `msg_buffer.query.errors`
   - Trigger: Error rate > 0.1%
   - Action: Check disk space, RocksDB health

### Performance Monitoring

```rust
// Query latency by storage tier
// msg_buffer.query.duration{storage="hot"}  < 1ms (expected)
// msg_buffer.query.duration{storage="cold"} < 10ms (expected)

// Buffer hit rate
// msg_buffer.hot_size / msg_buffer.size > 0.9 (target)
```

## Configuration Best Practices

### Ring Buffer Capacity

```rust
// Rule of thumb: Keep ~1 minute of messages in hot buffer
// If processing 1000 msg/sec, set capacity to 60_000
RingBufferConfig {
    capacity: throughput_per_sec * 60,
    overwrite_on_full: true,  // Always true for production
}
```

### Trim Policy

```rust
// Conservative: Keep 10x ring buffer capacity
let retain_size = ring_buffer_capacity * 10;

// Aggressive (if disk constrained): Keep 2x ring buffer
let retain_size = ring_buffer_capacity * 2;

// Trim periodically (every N messages or time-based)
if msg_count % 10_000 == 0 {
    let trim_point = current_id.saturating_sub(retain_size);
    buffer.trim(trim_point).await?;
}
```

## Testing

Run unit tests:
```bash
cd crates/msg_sync
cargo test
```

Run integration tests:
```bash
cargo test --test integration_test
```

## Performance Characteristics

- **Hot Path Query**: < 1ms (p99)
- **Cold Path Query**: < 10ms (p99)
- **Append Latency**: < 5ms (p99) - includes RocksDB persistence
- **Trim Operation**: < 100ms for 10K messages
- **Memory Usage**: ~100 bytes per message in hot buffer
- **Disk Usage**: ~150 bytes per message in RocksDB (with compression)

## Troubleshooting

### Gap Not Filling

- Check peer connectivity
- Verify peer has the required data (not trimmed)
- Check for ID sequence corruption

### High Query Latency

- Check hot buffer hit rate
- Increase ring buffer capacity if hit rate < 80%
- Check RocksDB compaction status

### Data Loss Alerts

- Review trim policy configuration
- Check if trim operations are Raft-driven
- Verify retention period matches business requirements
