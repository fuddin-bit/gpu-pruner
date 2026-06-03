# Local Testing Guide for Prometheus Metrics

This guide shows how to test the Prometheus metrics functionality locally without cluster access.

## Option 1: Quick Test with Dummy Prometheus (Easiest)

This runs gpu-pruner with a fake Prometheus URL just to test the metrics endpoint.

### Start gpu-pruner

```bash
./target/release/gpu-pruner \
  --prometheus-url http://localhost:9999 \
  --dashboard-port 8080 \
  --run-mode dry-run \
  --daemon-mode \
  --check-interval 30
```

This will:
- Fail to connect to Prometheus (expected)
- Increment `query_failures_total` every 30 seconds
- Still expose `/metrics` and `/api/status` endpoints

### Test the Endpoints

In another terminal:

```bash
# Test metrics endpoint
curl http://localhost:8080/metrics

# Test dashboard API
curl http://localhost:8080/api/status | jq .

# Watch metrics update in real-time
watch -n 5 'curl -s http://localhost:8080/metrics | grep gpu_pruner'
```

You should see `query_failures_total` incrementing every 30 seconds.

## Option 2: Local Prometheus Setup (Full Testing)

This sets up a complete local Prometheus instance that scrapes gpu-pruner.

### Step 1: Create Prometheus Configuration

Create `prometheus-local.yml`:

```bash
cat > /tmp/prometheus-local.yml << 'EOF'
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'gpu-pruner'
    static_configs:
      - targets: ['host.docker.internal:8080']
        labels:
          app: 'gpu-pruner'
          environment: 'local'
    metrics_path: /metrics
    scrape_interval: 10s
EOF
```

### Step 2: Run Prometheus in Docker

```bash
docker run -d \
  --name prometheus-local \
  -p 9090:9090 \
  -v /tmp/prometheus-local.yml:/etc/prometheus/prometheus.yml \
  prom/prometheus:latest \
  --config.file=/etc/prometheus/prometheus.yml \
  --web.enable-lifecycle
```

### Step 3: Run gpu-pruner

Since gpu-pruner needs a real Prometheus instance, you can point it to your local one:

```bash
./target/release/gpu-pruner \
  --prometheus-url http://localhost:9090 \
  --dashboard-port 8080 \
  --run-mode dry-run \
  --daemon-mode \
  --check-interval 30
```

It will fail queries (no DCGM metrics in local Prometheus) but that's fine for testing.

### Step 4: Access Prometheus UI

Open http://localhost:9090

**Check targets:**
- Go to Status → Targets
- Should see `gpu-pruner` target as **UP**

**Query metrics:**
```promql
rate(gpu_pruner_query_failures_total[1m])
gpu_pruner_idle_gpus
```

### Step 5: Cleanup

```bash
docker stop prometheus-local
docker rm prometheus-local
rm /tmp/prometheus-local.yml
```

## Option 3: Test with Port-Forward to Real Cluster Prometheus

If you have read access to the cluster's Prometheus, you can query it locally.

### Step 1: Port-forward to Cluster Prometheus

```bash
kubectl port-forward -n openshift-monitoring svc/thanos-querier 9091:9090
```

### Step 2: Run gpu-pruner Locally

```bash
./target/release/gpu-pruner \
  --prometheus-url http://localhost:9091 \
  --dashboard-port 8080 \
  --run-mode dry-run \
  --daemon-mode \
  --check-interval 60
```

This will:
- Query real cluster metrics (DCGM, kube-state-metrics)
- Actually find idle GPUs if any exist
- Increment success counters
- Update all gauges with real data

### Step 3: Monitor Metrics

```bash
# Watch metrics update with real data
watch -n 10 'curl -s http://localhost:8080/metrics'

# Or use Prometheus to query the local instance
# (Start a second Prometheus instance as in Option 2)
```

## Verification Checklist

Test each metric to ensure it's working:

### Counters (should increment over time)

