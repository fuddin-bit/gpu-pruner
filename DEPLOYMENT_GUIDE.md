# Deployment Guide for GPU Pruner Dashboard

Since Docker is not installed locally, we'll use GitHub Actions to build and push the image.

## Step 1: Commit and Push Your Changes

```bash
cd /Users/fuddin/Documents/gpu-pruner/gpu-pruner

# Stage all changes
git add .

# Commit with a descriptive message
git commit -m "feat: Add web dashboard for monitoring idle GPU workloads

- Add Axum-based web server with dashboard UI
- Add REST API endpoint at /api/status
- Add Kubernetes Service and OpenShift Route manifests
- Update deployment to expose dashboard on port 8080
- Add comprehensive documentation

Implements requirements from intern project:
- Current running workload display
- Idle GPU workloads list
- Resource consumption metrics
- Web UI for easy monitoring"

# Push to GitHub
git push origin main
```

## Step 2: Create and Push a Release Tag

The GitHub Actions workflow triggers on version tags (v*):

```bash
# Create a new version tag
git tag -a v0.4.1 -m "Release v0.4.1 - Add dashboard feature"

# Push the tag to trigger the build
git push origin v0.4.1
```

This will automatically:
- Build the Docker image with the dashboard
- Push to `ghcr.io/fuddin-bit/gpu-pruner:v0.4.1-otel`
- Push to `ghcr.io/fuddin-bit/gpu-pruner:latest-otel`

## Step 3: Monitor the Build

1. Go to: https://github.com/fuddin-bit/gpu-pruner/actions
2. Watch the "Release" workflow progress
3. Wait for both matrix builds (otel and default) to complete

## Step 4: Update Deployment on Waldorf

Once the image is built, update the deployment:

```bash
# Update the image in the deployment
kubectl set image deployment/gpu-pruner \
  container=ghcr.io/fuddin-bit/gpu-pruner:latest-otel \
  -n gpu-pruner-system \
  --context coreweave-waldorf

# Or apply the full manifest
kubectl apply -k gpu-pruner/hack/ --context coreweave-waldorf
```

## Step 5: Verify Dashboard is Running

```bash
# Check pod status
kubectl get pods -n gpu-pruner-system --context coreweave-waldorf

# Check logs for dashboard startup
kubectl logs -n gpu-pruner-system deployment/gpu-pruner \
  --context coreweave-waldorf | grep -i dashboard

# Get the route URL
kubectl get route -n gpu-pruner-system gpu-pruner-dashboard \
  --context coreweave-waldorf \
  -o jsonpath='{.spec.host}'
```

## Step 6: Access the Dashboard

Open the route URL in your browser. You should see:
- Total Pods Checked
- Idle Workloads count
- List of idle workloads with details

The dashboard auto-refreshes every 10 seconds.

## Alternative: Install Docker Desktop

If you prefer to build locally in the future:

1. Download Docker Desktop for Mac: https://www.docker.com/products/docker-desktop/
2. Install and start Docker Desktop
3. Then you can build locally:
   ```bash
   docker build -t ghcr.io/fuddin-bit/gpu-pruner:latest-otel -f Dockerfile.rhel .
   docker push ghcr.io/fuddin-bit/gpu-pruner:latest-otel
   ```

## Troubleshooting

### Image Pull Errors

If Kubernetes can't pull the image:

```bash
# Make sure the image is public or add imagePullSecrets
kubectl get deployment gpu-pruner -n gpu-pruner-system \
  --context coreweave-waldorf -o yaml | grep imagePullPolicy
```

### Dashboard Not Accessible

```bash
# Port-forward to test locally
kubectl port-forward -n gpu-pruner-system \
  deployment/gpu-pruner 8080:8080 \
  --context coreweave-waldorf

# Then access http://localhost:8080
```

### No Data in Dashboard

- Check prometheus connection is working
- Verify DCGM metrics are being collected
- Ensure the query is returning results (check logs)
- Wait 3-5 minutes for first query cycle to complete

## Quick Commands Reference

```bash
# Check current deployment image
kubectl get deployment gpu-pruner -n gpu-pruner-system \
  --context coreweave-waldorf \
  -o jsonpath='{.spec.template.spec.containers[0].image}'

# Restart deployment to pull new image
kubectl rollout restart deployment/gpu-pruner \
  -n gpu-pruner-system \
  --context coreweave-waldorf

# Watch rollout status
kubectl rollout status deployment/gpu-pruner \
  -n gpu-pruner-system \
  --context coreweave-waldorf

# Get route URL
kubectl get route gpu-pruner-dashboard \
  -n gpu-pruner-system \
  --context coreweave-waldorf \
  -o jsonpath='https://{.spec.host}{"\n"}'
```
