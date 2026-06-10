# GPU Utilization Queries

This document explains every PromQL query `gpu-pruner` uses to detect idle GPUs. The queries are rendered at runtime from `gpu-pruner/src/query.promql.j2` based on CLI flags.

## Prerequisites

- **DCGM exporter** running on GPU nodes
- **Prometheus** scraping DCGM metrics
- Port-forward for local testing:

```bash
kubectl port-forward -n llm-d-monitoring svc/llmd-kube-prometheus-stack-prometheus 9090:9090
```

Run queries at http://localhost:9090/graph or via curl:

```bash
curl -sG 'http://localhost:9090/api/v1/query' \
  --data-urlencode 'query=<PROMQL_HERE>' | jq .
```

## CLI flags that shape the queries

| Flag | Default | Effect on query |
|------|---------|-----------------|
| `--duration` / `-t` | `30` | Lookback window `[Nm]` in minutes |
| `--honor-labels` | `false` | Use `pod`/`namespace` instead of `exported_pod`/`exported_namespace` |
| `--namespace` / `-n` | none | Regex filter on namespace label |
| `--model-name` / `-m` | none | Regex filter on `modelName` |
| `--idle-threshold` | `0.01` | Max utilization (0.0–1.0) to still count as idle; tolerates DCGM noise |
| `--power-threshold` | none | Exclude idle candidates with high power draw |

---

## 1. Graphics engine active (primary metric)

Measures the fraction of time the GPU graphics/compute engine was active over the lookback window. Range: **0.0–1.0**.

### With `--honor-labels` (native DCGM labels)

```promql
max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{
  pod != ""
}[30m])
```

### Default (`exported_*` labels)

```promql
max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{
  exported_pod != ""
}[30m])
```

### With namespace filter (`--namespace=llm-d-optimized-baseline`)

```promql
max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{
  pod != "",
  namespace =~ "llm-d-optimized-baseline"
}[30m])
```

### With model filter (`--model-name="NVIDIA H200"`)

```promql
max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{
  pod != "",
  modelName =~ "NVIDIA H200"
}[30m])
```

**Notes:**
- `max_over_time` uses the **peak** value in the window, not the average.
- A value of `0` means the engine was never active during the window.
- Tiny non-zero values (e.g. `0.00007`) are DCGM background noise; gpu-pruner uses `< 0.01` by default (configurable via `--idle-threshold`) instead of strict `== 0`.

---

## 2. GPU utilization % (fallback metric)

Classic DCGM GPU utilization percentage. Divided by 100 so it matches the 0.0–1.0 scale of engine active.

### With `--honor-labels`

```promql
max_over_time(DCGM_FI_DEV_GPU_UTIL{
  pod != ""
}[30m]) / 100
```

### Default (`exported_*` labels)

```promql
max_over_time(DCGM_FI_DEV_GPU_UTIL{
  exported_pod != ""
}[30m]) / 100
```

**Notes:**
- Used as a fallback when `DCGM_FI_PROF_GR_ENGINE_ACTIVE` is missing for a series.
- When **both** metrics exist, PromQL `or` keeps the **left-hand** (engine active) value.

---

## 3. Combined utilization per GPU

Aggregates both metrics per GPU, grouped by pod, namespace, and hardware labels.

### With `--honor-labels`

```promql
sum by (Hostname, container, pod, namespace, gpu, modelName) (
  max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{
    pod != ""
  }[30m])
  or
  max_over_time(DCGM_FI_DEV_GPU_UTIL{
    pod != ""
  }[30m]) / 100
)
```

### Default (`exported_*` labels)

```promql
sum by (Hostname, exported_container, exported_pod, exported_namespace, gpu, modelName) (
  max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{
    exported_pod != ""
  }[30m])
  or
  max_over_time(DCGM_FI_DEV_GPU_UTIL{
    exported_pod != ""
  }[30m]) / 100
)
```

**Notes:**
- This is the core "is this GPU busy?" calculation.
- `sum by` collapses duplicate label sets (one series per GPU).
- Result `0` = idle; non-zero = some activity detected.

