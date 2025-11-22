use std::collections::HashMap;

use crate::infra::kafka::{ConsumerConfig, ProducerConfig};
use getset::Getters;
use opentelemetry::{
    global,
    trace::{Tracer, TracerProvider},
};
use opentelemetry_otlp::WithExportConfig;
use tracing::{Level, info};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{Layer, Registry, filter, layer::SubscriberExt};

#[derive(Debug, Clone)]
pub enum Env {
    Dev = 1,
    SandBox = 2,
    Prod = 3,
}

#[derive(Debug, Clone, Getters)]
pub struct AppConfig {
    #[getset(get = "pub")]
    app_name: String,
    #[getset(get = "pub")]
    trace_endpoint: String,
    #[getset(get = "pub")]
    grpc_server_endpoint: String,
    #[getset(get = "pub")]
    env: Env,
    #[getset(get = "pub")]
    kafka_producers: HashMap<String, ProducerConfig>,
    #[getset(get = "pub")]
    kafka_consumers: HashMap<String, ConsumerConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            app_name: "trade_engine".to_string(),
            trace_endpoint: "http://localhost:4318".to_string(),
            grpc_server_endpoint: "[::1]:8080".to_string(),
            env: Env::Dev,
            kafka_producers: HashMap::new(),
            kafka_consumers: HashMap::new(),
        }
    }
}

impl AppConfig {
    pub fn dev() -> Self {
        let mut config = Self::default();
        if let Ok(app_name) = std::env::var("APP_NAME") {
            config.app_name = app_name;
        }
        if let Ok(endpoint) = std::env::var("TRACE_ENDPOINT") {
            config.trace_endpoint = endpoint;
        }
        if let Ok(endpoint) = std::env::var("GRPC_SERVER_ENDPOINT") {
            config.grpc_server_endpoint = endpoint;
        }
        if let Ok(env_str) = std::env::var("ENV") {
            config.env = match env_str.to_lowercase().as_str() {
                "dev" => Env::Dev,
                "sandbox" => Env::SandBox,
                "prod" => Env::Prod,
                _ => Env::Dev,
            }
        }

        config
    }

    pub fn with_kafka_producer(&mut self, name: &str, cfg: ProducerConfig) -> &mut Self {
        self.kafka_producers.insert(name.to_string(), cfg);
        self
    }

    pub fn with_kafka_consumer(&mut self, name: &str, cfg: ConsumerConfig) -> &mut Self {
        self.kafka_consumers.insert(name.to_string(), cfg);
        self
    }

    pub fn print_args(&self) -> &Self {
        info!("AppConfig:trace_endpoint={}", self.trace_endpoint);
        info!(
            "AppConfig:grpc_server_endpoint={}",
            self.grpc_server_endpoint
        );
        info!("AppConfig:env={:?}", self.env);
        self
    }

    pub async fn init_tracer(&self) -> Result<(), Box<dyn std::error::Error>> {
        use opentelemetry::KeyValue;
        use opentelemetry_otlp::SpanExporter;
        use opentelemetry_sdk::Resource;
        use opentelemetry_sdk::trace::TracerProviderBuilder;

        let exporter = SpanExporter::builder()
            .with_tonic() // 4318
            .with_endpoint(self.trace_endpoint.as_str())
            .with_timeout(std::time::Duration::from_secs(2))
            .build()?;

        let resource = Resource::builder()
            .with_attributes(vec![KeyValue::new("service.name", self.app_name.clone())])
            .build();

        let provider = TracerProviderBuilder::default()
            .with_batch_exporter(exporter)
            .with_resource(resource)
            .build();

        let tracer = provider.tracer(self.app_name().clone());
        let subscriber = Registry::default()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_file(true)
                    .with_line_number(true)
                    .with_thread_ids(true)
                    .with_filter(tracing_subscriber::filter::LevelFilter::from_level(
                        Level::INFO,
                    )),
            ) // stdout logs
            .with(
                OpenTelemetryLayer::new(tracer).with_filter(filter::filter_fn(|metadata| {
                    metadata.level() <= &Level::INFO
                        && metadata.target().starts_with("trade_engine")
                })),
            ); // export spans to jaeger

        tracing::subscriber::set_global_default(subscriber).expect("Failed to set subscriber");

        // send a span to the exporter to verify connection
        global::tracer(self.app_name().clone()).in_span("init_tracer", |_span| {
            info!("try_send_span to {}", self.trace_endpoint);
        });

        info!("tracing initialized with endpoint {}", self.trace_endpoint);
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("App down");
        // todo: shutdown opentelemetry
        Ok(())
    }
}
