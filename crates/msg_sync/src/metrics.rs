use prometheus::{Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge, Opts, Registry};
use tracing::warn;

/// Metrics for message buffer operations
#[derive(Clone)]
pub struct MsgBufMetrics {
    // Counters
    pub append_total: IntCounter,
    pub append_errors: IntCounter,
    pub trim_total: IntCounter,
    pub trim_errors: IntCounter,
    pub query_total: IntCounterVec,
    pub query_errors: IntCounter,
    pub data_loss_detected: IntCounter,

    // Histograms
    pub append_duration: Histogram,
    pub query_duration: Histogram,
    pub trim_duration: Histogram,
    pub sync_lag: Histogram,

    // Gauges
    pub buffer_size: IntGauge,
    pub hot_buffer_size: IntGauge,
    pub min_id: IntGauge,
    pub max_id: IntGauge,
}

impl MsgBufMetrics {
    pub fn new(registry: &Registry, component: &str) -> Result<Self, prometheus::Error> {
        let append_total = IntCounter::with_opts(
            Opts::new(
                "msg_buffer_append_total",
                "Total number of append operations",
            )
            .const_label("component", component),
        )?;

        let append_errors = IntCounter::with_opts(
            Opts::new(
                "msg_buffer_append_errors_total",
                "Total number of append errors",
            )
            .const_label("component", component),
        )?;

        let trim_total = IntCounter::with_opts(
            Opts::new("msg_buffer_trim_total", "Total number of trim operations")
                .const_label("component", component),
        )?;

        let trim_errors = IntCounter::with_opts(
            Opts::new(
                "msg_buffer_trim_errors_total",
                "Total number of trim errors",
            )
            .const_label("component", component),
        )?;

        let query_total = IntCounterVec::new(
            Opts::new("msg_buffer_query_total", "Total number of query operations")
                .const_label("component", component),
            &["storage"],
        )?;

        let query_errors = IntCounter::with_opts(
            Opts::new(
                "msg_buffer_query_errors_total",
                "Total number of query errors",
            )
            .const_label("component", component),
        )?;

        let data_loss_detected = IntCounter::with_opts(
            Opts::new(
                "msg_buffer_data_loss_detected_total",
                "Number of times data loss was detected",
            )
            .const_label("component", component),
        )?;

        let append_duration = Histogram::with_opts(
            HistogramOpts::new(
                "msg_buffer_append_duration_milliseconds",
                "Duration of append operations in milliseconds",
            )
            .const_label("component", component),
        )?;

        let query_duration = Histogram::with_opts(
            HistogramOpts::new(
                "msg_buffer_query_duration_milliseconds",
                "Duration of query operations in milliseconds",
            )
            .const_label("component", component)
            .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0]),
        )?;

        let trim_duration = Histogram::with_opts(
            HistogramOpts::new(
                "msg_buffer_trim_duration_milliseconds",
                "Duration of trim operations in milliseconds",
            )
            .const_label("component", component),
        )?;

        let sync_lag = Histogram::with_opts(
            HistogramOpts::new(
                "msg_buffer_sync_lag_ids",
                "Lag between requested ID and current max ID",
            )
            .const_label("component", component)
            .buckets(vec![1.0, 10.0, 100.0, 500.0, 1000.0, 5000.0, 10000.0]),
        )?;

        let buffer_size = IntGauge::with_opts(
            Opts::new("msg_buffer_size", "Total number of messages in buffer")
                .const_label("component", component),
        )?;

        let hot_buffer_size = IntGauge::with_opts(
            Opts::new(
                "msg_buffer_hot_size",
                "Number of messages in hot (RingBuffer) storage",
            )
            .const_label("component", component),
        )?;

        let min_id = IntGauge::with_opts(
            Opts::new("msg_buffer_min_id", "Minimum ID in buffer")
                .const_label("component", component),
        )?;

        let max_id = IntGauge::with_opts(
            Opts::new("msg_buffer_max_id", "Maximum ID in buffer")
                .const_label("component", component),
        )?;

        // Register all metrics
        registry.register(Box::new(append_total.clone()))?;
        registry.register(Box::new(append_errors.clone()))?;
        registry.register(Box::new(trim_total.clone()))?;
        registry.register(Box::new(trim_errors.clone()))?;
        registry.register(Box::new(query_total.clone()))?;
        registry.register(Box::new(query_errors.clone()))?;
        registry.register(Box::new(data_loss_detected.clone()))?;
        registry.register(Box::new(append_duration.clone()))?;
        registry.register(Box::new(query_duration.clone()))?;
        registry.register(Box::new(trim_duration.clone()))?;
        registry.register(Box::new(sync_lag.clone()))?;
        registry.register(Box::new(buffer_size.clone()))?;
        registry.register(Box::new(hot_buffer_size.clone()))?;
        registry.register(Box::new(min_id.clone()))?;
        registry.register(Box::new(max_id.clone()))?;

        Ok(Self {
            append_total,
            append_errors,
            trim_total,
            trim_errors,
            query_total,
            query_errors,
            data_loss_detected,
            append_duration,
            query_duration,
            trim_duration,
            sync_lag,
            buffer_size,
            hot_buffer_size,
            min_id,
            max_id,
        })
    }

    /// Record an append operation
    pub fn record_append(&self, _: u64, duration_ms: f64, success: bool) {
        if success {
            self.append_total.inc();
            self.append_duration.observe(duration_ms);
        } else {
            self.append_errors.inc();
        }
    }

    /// Record a query operation
    pub fn record_query(&self, duration_ms: f64, success: bool, is_hot: bool) {
        if success {
            let storage = if is_hot { "hot" } else { "cold" };
            self.query_total.with_label_values(&[storage]).inc();
            self.query_duration.observe(duration_ms);
        } else {
            self.query_errors.inc();
        }
    }

    /// Record a trim operation
    pub fn record_trim(&self, _: u64, duration_ms: f64, success: bool) {
        if success {
            self.trim_total.inc();
            self.trim_duration.observe(duration_ms);
        } else {
            self.trim_errors.inc();
        }
    }

    /// Record sync lag
    pub fn record_sync_lag(&self, requested_id: u64, current_max_id: u64) {
        if current_max_id >= requested_id {
            let lag = (current_max_id - requested_id) as f64;
            self.sync_lag.observe(lag);
        }
    }

    /// Record data loss detection
    pub fn record_data_loss(&self, requested_id: u64, min_available_id: u64) {
        self.data_loss_detected.inc();
        warn!(
            requested_id = requested_id,
            min_available_id = min_available_id,
            "Data loss detected: requested data has been trimmed"
        );
    }

    /// Update buffer size gauges
    pub fn update_buffer_size(&self, total: u64, hot: usize) {
        self.buffer_size.set(total as i64);
        self.hot_buffer_size.set(hot as i64);
    }

    /// Update min/max ID gauges
    pub fn update_id_range(&self, min: Option<u64>, max: Option<u64>) {
        if let Some(min_val) = min {
            self.min_id.set(min_val as i64);
        }
        if let Some(max_val) = max {
            self.max_id.set(max_val as i64);
        }
    }
}
