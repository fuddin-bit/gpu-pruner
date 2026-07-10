export type TimeWindow = "1h" | "7d" | "30d";

export interface ScaleDownsSummary {
  lifetime: number;
}

export interface IdleWorkloadsSummary {
  current: number;
}

export interface SummaryResponse {
  scale_downs: ScaleDownsSummary;
  idle_workloads: IdleWorkloadsSummary;
  pods_checked: number;
  updated_at: string;
}

export interface ScaleDownsStats {
  lifetime: number;
  in_window?: number;
  window: string;
}

export interface StatsResponse {
  scale_downs: ScaleDownsStats;
  idle_workloads: IdleWorkloadsSummary;
  prometheus_available: boolean;
  pods_checked: number;
  updated_at: string;
}

async function parseJson<T>(response: Response): Promise<T> {
  if (!response.ok) {
    throw new Error(`Request failed: ${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<T>;
}

export async function fetchSummary(): Promise<SummaryResponse> {
  return parseJson<SummaryResponse>(await fetch("/api/v1/summary"));
}

export async function fetchStats(window: TimeWindow): Promise<StatsResponse> {
  return parseJson<StatsResponse>(
    await fetch(`/api/v1/stats?window=${encodeURIComponent(window)}`),
  );
}

export interface IdleGpuHoursEntry {
  rank: number;
  namespace: string;
  pod: string;
  idle_hours: number;
}

export interface IdleGpuHoursResponse {
  entries: IdleGpuHoursEntry[];
  prometheus_available: boolean;
  updated_at: string;
}

// Kermit DCGM scrape keeps exporter identity on namespace/pod and the real
// workload on exported_namespace/exported_pod. Waldorf (honorLabels) flips
// that; use --honor-labels + the namespace/pod form of this query there.
const IDLE_GPU_HOURS_QUERY =
  'sort_desc(sum by (exported_namespace, exported_pod) (count_over_time((DCGM_FI_PROF_GR_ENGINE_ACTIVE{exported_namespace!~"llm-d-nightly-.*|bench-guide-.*|cw-.*",exported_pod!="",exported_pod!~"dcgm-exporter-.*"} < 0.01)[7d:1m]) / 60))';

const IDLE_GPU_HOURS_LIMIT = 25;

interface PrometheusInstantVectorResponse {
  status?: string;
  data?: {
    resultType?: string;
    result?: Array<{
      metric?: Record<string, string>;
      value?: [number, string];
    }>;
  };
}

export async function fetchIdleGpuHours(): Promise<IdleGpuHoursResponse> {
  const updatedAt = new Date().toISOString();

  try {
    const response = await fetch(
      `/prom/api/v1/query?query=${encodeURIComponent(IDLE_GPU_HOURS_QUERY)}`,
    );

    if (!response.ok) {
      return {
        entries: [],
        prometheus_available: false,
        updated_at: updatedAt,
      };
    }

    const body = (await response.json()) as PrometheusInstantVectorResponse;
    if (body.status !== "success") {
      return {
        entries: [],
        prometheus_available: false,
        updated_at: updatedAt,
      };
    }

    const results = body.data?.result ?? [];
    const entries: IdleGpuHoursEntry[] = [];

    for (const result of results) {
      if (entries.length >= IDLE_GPU_HOURS_LIMIT) {
        break;
      }

      const namespace =
        result.metric?.namespace ?? result.metric?.exported_namespace;
      const pod = result.metric?.pod ?? result.metric?.exported_pod;
      if (!namespace || !pod) {
        continue;
      }

      const raw = result.value?.[1];
      if (raw == null) {
        continue;
      }

      const idleHours = Number(raw);
      if (!Number.isFinite(idleHours)) {
        continue;
      }

      entries.push({
        rank: entries.length + 1,
        namespace,
        pod,
        idle_hours: idleHours,
      });
    }

    return {
      entries,
      prometheus_available: true,
      updated_at: updatedAt,
    };
  } catch {
    return {
      entries: [],
      prometheus_available: false,
      updated_at: updatedAt,
    };
  }
}
