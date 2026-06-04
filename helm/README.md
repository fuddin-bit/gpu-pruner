# Grafana Helm Chart Deployment Files

This directory contains Helm values files and Kubernetes manifests for deploying Grafana with the GPU Pruner dashboard.

## Files

| File | Description |
|------|-------------|
| `grafana-values.yaml` | **Base values file** - Core Grafana configuration with dashboard provisioning, Prometheus datasource, and resource settings. Use this as the foundation for all deployments. |
| `grafana-values-openshift.yaml` | **OpenShift overrides** - Route configuration, token-based Prometheus authentication, and OpenShift-specific security context. Merge with base values. |
| `grafana-values-vanilla-k8s.yaml` | **Vanilla Kubernetes overrides** - Ingress configuration for nginx/traefik, standard K8s security context. Merge with base values. |
| `grafana-dashboard-configmap.yaml` | **Dashboard ConfigMap** - Alternative provisioning method using ConfigMap + sidecar instead of direct URL import. |

## Quick Start

### OpenShift Deployment

```bash
helm repo add grafana https://grafana.github.io/helm-charts
helm repo update

helm install gpu-grafana grafana/grafana \
  -f helm/grafana-values.yaml \
  -f helm/grafana-values-openshift.yaml \
  --set adminPassword='YOUR_SECURE_PASSWORD' \
  -n monitoring --create-namespace

# Grant Prometheus access
oc adm policy add-cluster-role-to-user cluster-monitoring-view -z grafana -n monitoring
```

Access via Route:
```bash
oc get route -n monitoring grafana -o jsonpath='{.spec.host}'
```

### Vanilla Kubernetes Deployment

```bash
helm install gpu-grafana grafana/grafana \
  -f helm/grafana-values.yaml \
  -f helm/grafana-values-vanilla-k8s.yaml \
  --set adminPassword='YOUR_SECURE_PASSWORD' \
  --set ingress.hosts[0]='grafana-gpu.example.com' \
  --set datasources."datasources\.yaml".datasources[0].url='http://prometheus-k8s.monitoring.svc.cluster.local:9090' \
  -n monitoring --create-namespace
```

Access via Ingress: `https://grafana-gpu.example.com`

## Configuration

### Required Customizations

Before deploying, update these values in `grafana-values.yaml` or via `--set`:

1. **Admin Password**:
   ```bash
   --set adminPassword='YOUR_SECURE_PASSWORD'
   ```
   Or use a secret (recommended):
   ```bash
   kubectl create secret generic grafana-admin-secret \
     -n monitoring \
     --from-literal=admin-password='YOUR_PASSWORD'
   
   --set admin.existingSecret=grafana-admin-secret
   ```

2. **Prometheus URL**:
   ```bash
   --set datasources."datasources\.yaml".datasources[0].url='http://YOUR_PROMETHEUS:9090'
   ```

3. **Ingress Hostname** (vanilla K8s):
   ```bash
   --set ingress.hosts[0]='grafana-gpu.example.com'
   ```

4. **Route Hostname** (OpenShift):
   ```bash
   --set route.host='grafana-gpu.apps.example.com'
   ```

### Critical: Datasource UID

**Do NOT modify the datasource UID** in the values file. The dashboard has a hardcoded UID:

```yaml
datasources:
  datasources.yaml:
    datasources:
      - uid: PBFA97CFB590B2093  # Must match gpu-dashboard.json
```

If you change this UID, the dashboard will not work.

## Dashboard Provisioning Methods

### Method 1: Direct URL Import (Default)

Configured in `grafana-values.yaml`:

```yaml
dashboards:
  gpu-pruner:
    gpu-dashboard:
      url: https://raw.githubusercontent.com/wseaton/gpu-pruner/main/gpu-dashboard.json
```

**Pros**: Simple, automatic updates when repo changes  
**Cons**: Requires internet access from Grafana pod

### Method 2: ConfigMap + Sidecar

Apply the ConfigMap:

```bash
kubectl apply -f helm/grafana-dashboard-configmap.yaml
```

Enable sidecar in values:

