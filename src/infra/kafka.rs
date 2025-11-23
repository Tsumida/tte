use getset::Getters;
use rdkafka::consumer::Consumer;

use crate::pbcode::oms::TradePair;

#[derive(Debug, Clone, Getters)]
pub struct ProducerConfig {
    #[getset(get = "pub")]
    pub trade_pair: TradePair,
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

impl ProducerConfig {
    pub fn create_producer(
        &self,
    ) -> Result<rdkafka::producer::FutureProducer, rdkafka::error::KafkaError> {
        let producer: rdkafka::producer::FutureProducer = rdkafka::config::ClientConfig::new()
            .set("bootstrap.servers", &self.bootstrap_servers)
            .set("acks", &self.acks.to_string())
            .set("message.timeout.ms", &self.message_timeout_ms.to_string())
            .create()?;

        Ok(producer)
    }
}

/// 消费者配置
#[derive(Debug, Clone, Getters)]
pub struct ConsumerConfig {
    #[getset(get = "pub")]
    pub trade_pair: TradePair,
    #[getset(get = "pub")]
    pub bootstrap_servers: String,
    #[getset(get = "pub")]
    pub topics: Vec<String>,
    #[getset(get = "pub")]
    pub group_id: String,
    #[getset(get = "pub")]
    pub auto_offset_reset: String,
}

impl ConsumerConfig {
    pub fn subscribe(
        &self,
    ) -> Result<rdkafka::consumer::StreamConsumer, rdkafka::error::KafkaError> {
        let consumer: rdkafka::consumer::StreamConsumer = rdkafka::config::ClientConfig::new()
            .set("bootstrap.servers", &self.bootstrap_servers)
            .set("group.id", &self.group_id)
            .set("auto.offset.reset", &self.auto_offset_reset.to_string())
            .create()
            .expect("Consumer creation failed");

        consumer
            .subscribe(
                &self
                    .topics
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<&str>>(),
            )
            .expect("Can't subscribe to specified topics");

        Ok(consumer)
    }
}
