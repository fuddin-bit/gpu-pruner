# GPU Pruner Dashboard

The GPU Pruner Dashboard provides a real-time web interface to monitor GPU workloads in your Kubernetes cluster.

## Features

- **Real-time Monitoring**: View current GPU workload status with automatic updates every 10 seconds
- **Idle Workload Detection**: See which workloads are consuming GPUs without actively using them
- **Resource Statistics**: Track total pods checked, idle workloads, and wasted GPU resources
- **Clean UI**: Modern, responsive web interface accessible from any browser

## Enabling the Dashboard

The dashboard is an optional feature that can be enabled by passing the `--dashboard-port` flag to the gpu-pruner binary:

```bash
gpu-pruner --dashboard-port=8080 \
  -d \
  --run-mode=scale-down \
  --prometheus-url=http://prometheus-k8s.openshift-monitoring.svc:9090
```

## Kubernetes Deployment

### Deploy the Service

The dashboard requires a Kubernetes Service to be accessible:

```bash
kubectl apply -f gpu-pruner/hack/service.yaml
```

### Deploy the Route (OpenShift)

For external access on OpenShift clusters:

```bash
kubectl apply -f gpu-pruner/hack/route.yaml
```

The route will be available at: `https://gpu-pruner-dashboard-gpu-pruner-system.<cluster-domain>`

### Update the Deployment

The deployment manifest in `gpu-pruner/hack/deployment.yaml` has been updated to include:
- The `--dashboard-port=8080` argument
- Port configuration for the dashboard
- Health check endpoints (optional)

Apply the updated deployment:

```bash
kubectl apply -f gpu-pruner/hack/deployment.yaml
```

## Accessing the Dashboard

### Local Port Forward

For local testing or development:

```bash
kubectl port-forward -n gpu-pruner-system deployment/gpu-pruner 8080:8080
```

Then open your browser to: http://localhost:8080

### Via OpenShift Route

Once the route is deployed, you can access the dashboard at the route URL:

```bash
# Get the route URL
kubectl get route -n gpu-pruner-system gpu-pruner-dashboard -o jsonpath='{.spec.host}'
```

## Dashboard Data

The dashboard displays:

1. **Total Pods Checked**: Number of pods queried in the last scan
2. **Idle Workloads**: Number of workloads identified as idle
3. **Wasted GPU Resources**: Count of GPU resources being wasted by idle workloads
4. **Idle Workload Table**: Detailed list showing:
   - Namespace
   - Workload name
   - Resource type (Deployment, StatefulSet, etc.)

## Grafana Dashboard

A Grafana dashboard is included in `gpu-dashboard.json` for more detailed GPU monitoring and analytics. Import it into your Grafana instance to visualize:

