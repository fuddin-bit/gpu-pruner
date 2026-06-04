# CoreWeave Kubernetes (CKS) Ingress Guide

This guide explains how to expose services for external access in CoreWeave Kubernetes Service (CKS).

## Overview

CoreWeave uses a **LoadBalancer + DNS annotation** pattern rather than traditional Ingress controllers. The cluster has **Istio** installed but standard users don't have permissions to create Gateway API resources or VirtualServices.

## Available Methods

### Method 1: LoadBalancer Service with DNS (Recommended for CKS)

CoreWeave provides an **External Hostname Controller** that automatically creates DNS records for LoadBalancer services.

#### How It Works

1. Create a LoadBalancer service
2. Add the `service.beta.kubernetes.io/external-hostname` annotation
3. CoreWeave assigns a public IP and creates a DNS record in `.coreweave.app` domain
4. DNS status is reflected in `.status.conditions` field of the Service

#### Example: Expose Grafana with LoadBalancer

**IMPORTANT**: You must add the `service.beta.kubernetes.io/coreweave-load-balancer-type: public` annotation to get a **public IP**. Without this annotation, CoreWeave assigns an internal VIP only.

```yaml
apiVersion: v1
kind: Service
metadata:
  name: gpu-grafana
  namespace: fuddin-dev
  annotations:
    service.beta.kubernetes.io/coreweave-load-balancer-type: "public"  # REQUIRED for public IP
    service.beta.kubernetes.io/external-hostname: "gpu-grafana"
    # This creates: gpu-grafana-<hash>.coreweave.app
spec:
  type: LoadBalancer
  selector:
    app.kubernetes.io/name: grafana
    app.kubernetes.io/instance: gpu-grafana
  ports:
    - name: http
      port: 80
      targetPort: 3000
      protocol: TCP
```

Apply and check the assigned hostname:

```bash
kubectl apply -f grafana-loadbalancer.yaml

# Wait for external IP assignment
kubectl get svc gpu-grafana -n fuddin-dev -w

# Check the assigned DNS name in status
kubectl get svc gpu-grafana -n fuddin-dev -o jsonpath='{.status.conditions[?(@.type=="ExternalRecords")].message}'
```

The service will be accessible at: `http://gpu-grafana-<hash>.coreweave.app`

#### Wildcard DNS

For wildcard DNS records (e.g., for multiple subdomains):

```yaml
metadata:
  annotations:
    service.beta.kubernetes.io/external-hostname: "*"
    # Creates: *.abc123-mycluster.coreweave.app
```

### Method 2: Port-Forward (Development/Testing)

For temporary access without exposing services publicly:

```bash
# Forward local port 3000 to Grafana service
kubectl port-forward -n fuddin-dev svc/gpu-grafana 3000:80

# Access at http://localhost:3000
```

**Pros**: 
- No cluster configuration needed
- Works immediately
- No public exposure

**Cons**:
- Only accessible from your machine
- Connection breaks when command terminates
- Not suitable for production

### Method 3: Istio VirtualService (Requires Permissions)

CoreWeave has **Istio** installed, but standard users don't have permissions to create VirtualServices or Gateways. This method requires cluster admin assistance.

If you have permissions, you would create:

```yaml
apiVersion: networking.istio.io/v1
kind: VirtualService
metadata:
  name: grafana-vs
  namespace: fuddin-dev
spec:
  hosts:
    - "grafana.example.com"
  gateways:
    - istio-system/public-gateway  # Shared cluster gateway
  http:
    - match:
        - uri:
            prefix: /
      route:
        - destination:
            host: gpu-grafana.fuddin-dev.svc.cluster.local
            port:
              number: 80
```

**Note**: This requires a shared Gateway to exist and permissions to create VirtualServices.

## Comparison of Methods

