import type { IncomingMessage, ServerResponse } from "http";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";

const PROMETHEUS_URL = process.env.PROMETHEUS_URL ?? "http://localhost:9090";
const GPU_PRUNER_URL =
  process.env.GPU_PRUNER_URL ??
  process.env.GPU_PRUNER_METRICS_URL ??
  "http://localhost:8080";

const ALLOWED_WINDOWS = new Set(["1h", "7d", "30d"]);

function parseMetricValue(metricsText: string, name: string): number | null {
  const pattern = new RegExp(`^${name}(?:\\{[^}]*\\})?\\s+(\\S+)`, "gm");
  let total = 0;
  let found = false;

  for (const match of metricsText.matchAll(pattern)) {
    const value = Number(match[1]);
    if (!Number.isFinite(value)) {
      continue;
    }
    total += value;
    found = true;
  }

  return found ? total : null;
}

function parsePrometheusScalar(data: {
  data?: { result?: Array<{ value?: [number, string] }> };
}): number | null {
  const results = data.data?.result ?? [];
  let total = 0;
  let found = false;

  for (const result of results) {
    const raw = result.value?.[1];
    if (raw == null) {
      continue;
    }
    const value = Number(raw);
    if (!Number.isFinite(value)) {
      continue;
    }
    total += value;
    found = true;
  }

  return found ? Math.max(0, Math.round(total)) : null;
}

async function fetchText(url: string): Promise<string> {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.text();
}

function sendJson(res: ServerResponse, status: number, body: unknown) {
  res.statusCode = status;
  res.setHeader("Content-Type", "application/json");
  res.end(JSON.stringify(body));
}

function kermitDevApi(): Plugin {
  return {
    name: "kermit-dev-api",
    configureServer(server) {
      server.middlewares.use(
        async (req: IncomingMessage, res: ServerResponse, next) => {
          if (!req.url?.startsWith("/api/v1/stats")) {
            next();
            return;
          }

          const url = new URL(req.url, "http://localhost");
          const window = url.searchParams.get("window") ?? "7d";

          if (!ALLOWED_WINDOWS.has(window)) {
            sendJson(res, 400, {
              error: "invalid window",
              allowed: [...ALLOWED_WINDOWS],
            });
            return;
          }

          try {
            const [promResponse, metricsText] = await Promise.all([
              fetch(
                `${PROMETHEUS_URL}/api/v1/query?query=${encodeURIComponent(
                  `sum(increase(gpu_pruner_scale_successes_total[${window}]))`,
                )}`,
              ),
              fetchText(`${GPU_PRUNER_URL}/metrics`),
            ]);

            let inWindow: number | null = null;
            let prometheusAvailable = false;

            if (promResponse.ok) {
              const promData = (await promResponse.json()) as {
                data?: { result?: Array<{ value?: [number, string] }> };
              };
              const parsed = parsePrometheusScalar(promData);
              if (parsed != null) {
                inWindow = parsed;
                prometheusAvailable = true;
              }
            }

            const lifetime = parseMetricValue(
              metricsText,
              "gpu_pruner_scale_successes_total",
            );
            const idleWorkloads = parseMetricValue(
              metricsText,
              "gpu_pruner_idle_gpus",
            );
            const podsChecked = parseMetricValue(
              metricsText,
              "gpu_pruner_pods_checked_total",
            );

            sendJson(res, 200, {
              scale_downs: {
                lifetime: lifetime ?? 0,
                in_window: inWindow ?? undefined,
                window,
              },
              idle_workloads: {
                current: idleWorkloads ?? 0,
              },
              prometheus_available: prometheusAvailable,
              pods_checked: podsChecked ?? 0,
              updated_at: new Date().toISOString(),
            });
          } catch (error) {
            const message =
              error instanceof Error ? error.message : "Failed to load stats";
            sendJson(res, 502, { error: message });
          }
        },
      );
    },
  };
}

export default defineConfig({
  plugins: [react(), kermitDevApi()],
  base: "/",
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  server: {
    proxy: {
      "/api": {
        target: GPU_PRUNER_URL,
        bypass(req) {
          if (req.url?.startsWith("/api/v1/stats")) {
            return req.url;
          }
        },
      },
      "/metrics": GPU_PRUNER_URL,
      "/prom": {
        target: PROMETHEUS_URL,
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/prom/, ""),
      },
    },
  },
});
