# Grafana GPU Dashboard - Quick Start Guide

One-command deployments for common scenarios.

## Prerequisites Check

```bash
# Verify prerequisites are met
kubectl get svc -A | grep prometheus      # ✓ Prometheus exists
kubectl get pods -A | grep dcgm           # ✓ DCGM exporter running
kubectl get deploy -A | grep kube-state  # ✓ kube-state-metrics deployed
helm version                              # ✓ Helm 3.x installed
```

## Scenario 1: OpenShift with Prometheus Operator

```bash
# One command deployment
helm repo add grafana https://grafana.github.io/helm-charts
helm repo update

helm install gpu-grafana grafana/grafana \
  -f https://raw.githubusercontent.com/wseaton/gpu-pruner/main/helm/grafana-values.yaml \
  -f https://raw.githubusercontent.com/wseaton/gpu-pruner/main/helm/grafana-values-openshift.yaml \
  --set adminPassword='ChangeMe123!' \
  -n monitoring --create-namespace

# Grant Prometheus access
oc adm policy add-cluster-role-to-user cluster-monitoring-view -z grafana -n monitoring

# Get Route URL
echo "https://$(oc get route -n monitoring grafana -o jsonpath='{.spec.host}')"
```

**Login**: `admin` / `ChangeMe123!`

## Scenario 2: Vanilla Kubernetes with nginx Ingress

```bash
helm repo add grafana https://grafana.github.io/helm-charts
helm repo update

helm install gpu-grafana grafana/grafana \
  -f https://raw.githubusercontent.com/wseaton/gpu-pruner/main/helm/grafana-values.yaml \
  -f https://raw.githubusercontent.com/wseaton/gpu-pruner/main/helm/grafana-values-vanilla-k8s.yaml \
  --set adminPassword='ChangeMe123!' \
  --set ingress.hosts[0]='grafana.example.com' \
  --set datasources."datasources\.yaml".datasources[0].url='http://prometheus-k8s.monitoring.svc.cluster.local:9090' \
  -n monitoring --create-namespace

# Access via Ingress
echo "https://grafana.example.com"
```

**Login**: `admin` / `ChangeMe123!`

## Scenario 3: Local Testing with Port-Forward

```bash
helm repo add grafana https://grafana.github.io/helm-charts
helm repo update

helm install gpu-grafana grafana/grafana \
  -f https://raw.githubusercontent.com/wseaton/gpu-pruner/main/helm/grafana-values.yaml \
  --set adminPassword='admin' \
  --set persistence.enabled=false \
  --set datasources."datasources\.yaml".datasources[0].url='http://prometheus-k8s.monitoring.svc.cluster.local:9090' \
  -n monitoring --create-namespace

# Port-forward to access
kubectl port-forward -n monitoring svc/gpu-grafana 3000:3000
```

**Access**: http://localhost:3000  
**Login**: `admin` / `admin`

## Scenario 4: Air-Gapped Cluster (ConfigMap Method)

```bash
# Step 1: Download files
curl -O https://raw.githubusercontent.com/wseaton/gpu-pruner/main/helm/grafana-dashboard-configmap.yaml
curl -O https://raw.githubusercontent.com/wseaton/gpu-pruner/main/helm/grafana-values.yaml

# Step 2: Create ConfigMap
kubectl apply -f grafana-dashboard-configmap.yaml

# Step 3: Deploy Grafana with sidecar
helm install gpu-grafana grafana/grafana \
  -f grafana-values.yaml \
  --set adminPassword='ChangeMe123!' \
  --set sidecar.dashboards.enabled=true \
  --set sidecar.dashboards.label=grafana_dashboard \
  --set dashboards=null \
  -n monitoring --create-namespace
```

## Scenario 5: Using LoadBalancer Service

