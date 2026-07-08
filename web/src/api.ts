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
