# Design of Message Buffer

- MsgBuffer serves as a cache for the output results of the business state machine, designed to support point-to-point message synchronization (GapFill).
- Architecture: MsgBuffer is composed of a RingBuffer (memory) and RocksDB (persistent storage). The Raft output contains a unique, monotonic business ID (e.g., TradeID for OMS or MatchID for the Matching Engine).
- Write Path: Every Raft output is batch-written to both RocksDB and the RingBuffer. The RingBuffer is used to store "hot" (recent) data.
- Query Interface: Must support range queries based on ID (StartID + Limit, ordered by ID ascending). If the StartID exists in the RingBuffer, it should be read directly from memory; otherwise, it should be fetched from RocksDB.
- Data Retention: Supports deleting expired data (deleting all records where ID < X). This deletion must be driven by Raft logs to ensure consistency across the cluster.Usage Scenario: Upon becoming a new Leader, the node pulls messages from the primary channel (Kafka). If the "Previous ID" (prev_match_id / prev_trade_id) of the latest message is greater than the maximum ID of the state machine's output, it indicates a message gap. The system must then trigger a GapFill via point-to-point synchronization.
- Communication Layer: Synchronization should be as close to real-time as possible.

# API
```rust
pub trait OutputEvent{
    // Consecutive IDs so that event.prev_id is id - 1
    fn id() -> u64;
}

pub trait MsgBufReader<T: OutputEvent>{
    // Get next msg in order by id asc, it may block until sync() is done. 
    async fn next(&mut self) -> Result<T, anyhow::Error>;
}

pub trait MsgBufSync<T: OutputEvent>{
    // Synchronize all output from [start_id, min(start_id + limit, latest_match_id) )
    // The result is sorted by id in ascending order. 
    async fn sync(&mut self, start_id: u64, limit: usize) -> Result<Vec<T>, anyhow::Error>; 
}

pub trait MsgBufWriter<T: OutputEvent>{
    // Append a batch of OutputEvent, events are sorted by id ascending order.
    // Append make sure that batch are persisted. 
    async fn append(&mut self, batch: Vec<T>) -> Result<(), anyhow::Error>;
    // Delete all msg with id < before_id
    async fn trim(&mut self, before_id: u64) -> Result<(), anyhow::Error>; 
}
```  
You can change the API if needed. 

# Consistency

- Both OMS and ME are using MsgBuffer for data sync. 
- ME takes MatchReq as raft's input and req.trade_id as id, 
- OMS takes MatchResult as input and req.match_id as id, for releasing frozen asset and update order states. 
- While OMS get matchResult with match_id and corresponding trade_id x, it's time to trim all msg in buffer with id < x. 
- Trim() may be trigger by inner action or external mannual action.