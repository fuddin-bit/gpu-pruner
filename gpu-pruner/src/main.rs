use minijinja::{Environment, context};

#[cfg(feature = "otel")]
use std::sync::LazyLock;
#[cfg(feature = "otel")]
use {
    opentelemetry::global,
    opentelemetry::trace::TracerProvider,
    opentelemetry_otlp::{MetricExporter, SpanExporter},
    opentelemetry_sdk::Resource as OTELResource,
    opentelemetry_sdk::metrics::SdkMeterProvider,
    opentelemetry_sdk::trace::SdkTracerProvider,
    tracing_opentelemetry::{MetricsLayer, OpenTelemetryLayer},
};

use std::{
    collections::HashSet,
    fmt::Debug,
    sync::{Arc, atomic::AtomicUsize},
};
use tokio::{sync::mpsc::Sender, time};

use tracing_subscriber::EnvFilter;
#[cfg(not(feature = "otel"))]
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use futures::stream::StreamExt;

use prometheus_http_query::Client;
use serde::Serialize;
use serde_json::json;

use jiff::{SignedDuration, Timestamp};
use k8s_openapi::api::core::v1::Pod;
use kube::{Api, Client as KubeClient, Resource};

use clap::{Parser, ValueEnum};

use gpu_pruner::{
    Meta, PendingScaleStatus, PodMetricData, QueryResponse, RootObjectError, ScaleKind, Scaler,
    TlsMode, acknowledge_workload,
    api::{self, AppState},
    check_acknowledgment, check_pending_grace, clear_pending_scale_at, fetch_workload,
    find_root_object, get_enabled_resources, get_prom_client, get_prometheus_token,
    set_pending_scale_at,
    slack::SlackNotifier,
};

use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::net::SocketAddr;
use tower_http::{
    cors::{Any, CorsLayer},
    services::{ServeDir, ServeFile},
};

/// `gpu-pruner` is a tool to prune idle pods based on GPU utilization. It uses Prometheus to query
/// GPU utilization metrics and scales down pods that have been idle for a certain duration.
///
/// Requires a Prometheus instance to be running in the cluster w/ GPU metrics. Currently only supports
/// NVIDIA GPUs.
#[derive(Debug, Clone, Parser, Serialize)]
struct Cli {
    /// time in minutes of no gpu activity to use for pruning
    #[clap(short = 't', long, default_value = "30")]
    duration: i64,

    /// daemon mode to run in, if true, will run indefinitely
    #[clap(short, long)]
    daemon_mode: bool,

    /// Specify enabled resources with a string of letters
    ///
    /// - `d` for Deployment
    /// - `r` for ReplicaSet
    /// - `s` for StatefulSet
    /// - `i` for InferenceService
    /// - `n` for Notebook
    ///
    /// Note: LeaderWorkerSet is automatically enabled with any resource combination
    #[clap(short, long, default_value = "drsinl")]
    enabled_resources: String,

    /// interval in seconds to check for idle pods, only used in daemon mode
    #[clap(short, long, default_value = "180")]
    check_interval: u64,

    /// namespace to use for search filter, is passed down to prometheus as a pattern match
    #[clap(short, long)]
    namespace: Option<String>,

    /// Seconds of grace period to allow for metrics to be published.
    #[clap(short, long, default_value = "300")]
    grace_period: i64,

    /// model name of GPU to use for filter, eg. "NVIDIA A10G", is passed down to prometheus as a pattern match
    #[clap(short, long)]
    model_name: Option<String>,

    /// Maximum combined GPU utilization (0.0–1.0) to still consider a GPU idle.
    /// Defaults to 0.01 to tolerate DCGM background noise on DCGM_FI_PROF_GR_ENGINE_ACTIVE.
    #[clap(long, default_value_t = 0.01)]
    idle_threshold: f64,

    /// Power draw threshold in watts. When set, GPUs showing peak power usage above this value
    /// over the lookback window are excluded from idle candidates even if compute utilization is zero.
    /// Useful as a corroborating signal (e.g. 100 for A10G, 150 for A100/H100).
    #[clap(long)]
    power_threshold: Option<f64>,

    /// Set when the Prometheus ServiceMonitor uses honorLabels: true.
    /// Controls whether the query uses native DCGM label names (pod/namespace/container)
    /// or the Prometheus-prefixed names (exported_pod/exported_namespace/exported_container).
    #[clap(long, default_value = "false")]
    honor_labels: bool,

    /// Operation mode of the scaler process
    #[clap(short, long, default_value = "dry-run")]
    run_mode: Mode,

    /// Prometheus URL to query for GPU metrics
    /// eg. "http://prometheus-k8s.openshift-monitoring.svc:9090"
    #[clap(long)]
    prometheus_url: String,

    /// Prometheus token to use for authentication,
    /// if not provided, will try to authenticate using the service token
    /// of the currently logged in K8s user.
    #[clap(long)]
    prometheus_token: Option<String>,

    #[clap(long, default_value = "verify")]
    prometheus_tls_mode: TlsMode,

    /// Custom .crt file to use for TLS verification
    #[clap(long)]
    prometheus_tls_cert: Option<String>,

    /// Log format to use
    #[clap(short, long, default_value = "default")]
    log_format: LogFormat,