```bash
helm install gpu-grafana grafana/grafana \
  -f https://raw.githubusercontent.com/wseaton/gpu-pruner/main/helm/grafana-values.yaml \
  --set adminPassword='ChangeMe123!' \
  --set service.type=LoadBalancer \
  --set datasources."datasources\.yaml".datasources[0].url='http://prometheus:9090' \
  -n monitoring --create-namespace

# Get LoadBalancer IP
kubectl get svc -n monitoring gpu-grafana -o jsonpath='{.status.loadBalancer.ingress[0].ip}'
```

## Post-Installation

### Get Admin Password

```bash
kubectl get secret -n monitoring gpu-grafana -o jsonpath="{.data.admin-password}" | base64 --decode ; echo
```

### Verify Dashboard Loaded

```bash
kubectl port-forward -n monitoring svc/gpu-grafana 3000:3000 &
curl -s -u admin:YOUR_PASSWORD http://localhost:3000/api/search?query=gpu | jq .
```

Expected: JSON with dashboard titled "Waldorf GPU Usage & Idle Tracker"

### Test Prometheus Datasource

Grafana UI → Configuration → Data Sources → Prometheus → Save & Test

Should show: **"Data source is working"** ✓

## Customization

### Change Prometheus URL

```bash
--set datasources."datasources\.yaml".datasources[0].url='http://YOUR_PROMETHEUS:9090'
```

### Change Ingress Hostname

```bash
--set ingress.hosts[0]='grafana-gpu.yourdomain.com'
```

### Enable Persistence

```bash
--set persistence.enabled=true \
--set persistence.size=20Gi \
--set persistence.storageClassName=fast-ssd
```

### Increase Resources

```bash
--set resources.limits.cpu=1000m \
--set resources.limits.memory=1Gi \
--set resources.requests.cpu=500m \
--set resources.requests.memory=512Mi
```

## Troubleshooting

### Dashboard Shows "No Data"

```bash
# Test Prometheus connectivity
kubectl exec -n monitoring deploy/gpu-grafana -- wget -qO- http://prometheus-k8s.monitoring.svc.cluster.local:9090/api/v1/query?query=up

# Check DCGM metrics exist
kubectl port-forward -n <prometheus-ns> svc/<prometheus-svc> 9090:9090 &
curl -s 'http://localhost:9090/api/v1/query?query=DCGM_FI_DEV_GPU_UTIL' | jq .
```

### Can't Access Grafana

```bash
# Check pod status
kubectl get pods -n monitoring -l app.kubernetes.io/name=grafana

# Check logs
kubectl logs -n monitoring -l app.kubernetes.io/name=grafana --tail=50

# Use port-forward as fallback
kubectl port-forward -n monitoring svc/gpu-grafana 3000:3000
```

### Forgot Admin Password

```bash
# Retrieve existing password
kubectl get secret -n monitoring gpu-grafana -o jsonpath="{.data.admin-password}" | base64 --decode ; echo

# Or reset it
helm upgrade gpu-grafana grafana/grafana \
  -f helm/grafana-values.yaml \
  --set adminPassword='NewPassword123!' \
  --reuse-values \
  -n monitoring
```

## Upgrading

```bash
helm repo update
helm upgrade gpu-grafana grafana/grafana \
  -f helm/grafana-values.yaml \
  -n monitoring
```

## Uninstalling

```bash
helm uninstall gpu-grafana -n monitoring

# Optional: Delete persistent data
kubectl delete pvc -n monitoring -l app.kubernetes.io/name=grafana
```

## Next Steps

- **Detailed Guide**: [GRAFANA_DEPLOYMENT.md](../GRAFANA_DEPLOYMENT.md)
- **Dashboard Features**: [DASHBOARD.md](../DASHBOARD.md)
- **Configuration Reference**: [helm/README.md](README.md)

## Need Help?

- GitHub Issues: https://github.com/wseaton/gpu-pruner/issues
- Full Documentation: [GRAFANA_DEPLOYMENT.md](../GRAFANA_DEPLOYMENT.md)
