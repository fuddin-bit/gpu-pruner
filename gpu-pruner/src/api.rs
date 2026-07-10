use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use prometheus_http_query::{Client as PromClient, response::Data};
use serde::{Deserialize, Serialize};

use crate::metrics;

const ALLOWED_WINDOWS: &[&str] = &["1h", "7d", "30d"];

#[derive(Clone)]
pub struct AppState {
    pub prom_client: Arc<PromClient>,
    pub honor_labels: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScaleDownsSummary {
    pub lifetime: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IdleWorkloadsSummary {
    pub current: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SummaryResponse {
    pub scale_downs: ScaleDownsSummary,
    pub idle_workloads: IdleWorkloadsSummary,
    pub pods_checked: i64,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScaleDownsStats {
    pub lifetime: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_window: Option<u64>,
    pub window: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatsResponse {
    pub scale_downs: ScaleDownsStats,
    pub idle_workloads: IdleWorkloadsSummary,
    pub prometheus_available: bool,
    pub pods_checked: i64,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct StatsQuery {
    pub window: Option<String>,
}

fn now_rfc3339() -> String {
    jiff::Timestamp::now().to_string()
}

fn snapshot_response() -> (SummaryResponse, i64) {
    let snap = metrics::snapshot();
    (
        SummaryResponse {
            scale_downs: ScaleDownsSummary {
                lifetime: snap.scale_successes,
            },
            idle_workloads: IdleWorkloadsSummary {
                current: snap.idle_workloads,
            },
            pods_checked: snap.pods_checked,
            updated_at: now_rfc3339(),
        },
        snap.pods_checked,
    )
}

pub async fn summary_handler() -> Json<SummaryResponse> {
    let (summary, _) = snapshot_response();
    Json(summary)
}

fn normalize_window(window: Option<String>) -> Result<String, Box<Response>> {
    let window = window.unwrap_or_else(|| "7d".to_string());
    if ALLOWED_WINDOWS.contains(&window.as_str()) {
        Ok(window)
    } else {
        Err(Box::new(
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid window",
                    "allowed": ALLOWED_WINDOWS,
                })),
            )
                .into_response(),
        ))
    }
}

fn parse_prom_scalar(data: &Data) -> Option<f64> {
    match data {
        Data::Vector(samples) => Some(
            samples
                .iter()
                .map(|sample| sample.sample().value())
                .sum::<f64>(),
        ),
        Data::Scalar(sample) => Some(sample.value()),
        Data::Matrix(_) => None,
    }
}

async fn query_scale_downs_in_window(
    prom_client: &PromClient,
    window: &str,
) -> anyhow::Result<u64> {
    let query = format!("sum(increase(gpu_pruner_scale_successes_total[{window}]))");
    let response = prom_client.query(query).get().await?;
    let value = parse_prom_scalar(response.data())
        .ok_or_else(|| anyhow::anyhow!("unexpected Prometheus response type"))?;
    Ok(value.round().max(0.0) as u64)
}

pub async fn stats_handler(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
) -> Result<Json<StatsResponse>, Response> {
    let window = normalize_window(query.window).map_err(|e| *e)?;
    let snap = metrics::snapshot();

    let mut in_window = None;
    let mut prometheus_available = false;

    match query_scale_downs_in_window(&state.prom_client, &window).await {
        Ok(count) => {
            in_window = Some(count);
            prometheus_available = true;
        }
        Err(e) => {
            tracing::warn!("Failed to query Prometheus for scale-down stats: {e}");
        }
    }

    Ok(Json(StatsResponse {
        scale_downs: ScaleDownsStats {
            lifetime: snap.scale_successes,
            in_window,
            window,
        },
        idle_workloads: IdleWorkloadsSummary {
            current: snap.idle_workloads,
        },
        prometheus_available,
        pods_checked: snap.pods_checked,
        updated_at: now_rfc3339(),
    }))
}

const IDLE_GPU_HOURS_LIMIT: usize = 25;
const EXCLUDED_NAMESPACES: &str = "llm-d-nightly-.*|bench-guide-.*|cw-.*";
const EXCLUDED_PODS: &str = "dcgm-exporter-.*";

#[derive(Debug, Serialize, Deserialize)]
pub struct IdleGpuHoursEntry {
    pub rank: usize,
    pub namespace: String,
    pub pod: String,
    pub idle_hours: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IdleGpuHoursResponse {
    pub entries: Vec<IdleGpuHoursEntry>,
    pub prometheus_available: bool,
    pub updated_at: String,
}

fn idle_gpu_hours_query(honor_labels: bool) -> String {
    let (ns_label, pod_label) = if honor_labels {
        ("namespace", "pod")
    } else {
        ("exported_namespace", "exported_pod")
    };

    format!(
        "sort_desc(sum by ({ns_label}, {pod_label}) (count_over_time((DCGM_FI_PROF_GR_ENGINE_ACTIVE{{{ns_label}!~\"{EXCLUDED_NAMESPACES}\",{pod_label}!=\"\",{pod_label}!~\"{EXCLUDED_PODS}\"}} < 0.01)[7d:1m]) / 60))"
    )
}

fn parse_idle_gpu_hours(data: &Data, honor_labels: bool) -> Vec<IdleGpuHoursEntry> {
    let (ns_key, pod_key) = if honor_labels {
        ("namespace", "pod")
    } else {
        ("exported_namespace", "exported_pod")
    };

    let Data::Vector(samples) = data else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    for sample in samples {
        if entries.len() >= IDLE_GPU_HOURS_LIMIT {
            break;
        }

        let metric = sample.metric();
        let namespace = metric
            .get(ns_key)
            .or_else(|| metric.get("namespace"))
            .or_else(|| metric.get("exported_namespace"))
            .cloned();
        let pod = metric
            .get(pod_key)
            .or_else(|| metric.get("pod"))
            .or_else(|| metric.get("exported_pod"))
            .cloned();

        let (Some(namespace), Some(pod)) = (namespace, pod) else {
            continue;
        };
        if namespace.is_empty() || pod.is_empty() {
            continue;
        }

        let idle_hours = sample.sample().value();
        if !idle_hours.is_finite() {
            continue;
        }

        entries.push(IdleGpuHoursEntry {
            rank: entries.len() + 1,
            namespace,
            pod,
            idle_hours,
        });
    }

    entries
}

pub async fn idle_gpu_hours_handler(State(state): State<AppState>) -> Json<IdleGpuHoursResponse> {
    let updated_at = now_rfc3339();
    let query = idle_gpu_hours_query(state.honor_labels);

    match state.prom_client.query(query).get().await {
        Ok(response) => Json(IdleGpuHoursResponse {
            entries: parse_idle_gpu_hours(response.data(), state.honor_labels),
            prometheus_available: true,
            updated_at,
        }),
        Err(e) => {
            tracing::warn!("Failed to query Prometheus for idle GPU hours: {e}");
            Json(IdleGpuHoursResponse {
                entries: Vec::new(),
                prometheus_available: false,
                updated_at,
            })
        }
    }
}

pub fn web_dist_dir() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("GPU_PRUNER_WEB_DIST") {
        return std::path::PathBuf::from(path);
    }

    let container = std::path::PathBuf::from("/opt/gpu-pruner/web/dist");
    if container.exists() {
        container
    } else {
        std::path::PathBuf::from("web/dist")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, routing::get};
    use tower::ServiceExt;

    #[test]
    fn normalize_window_accepts_known_values() {
        assert!(normalize_window(Some("7d".into())).is_ok());
        assert!(normalize_window(Some("1h".into())).is_ok());
        assert!(normalize_window(Some("30d".into())).is_ok());
        assert!(normalize_window(None).unwrap() == "7d");
        assert!(normalize_window(Some("1w".into())).is_err());
    }

    #[tokio::test]
    async fn summary_endpoint_returns_json() {
        metrics::IDLE_GPUS.set(2);
        metrics::PODS_CHECKED.set(5);

        let app = Router::new().route("/api/v1/summary", get(summary_handler));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/summary")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let summary: SummaryResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(summary.idle_workloads.current, 2);
        assert_eq!(summary.pods_checked, 5);
    }
}