    /// Slack webhook URL for notifications. Can also be set via SLACK_WEBHOOK_URL env var.
    /// Messages will be sent to the configured channel when idle GPUs are detected.
    #[clap(long)]
    slack_webhook_url: Option<String>,

    /// Slack channel to send notifications to
    #[clap(long, default_value = "#test-pruner")]
    slack_channel: String,

    /// Port to listen for Slack interactive component callbacks (button clicks).
    /// Required if you want users to acknowledge idle GPUs from Slack messages.
    #[clap(long)]
    slack_interaction_port: Option<u16>,

    /// Seconds to wait after Slack notification before scaling down (Slack only).
    #[clap(long, default_value = "300")]
    ack_grace_period: u64,

    /// Namespace-to-Slack-mention mapping as JSON. Can also be set via SLACK_NAMESPACE_MENTIONS env var.
    /// Format: {"namespace": "<@USER_ID>", "prefix-": "<@USER_ID>"}
    /// Example: {"fuddin-dev":"<@U123>","alice-":"<@UALICE>"}
    /// Supports exact namespace matching and prefix matching (e.g., "alice-" matches alice-dev, alice-prod).
    #[clap(long)]
    slack_namespace_mentions: Option<String>,
}

#[derive(Debug, Clone, ValueEnum, Default, Serialize)]
enum Mode {
    ScaleDown,
    #[default]
    DryRun,
}

#[derive(Debug, Clone, ValueEnum, Default, Serialize)]
enum LogFormat {
    Json,
    #[default]
    Default,
    Pretty,
}

static QUERY_FAILURES: AtomicUsize = AtomicUsize::new(0);

// Slack interaction handler state
#[derive(Clone)]
struct SlackInteractionState {
    kube_client: KubeClient,
}

// Slack interaction payload structures
#[derive(Debug, serde::Deserialize)]
struct SlackVerificationPayload {
    #[serde(rename = "type")]
    payload_type: String,
    challenge: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct SlackInteractionPayload {
    user: SlackUser,
    actions: Vec<SlackAction>,
    response_url: String,
}

#[derive(Debug, serde::Deserialize)]
struct SlackUser {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    username: String,
}

#[derive(Debug, serde::Deserialize)]
struct SlackAction {
    value: String,
}

// Handler for Slack interactive component callbacks
async fn handle_slack_interaction(
    State(state): State<SlackInteractionState>,
    body: String,
) -> Response {
    tracing::info!("Received Slack interaction callback");

    // Parse the form-encoded payload
    let payload_str = match serde_urlencoded::from_str::<Vec<(String, String)>>(&body) {
        Ok(params) => {
            // Slack sends the payload in a "payload" field
            params
                .into_iter()
                .find(|(k, _)| k == "payload")
                .map(|(_, v)| v)
                .unwrap_or_default()
        }
        Err(e) => {
            tracing::error!("Failed to parse form data: {}", e);
            return (StatusCode::BAD_REQUEST, "Invalid form data").into_response();
        }
    };

    // Parse the JSON payload
    if payload_str.is_empty() {
        tracing::info!("Received empty Slack interaction payload (URL verification probe)");
        return (StatusCode::OK, "OK").into_response();
    }

    if let Ok(verification) = serde_json::from_str::<SlackVerificationPayload>(&payload_str)
        && verification.payload_type == "url_verification"
        && let Some(challenge) = verification.challenge
    {
        tracing::info!("Responding to Slack URL verification challenge");
        return (StatusCode::OK, challenge).into_response();
    }

    let payload: SlackInteractionPayload = match serde_json::from_str(&payload_str) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to parse Slack payload JSON: {}", e);
            return (StatusCode::BAD_REQUEST, "Invalid JSON payload").into_response();
        }
    };

    // Extract action value (format: kind:namespace:name:duration)
    let action_value = match payload.actions.first() {
        Some(action) => &action.value,
        None => {
            tracing::error!("No action found in payload");
            return (StatusCode::BAD_REQUEST, "No action found").into_response();
        }
    };

    let parts: Vec<&str> = action_value.split(':').collect();
    if parts.len() != 4 {
        tracing::error!("Invalid action value format: {}", action_value);
        return (StatusCode::BAD_REQUEST, "Invalid action value").into_response();
    }

    let (kind, namespace, name, duration_str) = (parts[0], parts[1], parts[2], parts[3]);
    let duration_hours: u32 = match duration_str.parse() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Failed to parse duration: {}", e);
            return (StatusCode::BAD_REQUEST, "Invalid duration").into_response();
        }
    };

    // Get user identifier - prefer name, fallback to username, then id
    let user = if !payload.user.name.is_empty() {
        &payload.user.name
    } else if !payload.user.username.is_empty() {
        &payload.user.username
    } else {
        &payload.user.id
    };

    // Apply acknowledgment
    match acknowledge_workload(
        state.kube_client.clone(),
        kind,
        name,
        namespace,
        duration_hours,
        user,
    )
    .await
    {
        Ok(_) => {
            tracing::info!(
                "Successfully acknowledged [{kind}] {namespace}:{name} for {duration_hours}h by {user}"
            );

            // Send response back to Slack to update the message
            let response_message = json!({
                "replace_original": true,
                "attachments": [{
                    "color": "good",
                    "title": "✓ GPU Idle Acknowledgment Confirmed",
                    "fields": [
                        {
                            "title": "Resource",
                            "value": format!("{}: {}", kind, name),
                            "short": true
                        },
                        {
                            "title": "Namespace",
                            "value": namespace,
                            "short": true
                        },
                        {
                            "title": "Acknowledged By",
                            "value": user,
                            "short": true
                        },
                        {
                            "title": "Duration",
                            "value": format!("{} hours", duration_hours),
                            "short": true
                        },
                        {
                            "title": "Status",
                            "value": format!("GPU will not be scaled down for the next {} hours", duration_hours),
                            "short": false
                        }
                    ],
                    "footer": "gpu-pruner",
                    "ts": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                }]
            });

            // Send the response to Slack via response_url
            if let Err(e) = reqwest::Client::new()
                .post(&payload.response_url)
                .json(&response_message)
                .send()
                .await
            {
                tracing::error!("Failed to send response to Slack: {}", e);
            }

            (StatusCode::OK, "Acknowledged").into_response()
        }
        Err(e) => {
            tracing::error!("Failed to acknowledge workload: {}", e);

            // Send error response to Slack
            let error_message = json!({
                "replace_original": false,
                "text": format!("❌ Failed to acknowledge: {}", e),
                "response_type": "ephemeral"
            });

            if let Err(e) = reqwest::Client::new()
                .post(&payload.response_url)
                .json(&error_message)
                .send()
                .await
            {
                tracing::error!("Failed to send error response to Slack: {}", e);
            }

            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to acknowledge").into_response()
        }
    }
}

