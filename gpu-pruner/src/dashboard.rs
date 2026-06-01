use axum::{
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{Meta, ScaleKind};

#[derive(Clone, Debug, serde::Serialize)]
pub struct WorkloadInfo {
    pub name: String,
    pub namespace: String,
    pub kind: String,
    pub gpu_model: Option<String>,
    pub idle_duration: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct DashboardState {
    pub idle_workloads: Vec<WorkloadInfo>,
    pub total_idle_gpus: usize,
    pub total_pods_checked: usize,
    pub last_update: String,
}

impl Default for DashboardState {
    fn default() -> Self {
        Self {
            idle_workloads: Vec::new(),
            total_idle_gpus: 0,
            total_pods_checked: 0,
            last_update: chrono::Utc::now().to_rfc3339(),
        }
    }
}

pub type SharedDashboardState = Arc<RwLock<DashboardState>>;

pub async fn update_dashboard_state(
    state: SharedDashboardState,
    idle_workloads: Vec<ScaleKind>,
    total_pods: usize,
) {
    let workloads: Vec<WorkloadInfo> = idle_workloads
        .iter()
        .map(|w| WorkloadInfo {
            name: w.name(),
            namespace: w.namespace().unwrap_or_default(),
            kind: w.kind(),
            gpu_model: None,
            idle_duration: None,
        })
        .collect();

    let mut state = state.write().await;
    state.idle_workloads = workloads;
    state.total_idle_gpus = idle_workloads.len();
    state.total_pods_checked = total_pods;
    state.last_update = chrono::Utc::now().to_rfc3339();
}

async fn dashboard_html() -> impl IntoResponse {
    Html(include_str!("dashboard.html"))
}

async fn api_status(State(state): State<SharedDashboardState>) -> impl IntoResponse {
    let state = state.read().await;
    Json(state.clone())
}

pub fn create_router(state: SharedDashboardState) -> Router {
    Router::new()
        .route("/", get(dashboard_html))
        .route("/api/status", get(api_status))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn run_server(state: SharedDashboardState, port: u16) -> anyhow::Result<()> {
    let app = create_router(state);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));

    tracing::info!("Dashboard server starting on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
