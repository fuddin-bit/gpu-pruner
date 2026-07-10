import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  Card,
  CardBody,
  CardTitle,
  Skeleton,
} from "@patternfly/react-core";
import {
  fetchIdleGpuHours,
  type IdleGpuHoursResponse,
} from "../api";

const REFRESH_INTERVAL_MS = 30_000;

function formatIdleHours(value: number): string {
  return value.toFixed(1);
}

function formatUpdatedAt(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return iso;
  }
  return date.toLocaleString();
}

export function IdleGpuLeaderboard() {
  const [data, setData] = useState<IdleGpuHoursResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setError(null);
      const response = await fetchIdleGpuHours();
      setData(response);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to load idle GPU hours",
      );
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    setLoading(true);
    void load();
  }, [load]);

  useEffect(() => {
    const timer = globalThis.setInterval(() => {
      void load();
    }, REFRESH_INTERVAL_MS);
    return () => globalThis.clearInterval(timer);
  }, [load]);

  return (
    <Card style={{ marginTop: "1.5rem" }}>
      <CardTitle>Top Idle GPU Hours (7 Days)</CardTitle>
      <CardBody>
        {error && (
          <Alert variant="danger" title="Failed to load leaderboard" isInline>
            {error}
          </Alert>
        )}

        {!data?.prometheus_available && data && !loading && !error && (
          <Alert
            variant="warning"
            title="Prometheus unavailable"
            isInline
            style={{ marginBottom: "1rem" }}
          >
            Could not query Prometheus for idle GPU hours via /prom. Check
            that the dashboard can reach the configured Prometheus URL.
          </Alert>
        )}

        {loading ? (
          <Skeleton height="200px" />
        ) : data?.prometheus_available && data.entries.length === 0 ? (
          <p style={{ color: "#6a6e73" }}>No idle GPU hours found.</p>
        ) : data?.prometheus_available ? (
          <>
            <table
              style={{
                width: "100%",
                borderCollapse: "collapse",
                fontSize: "0.875rem",
              }}
            >
              <thead>
                <tr style={{ textAlign: "left", borderBottom: "2px solid #d2d2d2" }}>
                  <th style={{ padding: "0.5rem 0.75rem" }}>Rank</th>
                  <th style={{ padding: "0.5rem 0.75rem" }}>Namespace</th>
                  <th style={{ padding: "0.5rem 0.75rem" }}>Pod</th>
                  <th style={{ padding: "0.5rem 0.75rem", textAlign: "right" }}>
                    Idle Hours
                  </th>
                </tr>
              </thead>
              <tbody>
                {data.entries.map((entry) => (
                  <tr
                    key={`${entry.namespace}/${entry.pod}`}
                    style={{ borderBottom: "1px solid #f0f0f0" }}
                  >
                    <td style={{ padding: "0.5rem 0.75rem" }}>{entry.rank}</td>
                    <td style={{ padding: "0.5rem 0.75rem" }}>
                      {entry.namespace}
                    </td>
                    <td style={{ padding: "0.5rem 0.75rem" }}>{entry.pod}</td>
                    <td
                      style={{
                        padding: "0.5rem 0.75rem",
                        textAlign: "right",
                        fontVariantNumeric: "tabular-nums",
                      }}
                    >
                      {formatIdleHours(entry.idle_hours)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p
              style={{
                marginTop: "1rem",
                color: "#6a6e73",
                fontSize: "0.875rem",
              }}
            >
              Updated {formatUpdatedAt(data.updated_at)}. Auto-refreshes every
              30 seconds.
            </p>
          </>
        ) : null}
      </CardBody>
    </Card>
  );
}