#[cfg(feature = "otel")]
static RESOURCE: LazyLock<OTELResource> = LazyLock::new(|| {
    OTELResource::builder()
        .with_service_name("gpu-pruner")
        .build()
});

#[cfg(feature = "otel")]
fn init_metrics() -> anyhow::Result<SdkMeterProvider> {
    let exporter = MetricExporter::builder().with_tonic().build()?;

    let provider = SdkMeterProvider::builder()
        .with_periodic_exporter(exporter)
        .with_resource(RESOURCE.clone())
        .build();

    Ok(provider)
}

fn setup_logging(args: &Cli) -> OtelGuard {
    // Add a tracing filter to filter events from crates used by opentelemetry-otlp.
    // The filter levels are set as follows:
    // - Allow `info` level and above by default.
    // - Restrict `hyper`, `tonic`, and `reqwest` to `error` level logs only.
    // This ensures events generated from these crates within the OTLP Exporter are not looped back,
    // thus preventing infinite event generation.
    // Note: This will also drop events from these crates used outside the OTLP Exporter.
    // For more details, see: https://github.com/open-telemetry/opentelemetry-rust/issues/761
    #[cfg(feature = "otel")]
    let filter = EnvFilter::from_default_env()
        .add_directive("hyper=error".parse().unwrap())
        .add_directive("tonic=error".parse().unwrap())
        .add_directive("reqwest=error".parse().unwrap());

    #[cfg(not(feature = "otel"))]
    let filter = EnvFilter::from_default_env();
    let reg = tracing_subscriber::registry().with(filter);

    let json_layer = if let LogFormat::Json = args.log_format {
        Some(tracing_subscriber::fmt::layer().json())
    } else {
        None
    };

    let pretty_layer = if let LogFormat::Pretty = args.log_format {
        Some(tracing_subscriber::fmt::layer().pretty())
    } else {
        None
    };

    let default_layer = if let LogFormat::Default = args.log_format {
        Some(tracing_subscriber::fmt::layer())
    } else {
        None
    };

    #[cfg(feature = "otel")]
    let meter_provider = get_meter_provider();

    #[cfg(feature = "otel")]
    let metrics_layer = {
        let _meter = global::meter("gpu_pruner::main");
        Some(MetricsLayer::new(meter_provider.clone()))
    };

    #[cfg(not(feature = "otel"))]
    let metrics_layer: Option<Box<dyn Layer<_> + Send + Sync>> = None;

    #[cfg(feature = "otel")]
    let (otel_layer, tracer_provider) = {
        let exporter = SpanExporter::builder()
            .with_tonic()
            .build()
            .expect("failed to create span exporter");

        let provider = SdkTracerProvider::builder()
            .with_resource(RESOURCE.clone())
            .with_batch_exporter(exporter)
            .build();

        global::set_tracer_provider(provider.clone());
        let tracer = provider.tracer("gpu_pruner::main");
        (Some(OpenTelemetryLayer::new(tracer)), provider)
    };

    #[cfg(not(feature = "otel"))]
    let otel_layer: Option<Box<dyn Layer<_> + Send + Sync>> = None;

    reg.with(json_layer)
        .with(default_layer)
        .with(pretty_layer)
        .with(metrics_layer)
        .with(otel_layer)
        .init();

    #[cfg(feature = "otel")]
    {
        OtelGuard {
            meter_provider,
            tracer_provider,
        }
    }

    #[cfg(not(feature = "otel"))]
    OtelGuard
}

#[cfg(feature = "otel")]
fn get_meter_provider() -> SdkMeterProvider {
    let meter_provider = init_metrics().expect("failed to init metrics");
    global::set_meter_provider(meter_provider.clone());
    meter_provider
}

