use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    body::Body,
    extract::{OriginalUri, Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use prometheus_http_query::{Client as PromClient, response::Data};
use serde::{Deserialize, Serialize};

use crate::metrics;

const ALLOWED_WINDOWS: &[&str] = &["1h", "7d", "30d"];

#[derive(Clone)]
pub struct ClusterConfig {
    pub http_client: reqwest::Client,
    pub prometheus_url: String,
    pub honor_labels: bool,
}

#[derive(Clone)]
pub struct AppState {
    pub prom_client: Arc<PromClient>,
    pub http_client: reqwest::Client,
    pub prometheus_url: String,
    pub clusters: HashMap<String, ClusterConfig>,
    pub namespace: String,
    pub pod_name: String,
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
    namespace: &str,
    pod_name: &str,
) -> anyhow::Result<u64> {
    let query = format!(
        "sum(increase(gpu_pruner_scale_successes_total{{namespace=\"{namespace}\",pod=\"{pod_name}\"}}[{window}]))"
    );
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

    match query_scale_downs_in_window(
        &state.prom_client,
        &window,
        &state.namespace,
        &state.pod_name,
    )
    .await
    {
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

async fn proxy_to_prometheus(http_client: &reqwest::Client, target: &str) -> Response {
    match http_client.get(target).send().await {
        Ok(upstream) => {
            let status =
                StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
            match upstream.bytes().await {
                Ok(bytes) => {
                    let mut builder = Response::builder().status(status);
                    if let Some(ct) = content_type {
                        builder = builder.header(header::CONTENT_TYPE, ct);
                    }
                    builder.body(Body::from(bytes)).unwrap_or_else(|e| {
                        tracing::error!("Failed to build Prometheus proxy response: {e}");
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    })
                }
                Err(e) => {
                    tracing::warn!("Failed to read Prometheus proxy body: {e}");
                    (
                        StatusCode::BAD_GATEWAY,
                        Json(serde_json::json!({
                            "status": "error",
                            "error": "failed to read Prometheus response",
                        })),
                    )
                        .into_response()
                }
            }
        }
        Err(e) => {
            tracing::warn!("Prometheus proxy request failed: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "status": "error",
                    "error": "failed to reach Prometheus",
                })),
            )
                .into_response()
        }
    }
}

pub async fn prom_cluster_proxy_handler(
    State(state): State<AppState>,
    Path(cluster): Path<String>,
    OriginalUri(uri): OriginalUri,
) -> Response {
    prom_cluster_proxy(state, cluster, uri).await
}

pub async fn prom_cluster_proxy_handler_rest(
    State(state): State<AppState>,
    Path((cluster, _rest)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
) -> Response {
    prom_cluster_proxy(state, cluster, uri).await
}

async fn prom_cluster_proxy(state: AppState, cluster: String, uri: axum::http::Uri) -> Response {
    let config = match state.clusters.get(&cluster) {
        Some(c) => c,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "status": "error",
                    "error": format!("unknown cluster: {cluster}"),
                })),
            )
                .into_response();
        }
    };

    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let prefix = format!("/prom/{cluster}");
    let stripped = path_and_query.strip_prefix(&prefix).unwrap_or("/");
    let target = format!(
        "{}{}",
        config.prometheus_url.trim_end_matches('/'),
        if stripped.is_empty() { "/" } else { stripped }
    );
    proxy_to_prometheus(&config.http_client, &target).await
}

#[derive(Debug, Serialize)]
pub struct ClusterInfo {
    pub name: String,
    pub honor_labels: bool,
}

pub async fn clusters_handler(State(state): State<AppState>) -> Json<Vec<ClusterInfo>> {
    let mut clusters: Vec<ClusterInfo> = state
        .clusters
        .iter()
        .map(|(name, config)| ClusterInfo {
            name: name.clone(),
            honor_labels: config.honor_labels,
        })
        .collect();
    clusters.sort_by(|a, b| a.name.cmp(&b.name));
    Json(clusters)
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
