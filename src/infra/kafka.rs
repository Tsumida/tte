use getset::Getters;

#[derive(Debug, Clone, Getters)]
pub struct ProducerConfig {
    #[getset(get = "pub")]
    pub name: String,
    #[getset(get = "pub")]
    pub bootstrap_servers: String,
    #[getset(get = "pub")]
    pub topic: String,
    /// ack 级别: "0", "1", "all" (对应 -1)
    #[getset(get = "pub")]
    pub acks: i8,
    #[getset(get = "pub")]
    pub message_timeout_ms: u64,
}

impl Default for ProducerConfig {
    fn default() -> Self {
        Self {
            name: "default_producer".to_string(),
            bootstrap_servers: "localhost:9092".to_string(),
            topic: "default_topic".to_string(),
            acks: -1, // "all"
            message_timeout_ms: 5000,
        }
    }
}

/// 消费者配置
#[derive(Debug, Clone, Getters)]
pub struct ConsumerConfig {
    #[getset(get = "pub")]
    pub name: String,
    #[getset(get = "pub")]
    pub bootstrap_servers: String,
    #[getset(get = "pub")]
    pub topics: Vec<String>,
    #[getset(get = "pub")]
    pub group_id: String,
    #[getset(get = "pub")]
    pub auto_offset_reset: rdkafka::Offset,
}