#[cfg(feature = "otel")]
struct OtelGuard {
    meter_provider: SdkMeterProvider,
    tracer_provider: SdkTracerProvider,
}

#[cfg(not(feature = "otel"))]
struct OtelGuard;

#[cfg(feature = "otel")]
impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Err(err) = self.tracer_provider.shutdown() {
            eprintln!("{err:?}");
        }
        if let Err(err) = self.meter_provider.shutdown() {
            eprintln!("{err:?}");
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    let _guard = setup_logging(&args);

    // Initialize Prometheus metrics registry
    gpu_pruner::metrics::init();

    let enabled_resources = get_enabled_resources(&args.enabled_resources);
    tracing::info!("Enabled resources: {enabled_resources:?}");

    // Initialize Slack notifier if webhook URL is provided
    let slack_notifier = args
        .slack_webhook_url
        .clone()
        .or_else(|| std::env::var("SLACK_WEBHOOK_URL").ok())
        .map(|url| {
            tracing::info!(
                "Slack notifications enabled for channel: {}",
                args.slack_channel
            );
            Arc::new(SlackNotifier::new(url, args.slack_channel.clone()))
        });

    if slack_notifier.is_none() {
        tracing::info!("Slack notifications disabled (no webhook URL configured)");
    }

    // Initialize namespace mention mapper if configured
    let namespace_mention_mapper = args
        .slack_namespace_mentions
        .clone()
        .or_else(|| std::env::var("SLACK_NAMESPACE_MENTIONS").ok())
        .and_then(
            |json_str| match gpu_pruner::NamespaceMentionMapper::from_json(&json_str) {
                Ok(mapper) => {
                    tracing::info!(
                        "Namespace mention mapping enabled with {} entries",
                        mapper.mappings.len()
                    );
                    Some(Arc::new(mapper))
                }
                Err(e) => {
                    tracing::error!("Failed to parse namespace mention mapping JSON: {}", e);
                    None
                }
            },
        );

    if namespace_mention_mapper.is_none() && slack_notifier.is_some() {
        tracing::info!(
            "Namespace mention mapping not configured - will only use annotation-based mentions"
        );
    }

    let env: Environment = Environment::new();
    let query = env.render_str(include_str!("query.promql.j2"), context! { args })?;
    tracing::info!("Running w/ Query: {query}");

    let prom_client = Arc::new(build_prom_client(&args).await);

    let (tx, mut rx) = tokio::sync::mpsc::channel::<ScaleKind>(100);

    let query_task = {
        let args = args.clone();
        let slack_notifier = slack_notifier.clone();
        let namespace_mention_mapper = namespace_mention_mapper.clone();
        tokio::spawn(async move {
            let mut interval =
                time::interval(tokio::time::Duration::from_secs(args.check_interval));
            loop {
                if args.daemon_mode {
                    interval.tick().await;
                }

                let client = build_prom_client(&args).await;
                match run_query_and_scale(
                    client,
                    query.clone(),
                    &args,
                    slack_notifier.clone(),
                    namespace_mention_mapper.clone(),
                    tx.clone(),
                )
                .await
                {
                    Ok(qr) => {
                        QUERY_FAILURES.store(0, std::sync::atomic::Ordering::Relaxed);
                        gpu_pruner::metrics::QUERY_SUCCESSES.inc();
                        gpu_pruner::metrics::QUERY_CANDIDATES.inc_by(qr.num_pods as u64);
                        gpu_pruner::metrics::QUERY_SHUTDOWN_EVENTS
                            .inc_by(qr.shutdown_events as u64);
                        tracing::info!(monotonic_counter.query_successes = 1, "Query succeeded");
                        tracing::info!(
                            counter.query_returned_candidates = qr.num_pods,
                            "Returned candidates"
                        );
                        tracing::info!(
                            counter.query_returned_shutdown_events = qr.shutdown_events,
                            "Returned shutdown events"
                        );
                    }
                    Err(e) => {
                        let failures =
                            QUERY_FAILURES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        gpu_pruner::metrics::QUERY_FAILURES.inc();
                        tracing::error!(
                            monotonic_counter.query_failures = 1,
                            "Failed to run query and scale down: {e}"
                        );
                        if failures > 5 {
                            tracing::error!("Too many consecutive failures, exiting");
                            break;
                        }
                    }
                }

                if !args.daemon_mode {
                    break;
                }
            }
            drop(tx);
        })
    };

    let scale_down_task = {
        tokio::spawn(async move {
            let kube_client = KubeClient::try_default()
                .await
                .expect("failed to get kube client");

            while let Some(sk) = rx.recv().await {
                // Check if the resource is enabled
                if !enabled_resources.contains(sk.clone().into()) {
                    tracing::info!(
                        "Skipping resource type {kind:?} because it is not enabled",
                        kind = sk.kind()
                    );
                    continue;
                }

                if let Err(e) = sk.scale(kube_client.clone()).await {
                    gpu_pruner::metrics::SCALE_FAILURES.inc();
                    tracing::error!(
                        monotonic_counter.scale_failures = 1,
                        "Failed to scale resource! {e}"
                    );
                    continue;
                }

                let kind = sk.kind();
                let name = sk.name();
                let namespace = sk.namespace().unwrap_or_else(|| "default".to_string());

                gpu_pruner::metrics::SCALE_SUCCESSES.inc();
                gpu_pruner::metrics::SCALES_TOTAL
                    .with_label_values(&[&kind, &namespace, &name])
                    .inc();
                tracing::info!(
                    monotonic_counter.scale_successes = 1,
                    "Scaled Resource: [{kind}] - {namespace}:{name}",
                    kind = kind,
                    name = name,
                    namespace = namespace
                )
            }
        })
    };

    // Dashboard, API, and Prometheus metrics endpoint (port 8080)
    let metrics_task = {
        let prom_client = prom_client.clone();
        let honor_labels = args.honor_labels;
        tokio::spawn(async move {
            let web_dist = api::web_dist_dir();
            let index_path = web_dist.join("index.html");
            if web_dist.exists() {
                tracing::info!("Serving dashboard from {}", web_dist.display());
            } else {
                tracing::warn!(
                    "Dashboard static files not found at {} — API and /metrics still available",
                    web_dist.display()
                );
            }
            let serve_dir = ServeDir::new(&web_dist).not_found_service(ServeFile::new(index_path));

            let app_state = AppState {
                prom_client,
                honor_labels,
            };

            let app = Router::new()
                .route(
                    "/metrics",
                    get(|| async {
                        (
                            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                            gpu_pruner::metrics::render(),
                        )
                    }),
                )
                .route("/api/v1/summary", get(api::summary_handler))
                .route("/api/v1/stats", get(api::stats_handler))
                .route("/api/v1/idle-gpu-hours", get(api::idle_gpu_hours_handler))
                .fallback_service(serve_dir)
                .layer(
                    CorsLayer::new()
                        .allow_origin(Any)
                        .allow_methods(Any)
                        .allow_headers(Any),
                )
                .with_state(app_state);

            let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
            tracing::info!("Starting dashboard server on {}", addr);
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .expect("failed to bind dashboard server");
            axum::serve(listener, app)
                .await
                .expect("failed to start dashboard server");
            Ok::<(), anyhow::Error>(())
        })
    };

    // Spawn Slack interaction HTTP server if port is configured
    let slack_interaction_task = if let Some(port) = args.slack_interaction_port {
        let kube_client = KubeClient::try_default()
            .await
            .expect("failed to get kube client for slack interactions");

        let state = SlackInteractionState { kube_client };

        let app = Router::new()
            .route("/slack/interactions", post(handle_slack_interaction))
            .with_state(state);

        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        tracing::info!("Starting Slack interaction server on {}", addr);

        Some(tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .expect("failed to bind slack interaction server");
            axum::serve(listener, app)
                .await
                .expect("failed to start slack interaction server");
            Ok::<(), anyhow::Error>(())
        }))
    } else {
        tracing::info!("Slack interaction server disabled (no --slack-interaction-port set)");
        None
    };

    // Wait for all tasks
    if let Some(interaction_task) = slack_interaction_task {
        _ = tokio::try_join! {
            query_task,
            scale_down_task,
            metrics_task,
            interaction_task
        }?;
    } else {
        _ = tokio::try_join! {
            query_task,
            scale_down_task,
            metrics_task
        }?;
    }

    Ok(())
}

