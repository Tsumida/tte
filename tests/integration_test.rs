use futures::stream::TryStreamExt;
use rdkafka::{Message, consumer::Consumer};
use tokio::time::sleep;
use trade_engine::infra::kafka::{ConsumerConfig, ProducerConfig};

fn dev_config() -> (ProducerConfig, ConsumerConfig) {
    let prod_cfg = ProducerConfig {
        name: "test".to_string(),
        bootstrap_servers: "localhost:9092".to_string(),
        acks: -1, // all
        topic: "test".to_string(),
        message_timeout_ms: 1000,
    };

    let consumer_cfg = ConsumerConfig {
        name: "test_consumer".to_string(),
        bootstrap_servers: "localhost:9092".to_string(),
        topics: vec!["test".to_string()],
        group_id: "test_group".to_string(),
        auto_offset_reset: "earliest".to_string(),
    };

    (prod_cfg, consumer_cfg)
}

#[tokio::test]
async fn test_kafka_prod_consume() -> Result<(), Box<dyn std::error::Error>> {
    let (prod_cfg, consumer_cfg) = dev_config();

    let prod = rdkafka::config::ClientConfig::new()
        .set("bootstrap.servers", &prod_cfg.bootstrap_servers)
        .set("acks", &prod_cfg.acks.to_string())
        .create::<rdkafka::producer::FutureProducer>()
        .expect("Producer creation failed");

    let consumer = rdkafka::config::ClientConfig::new()
        .set("bootstrap.servers", &consumer_cfg.bootstrap_servers)
        .set("group.id", &consumer_cfg.group_id)
        .set("auto.offset.reset", &consumer_cfg.auto_offset_reset)
        .create::<rdkafka::consumer::StreamConsumer>()
        .expect("Consumer creation failed");
    consumer
        .subscribe(
            consumer_cfg
                .topics()
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<&str>>()
                .as_slice(),
        )
        .expect("can't subscribe");

    // consumer: consumer all data until receive exit signal.
    let _ = tokio::spawn(async move {
        let r = consumer
            .stream()
            .try_for_each(|message| {
                if let Some(payload) = message.payload() {
                    let s = std::str::from_utf8(payload).unwrap();
                    println!("msg at offset {}: {}", message.offset(), s);
                }
                futures::future::ready(Ok(()))
            })
            .await;
        if let Err(e) = r {
            println!("Error while receiving from Kafka: {:?}", e);
        }
    });

    // prod: producer 1->20 msg with payload="helloworld" and send exit signal.
    for i in 1..=20 {
        let key = format!("key_{}", i);
        let payload = format!("helloworld_{}", i);
        let record = rdkafka::producer::FutureRecord::to(&prod_cfg.topic)
            .payload(&payload)
            .key(&key);
        let d = prod
            .send(record, std::time::Duration::from_secs(0))
            .await
            .expect("no err expected");
        println!("Sent: {:?}", d);
    }

    sleep(std::time::Duration::from_secs(3)).await;
    Ok(())
}
