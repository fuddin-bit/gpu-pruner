# Deploying Grafana with GPU Dashboard using Helm

This guide explains how to deploy a standalone Grafana instance with the GPU Pruner dashboard pre-configured using the official Grafana Helm chart.

## Table of Contents

- [Overview](#overview)
- [Prerequisites](#prerequisites)
- [Quick Start](#quick-start)
- [Installation](#installation)
- [Configuration](#configuration)
- [Validation](#validation)
- [Troubleshooting](#troubleshooting)
- [Customization](#customization)
- [Security Considerations](#security-considerations)

## Overview

The GPU Pruner project includes a comprehensive Grafana dashboard (`gpu-dashboard.json`) that visualizes:

- **Cluster GPU Overview**: Total GPUs, VRAM allocation, engine activity
- **GPU Utilization Heatmap**: Per-node GPU utilization over time
- **Running GPU Workloads**: All pods with GPU requests
- **Idle GPU Workloads**: GPUs with zero compute activity for 30+ minutes
- **Idle GPU Time by Deployment**: Deployments producing the most allocated GPU idle time
- **GPU Allocation Leaderboard**: Total GPU requests per namespace

This deployment uses the **official Grafana Helm chart** (`grafana/grafana`) to create a dedicated Grafana instance for GPU monitoring, separate from the gpu-pruner deployment.

## Prerequisites

### Required Components

Before deploying Grafana, ensure these components are running in your Kubernetes cluster:

1. **Prometheus** - Collecting metrics from DCGM exporter and kube-state-metrics
2. **DCGM Exporter** - DaemonSet on GPU nodes exposing NVIDIA GPU metrics
3. **kube-state-metrics** - With pod labels enabled for deployment-level analysis

### Required Tools

- **Helm 3.x** - [Install Helm](https://helm.sh/docs/intro/install/)
- **kubectl** - Configured with cluster access
- **Kubernetes 1.19+** - With GPU nodes

### Validation Commands

Verify prerequisites before proceeding:

```bash
# Check Prometheus is accessible
kubectl get svc -A | grep prometheus

# Verify DCGM exporter pods on GPU nodes
kubectl get pods -A | grep dcgm

# Check kube-state-metrics
kubectl get deploy -A | grep kube-state-metrics

# Test Prometheus query (requires port-forward)
kubectl port-forward -n <prometheus-namespace> svc/<prometheus-service> 9090:9090 &
curl -s 'http://localhost:9090/api/v1/query?query=DCGM_FI_DEV_GPU_UTIL' | jq '.data.result | length'
# Should return number of GPUs

# Verify kube_pod_labels metric exists
curl -s 'http://localhost:9090/api/v1/query?query=kube_pod_labels' | jq '.data.result | length'
# Should return > 0
```

### kube-state-metrics Configuration

For the "Idle GPU Time by Deployment" panel to work, kube-state-metrics **must** be configured with:

```yaml
--metric-labels-allowlist=pods=[*]
```

This enables the `kube_pod_labels` metric. Verify with:

```bash
kubectl get deploy kube-state-metrics -n <namespace> -o yaml | grep metric-labels-allowlist
```

If missing, update the deployment:

```bash
kubectl set env deployment/kube-state-metrics \
  -n <namespace> \
  KUBE_STATE_METRICS_ARGS='--metric-labels-allowlist=pods=[*]'
```

## Quick Start

For a standard Kubernetes cluster with Prometheus Operator:

```bash
# Add Grafana Helm repository
helm repo add grafana https://grafana.github.io/helm-charts
helm repo update

# Install Grafana with GPU dashboard
helm install gpu-grafana grafana/grafana \
  -f helm/grafana-values.yaml \
  -f helm/grafana-values-vanilla-k8s.yaml \
  --set adminPassword='YOUR_SECURE_PASSWORD' \
  --set ingress.hosts[0]='grafana-gpu.example.com' \
  --set datasources."datasources\.yaml".datasources[0].url='http://prometheus-k8s.monitoring.svc.cluster.local:9090' \
  -n monitoring --create-namespace

# Get admin password (if not set above)
kubectl get secret -n monitoring gpu-grafana -o jsonpath="{.data.admin-password}" | base64 --decode ; echo

# Access Grafana (port-forward)
kubectl port-forward -n monitoring svc/gpu-grafana 3000:3000
```

Open http://localhost:3000 and login with `admin` / password from above.

## Installation

### Step 1: Add Grafana Helm Repository

```bash
helm repo add grafana https://grafana.github.io/helm-charts
helm repo update
```

### Step 2: Choose Your Environment

Select the appropriate values file for your Kubernetes environment:

#### Option A: OpenShift

```bash
helm install gpu-grafana grafana/grafana \
  -f helm/grafana-values.yaml \
  -f helm/grafana-values-openshift.yaml \
  --set adminPassword='YOUR_SECURE_PASSWORD' \
  -n monitoring --create-namespace
```

**Note**: For OpenShift, you'll need to create a ClusterRoleBinding for Prometheus access:

```bash
oc adm policy add-cluster-role-to-user cluster-monitoring-view -z grafana -n monitoring
```

And set the Prometheus token in the datasource configuration (see [Configuration](#configuration)).

#### Option B: Vanilla Kubernetes

```bash
helm install gpu-grafana grafana/grafana \
  -f helm/grafana-values.yaml \
  -f helm/grafana-values-vanilla-k8s.yaml \
  --set adminPassword='YOUR_SECURE_PASSWORD' \
  --set ingress.hosts[0]='grafana-gpu.example.com' \
  --set datasources."datasources\.yaml".datasources[0].url='http://your-prometheus:9090' \
  -n monitoring --create-namespace
```

Update the Ingress hostname and Prometheus URL to match your environment.

### Step 3: Verify Deployment

```bash
# Check pod status
kubectl get pods -n monitoring -l app.kubernetes.io/name=grafana

# Check logs
kubectl logs -n monitoring -l app.kubernetes.io/name=grafana

# Verify service
kubectl get svc -n monitoring gpu-grafana
```

Expected output:
```
NAME          READY   STATUS    RESTARTS   AGE
gpu-grafana   1/1     Running   0          2m
```

### Step 4: Access Grafana

#### Via Port Forward (for testing)

```bash
kubectl port-forward -n monitoring svc/gpu-grafana 3000:3000
```

Open http://localhost:3000

#### Via Ingress (production)

Access via the configured hostname: https://grafana-gpu.example.com

#### Via OpenShift Route (OpenShift only)

Get the route URL:

```bash
oc get route -n monitoring grafana -o jsonpath='{.spec.host}'
```

### Step 5: Login

- **Username**: `admin`
- **Password**: The password you set via `--set adminPassword` or retrieve it:

```bash
kubectl get secret -n monitoring gpu-grafana -o jsonpath="{.data.admin-password}" | base64 --decode ; echo
```

## Configuration

### Prometheus Datasource URL

The most critical configuration is the Prometheus datasource URL. Update it to match your cluster:

**In `helm/grafana-values.yaml`** or via `--set`:

```yaml
datasources:
  datasources.yaml:
    datasources:
      - url: http://YOUR_PROMETHEUS_SERVICE:9090
```

Common patterns:

| Environment | Prometheus URL |
|-------------|----------------|
| OpenShift | `http://thanos-querier.openshift-monitoring.svc.cluster.local:9090` |
| Prometheus Operator | `http://prometheus-k8s.monitoring.svc.cluster.local:9090` |
| kube-prometheus-stack | `http://prometheus-kube-prometheus-prometheus.monitoring.svc.cluster.local:9090` |
| Custom | `http://prometheus.prometheus.svc.cluster.local:9090` |

### Datasource UID (Critical)

The dashboard has a **hardcoded datasource UID**: `PBFA97CFB590B2093`

**Do NOT change this UID** in the values file:

```yaml
datasources:
  datasources.yaml:
    datasources:
      - uid: PBFA97CFB590B2093  # Must match dashboard
```

If you need a different UID, you'll have to manually edit the dashboard after import (see [Troubleshooting](#datasource-uid-mismatch)).

### Admin Credentials

**Option 1: Set via Helm** (simple, not recommended for production)

```bash
--set adminPassword='YOUR_PASSWORD'
```

**Option 2: Use existing secret** (recommended for production)

Create a secret first:

```bash
kubectl create secret generic grafana-admin-secret \
  -n monitoring \
  --from-literal=admin-user=admin \
  --from-literal=admin-password='YOUR_SECURE_PASSWORD'
```

Update values:

```yaml
admin:
  existingSecret: grafana-admin-secret
  userKey: admin-user
  passwordKey: admin-password
```

### Ingress Configuration

For **Nginx Ingress**:

```yaml
ingress:
  enabled: true
  ingressClassName: nginx
  annotations:
    cert-manager.io/cluster-issuer: letsencrypt-prod
  hosts:
    - grafana-gpu.example.com
  tls:
    - secretName: grafana-tls
      hosts:
        - grafana-gpu.example.com
```

For **Traefik Ingress**:

```yaml
ingress:
  enabled: true
  ingressClassName: traefik
  annotations:
    cert-manager.io/cluster-issuer: letsencrypt-prod
    traefik.ingress.kubernetes.io/router.entrypoints: websecure
```

### OpenShift Route

For OpenShift (already configured in `grafana-values-openshift.yaml`):

```yaml
route:
  enabled: true
  host: grafana-gpu.apps.example.com
  tls:
    enabled: true
    termination: edge
```

Or create manually:

```bash
oc create route edge grafana \
  --service=gpu-grafana \
  --hostname=grafana-gpu.apps.example.com \
  -n monitoring
```

### Resource Limits

Adjust based on your cluster size and dashboard usage:

```yaml
resources:
  limits:
    cpu: 500m      # Increase for large clusters or many concurrent users
    memory: 512Mi  # Increase if dashboards are slow to load
  requests:
    cpu: 250m
    memory: 256Mi
```

### Persistence

Enable persistence to retain dashboard edits and datasource configurations:

```yaml
persistence:
  enabled: true
  size: 10Gi
  storageClassName: default  # Adjust for your cluster
```

## Validation

### 1. Verify Grafana Pod is Running

```bash
kubectl get pods -n monitoring -l app.kubernetes.io/name=grafana
```

Expected: `STATUS: Running`

### 2. Check Grafana Logs

```bash
kubectl logs -n monitoring -l app.kubernetes.io/name=grafana --tail=50
```

Look for:
- `HTTP Server Listen`
- No error messages about datasource or dashboard provisioning

### 3. Test Prometheus Datasource

Access Grafana UI → Configuration → Data Sources → Prometheus → Save & Test

Expected: **"Data source is working"** (green checkmark)

If it fails, verify:
- Prometheus URL is correct
- Network connectivity from Grafana pod to Prometheus service
- Prometheus is healthy: `kubectl get pods -n <prometheus-namespace>`

### 4. Verify Dashboard is Loaded

Grafana UI → Dashboards → GPU Monitoring → Waldorf GPU Usage & Idle Tracker

Or check via API:

```bash
kubectl port-forward -n monitoring svc/gpu-grafana 3000:3000 &
curl -s -u admin:YOUR_PASSWORD http://localhost:3000/api/search?query=gpu | jq .
```

Expected: JSON response with dashboard UID and title.

### 5. Validate Dashboard Panels

Check each panel shows data (not "No data"):

| Panel | Validation Query | Expected Result |
|-------|------------------|-----------------|
| Total GPUs | `count(DCGM_FI_DEV_GPU_UTIL)` | Number of GPUs in cluster |
| VRAM allocated | `count(DCGM_FI_DEV_FB_USED > 0)` | Number with VRAM in use |
| Engine idle | `count(max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE[30m]) == 0)` | Idle GPU count |
| Running GPU Workloads | `sum by (namespace, pod) (kube_pod_container_resource_requests{resource="nvidia_com_gpu"})` | List of pods with GPUs |
| Peak GPU temperature | `max(DCGM_FI_DEV_GPU_TEMP)` | Max die temp (°C) |
| Peak power | `max(DCGM_FI_DEV_POWER_USAGE)` | Max per-GPU watts |
| XID errors | `sum(DCGM_FI_DEV_XID_ERRORS)` | Should be 0 in healthy clusters |
| VRAM utilization % | `100 * avg by (Hostname, gpu) (DCGM_FI_DEV_FB_USED / DCGM_FI_DEV_FB_TOTAL)` | Per-GPU VRAM fill |
| SM active (profiling) | `avg by (Hostname) (DCGM_FI_PROF_SM_ACTIVE)` | No data if profiling disabled |

### 6. Test Prometheus Queries Manually

Port-forward to Prometheus:

```bash
kubectl port-forward -n <prometheus-namespace> svc/<prometheus-service> 9090:9090
```

Run test queries:

```bash
# Check DCGM metrics exist
curl -s 'http://localhost:9090/api/v1/query?query=DCGM_FI_DEV_GPU_UTIL' | jq '.data.result | length'

# GPU health metrics (GPU Health & DCGM dashboard row)
curl -s 'http://localhost:9090/api/v1/query?query=DCGM_FI_DEV_GPU_TEMP' | jq '.data.result | length'
curl -s 'http://localhost:9090/api/v1/query?query=DCGM_FI_DEV_POWER_USAGE' | jq '.data.result | length'
curl -s 'http://localhost:9090/api/v1/query?query=DCGM_FI_DEV_MEM_COPY_UTIL' | jq '.data.result | length'
curl -s 'http://localhost:9090/api/v1/query?query=DCGM_FI_DEV_XID_ERRORS' | jq '.data.result | length'

# Check kube-state-metrics
curl -s 'http://localhost:9090/api/v1/query?query=kube_pod_container_resource_requests{resource="nvidia_com_gpu"}' | jq '.data.result | length'

# Check pod labels (for deployment analysis)
curl -s 'http://localhost:9090/api/v1/query?query=kube_pod_labels' | jq '.data.result | length'
```

All should return `> 0` results.

## Troubleshooting

### Dashboard Shows "No Data"

**Cause**: Prometheus datasource not configured correctly or metrics not available.

**Solution**:

1. Verify Prometheus datasource connection:
   - Grafana UI → Configuration → Data Sources → Prometheus → Save & Test
   - Should show "Data source is working"

2. Check Prometheus has DCGM metrics:
   ```bash
   kubectl port-forward -n <prometheus-namespace> svc/<prometheus-service> 9090:9090 &
   curl -s 'http://localhost:9090/api/v1/query?query=DCGM_FI_DEV_GPU_UTIL' | jq .
   ```

3. Verify DCGM exporter is running:
   ```bash
   kubectl get pods -A | grep dcgm
   ```

4. Check Prometheus is scraping DCGM exporter:
   - Prometheus UI → Status → Targets
   - Look for `dcgm-exporter` job with state UP

### Datasource UID Mismatch

**Cause**: Dashboard expects datasource UID `PBFA97CFB590B2093` but your datasource has a different UID.

**Symptoms**: Dashboard panels show "Data source not found" or use wrong datasource.

**Solution A: Match the UID** (recommended)

Configure datasource with the exact UID:

```yaml
datasources:
  datasources.yaml:
    datasources:
      - uid: PBFA97CFB590B2093
```

Then reinstall or update Grafana:

```bash
helm upgrade gpu-grafana grafana/grafana -f helm/grafana-values.yaml -n monitoring
```

**Solution B: Remap Dashboard**

1. Open dashboard in Grafana
2. Click gear icon (Dashboard settings)
3. Go to JSON Model
4. Find and replace all instances of `"uid": "PBFA97CFB590B2093"` with your datasource UID
5. Save dashboard

### Missing Metrics

**Problem**: Some panels show "No data" but others work.

**Missing `DCGM_FI_PROF_GR_ENGINE_ACTIVE`**:

- Older DCGM versions may not export this metric
- Fallback: Dashboard uses `DCGM_FI_DEV_GPU_UTIL / 100` as alternative
- Verify with: `curl -s 'http://localhost:9090/api/v1/query?query=DCGM_FI_PROF_GR_ENGINE_ACTIVE'`

**Missing `kube_pod_labels`**:

- kube-state-metrics not configured with `--metric-labels-allowlist=pods=[*]`
- Affects "Idle GPU Time by Deployment" panel only
- Fix:
  ```bash
  kubectl set env deployment/kube-state-metrics \
    -n <namespace> \
    KUBE_STATE_METRICS_ARGS='--metric-labels-allowlist=pods=[*]'
  ```

### Pod Labels Not Showing in Deployment Analysis

**Cause**: Pods don't have the `app` label set to deployment name.

**Solution**: Ensure your GPU workloads have proper labels:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-gpu-workload
spec:
  template:
    metadata:
      labels:
        app: my-gpu-workload  # This label is required
```

### OpenShift: Prometheus 403 Forbidden

**Cause**: Grafana service account doesn't have permission to query Prometheus.

**Solution**:

```bash
oc adm policy add-cluster-role-to-user cluster-monitoring-view -z grafana -n monitoring
```

And set the Prometheus token:

```bash
# Get token
TOKEN=$(oc serviceaccounts get-token grafana -n monitoring)

# Update datasource secureJsonData
kubectl edit secret gpu-grafana -n monitoring
# Add: httpHeaderValue1: 'Bearer <TOKEN>'
```

Or use `--set` during Helm install:

```bash
--set datasources."datasources\.yaml".datasources[0].secureJsonData.httpHeaderValue1="Bearer $TOKEN"
```

### Dashboard Not Auto-Imported

**Cause**: Dashboard provisioning failed or URL is unreachable.

**Solution A: Manual Import**

1. Download dashboard: `curl -O https://raw.githubusercontent.com/wseaton/gpu-pruner/main/gpu-dashboard.json`
2. Grafana UI → Dashboards → Import → Upload JSON file
3. Select Prometheus datasource (UID `PBFA97CFB590B2093`)
4. Click Import

**Solution B: Use ConfigMap Method**

```bash
# Create ConfigMap
kubectl apply -f helm/grafana-dashboard-configmap.yaml

# Update Grafana values to use sidecar
helm upgrade gpu-grafana grafana/grafana \
  -f helm/grafana-values.yaml \
  --set sidecar.dashboards.enabled=true \
  -n monitoring
```

### Ingress Not Working

**Missing Ingress Controller**:

Verify ingress controller is installed:

```bash
kubectl get pods -n ingress-nginx  # or kube-system, or traefik-system
```

**Certificate Issues**:

If using cert-manager:

```bash
kubectl get certificate -n monitoring
kubectl describe certificate grafana-tls -n monitoring
```

**Alternative: Use NodePort or LoadBalancer**:

```bash
helm upgrade gpu-grafana grafana/grafana \
  -f helm/grafana-values.yaml \
  --set service.type=NodePort \
  --set service.nodePort=30300 \
  -n monitoring
```

Access via: `http://<node-ip>:30300`

## Customization

### Adjust Idle Detection Window

The dashboard uses a **30-minute window** to detect idle GPUs. To customize:

1. Open dashboard in Grafana
2. Edit panel (e.g., "Engine idle (30m)")
3. Change query from `[30m]` to desired duration:
   ```promql
   count(max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE[60m]) == 0)  # 60 minutes
   ```
4. Update panel title to reflect new duration
5. Save dashboard

### Modify GPU Model Assumptions

The "GPU Memory per GPU" panel shows 140 GiB for H200 GPUs. For other models:

1. Edit panel
2. Update query to use actual DCGM metric:
   ```promql
   max(DCGM_FI_DEV_FB_TOTAL) / 1024  # Returns actual GPU memory in GiB
   ```
3. Or hardcode for your GPU model:
   ```promql
   80  # For A100 80GB
   ```

### Add Custom Panels

The dashboard includes a **GPU Health & DCGM** row with temperature, power, VRAM %, memory-copy utilization, XID errors, and optional profiling metrics. See [`DASHBOARD.md`](DASHBOARD.md#gpu-health--dcgm) for PromQL reference.

To add more panels:

1. Dashboard → Add panel → Add a new panel
2. Select Prometheus datasource
3. Enter PromQL query
4. Configure visualization (graph, table, stat, etc.)
5. Save panel

Example metric already on the dashboard:

```promql
avg by (Hostname) (DCGM_FI_DEV_GPU_TEMP)
```

### Dashboard Refresh Rate

Change auto-refresh interval:

1. Dashboard settings (gear icon) → Time options
2. Set refresh interval (e.g., 30s, 1m, 5m)
3. Save

### Create Alerts

To alert on idle GPUs:

1. Edit "Engine idle (30m)" panel
2. Click "Alert" tab → Create alert rule
3. Configure:
   - **Condition**: `WHEN last() OF query(A) IS ABOVE 5`
   - **Evaluate every**: 5m
   - **For**: 10m (grace period)
4. Add notification channel
5. Save

## Security Considerations

### 1. Secure Admin Credentials

**Never hardcode passwords in values files**. Use Kubernetes secrets:

```bash
kubectl create secret generic grafana-admin-secret \
  -n monitoring \
  --from-literal=admin-password="$(openssl rand -base64 32)"
```

Update values:

```yaml
admin:
  existingSecret: grafana-admin-secret
  passwordKey: admin-password
```

### 2. RBAC for Prometheus Access

Grant minimal permissions to Grafana service account:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRoleBinding
metadata:
  name: grafana-prometheus-reader
roleRef:
  apiGroup: rbac.authorization.k8s.io
  kind: ClusterRole
  name: view  # Or create custom role with only Prometheus read access
subjects:
- kind: ServiceAccount
  name: grafana
  namespace: monitoring
```

### 3. Network Policies

Restrict Grafana network access:

```yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: grafana-netpol
  namespace: monitoring
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: grafana
  policyTypes:
  - Ingress
  - Egress
  ingress:
  - from:
    - namespaceSelector:
        matchLabels:
          name: ingress-nginx  # Allow ingress controller
    ports:
    - protocol: TCP
      port: 3000
  egress:
  - to:
    - namespaceSelector:
        matchLabels:
          name: monitoring  # Allow Prometheus access
    ports:
    - protocol: TCP
      port: 9090
  - to:  # Allow DNS
    - namespaceSelector:
        matchLabels:
          name: kube-system
    ports:
    - protocol: UDP
      port: 53
```

### 4. TLS/HTTPS

Always use TLS for production deployments:

**With Ingress + cert-manager**:

```yaml
ingress:
  annotations:
    cert-manager.io/cluster-issuer: letsencrypt-prod
  tls:
    - secretName: grafana-tls
      hosts:
        - grafana-gpu.example.com
```

**With OpenShift Route**:

```yaml
route:
  tls:
    enabled: true
    termination: edge  # or reencrypt for end-to-end TLS
```

### 5. Anonymous Access

Disable anonymous access in production:

```yaml
grafana.ini:
  auth.anonymous:
    enabled: false
```

### 6. Datasource Token Rotation

For OpenShift or token-based Prometheus auth, rotate tokens regularly:

```bash
# Generate new token
NEW_TOKEN=$(oc serviceaccounts get-token grafana -n monitoring)

# Update secret
kubectl patch secret gpu-grafana -n monitoring \
  -p "{\"data\":{\"prometheus-token\":\"$(echo -n $NEW_TOKEN | base64)\"}}"

# Restart Grafana
kubectl rollout restart deployment gpu-grafana -n monitoring
```

### 7. Audit Logging

Enable Grafana audit logging:

```yaml
grafana.ini:
  log:
    mode: console
    level: info
  log.console:
    format: json
  security:
    disable_initial_admin_creation: false
```

## Additional Resources

- [Grafana Helm Chart Documentation](https://github.com/grafana/helm-charts/tree/main/charts/grafana)
- [GPU Pruner Dashboard Documentation](DASHBOARD.md)
- [Prometheus Deployment Guide](PROMETHEUS_DEPLOYMENT.md)
- [DCGM Exporter Setup](https://docs.nvidia.com/datacenter/cloud-native/gpu-telemetry/dcgm-exporter.html)
- [kube-state-metrics Configuration](https://github.com/kubernetes/kube-state-metrics/blob/main/docs/cli-arguments.md)

## Support

For issues or questions:

- GitHub Issues: https://github.com/wseaton/gpu-pruner/issues
- Dashboard Documentation: [DASHBOARD.md](DASHBOARD.md)
- Grafana Community: https://community.grafana.com/