async fn build_prom_client(args: &Cli) -> Client {
    let token = get_prometheus_token()
        .await
        .expect("failed to get prometheus token");
    get_prom_client(
        &args.prometheus_url,
        token,
        args.prometheus_tls_mode,
        args.prometheus_tls_cert.clone(),
    )
    .expect("failed to build prometheus client")
}

#[tracing::instrument(skip_all)]
async fn run_query_and_scale(
    client: Client,
    query: String,
    args: &Cli,
    slack_notifier: Option<Arc<SlackNotifier>>,
    namespace_mention_mapper: Option<Arc<gpu_pruner::NamespaceMentionMapper>>,
    tx: Sender<ScaleKind>,
) -> anyhow::Result<QueryResponse> {
    let response = match client.query(query).get().await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!("Failed to run query! {e}");
            return Err(anyhow::anyhow!("Failed to run query! {e}"));
        }
    };

    let vec = response
        .data()
        .clone()
        .into_vector()
        .expect("expected vector response from prometheus");

    let kube_client: KubeClient = KubeClient::try_default().await?;

    let lookback_duration =
        SignedDuration::from_mins(args.duration) + SignedDuration::from_secs(args.grace_period);

    // Dedup by (pod, namespace) before processing. Multi-GPU pods produce one
    // series per GPU, but we only need to resolve the owner chain once per pod.
    let num_pods = vec.len();
    let mut seen_pods = HashSet::new();
    let unique_pods: Vec<_> = vec
        .into_iter()
        .filter_map(|v| {
            let pmd: PodMetricData = match (&v).try_into() {
                Ok(pmd) => pmd,
                Err(e) => {
                    tracing::error!("Failed to unwrap pod fields: {e}");
                    return None;
                }
            };
            let key = (pmd.name.clone(), pmd.namespace.clone());
            if seen_pods.insert(key) {
                Some(pmd)
            } else {
                None
            }
        })
        .collect();

    tracing::info!(
        "Query returned {num_pods} series across {} unique pods",
        unique_pods.len()
    );
    gpu_pruner::metrics::PODS_CHECKED.set(unique_pods.len() as i64);

    // Process pods concurrently (up to 10 at a time) instead of serially.
    // Each pod requires 1-3 API calls (get pod, walk owner refs), so parallelism
    // cuts wall-clock time significantly on large result sets.
    let results: Vec<Option<ScaleKind>> = futures::stream::iter(unique_pods)
        .map(|pmd| {
            let kube_client = kube_client.clone();
            async move {

                let api = Api::<Pod>::namespaced(kube_client.clone(), &pmd.namespace);
                let pod = match api.get_opt(&pmd.name).await {
                    Ok(Some(pod)) => pod,
                    Ok(None) => {
                        tracing::info!(
                            "Skipping {ns}:{name}, pod no longer exists",
                            ns = &pmd.namespace,
                            name = &pmd.name
                        );
                        return None;
                    }
                    Err(e) => {
                        tracing::error!(
                            "Skipping {ns}:{name}, retrieval error: {e}",
                            ns = &pmd.namespace,
                            name = &pmd.name
                        );
                        return None;
                    }
                };

                if let Some(status) = pod.status.as_ref()
                    && let Some(phase) = status.phase.as_ref()
                    && phase == "Pending"
                {
                    tracing::info!(
                        "Skipping pod {namespace}:{pod_name}, it's still pending",
                        namespace = &pmd.namespace,
                        pod_name = &pmd.name
                    );
                    return None;
                }

                let Some(create_time) = pod.metadata.creation_timestamp.clone() else {
                    tracing::warn!(
                        "Pod {namespace}:{pod_name} has no creation timestamp, skipping",
                        namespace = &pmd.namespace,
                        pod_name = &pmd.name
                    );
                    return None;
                };

                let lookback_start = Timestamp::now() - lookback_duration;
                let status = pod
                    .status
                    .as_ref()
                    .and_then(|s| s.phase.as_deref())
                    .unwrap_or("Unknown");

                tracing::info!(
                    "Pod {ns}:{name} | status={status} | created={created} | lookback={lookback_start}",
                    ns = &pmd.namespace,
                    name = &pmd.name,
                    created = create_time.0,
                );

                if create_time.0 >= lookback_start {
                    return None;
                }

                tracing::info!(
                    "Pod {ns}:{name} is idle and eligible for scaledown",
                    ns = &pmd.namespace,
                    name = &pmd.name,
                );
                match find_root_object(kube_client.clone(), pod.meta()).await {
                    Ok(obj) => Some(obj),
                    Err(e @ RootObjectError::NonScalable { .. }) => {
                        tracing::debug!(
                            "Skipping {ns}:{name}, {e}",
                            ns = &pmd.namespace,
                            name = &pmd.name,
                        );
                        None
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Skipping {ns}:{name}, no scalable root object: {e}",
                            ns = &pmd.namespace,
                            name = &pmd.name,
                        );
                        None
                    }
                }
            }
        })
        .buffer_unordered(10)
        .collect()
        .await;

    let shutdown_events: HashSet<ScaleKind> = results.into_iter().flatten().collect();

    let num_shutdown_events = shutdown_events.len();
    gpu_pruner::metrics::IDLE_GPUS.set(num_shutdown_events as i64);

    // Check acknowledgment status for all idle workloads
    let workloads_with_ack: Vec<(ScaleKind, Option<gpu_pruner::AckStatus>)> =
        futures::stream::iter(shutdown_events.iter().cloned())
            .then(|obj| async {
                let ack_status = check_acknowledgment(kube_client.clone(), &obj).await.ok();
                (obj, ack_status)
            })
            .collect()
            .await;

    // Count acknowledged workloads and update metrics
    let acknowledged_count = workloads_with_ack
        .iter()
        .filter(|(_, ack)| ack.as_ref().map(|a| a.acknowledged).unwrap_or(false))
        .count();

    gpu_pruner::metrics::ACKNOWLEDGED_WORKLOADS.set(acknowledged_count as i64);

    let slack_grace_enabled = slack_notifier.is_some() && args.ack_grace_period > 0;

    for (obj, ack_status) in workloads_with_ack {
        if ack_status.as_ref().is_some_and(|a| a.acknowledged) {
            let ack = ack_status.as_ref().unwrap();
            tracing::info!(
                "Skipping [{}] {}:{} - acknowledged until {} by {}",
                obj.kind(),
                obj.namespace().unwrap_or_default(),
                obj.name(),
                ack.expires_at.as_ref().unwrap_or(&"unknown".to_string()),
                ack.by_user.as_ref().unwrap_or(&"unknown".to_string())
            );
            gpu_pruner::metrics::SCALEDOWNS_PREVENTED_TOTAL.inc();
            continue;
        }

        let kind = obj.kind();
        let name = obj.name();
        let namespace = obj.namespace().unwrap_or_else(|| "default".to_string());

        if slack_grace_enabled {
            match check_pending_grace(&obj, args.ack_grace_period) {
                PendingScaleStatus::NotPending => {
                    if matches!(args.run_mode, Mode::DryRun) {
                        tracing::info!(
                            "Dry-run: Would notify [{}] {}:{} and wait {}s before scale-down",
                            kind,
                            namespace,
                            name,
                            args.ack_grace_period
                        );
                        continue;
                    }

                    if let Some(notifier) = slack_notifier.as_ref() {
                        // Extract mentions from workload annotations or namespace mapping
                        let mentions = gpu_pruner::get_slack_mentions(
                            &obj,
                            namespace_mention_mapper.as_deref(),
                        );

                        match notifier
                            .send_notification(&obj, args.duration, args.ack_grace_period, mentions)
                            .await
                        {
                            Ok(_) => gpu_pruner::metrics::SLACK_NOTIFICATIONS_SENT.inc(),
                            Err(e) => {
                                gpu_pruner::metrics::SLACK_NOTIFICATION_FAILURES.inc();
                                tracing::error!("Failed to send Slack notification: {e}");
                            }
                        }
                    }

                    if let Err(e) =
                        set_pending_scale_at(kube_client.clone(), &kind, &name, &namespace).await
                    {
                        tracing::error!(
                            "Failed to set pending-scale annotation for [{kind}] {namespace}:{name}: {e}"
                        );
                    } else {
                        tracing::info!(
                            "Started {}s ack grace period for [{kind}] {namespace}:{name}",
                            args.ack_grace_period
                        );
                    }
                    continue;
                }
                PendingScaleStatus::InGrace { until } => {
                    tracing::info!(
                        "Skipping [{}] {}:{} - ack grace period until {}",
                        kind,
                        namespace,
                        name,
                        until.to_rfc3339()
                    );
                    continue;
                }
                PendingScaleStatus::GraceExpired => {
                    let scale_obj = match fetch_workload(
                        kube_client.clone(),
                        &kind,
                        &name,
                        &namespace,
                    )
                    .await
                    {
                        Ok(fresh) => fresh,
                        Err(e) => {
                            tracing::warn!(
                                "Failed to re-fetch [{kind}] {namespace}:{name}, using cached object: {e}"
                            );
                            obj.clone()
                        }
                    };

                    if check_acknowledgment(kube_client.clone(), &scale_obj)
                        .await
                        .is_ok_and(|a| a.acknowledged)
                    {
                        tracing::info!(
                            "Skipping [{}] {}:{} - acknowledged during grace period",
                            kind,
                            namespace,
                            name
                        );
                        gpu_pruner::metrics::SCALEDOWNS_PREVENTED_TOTAL.inc();
                        let _ =
                            clear_pending_scale_at(kube_client.clone(), &kind, &name, &namespace)
                                .await;
                        continue;
                    }

                    if matches!(args.run_mode, Mode::DryRun) {
                        tracing::info!(
                            "Dry-run: Would scale [{}] {}:{} after grace period expired",
                            kind,
                            namespace,
                            name
                        );
                        continue;
                    }

                    let _ =
                        clear_pending_scale_at(kube_client.clone(), &kind, &name, &namespace).await;

                    tracing::info!(
                        "Sending [{}] {}:{} for scaledown after grace period expired",
                        kind,
                        namespace,
                        name
                    );
                    if let Err(e) = tx.send(scale_obj).await {
                        tracing::error!("Failed to send object for scaledown: {:?}", e);
                    }
                    continue;
                }
            }
        }

        if matches!(args.run_mode, Mode::DryRun) {
            tracing::info!(
                "Dry-run: Would have sent [{}] {}:{} for scaledown",
                kind,
                namespace,
                name
            );
            continue;
        }

        tracing::info!("Sending [{}] {}:{} for scaledown", kind, namespace, name);
        if let Err(e) = tx.send(obj).await {
            tracing::error!("Failed to send object for scaledown: {:?}", e);
        }
    }

    Ok(QueryResponse {
        num_pods,
        shutdown_events: num_shutdown_events,
    })
}