```bash
# Run this script to watch counters increment
cat > /tmp/watch_metrics.sh << 'EOF'
#!/bin/bash
while true; do
  echo "=== $(date) ==="
  curl -s http://localhost:8080/metrics | grep -E "query_|scale_" | grep -v "^#"
  echo ""
  sleep 10
done
EOF
chmod +x /tmp/watch_metrics.sh
/tmp/watch_metrics.sh
```

Expected to increment:
- ✅ `gpu_pruner_query_successes_total` - if Prometheus queries succeed
- ✅ `gpu_pruner_query_failures_total` - if Prometheus queries fail
- ✅ `gpu_pruner_query_candidates_total` - when idle GPUs found
- ✅ `gpu_pruner_query_shutdown_events_total` - when shutdown events detected
- ✅ `gpu_pruner_scale_successes_total` - if run-mode is not dry-run (won't increment in dry-run)
- ✅ `gpu_pruner_scale_failures_total` - if scale operations fail

### Gauges (should reflect current state)

```bash
curl -s http://localhost:8080/metrics | grep -E "idle_gpus|pods_checked"
```

Expected values:
- `gpu_pruner_idle_gpus` - should match number in `/api/status` response
- `gpu_pruner_pods_checked_total` - should match "total_pods_checked" in `/api/status`

### Cross-check with Dashboard API

```bash
# Metrics endpoint
curl -s http://localhost:8080/metrics | grep "gpu_pruner_idle_gpus"

# Dashboard API (should show same value)
curl -s http://localhost:8080/api/status | jq '.total_idle_gpus'
```

## Testing Different Scenarios

### Scenario 1: Query Failures

```bash
# Point to non-existent Prometheus
./target/release/gpu-pruner \
  --prometheus-url http://localhost:9999 \
  --dashboard-port 8080 \
  --run-mode dry-run \
  --check-interval 10

# Watch failures increment
watch -n 5 'curl -s http://localhost:8080/metrics | grep query_failures_total'
```

Expected: `query_failures_total` increments every 10 seconds

### Scenario 2: Query Successes

```bash
# Port-forward to real Prometheus first
kubectl port-forward -n openshift-monitoring svc/thanos-querier 9091:9090 &

# Run gpu-pruner
./target/release/gpu-pruner \
  --prometheus-url http://localhost:9091 \
  --dashboard-port 8080 \
  --run-mode dry-run \
  --check-interval 30

# Watch successes increment
watch -n 5 'curl -s http://localhost:8080/metrics | grep query_successes_total'
```

Expected: `query_successes_total` increments every 30 seconds

### Scenario 3: Dashboard Integration

```bash
# Run gpu-pruner with real Prometheus
./target/release/gpu-pruner \
  --prometheus-url http://localhost:9091 \
  --dashboard-port 8080 \
  --run-mode dry-run \
  --daemon-mode

# Open dashboard in browser
open http://localhost:8080

# Compare values
curl -s http://localhost:8080/api/status | jq .
curl -s http://localhost:8080/metrics | grep -E "idle_gpus|pods_checked"
```

Expected: Values in `/api/status` and `/metrics` match

## Advanced: Load Testing

Generate load to see metrics increment quickly:

```bash
# Run in non-daemon mode with short interval (will run once)
for i in {1..10}; do
  ./target/release/gpu-pruner \
    --prometheus-url http://localhost:9999 \
    --dashboard-port 8080 \
    --run-mode dry-run &
  sleep 2
  curl -s http://localhost:8080/metrics | grep query_failures_total
  pkill gpu-pruner
done
```

## Prometheus Query Examples

If you set up local Prometheus (Option 2), try these queries:

```promql
# Query rate over time
rate(gpu_pruner_query_successes_total[1m])

# Failure ratio
rate(gpu_pruner_query_failures_total[5m]) 
  / 
(rate(gpu_pruner_query_successes_total[5m]) + rate(gpu_pruner_query_failures_total[5m]))

# Total queries (success + failure)
sum(gpu_pruner_query_successes_total) + sum(gpu_pruner_query_failures_total)

# Idle GPU trend
gpu_pruner_idle_gpus

# Candidates found per query
rate(gpu_pruner_query_candidates_total[5m]) / rate(gpu_pruner_query_successes_total[5m])
```

## Grafana Dashboard (Optional)

If you want to visualize locally:

```bash
# Run Grafana
docker run -d \
  --name grafana-local \
  -p 3000:3000 \
  grafana/grafana-oss:latest

# Login: admin/admin
# Add Prometheus datasource: http://host.docker.internal:9090
# Import dashboard (use gpu-dashboard.json as reference)
```

## Troubleshooting

### Port already in use

```bash
# Find process using port 8080
lsof -ti:8080

# Kill it
kill $(lsof -ti:8080)
```

### Metrics endpoint returns empty

**Issue:** Metrics not registered

**Solution:** Check logs for errors:
```bash
./target/release/gpu-pruner --prometheus-url http://localhost:9999 --dashboard-port 8080 2>&1 | grep -i metric
```

### Docker can't reach host.docker.internal

**For Linux:**
```bash
# Use --add-host flag
docker run -d \
  --add-host=host.docker.internal:host-gateway \
  --name prometheus-local \
  -p 9090:9090 \
  -v /tmp/prometheus-local.yml:/etc/prometheus/prometheus.yml \
  prom/prometheus:latest
```

## Quick Test Script

Here's a complete test script you can run:

```bash
cat > /tmp/test_metrics.sh << 'EOF'
#!/bin/bash
set -e

echo "🧪 Testing gpu-pruner Prometheus metrics locally..."
echo ""

# Start gpu-pruner in background
echo "▶️  Starting gpu-pruner on port 8080..."
./target/release/gpu-pruner \
  --prometheus-url http://localhost:9999 \
  --dashboard-port 8080 \
  --run-mode dry-run \
  --daemon-mode \
  --check-interval 10 > /tmp/gpu-pruner.log 2>&1 &

GPU_PID=$!
echo "   PID: $GPU_PID"
sleep 3

# Test endpoints
echo ""
echo "✅ Testing /metrics endpoint..."
METRICS=$(curl -s http://localhost:8080/metrics)
if echo "$METRICS" | grep -q "gpu_pruner_query_failures_total"; then
  echo "   ✓ Metrics endpoint working"
else
  echo "   ✗ Metrics endpoint failed"
  kill $GPU_PID
  exit 1
fi

echo ""
echo "✅ Testing /api/status endpoint..."
STATUS=$(curl -s http://localhost:8080/api/status)
if echo "$STATUS" | jq -e '.total_idle_gpus' > /dev/null 2>&1; then
  echo "   ✓ Dashboard API working"
else
  echo "   ✗ Dashboard API failed"
  kill $GPU_PID
  exit 1
fi

echo ""
echo "📊 Current metrics:"
curl -s http://localhost:8080/metrics | grep -E "^gpu_pruner" | grep -v "^#"

echo ""
echo "⏳ Waiting 15 seconds for metrics to update..."
sleep 15

echo ""
echo "📊 Updated metrics (query_failures should have incremented):"
curl -s http://localhost:8080/metrics | grep -E "^gpu_pruner" | grep -v "^#"

echo ""
echo "🎉 Test complete! Stopping gpu-pruner..."
kill $GPU_PID

echo ""
echo "Logs saved to /tmp/gpu-pruner.log"
EOF

chmod +x /tmp/test_metrics.sh
/tmp/test_metrics.sh
```

This script will:
1. Start gpu-pruner
2. Test both endpoints
3. Show initial metrics
4. Wait for updates
5. Show metrics again (should see increments)
6. Clean up

Run it with:
```bash
cd /Users/fuddin/Documents/gpu-pruner/gpu-pruner
/tmp/test_metrics.sh
```
