# Idle GPU Time by Deployment Query

## Quick Start

This query identifies **which Deployments are wasting the most GPU allocation time** by showing deployments with GPUs at 0% utilization.

### View in Grafana Dashboard

1. Import `gpu-dashboard.json` into Grafana
2. Scroll to the **"Idle GPU Time by Deployment (30m)"** panel
3. See deployments sorted by idle GPU-hours (worst offenders first)

### Run in Prometheus CLI

```bash
# Port-forward to Prometheus
kubectl port-forward -n <prometheus-namespace> svc/prometheus 9090:9090

# Extract the query from the dashboard
jq -r '.panels[] | select(.title == "Idle GPU Time by Deployment (30m)") | .targets[0].expr' gpu-dashboard.json

# Copy the output and paste into http://localhost:9090
```

## What It Shows

**Idle GPU-Hours** = Number of GPUs at 0% utilization × time window in hours

Example output:

| Deployment          | Namespace    | Idle GPU-Hours |
|---------------------|--------------|----------------|
| llama-70b-inference | ml-team      | 12.5           |
| stable-diffusion    | ml-team      | 8.0            |
| jupyter-notebook    | data-science | 4.5            |

**Interpretation:**
- `llama-70b-inference` has 12.5 idle GPU-hours = 25 GPUs idle for 30 minutes (or 12.5 GPUs idle for 1 hour)
- This deployment is the #1 priority for investigation/scaling

## Prerequisites

✅ **DCGM exporter** on GPU nodes  
✅ **kube-state-metrics** with pod labels enabled: `--metric-labels-allowlist=pods=[*]`  
✅ **Pod labels**: Deployments must create pods with `app=<deployment-name>` label  

### Check Prerequisites

```bash
# 1. Verify DCGM metrics exist
kubectl port-forward -n <prometheus-ns> svc/prometheus 9090:9090
# Visit http://localhost:9090 and query: DCGM_FI_PROF_GR_ENGINE_ACTIVE

# 2. Verify kube_pod_labels metric exists
# Query: kube_pod_labels{namespace="<your-namespace>"}
# Should show label_app and other pod labels

# 3. Verify pods have app label
kubectl get pods -n <namespace> --show-labels | grep "app="
```

## How It Works

1. **Detects idle GPUs**: Uses `DCGM_FI_PROF_GR_ENGINE_ACTIVE` metric with `max_over_time([30m])` to find GPUs where maximum utilization over 30 minutes equals 0
2. **Maps to deployments**: Joins with `kube_pod_labels` to extract deployment name from pod `app` label
3. **Aggregates**: Sums idle GPU count by deployment
4. **Calculates hours**: Multiplies by 0.5 (30-minute window = 0.5 hours)
5. **Sorts**: Shows worst offenders first

## Customization

### Change Time Window

To use a 1-hour window instead of 30 minutes:

1. Edit `gpu-dashboard.json`
2. Find the panel at line 218-273
3. In the `expr` field (line 225), change:
   - `[30m]` → `[1h]` (in both places)
   - `* 0.5` → `* 1.0` (at the end)
4. Update panel title from "(30m)" to "(1h)"

### Filter by Namespace

Add a namespace filter to the query:

```promql
# Add this to the innermost query
DCGM_FI_PROF_GR_ENGINE_ACTIVE{exported_pod != "", exported_namespace =~ "ml-.*"}
```

### Use Different Label

If your deployments use `app.kubernetes.io/name` instead of `app`:

In the dashboard JSON, change:
- `label_app` → `label_app_kubernetes_io_name` (throughout the query)

## Troubleshooting

### Query returns no results

**Possible causes:**

1. **No idle GPUs** (good!) - Verify with: `count(max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE[30m]) == 0)`
2. **Missing `kube_pod_labels`** - Enable in kube-state-metrics: `--metric-labels-allowlist=pods=[*]`
3. **Pods missing `app` label** - Check labels: `kubectl get pods -n <ns> --show-labels`

### Query returns pod names instead of deployment names

The `kube_pod_labels` join is failing. Use the fallback query (see `DASHBOARD.md`) which groups by pod name, then manually infer deployment from pod name prefix.

### Query is slow

For large clusters:
- Add namespace filter (see Customization above)
- Reduce Prometheus scrape frequency for DCGM metrics
- Create a Prometheus recording rule (see `DASHBOARD.md` for example)

## Where is the Query Stored?

**Single source of truth:** `gpu-dashboard.json` (lines 218-273)

The query is embedded in the Grafana panel configuration. To extract it programmatically:

```bash
jq -r '.panels[] | select(.title == "Idle GPU Time by Deployment (30m)") | .targets[0].expr' gpu-dashboard.json
```

## Related Queries

All queries are in `gpu-dashboard.json`. Extract any query by panel title:

```bash
# Total idle GPUs
jq -r '.panels[] | select(.title == "Idle GPUs (30m)") | .targets[0].expr' gpu-dashboard.json

# GPU utilization by node
jq -r '.panels[] | select(.title == "GPU Utilization Heatmap by Node") | .targets[0].expr' gpu-dashboard.json

# GPU allocation by namespace
jq -r '.panels[] | select(.title == "GPU Allocation Leaderboard (by Namespace)") | .targets[0].expr' gpu-dashboard.json
```

## Documentation

- **Full dashboard setup**: See [DASHBOARD.md](DASHBOARD.md)
- **gpu-pruner configuration**: See [README.md](README.md)
- **Query details**: See the "Using Dashboard Queries in Prometheus" section in [DASHBOARD.md](DASHBOARD.md)