| Method | Access | Setup Complexity | Cost | Use Case |
|--------|--------|------------------|------|----------|
| **LoadBalancer + DNS** | Public internet | Low | Charges for public IP | Production, public dashboards |
| **Port-Forward** | Local only | Very low | Free | Development, debugging |
| **Istio VirtualService** | Shared gateway | Medium | Shared cost | Multi-service routing, advanced traffic control |

## Recommended Approach for Grafana

### Option A: LoadBalancer (Public Access)

Best for production Grafana instance that multiple team members need to access.

```bash
# Update Grafana service to LoadBalancer
kubectl patch svc gpu-grafana -n fuddin-dev -p '{"spec":{"type":"LoadBalancer"}}'

# Add REQUIRED annotation for public IP
kubectl annotate svc gpu-grafana -n fuddin-dev \
  service.beta.kubernetes.io/coreweave-load-balancer-type="public"

# Add DNS annotation
kubectl annotate svc gpu-grafana -n fuddin-dev \
  service.beta.kubernetes.io/external-hostname="gpu-grafana"

# Wait for external IP
kubectl get svc gpu-grafana -n fuddin-dev -w

# Get the public IP
kubectl get svc gpu-grafana -n fuddin-dev -o jsonpath='{.status.loadBalancer.ingress[0].ip}'
```

### Option B: Port-Forward (Personal Access)

Best for personal dashboards or development:

```bash
# Add to your shell profile for automatic port-forward
alias grafana-forward='kubectl port-forward -n fuddin-dev svc/gpu-grafana 3000:80'

# Run whenever you need access
grafana-forward
```

## Current Grafana Setup

Your Grafana is currently deployed with:

- **Service Type**: ClusterIP (internal only)
- **Namespace**: `fuddin-dev`
- **Port**: 80 (service) → 3000 (pod)
- **Access Method**: Port-forward only

### Convert to LoadBalancer

```bash
# Method 1: kubectl patch
kubectl patch svc gpu-grafana -n fuddin-dev -p '{"spec":{"type":"LoadBalancer"}}'
kubectl annotate svc gpu-grafana -n fuddin-dev \
  service.beta.kubernetes.io/external-hostname="gpu-grafana-fuddin"

# Method 2: Helm upgrade
helm upgrade gpu-grafana grafana/grafana \
  --reuse-values \
  --set service.type=LoadBalancer \
  --set service.annotations."service\.beta\.kubernetes\.io/external-hostname"="gpu-grafana-fuddin" \
  -n fuddin-dev
```

## Cluster Architecture

CoreWeave Kubernetes (CKS) uses:

- **Istio** for service mesh (installed at cluster level)
- **Gateway API** (available but restricted permissions)
- **External Hostname Controller** for automatic DNS provisioning
- **LoadBalancer** services get public IPs automatically

### Installed Components

```bash
# Istio control plane
kubectl get svc -n istio-system istiod
# NAME     TYPE        CLUSTER-IP    EXTERNAL-IP   PORT(S)
# istiod   ClusterIP   10.16.0.170   <none>        15010/TCP,15012/TCP,443/TCP,15014/TCP

# Gateway API CRDs available
kubectl api-resources | grep gateway
# httproutes
# gateways.gateway.networking.k8s.io
# virtualservices (Istio)
```

### Permissions

Standard users in CKS can:
- ✅ Create/modify Services in their namespace
- ✅ Use LoadBalancer service type
- ✅ Add DNS annotations
- ❌ Create Gateway resources
- ❌ Create HTTPRoute resources
- ❌ Create VirtualService resources (Istio)
- ❌ List cluster-wide resources

## Troubleshooting

### LoadBalancer stuck in "Pending"

```bash
kubectl describe svc gpu-grafana -n fuddin-dev

# Check events for errors
kubectl get events -n fuddin-dev --sort-by='.lastTimestamp' | grep gpu-grafana
```

Common causes:
- Quota limits on public IPs
- Invalid annotation format
- Namespace resource limits

### DNS not resolving