#[cfg(test)]
mod tests {
    use minijinja::{Environment, context};
    use serde_json::json;

    const TEMPLATE: &str = include_str!("query.promql.j2");

    fn render(args: serde_json::Value) -> String {
        let env = Environment::new();
        env.render_str(TEMPLATE, context! { args }).unwrap()
    }

    #[test]
    fn query_uses_max_over_time() {
        let query = render(json!({ "duration": 30 }));
        assert!(
            query.contains("max_over_time("),
            "should use max_over_time, not avg_over_time"
        );
        assert!(
            !query.contains("avg_over_time("),
            "should not contain avg_over_time"
        );
    }

    #[test]
    fn query_includes_gpu_util_fallback() {
        let query = render(json!({ "duration": 30 }));
        assert!(
            query.contains("DCGM_FI_PROF_GR_ENGINE_ACTIVE"),
            "primary metric missing"
        );
        assert!(
            query.contains("DCGM_FI_DEV_GPU_UTIL"),
            "fallback metric missing"
        );
        assert!(
            query.contains("/ 100"),
            "fallback should normalize 0-100 to 0-1"
        );
    }

    #[test]
    fn query_uses_idle_threshold_not_strict_zero() {
        let query = render(json!({ "duration": 30 }));
        assert!(
            query.contains("< 0.01"),
            "default idle threshold should be 0.01, not == 0"
        );
        assert!(!query.contains("== 0"), "should not use strict == 0");
    }

