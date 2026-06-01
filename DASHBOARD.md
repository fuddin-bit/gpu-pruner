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