```bash
# Check service status
kubectl get svc gpu-grafana -n fuddin-dev -o yaml

# Look for ExternalRecords condition
kubectl get svc gpu-grafana -n fuddin-dev -o jsonpath='{.status.conditions[?(@.type=="ExternalRecords")]}'
```

The DNS record creation may take 1-2 minutes after the external IP is assigned.

### Port-forward connection refused

```bash
# Check if pod is running
kubectl get pods -n fuddin-dev -l app.kubernetes.io/name=grafana

# Check pod logs
kubectl logs -n fuddin-dev -l app.kubernetes.io/name=grafana --tail=50

# Test service internally
kubectl run -it --rm debug --image=curlimages/curl --restart=Never -- \
  curl http://gpu-grafana.fuddin-dev.svc.cluster.local
```

## Cost Considerations

- **Public IPs**: CoreWeave charges for LoadBalancer public IPs
- **Bandwidth**: Egress traffic may have costs
- **Port-Forward**: No additional cost (uses cluster credentials)

For cost-effective access:
1. Use port-forward for personal/development access
2. Use LoadBalancer only for production services that need public access
3. Share one LoadBalancer across multiple services using path-based routing (requires Istio VirtualService with permissions)

## Security Best Practices

### For LoadBalancer Services

1. **Enable authentication** in Grafana (already configured with admin password)
2. **Use HTTPS**: Add TLS certificate
3. **Restrict source IPs**: Use `loadBalancerSourceRanges`
4. **Monitor access logs**: Enable Grafana audit logging
5. **Use NetworkPolicies**: Restrict pod-to-pod communication

```yaml
spec:
  type: LoadBalancer
  loadBalancerSourceRanges:
    - "1.2.3.4/32"      # Your office IP
    - "5.6.7.8/24"      # Your VPN range
```

### For Port-Forward

- ✅ Automatically secured by Kubernetes RBAC
- ✅ Requires valid cluster credentials
- ✅ No public exposure
- ⚠️ Ensure your local machine is secured

## Next Steps

1. **Decide on access method**:
   - Public access → Use LoadBalancer with DNS
   - Personal access → Use port-forward

2. **If using LoadBalancer**:
   ```bash
   kubectl patch svc gpu-grafana -n fuddin-dev -p '{"spec":{"type":"LoadBalancer"}}'
   kubectl annotate svc gpu-grafana -n fuddin-dev \
     service.beta.kubernetes.io/external-hostname="gpu-grafana-fuddin"
   ```

3. **Monitor the service**:
   ```bash
   kubectl get svc gpu-grafana -n fuddin-dev -w
   ```

4. **Access Grafana**:
   - LoadBalancer: Wait for DNS record, then access via `http://<assigned-dns>.coreweave.app`
   - Port-forward: `kubectl port-forward -n fuddin-dev svc/gpu-grafana 3000:80`

## References

- [Create a Public DNS Name | CoreWeave](https://docs.coreweave.com/docs/products/networking/how-to/expose-service-dns)
- [Introduction to CoreWeave Kubernetes Service | CoreWeave](https://docs.coreweave.com/docs/products/cks)
- [Kubernetes Ingress Documentation](https://kubernetes.io/docs/concepts/services-networking/ingress/)
- [Exposing Applications for External Access | Kube by Example](https://kubebyexample.com/learning-paths/application-development-kubernetes/lesson-3-networking-kubernetes/exposing-0)

## Summary

**CoreWeave uses LoadBalancer services with DNS annotations, not traditional Ingress controllers.**

For your Grafana deployment:
- **Quick access**: `kubectl port-forward -n fuddin-dev svc/gpu-grafana 3000:80`
- **Public access**: Convert service to LoadBalancer with DNS annotation
- **Advanced routing**: Request VirtualService permissions from cluster admin

The simplest production-ready approach is to use LoadBalancer with the `service.beta.kubernetes.io/external-hostname` annotation.