    #[test]
    fn query_idle_threshold_is_configurable() {
        let query = render(json!({ "duration": 30, "idle_threshold": 0.05 }));
        assert!(
            query.contains("< 0.05"),
            "should use configured idle threshold"
        );
    }

    #[test]
    fn query_without_power_threshold_has_no_unless() {
        let query = render(json!({ "duration": 30 }));
        assert!(
            !query.contains("unless"),
            "should not have unless clause without power_threshold"
        );
        assert!(
            !query.contains("DCGM_FI_DEV_POWER_USAGE"),
            "should not reference power metric"
        );
    }

    #[test]
    fn query_with_power_threshold_adds_unless() {
        let query = render(json!({ "duration": 30, "power_threshold": 150.0 }));
        assert!(
            query.contains("unless on (exported_pod, exported_namespace)"),
            "should have unless clause"
        );
        assert!(
            query.contains("DCGM_FI_DEV_POWER_USAGE"),
            "should reference power metric"
        );
        assert!(
            query.contains(">= 150"),
            "should use the configured threshold"
        );
    }

    #[test]
    fn query_excludes_protected_namespaces() {
        let query = render(json!({ "duration": 30 }));
        let pattern = r#"!~ "(llm-d-nightly-.*|bench-guide-.*|cw-.*)""#;
        assert_eq!(
            query.matches(pattern).count(),
            4,
            "exclude filter should appear in all compute metric selectors"
        );
    }

