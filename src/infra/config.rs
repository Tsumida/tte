use getset::Getters;
use opentelemetry::{
    global,
    trace::{Span, Tracer},
};
use opentelemetry_otlp::WithExportConfig;
use tracing::info;

#[derive(Debug, Clone)]
pub enum Env {
    Dev = 1,
    SandBox = 2,
    Prod = 3,
}

#[derive(Debug, Clone, Getters)]
pub struct AppConfig {
    #[getset(get = "pub")]
    trace_endpoint: String,
    #[getset(get = "pub")]
    grpc_server_endpoint: String,
    #[getset(get = "pub")]
    env: Env,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            trace_endpoint: "http://localhost:4318".to_string(),
            grpc_server_endpoint: "[::1]:8080".to_string(),
            env: Env::Dev,
        }
    }
}

impl AppConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();
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

    pub fn init_logs(&self) -> &Self {
        tracing_subscriber::fmt()
            .with_file(true)
            .with_line_number(true)
            .with_thread_ids(true)
            .init();
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
            .with_timeout(std::time::Duration::from_secs(3))
            .build()?;

        let resource = Resource::builder()
            .with_attributes(vec![KeyValue::new("service.name", "trade_engine")])
            .build();

        let provider = TracerProviderBuilder::default()
            .with_batch_exporter(exporter)
            .with_resource(resource)
            .build();
        global::set_tracer_provider(provider);

        // send a span to the exporter to verify connection
        global::tracer("init_tracer").in_span("init_tracer", |_span| {
            info!("try_send_span to {}", self.trace_endpoint);
        });

        info!("tracing initialized with endpoint {}", self.trace_endpoint);
        Ok(())
    }
}