---

## 4. Node type enrichment (optional join)

Joins GPU metrics with `node_dmi_info` to attach hardware `node_type`. Falls back to un-enriched results when node info is missing.

```promql
sum by (Hostname, container, pod, namespace, gpu, modelName) (
  max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{pod != ""}[30m])
  or
  max_over_time(DCGM_FI_DEV_GPU_UTIL{pod != ""}[30m]) / 100
)
* on (Hostname) group_left(node_type) (
  label_replace(
    label_replace(node_dmi_info,
      "Hostname", "$1", "instance", "(.+)"
    ),
    "node_type", "$1", "product_name", "(.+)"
  )
)
or on (Hostname, container, pod, namespace, gpu, modelName)
sum by (Hostname, container, pod, namespace, gpu, modelName) (
  max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{pod != ""}[30m])
  or
  max_over_time(DCGM_FI_DEV_GPU_UTIL{pod != ""}[30m]) / 100
)
```

**Notes:**
- The `or` at the end ensures GPUs still appear even when `node_dmi_info` has no match.
- `node_type` is informational; it does not affect idle detection.

---

## 5. Full gpu-pruner idle detection query

This is the complete query rendered and sent to Prometheus. Returns GPUs considered **idle** (combined utilization below `--idle-threshold`, default `0.01`).

### With `--honor-labels`, `--duration=30`

```promql
(
  sum by (Hostname, container, pod, namespace, gpu, modelName) (
    max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{
      pod != ""
    }[30m])
    or
    max_over_time(DCGM_FI_DEV_GPU_UTIL{
      pod != ""
    }[30m]) / 100
  )
  * on (Hostname) group_left(node_type) (
    label_replace(
      label_replace(node_dmi_info,
        "Hostname", "$1", "instance", "(.+)"
      ),
      "node_type", "$1", "product_name", "(.+)"
    )
  )
  or on (Hostname, container, pod, namespace, gpu, modelName)
  sum by (Hostname, container, pod, namespace, gpu, modelName) (
    max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{
      pod != ""
    }[30m])
    or
    max_over_time(DCGM_FI_DEV_GPU_UTIL{
      pod != ""
    }[30m]) / 100
  )
) < 0.01
```

### Default (`exported_*` labels), `--duration=30`

```promql
(
  sum by (Hostname, exported_container, exported_pod, exported_namespace, gpu, modelName) (
    max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{
      exported_pod != ""
    }[30m])
    or
    max_over_time(DCGM_FI_DEV_GPU_UTIL{
      exported_pod != ""
    }[30m]) / 100
  )
  * on (Hostname) group_left(node_type) (
    label_replace(
      label_replace(node_dmi_info,
        "Hostname", "$1", "instance", "(.+)"
      ),
      "node_type", "$1", "product_name", "(.+)"
    )
  )
  or on (Hostname, exported_container, exported_pod, exported_namespace, gpu, modelName)
  sum by (Hostname, exported_container, exported_pod, exported_namespace, gpu, modelName) (
    max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{
      exported_pod != ""
    }[30m])
    or
    max_over_time(DCGM_FI_DEV_GPU_UTIL{
      exported_pod != ""
    }[30m]) / 100
  )
) < 0.01
```

**Notes:**
- Any series returned = gpu-pruner treats that GPU as idle.
- Override with `--idle-threshold=0.05` for a looser definition of idle.
- After the query, gpu-pruner resolves each pod to a scalable parent (Deployment, StatefulSet, etc.) in Kubernetes.
- Infrastructure pods (e.g. `dcgm-exporter` DaemonSets) may match this query but are skipped because they have no scalable root object.

---

## 6. Power draw exclusion (optional, `--power-threshold`)

When set, appends a `unless` clause to exclude GPUs that drew at or above the threshold (watts) during the lookback window, even if utilization is zero.

### Example: `--power-threshold=150` with `--honor-labels`

Full query becomes the idle query above, plus:

```promql
unless on (pod, namespace)
(
  max_over_time(DCGM_FI_DEV_POWER_USAGE{
    pod != ""
  }[30m]) >= 150
)
```

### Example: `--power-threshold=150` with default labels

```promql
unless on (exported_pod, exported_namespace)
(
  max_over_time(DCGM_FI_DEV_POWER_USAGE{
    exported_pod != ""
  }[30m]) >= 150
)
```

**Notes:**
- Useful to catch "compute idle but still drawing power" cases.
- Suggested starting points: `100` (A10G), `150` (A100/H100).

---

## 7. Manual testing queries

Simplified queries for debugging in Prometheus or curl.

### Count idle GPUs (gpu-pruner default, `< 0.01`)

```promql
(
  sum by (pod, namespace, gpu) (
    max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{pod != ""}[5m])
    or
    max_over_time(DCGM_FI_DEV_GPU_UTIL{pod != ""}[5m]) / 100
  )
) < 0.01
```

```bash
curl -sG 'http://localhost:9090/api/v1/query' \
  --data-urlencode 'query=(sum by (pod, namespace, gpu) (max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{pod != ""}[5m]) or max_over_time(DCGM_FI_DEV_GPU_UTIL{pod != ""}[5m]) / 100)) < 0.01' \
  | jq '.data.result | length'
```

### Inspect raw engine active for a workload

```promql
max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{
  pod =~ "optimized-baseline.*"
}[5m])
```

### Inspect raw GPU util % for a workload

```promql
max_over_time(DCGM_FI_DEV_GPU_UTIL{
  pod =~ "optimized-baseline.*"
}[5m])
```

### See the combined value gpu-pruner uses

```promql
sum by (pod, namespace, gpu) (
  max_over_time(DCGM_FI_PROF_GR_ENGINE_ACTIVE{
    pod =~ "optimized-baseline.*"
  }[5m])
  or
  max_over_time(DCGM_FI_DEV_GPU_UTIL{
    pod =~ "optimized-baseline.*"
  }[5m]) / 100
)
```

### Verify DCGM metrics exist

```promql
DCGM_FI_PROF_GR_ENGINE_ACTIVE
```

```promql
DCGM_FI_DEV_GPU_UTIL
```

### Check which label convention your cluster uses

```promql
count(DCGM_FI_PROF_GR_ENGINE_ACTIVE{exported_pod != ""})
```

```promql
count(DCGM_FI_PROF_GR_ENGINE_ACTIVE{pod != ""})
```

If the second count is non-zero and the first is zero, use `--honor-labels` with gpu-pruner.

---

## 8. Example: matching gpu-pruner CLI to query

This command:

```bash
cargo run --bin gpu-pruner -- \
  --prometheus-url=http://localhost:9090 \
  --run-mode=dry-run \
  --duration=5 \
  --honor-labels \
  --namespace=llm-d-optimized-baseline
```

Renders a query equivalent to section 5 with `[5m]`, `pod`/`namespace` labels, and `namespace =~ "llm-d-optimized-baseline"` on both DCGM metric selectors.

---

## Common pitfalls

| Symptom | Likely cause |
|---------|----------------|
| Query returns 0 series | Wrong label convention; try `--honor-labels` or `exported_*` labels |
| Pods running but not idle | Utilization above `--idle-threshold` (default `0.01`) |
| Idle GPUs found but no scale-down | Pod owner is a DaemonSet or unsupported resource type |
| vLLM pods show `0%` util but not idle | `DCGM_FI_PROF_GR_ENGINE_ACTIVE` ≈ `0.00007` wins over `DCGM_FI_DEV_GPU_UTIL` in `or` |
| New pods never pruned | `--grace-period` (default 300s) adds extra age check in application logic after the query |

---

## Source

Queries are defined in:

- Template: `gpu-pruner/src/query.promql.j2`
- Rendered in: `gpu-pruner/src/main.rs` (`Running w/ Query:` log line)
- Tests: `gpu-pruner/src/main.rs` (`query_*` unit tests)
