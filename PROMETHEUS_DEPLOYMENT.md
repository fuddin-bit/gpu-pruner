# Deploying Prometheus Metrics Support

This guide explains how to deploy the Prometheus metrics functionality added in commit `726e384`.

## Overview

The new metrics support adds:
- `/metrics` endpoint on port 8080 (alongside dashboard)
- 8 Prometheus metrics (6 counters, 2 gauges)
- ServiceMonitor for Prometheus Operator scraping
- Service resource for endpoint discovery

## Prerequisites

- Cluster admin access or permissions to:
  - Create/patch Deployments in `gpu-pruner-system` namespace
  - Create Services in `gpu-pruner-system` namespace
  - Create ServiceMonitors in `gpu-pruner-system` namespace
- Prometheus Operator installed (for ServiceMonitor support)

## Deployment Steps

### Step 1: Wait for CI Build (Automatic)

The GitHub Actions workflow automatically builds and pushes new images when you push to `main`:

- **Image with OTEL**: `ghcr.io/fuddin-bit/gpu-pruner:726e384-otel`
- **Image without OTEL**: `ghcr.io/fuddin-bit/gpu-pruner:726e384`

Check build status at:
https://github.com/fuddin-bit/gpu-pruner/actions

Wait for the `CI` workflow to complete (~5-10 minutes).

### Step 2: Update Deployment Image

Once the CI build completes, update the deployment to use the new image:

```bash
# Using the OTEL-enabled image (recommended)
kubectl set image deployment/gpu-pruner \
  container=ghcr.io/fuddin-bit/gpu-pruner:726e384-otel \
  -n gpu-pruner-system

# OR using the standard image
kubectl set image deployment/gpu-pruner \
  container=ghcr.io/fuddin-bit/gpu-pruner:726e384 \
  -n gpu-pruner-system
```

Verify the rollout:
```bash
kubectl rollout status deployment/gpu-pruner -n gpu-pruner-system
kubectl get pods -n gpu-pruner-system
```

### Step 3: Create Service

The Service exposes port 8080 for both dashboard and metrics:

```bash
kubectl apply -f gpu-pruner/hack/service.yaml
```

**service.yaml:**
```yaml
kind: Service
apiVersion: v1
metadata:
  name: gpu-pruner-dashboard
  namespace: gpu-pruner-system
  labels:
    app: gpu-pruner
spec:
  type: ClusterIP
  selector:
    app: gpu-pruner
  ports:
    - name: http
      protocol: TCP
      port: 8080
      targetPort: 8080
```

Verify:
```bash
kubectl get svc -n gpu-pruner-system
```

### Step 4: Create ServiceMonitor

The ServiceMonitor tells Prometheus to scrape the `/metrics` endpoint:

```bash
kubectl apply -f gpu-pruner/hack/servicemonitor.yaml
```

**servicemonitor.yaml:**
```yaml
apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: gpu-pruner
  namespace: gpu-pruner-system
  labels:
    app: gpu-pruner
spec:
  selector:
    matchLabels:
      app: gpu-pruner
  endpoints:
    - port: http
      path: /metrics
      interval: 30s
      scrapeTimeout: 10s
```

Verify:
```bash
kubectl get servicemonitor -n gpu-pruner-system
kubectl describe servicemonitor gpu-pruner -n gpu-pruner-system
```

## Verification

### Test /metrics Endpoint

Port-forward to the pod and curl the metrics endpoint:

```bash
kubectl port-forward -n gpu-pruner-system deployment/gpu-pruner 8080:8080
```

In another terminal:
```bash
curl http://localhost:8080/metrics
```

Expected output:
```
# HELP gpu_pruner_idle_gpus Current number of idle GPUs detected in last check
# TYPE gpu_pruner_idle_gpus gauge
gpu_pruner_idle_gpus 0
# HELP gpu_pruner_pods_checked_total Total number of pods analyzed in last query
# TYPE gpu_pruner_pods_checked_total gauge
gpu_pruner_pods_checked_total 0
# HELP gpu_pruner_query_candidates_total Total number of idle GPU candidates found across all queries
# TYPE gpu_pruner_query_candidates_total counter
gpu_pruner_query_candidates_total 0
# HELP gpu_pruner_query_failures_total Total number of failed Prometheus queries
# TYPE gpu_pruner_query_failures_total counter
gpu_pruner_query_failures_total 0
# HELP gpu_pruner_query_shutdown_events_total Total number of shutdown events detected
# TYPE gpu_pruner_query_shutdown_events_total counter
gpu_pruner_query_shutdown_events_total 0
# HELP gpu_pruner_query_successes_total Total number of successful Prometheus queries
# TYPE gpu_pruner_query_successes_total counter
gpu_pruner_query_successes_total 5
# HELP gpu_pruner_scale_failures_total Total number of failed scale operations
# TYPE gpu_pruner_scale_failures_total counter
gpu_pruner_scale_failures_total 0
# HELP gpu_pruner_scale_successes_total Total number of successful scale operations
# TYPE gpu_pruner_scale_successes_total counter
gpu_pruner_scale_successes_total 2
```

