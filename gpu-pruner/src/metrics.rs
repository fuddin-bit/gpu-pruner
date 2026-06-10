use lazy_static::lazy_static;
use prometheus::{Encoder, IntCounter, IntGauge, Registry, TextEncoder};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    // Query metrics
    pub static ref QUERY_SUCCESSES: IntCounter = IntCounter::new(
        "gpu_pruner_query_successes_total",
        "Total number of successful Prometheus queries"
    )
    .expect("metric can be created");

    pub static ref QUERY_FAILURES: IntCounter = IntCounter::new(
        "gpu_pruner_query_failures_total",
        "Total number of failed Prometheus queries"
    )
    .expect("metric can be created");

    pub static ref QUERY_CANDIDATES: IntCounter = IntCounter::new(
        "gpu_pruner_query_candidates_total",
        "Total number of idle GPU candidates found across all queries"
    )
    .expect("metric can be created");

    pub static ref QUERY_SHUTDOWN_EVENTS: IntCounter = IntCounter::new(
        "gpu_pruner_query_shutdown_events_total",
        "Total number of shutdown events detected"
    )
    .expect("metric can be created");

    // Scale operation metrics
    pub static ref SCALE_SUCCESSES: IntCounter = IntCounter::new(
        "gpu_pruner_scale_successes_total",
        "Total number of successful scale operations"
    )
    .expect("metric can be created");

    pub static ref SCALE_FAILURES: IntCounter = IntCounter::new(
        "gpu_pruner_scale_failures_total",
        "Total number of failed scale operations"
    )
    .expect("metric can be created");

    // Current state metrics (gauges)
    pub static ref IDLE_GPUS: IntGauge = IntGauge::new(
        "gpu_pruner_idle_gpus",
        "Current number of idle GPUs detected in last check"
    )
    .expect("metric can be created");

    pub static ref PODS_CHECKED: IntGauge = IntGauge::new(
        "gpu_pruner_pods_checked_total",
        "Total number of pods analyzed in last query"
    )
    .expect("metric can be created");

    // Acknowledgment metrics
    pub static ref ACKNOWLEDGED_WORKLOADS: IntGauge = IntGauge::new(
        "gpu_pruner_acknowledged_workloads",
        "Current number of workloads with active acknowledgments"
    )
    .expect("metric can be created");

    pub static ref ACKNOWLEDGMENTS_TOTAL: IntCounter = IntCounter::new(
        "gpu_pruner_acknowledgments_total",
        "Total number of acknowledgments created"
    )
    .expect("metric can be created");

    pub static ref SCALEDOWNS_PREVENTED_TOTAL: IntCounter = IntCounter::new(
        "gpu_pruner_scaledowns_prevented_total",
        "Total number of scale-downs prevented by acknowledgments"
    )
    .expect("metric can be created");

    // Slack notification metrics
    pub static ref SLACK_NOTIFICATIONS_SENT: IntCounter = IntCounter::new(
        "gpu_pruner_slack_notifications_sent_total",
        "Total number of Slack notifications successfully sent"
    )
    .expect("metric can be created");

    pub static ref SLACK_NOTIFICATION_FAILURES: IntCounter = IntCounter::new(
        "gpu_pruner_slack_notification_failures_total",
        "Total number of failed Slack notification attempts"
    )
    .expect("metric can be created");
}

pub fn init() {
    REGISTRY
        .register(Box::new(QUERY_SUCCESSES.clone()))
        .expect("query_successes can be registered");
    REGISTRY
        .register(Box::new(QUERY_FAILURES.clone()))
        .expect("query_failures can be registered");
    REGISTRY
        .register(Box::new(QUERY_CANDIDATES.clone()))
        .expect("query_candidates can be registered");
    REGISTRY
        .register(Box::new(QUERY_SHUTDOWN_EVENTS.clone()))
        .expect("query_shutdown_events can be registered");
    REGISTRY
        .register(Box::new(SCALE_SUCCESSES.clone()))
        .expect("scale_successes can be registered");
    REGISTRY
        .register(Box::new(SCALE_FAILURES.clone()))
        .expect("scale_failures can be registered");
    REGISTRY
        .register(Box::new(IDLE_GPUS.clone()))
        .expect("idle_gpus can be registered");
    REGISTRY
        .register(Box::new(PODS_CHECKED.clone()))
        .expect("pods_checked can be registered");
    REGISTRY
        .register(Box::new(ACKNOWLEDGED_WORKLOADS.clone()))
        .expect("acknowledged_workloads can be registered");
    REGISTRY
        .register(Box::new(ACKNOWLEDGMENTS_TOTAL.clone()))
        .expect("acknowledgments_total can be registered");
    REGISTRY
        .register(Box::new(SCALEDOWNS_PREVENTED_TOTAL.clone()))
        .expect("scaledowns_prevented_total can be registered");
    REGISTRY
        .register(Box::new(SLACK_NOTIFICATIONS_SENT.clone()))
        .expect("slack_notifications_sent can be registered");
    REGISTRY
        .register(Box::new(SLACK_NOTIFICATION_FAILURES.clone()))
        .expect("slack_notification_failures can be registered");
}

pub fn render() -> String {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}
