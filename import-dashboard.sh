#!/bin/bash
#
# Import GPU Dashboard to Grafana
#
# Usage: ./import-dashboard.sh [grafana-url] [admin-password]
#
# Example: ./import-dashboard.sh http://localhost:3000 GpuPruner2026!

GRAFANA_URL="${1:-http://localhost:3000}"
ADMIN_PASSWORD="${2:-GpuPruner2026!}"
DASHBOARD_FILE="gpu-dashboard.json"

echo "Importing GPU Dashboard to Grafana..."
echo "Grafana URL: $GRAFANA_URL"

# Test Grafana connectivity
echo -n "Testing Grafana connectivity... "
if curl -s "$GRAFANA_URL/api/health" | grep -q "ok"; then
    echo "✓ Connected"
else
    echo "✗ Failed to connect to Grafana"
    exit 1
fi

# Prepare dashboard JSON (wrap in API format)
DASHBOARD_JSON=$(cat "$DASHBOARD_FILE" | jq '{dashboard: ., overwrite: true, folderId: 0}')

# Import dashboard
echo -n "Importing dashboard... "
RESPONSE=$(curl -s -X POST \
    -H "Content-Type: application/json" \
    -u "admin:$ADMIN_PASSWORD" \
    -d "$DASHBOARD_JSON" \
    "$GRAFANA_URL/api/dashboards/db")

if echo "$RESPONSE" | jq -e '.status == "success"' > /dev/null 2>&1; then
    echo "✓ Success"
    DASHBOARD_URL=$(echo "$RESPONSE" | jq -r '.url')
    echo ""
    echo "Dashboard imported successfully!"
    echo "Access it at: $GRAFANA_URL$DASHBOARD_URL"
else
    echo "✗ Failed"
    echo ""
    echo "Error response:"
    echo "$RESPONSE" | jq .
    exit 1
fi