### Check Prometheus Targets

Access Prometheus UI and verify the target is being scraped:

```bash
# Port-forward to Prometheus (adjust namespace/service as needed)
kubectl port-forward -n openshift-monitoring svc/prometheus-k8s 9090:9090
```

Open http://localhost:9090/targets and look for:
- Target: `gpu-pruner-system/gpu-pruner`
- Status: **UP**

### Query Metrics in Prometheus

Test queries in Prometheus UI (http://localhost:9090/graph):

```promql
# Query success rate
rate(gpu_pruner_query_successes_total[5m])

# Current idle GPUs
gpu_pruner_idle_gpus

# Total scale operations
sum(gpu_pruner_scale_successes_total)

# Error rate
rate(gpu_pruner_query_failures_total[5m]) + rate(gpu_pruner_scale_failures_total[5m])
```

## Metrics Reference

| Metric Name | Type | Description |
|-------------|------|-------------|
| `gpu_pruner_query_successes_total` | Counter | Successful Prometheus queries |
| `gpu_pruner_query_failures_total` | Counter | Failed Prometheus queries |
| `gpu_pruner_query_candidates_total` | Counter | Total idle GPU candidates found across all queries |
| `gpu_pruner_query_shutdown_events_total` | Counter | Shutdown events detected |
| `gpu_pruner_scale_successes_total` | Counter | Successful scale operations |
| `gpu_pruner_scale_failures_total` | Counter | Failed scale operations |
| `gpu_pruner_idle_gpus` | Gauge | Current number of idle GPUs detected in last check |
| `gpu_pruner_pods_checked_total` | Gauge | Total pods analyzed in last query |

## Troubleshooting

### ServiceMonitor not created

**Error:** `servicemonitors.monitoring.coreos.com is forbidden`

**Solution:** You need permissions to create ServiceMonitors in the namespace:
```bash
# Ask cluster admin to run:
kubectl create rolebinding <your-name>-servicemonitor \
  --clusterrole=monitoring-edit \
  --user=<your-email> \
  -n gpu-pruner-system
```

### Metrics endpoint returns 404

**Issue:** Deployment not updated to new image

**Solution:** Check the running image:
```bash
kubectl get deployment gpu-pruner -n gpu-pruner-system -o jsonpath='{.spec.template.spec.containers[0].image}'
```

Should be `ghcr.io/fuddin-bit/gpu-pruner:726e384-otel` or later.

### Prometheus not scraping

**Check:**
1. ServiceMonitor exists: `kubectl get servicemonitor -n gpu-pruner-system`
2. Service exists: `kubectl get svc gpu-pruner-dashboard -n gpu-pruner-system`
3. Service selector matches deployment labels
4. Prometheus has RBAC to scrape the namespace

### Metrics not incrementing

**Check pod logs:**
```bash
kubectl logs -f deployment/gpu-pruner -n gpu-pruner-system
```

Look for:
- `Query succeeded` - should increment `query_successes_total`
- `Returned candidates` - should increment `query_candidates_total`
- `Scaled Resource` - should increment `scale_successes_total`

## Alternative: Without ServiceMonitor

If you don't have Prometheus Operator, you can scrape using pod annotations:

```bash
kubectl patch deployment gpu-pruner -n gpu-pruner-system --type=json -p='[
  {
    "op": "add",
    "path": "/spec/template/metadata/annotations",
    "value": {
      "prometheus.io/scrape": "true",
      "prometheus.io/port": "8080",
      "prometheus.io/path": "/metrics"
    }
  }
]'
```

Or manually configure Prometheus scrape_configs - see `gpu-pruner/hack/prometheus-scrape-config.yaml` for examples.

## Next Steps

1. Add Grafana dashboards for gpu-pruner metrics
2. Set up alerts (e.g., high failure rate, no successful queries in 10m)
3. Consider adding histogram metrics for query latency
4. Add labels to metrics (namespace, run_mode, etc.) for better filtering

## References

- ServiceMonitor API: https://github.com/prometheus-operator/prometheus-operator/blob/main/Documentation/api.md#servicemonitor
- Prometheus metrics best practices: https://prometheus.io/docs/practices/naming/
- GPU pruner dashboard: See `gpu-dashboard.json` for Grafana dashboard with DCGM metrics
