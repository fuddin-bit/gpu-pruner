# Grafana GPU Dashboard - Access Information

## ✅ Deployment Status: LIVE

Your Grafana instance with the GPU Pruner dashboard is now deployed and publicly accessible!

## 🌐 Access Details

**Public URL (DNS - may take 5-10 minutes to update)**: http://gpu-grafana-fuddin.6787d4-waldorf.coreweave.app

**Direct IP Access (Works immediately)**: http://166.19.16.227

**Credentials**:
- **Username**: `admin`
- **Password**: `GpuPruner2026!`

**External IP**: `166.19.16.227` (CoreWeave Public LoadBalancer)

## 📊 Dashboard Import

The GPU dashboard is **not yet imported**. After logging in, you need to import it:

### Option 1: Automated Import Script

```bash
./import-dashboard.sh http://gpu-grafana-fuddin.6787d4-waldorf.coreweave.app GpuPruner2026!
```

### Option 2: Manual Import via UI

1. Access http://gpu-grafana-fuddin.6787d4-waldorf.coreweave.app
2. Login with `admin` / `GpuPruner2026!`
3. Navigate to **Dashboards** → **Import** → **Upload JSON file**
4. Select `gpu-dashboard.json` from this repository
5. Choose **Prometheus** datasource (UID: `PBFA97CFB590B2093`)
6. Click **Import**

### Option 3: Import via API

```bash
GRAFANA_URL="http://gpu-grafana-fuddin.6787d4-waldorf.coreweave.app"
ADMIN_PASSWORD="GpuPruner2026!"

DASHBOARD_JSON=$(cat gpu-dashboard.json | jq '{dashboard: ., overwrite: true, folderId: 0}')

curl -X POST \
  -H "Content-Type: application/json" \
  -u "admin:$ADMIN_PASSWORD" \
  -d "$DASHBOARD_JSON" \
  "$GRAFANA_URL/api/dashboards/db"
```

## 🔍 Verify Datasource

After logging in, verify the Prometheus datasource is working:

1. Go to **Configuration** → **Data Sources** → **Prometheus**
2. Click **Save & Test**
3. Should show: "Data source is working" ✓

If the datasource test fails:
- Check the Prometheus URL: `http://prometheus-kube-prometheus-prometheus.monitoring.svc.cluster.local:9090`
- Verify Prometheus is accessible from the `fuddin-dev` namespace

## 📝 Service Configuration

**Namespace**: `fuddin-dev`  
**Service Type**: `LoadBalancer`  
**Service Name**: `gpu-grafana`  
**Internal Port**: `80` → Pod Port `3000`  
**NodePort**: `30265`

### DNS Configuration

**Annotation**: `service.beta.kubernetes.io/external-hostname: "gpu-grafana-fuddin"`  
**Auto-generated FQDN**: `gpu-grafana-fuddin.6787d4-waldorf.coreweave.app`

CoreWeave's External Hostname Controller automatically:
- Created the DNS record in the `.coreweave.app` domain
- Appended the cluster identifier `6787d4-waldorf` to prevent conflicts
- Set the DNS status in the Service `.status.conditions` field

## 🔧 Management Commands

### Check Service Status

```bash
kubectl get svc gpu-grafana -n fuddin-dev
```

### View Service Details

```bash
kubectl describe svc gpu-grafana -n fuddin-dev
```

### Check DNS Status

```bash
kubectl get svc gpu-grafana -n fuddin-dev -o jsonpath='{.status.conditions[?(@.type=="ExternalRecords")]}' | jq .
```

### Check Grafana Pods

```bash
kubectl get pods -n fuddin-dev -l app.kubernetes.io/name=grafana
```

### View Grafana Logs

```bash
kubectl logs -n fuddin-dev -l app.kubernetes.io/name=grafana --tail=100 -f
```

### Restart Grafana

```bash
kubectl rollout restart deployment gpu-grafana -n fuddin-dev
```

## ⚠️ Important Notes

### Persistence

**WARNING**: Persistence is currently **DISABLED**. This means:
- Dashboard customizations will be **lost** if the pod restarts
- Datasource changes will be **lost** if the pod restarts
- User accounts (other than admin) will be **lost** if the pod restarts

To enable persistence:

```bash
helm upgrade gpu-grafana grafana/grafana \
  --reuse-values \
  --set persistence.enabled=true \
  --set persistence.size=10Gi \
  -n fuddin-dev
```

### Security

- ✅ Authentication is enabled (admin password required)
- ⚠️ HTTP only (no HTTPS/TLS)
- ⚠️ No IP restrictions (publicly accessible)
- ⚠️ Default admin password (should be changed for production)

### Recommended Security Improvements

1. **Change admin password**:
   ```bash
   # Login to Grafana UI
   # Profile → Change Password
   ```