```yaml
sidecar:
  dashboards:
    enabled: true
    label: grafana_dashboard
```

**Pros**: Works in air-gapped clusters, no external dependencies  
**Cons**: Requires manual updates when dashboard changes

### Method 3: Manual Import

1. Download dashboard:
   ```bash
   curl -O https://raw.githubusercontent.com/wseaton/gpu-pruner/main/gpu-dashboard.json
   ```

2. Grafana UI → Dashboards → Import → Upload JSON

3. Select Prometheus datasource

**Pros**: Full control over dashboard version  
**Cons**: Not automated, requires UI access

## Prerequisites

Ensure these components are running before deploying Grafana:

- ✅ **Prometheus** - Accessible at configured URL
- ✅ **DCGM Exporter** - Running on GPU nodes
- ✅ **kube-state-metrics** - With `--metric-labels-allowlist=pods=[*]`
- ✅ **Persistent Storage** (optional) - For dashboard/datasource persistence

Validation:

```bash
# Check Prometheus
kubectl get svc -A | grep prometheus

# Check DCGM exporter
kubectl get pods -A | grep dcgm

# Verify DCGM metrics
kubectl port-forward -n <prometheus-ns> svc/<prometheus-svc> 9090:9090 &
curl -s 'http://localhost:9090/api/v1/query?query=DCGM_FI_DEV_GPU_UTIL' | jq '.data.result | length'
```

## Upgrading

To upgrade an existing deployment with new values:

```bash
helm upgrade gpu-grafana grafana/grafana \
  -f helm/grafana-values.yaml \
  -f helm/grafana-values-vanilla-k8s.yaml \
  -n monitoring
```

To upgrade the Grafana chart version:

```bash
helm repo update
helm search repo grafana/grafana --versions | head -5  # Check available versions

helm upgrade gpu-grafana grafana/grafana \
  -f helm/grafana-values.yaml \
  --version 8.0.0 \
  -n monitoring
```

## Uninstalling

```bash
helm uninstall gpu-grafana -n monitoring
```

To also delete persistent data:

```bash
kubectl delete pvc -n monitoring -l app.kubernetes.io/name=grafana
```

## Troubleshooting

### Dashboard shows "No data"

1. Verify Prometheus datasource:
   ```bash
   kubectl exec -n monitoring -it deploy/gpu-grafana -- \
     wget -O- http://prometheus-k8s.monitoring.svc.cluster.local:9090/api/v1/query?query=up
   ```

2. Check Grafana logs:
   ```bash
   kubectl logs -n monitoring -l app.kubernetes.io/name=grafana --tail=50
   ```

### Can't login to Grafana

Get admin password:

```bash
kubectl get secret -n monitoring gpu-grafana -o jsonpath="{.data.admin-password}" | base64 --decode ; echo
```

Reset admin password:

```bash
kubectl delete secret gpu-grafana -n monitoring
helm upgrade gpu-grafana grafana/grafana \
  -f helm/grafana-values.yaml \
  --set adminPassword='NEW_PASSWORD' \
  -n monitoring
```

### Ingress not working

Check ingress controller:

```bash
kubectl get pods -n ingress-nginx
```

Verify ingress resource:

```bash
kubectl get ingress -n monitoring
kubectl describe ingress gpu-grafana -n monitoring
```

Alternative: Use port-forward for testing:

```bash
kubectl port-forward -n monitoring svc/gpu-grafana 3000:3000
```

## Additional Documentation

- **[GRAFANA_DEPLOYMENT.md](../GRAFANA_DEPLOYMENT.md)** - Complete deployment guide with validation steps and security considerations
- **[DASHBOARD.md](../DASHBOARD.md)** - Dashboard features and usage guide
- **[gpu-dashboard.json](../gpu-dashboard.json)** - Dashboard source JSON

## Support

For issues or questions:

- GitHub Issues: https://github.com/wseaton/gpu-pruner/issues
- Grafana Documentation: https://grafana.com/docs/
- Helm Chart: https://github.com/grafana/helm-charts/tree/main/charts/grafana