- **Cluster GPU Overview**: Total GPUs; VRAM partition (FB>0 + FB=0 = Total); engine partition (idle 30m + active 30m = Total)
- **GPU Utilization Heatmap**: GPU utilization per node over time
- **Running GPU Workloads**: All pods with GPU requests
- **Idle GPU Workloads**: GPUs with zero compute activity for 30+ minutes
- **Idle GPU Time by Deployment**: Deployments producing the most allocated GPU idle time (see [Prometheus Queries](#prometheus-queries) below)
- **GPU Allocation Leaderboard**: Total GPU requests per namespace
- **GPU Health & DCGM**: Temperature, power, VRAM %, memory-copy util, XID errors, and optional DCGM profiling metrics

### Importing the Grafana Dashboard

1. Open Grafana and navigate to Dashboards → Import
2. Upload `gpu-dashboard.json` or copy-paste its contents
3. Select your Prometheus datasource
4. Click "Import"

The dashboard requires:
- Prometheus datasource with DCGM exporter metrics
- kube-state-metrics for pod resource information
- For deployment-level idle time analysis: kube-state-metrics with pod label metrics enabled

## Using Dashboard Queries in Prometheus

All queries used in the Grafana dashboard can be extracted and run directly in Prometheus for ad-hoc analysis.

### Extracting Queries from the Dashboard

To extract a query from `gpu-dashboard.json`:

```bash
# Extract the "Idle GPU Time by Deployment" query
jq -r '.panels[] | select(.title == "Idle GPU Time by Deployment (30m)") | .targets[0].expr' gpu-dashboard.json

# Extract all queries with their panel titles
jq -r '.panels[] | select(.targets) | "\(.title):\n\(.targets[0].expr)\n"' gpu-dashboard.json
```

### Cluster GPU overview partitions

The overview row uses **two independent partitions** of the same total. Each pair sums to **Total GPUs**; VRAM and engine counts are not opposites of each other (a GPU can have VRAM allocated while engine-idle).

| Panel | PromQL |
|-------|--------|
| Total GPUs | `count(DCGM_FI_DEV_GPU_UTIL)` |
| VRAM allocated (FB>0) | `count(DCGM_FI_DEV_FB_USED > 0)` |
| VRAM free (FB=0) | `count(DCGM_FI_DEV_FB_USED == 0)` |
| Engine idle (30m) | `count(max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE[30m]) == 0)` |
| Engine active (30m) | `count(max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE[30m]) > 0)` |

Equivalently: **Engine active** = Total − Engine idle, and **VRAM free** = Total − VRAM allocated, when the same DCGM time series are counted.

### GPU Health & DCGM

Panels in the **GPU Health & DCGM** row use additional dcgm-exporter counters. Profiling panels show no data unless your exporter exposes `DCGM_FI_PROF_*` metrics (same requirement as `DCGM_FI_PROF_GR_ENGINE_ACTIVE`).

| Panel | PromQL |
|-------|--------|
| Peak GPU temperature | `max(DCGM_FI_DEV_GPU_TEMP)` |
| Peak power (W) | `max(DCGM_FI_DEV_POWER_USAGE)` |
| XID errors (total) | `sum(DCGM_FI_DEV_XID_ERRORS)` |
| GPU temperature by node | `avg by (Hostname) (DCGM_FI_DEV_GPU_TEMP)` |
| Power draw by node | `sum by (Hostname) (DCGM_FI_DEV_POWER_USAGE)` |
| VRAM utilization % | `100 * avg by (Hostname, gpu) (DCGM_FI_DEV_FB_USED / DCGM_FI_DEV_FB_TOTAL)` |
| Memory copy utilization | `avg by (Hostname) (DCGM_FI_DEV_MEM_COPY_UTIL)` |
| XID errors (1h increase) | `sum by (Hostname, gpu) (increase(DCGM_FI_DEV_XID_ERRORS[1h]))` |
| SM active by node | `avg by (Hostname) (DCGM_FI_PROF_SM_ACTIVE)` |
| Tensor pipe active by node | `avg by (Hostname) (DCGM_FI_PROF_PIPE_TENSOR_ACTIVE)` |
| DRAM active by node | `avg by (Hostname) (DCGM_FI_PROF_DRAM_ACTIVE)` |

Note: gpu-pruner idle detection uses [`query.promql.j2`](gpu-pruner/src/query.promql.j2) at runtime; Grafana idle panels use related but simpler PromQL for visualization.

### Idle GPU Time by Deployment Query

This query identifies which Kubernetes Deployments are producing the most allocated GPU idle time while GPU utilization is at 0%.

**What it measures:**
- Idle GPU-hours per deployment over a 30-minute window
- Deployments are sorted by total idle time (worst offenders first)

**How it works:**
1. Detects GPUs with 0% utilization over 30 minutes using DCGM metrics (`DCGM_FI_PROF_GR_ENGINE_ACTIVE` or fallback to `DCGM_FI_DEV_GPU_UTIL`)
2. Counts idle GPUs per pod (multi-GPU pods contribute multiple GPUs)
3. Joins with `kube_pod_labels` to extract deployment name from pod `app` label
4. Aggregates by deployment and calculates idle GPU-hours (GPU count × 0.5 hours)
5. Sorts descending to show worst offenders first

**To run in Prometheus:**

1. Port-forward to Prometheus:
   ```bash
   kubectl port-forward -n <prometheus-namespace> svc/prometheus 9090:9090
   ```

2. Extract and copy the query:
   ```bash
   jq -r '.panels[] | select(.title == "Idle GPU Time by Deployment (30m)") | .targets[0].expr' gpu-dashboard.json
   ```

3. Paste into Prometheus UI at http://localhost:9090 and click "Execute"

**Example output:**

| label_app (Deployment) | namespace     | Value (Idle GPU-Hours) |
|------------------------|---------------|------------------------|
| llama-70b-inference    | ml-team       | 12.5                   |
| stable-diffusion       | ml-team       | 8.0                    |
| jupyter-notebook       | data-science  | 4.5                    |

**Prerequisites:**
- DCGM exporter running on GPU nodes
- kube-state-metrics with pod label metrics enabled (`--metric-labels-allowlist=pods=[*]`)
- Pods labeled with `app=<deployment-name>` (standard Kubernetes convention)

**Limitations:**
- Requires `kube_pod_labels` metric from kube-state-metrics
- Depends on pods having the `app` label set to the deployment name
- Does not traverse ReplicaSet ownership (relies on label convention)
- Only tracks Deployments (not StatefulSets, InferenceServices, etc.)
- 30-minute window is fixed in this query

**Fallback query** (if `kube_pod_labels` is unavailable):
```promql
sort_desc(
  sum by (exported_pod, exported_namespace) (
    (
      max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{exported_pod != ""}[30m])
      or
      max_over_time(DCGM_FI_DEV_GPU_UTIL{exported_pod != ""}[30m]) / 100
    ) == 0
  ) * 0.5
)
```

This shows idle GPU-hours by pod name instead. Deployment names can be manually inferred from pod name prefixes (e.g., `llama-inference-7b56f9-abc` → deployment `llama-inference`).

## API Endpoint

The dashboard also exposes a REST API endpoint for programmatic access:

### GET `/api/status`

Returns JSON with current dashboard state:

```json
{
  "idle_workloads": [
    {
      "name": "my-deployment",
      "namespace": "user-dev",
      "kind": "Deployment",
      "gpu_model": null,
      "idle_duration": null
    }
  ],
  "total_idle_gpus": 4,
  "total_pods_checked": 32,
  "last_update": "2026-06-01T17:57:53.364820Z"
}
```

## Architecture

The dashboard consists of:

1. **Backend API**: Rust-based Axum web server
   - Serves static HTML/CSS/JS
   - Provides REST API at `/api/status`
   - Updates state from gpu-pruner's query loop

2. **Frontend**: Single-page application
   - Pure HTML/CSS/JavaScript (no build step required)
   - Auto-refreshes every 10 seconds
   - Responsive design for mobile and desktop

## Configuration

### Port Configuration

Change the dashboard port via the `--dashboard-port` flag:

```bash
gpu-pruner --dashboard-port=9090 ...
```

### CORS

The dashboard includes CORS headers to allow API access from other origins. This is configured in the `tower_http::cors::CorsLayer`.

## Troubleshooting

### Dashboard not accessible

1. Check if the pod is running:
   ```bash
   kubectl get pods -n gpu-pruner-system
   ```

2. Check logs for dashboard startup:
   ```bash
   kubectl logs -n gpu-pruner-system deployment/gpu-pruner | grep -i dashboard
   ```

3. Verify the service is configured:
   ```bash
   kubectl get svc -n gpu-pruner-system gpu-pruner-dashboard
   ```

### No data showing

1. Verify prometheus connection is working (check logs)
2. Ensure DCGM metrics are being collected
3. Check that pods with GPUs exist in the cluster

### API returns empty data

This is normal if:
- No GPUs are idle (all resources being used efficiently)
- The first query hasn't completed yet (wait ~3 minutes after startup)

## Development

### Local Testing

Run locally with:

```bash
cargo run -- \
  --dashboard-port=8080 \
  -d \
  --prometheus-url=http://localhost:9090 \
  --run-mode=dry-run
```

### Building with Dashboard

The dashboard is included by default when building:

```bash
cargo build --release
```

Or with OpenTelemetry:

```bash
cargo build --release --features otel
```