    #[test]
    fn query_excludes_dcgm_exporter_pods() {
        let query = render(json!({ "duration": 30 }));
        let pattern = r#"!~ "dcgm-exporter-.*""#;
        assert_eq!(
            query.matches(pattern).count(),
            4,
            "dcgm-exporter pod exclude should appear in all compute metric selectors"
        );
    }

    #[test]
    fn query_excludes_protected_namespaces_with_power_threshold() {
        let query = render(json!({ "duration": 30, "power_threshold": 150.0 }));
        let ns_pattern = r#"!~ "(llm-d-nightly-.*|bench-guide-.*|cw-.*)""#;
        let pod_pattern = r#"!~ "dcgm-exporter-.*""#;
        assert_eq!(
            query.matches(ns_pattern).count(),
            5,
            "exclude filter should appear in compute and power metric selectors"
        );
        assert_eq!(
            query.matches(pod_pattern).count(),
            5,
            "dcgm-exporter pod exclude should appear in compute and power metric selectors"
        );
    }

    #[test]
    fn query_with_namespace_filter() {
        let query = render(json!({ "duration": 15, "namespace": "ml-team" }));
        let count = query.matches("exported_namespace =~ \"ml-team\"").count();
        // idle_gpus block appears twice (enriched + bare fallback), 2 metrics each = 4
        assert_eq!(
            count, 4,
            "namespace filter should appear in all compute metric selectors"
        );
    }

    #[test]
    fn query_with_namespace_and_power_threshold() {
        let query = render(json!({
            "duration": 15,
            "namespace": "ml-team",
            "power_threshold": 100.0
        }));
        let count = query.matches("exported_namespace =~ \"ml-team\"").count();
        // 4 from compute (2 paths x 2 metrics) + 1 from power = 5
        assert_eq!(
            count, 5,
            "namespace filter should appear in all metric selectors"
        );
    }

    #[test]
    fn query_with_model_name_filter() {
        let query = render(json!({ "duration": 30, "model_name": "NVIDIA A100" }));
        let count = query.matches("modelName =~ \"NVIDIA A100\"").count();
        // idle_gpus block appears twice (enriched + bare fallback), 2 metrics each = 4
        assert_eq!(
            count, 4,
            "model_name filter should appear in all compute metric selectors"
        );
    }

    #[test]
    fn query_duration_is_interpolated() {
        let query = render(json!({ "duration": 45 }));
        assert!(
            query.contains("[45m]"),
            "duration should be interpolated into range selector"
        );
    }

    #[test]
    fn query_default_uses_exported_labels() {
        let query = render(json!({ "duration": 30 }));
        assert!(
            query.contains("exported_pod"),
            "default should use exported_pod"
        );
        assert!(
            query.contains("exported_namespace"),
            "default should use exported_namespace"
        );
        assert!(
            query.contains("exported_container"),
            "default should use exported_container"
        );
    }

    #[test]
    fn query_honor_labels_uses_native_labels() {
        let query = render(json!({ "duration": 30, "honor_labels": true }));
        assert!(
            !query.contains("exported_pod"),
            "honor_labels should not use exported_pod"
        );
        assert!(
            !query.contains("exported_namespace"),
            "honor_labels should not use exported_namespace"
        );
        assert!(
            query.contains("pod !="),
            "honor_labels should filter on pod"
        );
        assert!(
            query.contains("sum by (Hostname, container, pod, namespace"),
            "honor_labels should group by native labels"
        );
    }

    #[test]
    fn query_honor_labels_with_power_threshold() {
        let query = render(json!({
            "duration": 30,
            "honor_labels": true,
            "power_threshold": 120.0
        }));
        assert!(
            query.contains("unless on (pod, namespace)"),
            "honor_labels power unless should use native labels"
        );
    }
}