2. **Enable HTTPS** (requires TLS certificate):
   ```bash
   # Add TLS certificate to cluster
   kubectl create secret tls grafana-tls \
     --cert=grafana.crt \
     --key=grafana.key \
     -n fuddin-dev
   
   # Update service annotation
   kubectl annotate svc gpu-grafana -n fuddin-dev \
     service.beta.kubernetes.io/external-hostname-tls="grafana-tls"
   ```

3. **Restrict source IPs** (optional):
   ```bash
   kubectl patch svc gpu-grafana -n fuddin-dev -p '{
     "spec": {
       "loadBalancerSourceRanges": ["YOUR.IP.ADDRESS/32"]
     }
   }'
   ```

4. **Enable persistence** (as shown above)

## 📊 Expected Dashboard Features

Once imported, the GPU dashboard will show:

- **Cluster GPU Overview**
  - Total GPUs
  - VRAM allocation (FB>0 vs FB=0)
  - Engine activity (idle 30m vs active 30m)
  - GPU memory per GPU

- **GPU Utilization Heatmap**
  - Per-node GPU utilization over time

- **Running GPU Workloads**
  - All pods with GPU requests
  - Grouped by namespace

- **Idle GPU Workloads**
  - GPUs with zero compute activity for 30+ minutes
  - Identifies wasted resources

- **Idle GPU Time by Deployment**
  - Historical analysis of which deployments waste the most GPU time
  - Requires `kube_pod_labels` metric from kube-state-metrics

- **GPU Allocation Leaderboard**
  - Total GPU requests per namespace

## 🐛 Troubleshooting

### Cannot access the URL

**Check DNS propagation**:
```bash
nslookup gpu-grafana-fuddin.6787d4-waldorf.coreweave.app
```

Should return: `10.16.4.0`

**Check from your browser**:
- Try: http://gpu-grafana-fuddin.6787d4-waldorf.coreweave.app
- If DNS fails, try direct IP: http://10.16.4.0 (may not work from external networks)

### Dashboard shows "No Data"

1. **Verify Prometheus datasource**:
   - Configuration → Data Sources → Prometheus → Save & Test

2. **Check Prometheus is accessible**:
   ```bash
   kubectl run curl-test --image=curlimages/curl:latest --rm -i --restart=Never -n fuddin-dev -- \
     curl -s 'http://prometheus-kube-prometheus-prometheus.monitoring.svc.cluster.local:9090/api/v1/query?query=up'
   ```

3. **Verify DCGM metrics exist**:
   ```bash
   # Port-forward to Prometheus
   kubectl port-forward -n monitoring svc/prometheus-kube-prometheus-prometheus 9090:9090 &
   
   # Query DCGM metrics
   curl -s 'http://localhost:9090/api/v1/query?query=DCGM_FI_DEV_GPU_UTIL' | jq '.data.result | length'
   ```

### "Idle GPU Time by Deployment" panel empty

This panel requires `kube_pod_labels` metric. Verify kube-state-metrics is configured with:

```bash
kubectl get deploy kube-state-metrics -A -o yaml | grep metric-labels-allowlist
```

Should show: `--metric-labels-allowlist=pods=[*]`

### Grafana pod not running

```bash
# Check pod status
kubectl get pods -n fuddin-dev -l app.kubernetes.io/name=grafana

# Check logs
kubectl logs -n fuddin-dev -l app.kubernetes.io/name=grafana --tail=50

# Describe pod for events
kubectl describe pod -n fuddin-dev -l app.kubernetes.io/name=grafana
```

## 📚 Additional Resources

- **Main Documentation**: [GRAFANA_DEPLOYMENT.md](GRAFANA_DEPLOYMENT.md)
- **Dashboard Features**: [DASHBOARD.md](DASHBOARD.md)
- **Helm Configuration**: [helm/README.md](helm/README.md)
- **CoreWeave Ingress**: [COREWEAVE_INGRESS_GUIDE.md](COREWEAVE_INGRESS_GUIDE.md)
- **Import Script**: [import-dashboard.sh](import-dashboard.sh)

## 🎯 Next Steps

1. ✅ **Access Grafana**: http://gpu-grafana-fuddin.6787d4-waldorf.coreweave.app
2. ✅ **Login**: `admin` / `GpuPruner2026!`
3. ⏳ **Import Dashboard**: Use the import script or manual UI import
4. ⏳ **Verify Datasource**: Configuration → Data Sources → Prometheus → Save & Test
5. ⏳ **Enable Persistence**: To prevent data loss on pod restart
6. ⏳ **Change Password**: For production security

## 📞 Support

For issues or questions:
- **GitHub Issues**: https://github.com/wseaton/gpu-pruner/issues
- **CoreWeave Docs**: https://docs.coreweave.com/
- **Deployment Guide**: [GRAFANA_DEPLOYMENT.md](GRAFANA_DEPLOYMENT.md)

---

**Deployment Date**: 2026-06-04  
**Deployed By**: fuddin@redhat.com  
**Cluster**: coreweave-waldorf (6787d4)  
**Namespace**: fuddin-dev
