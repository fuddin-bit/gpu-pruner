#!/usr/bin/env bash
# Deploy gpu-pruner to a namespace you control (skips ClusterRole/Bindings/ServiceMonitor).
#
# Usage:
#   NS=fuddin-dev ./gpu-pruner/hack/apply-user-namespace.sh
#   NS=rob-dev CTX=coreweave-waldorf ./gpu-pruner/hack/apply-user-namespace.sh
set -euo pipefail

NS="${NS:-fuddin-dev}"
CTX="${CTX:-coreweave-waldorf}"
HACK="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

apply() {
  local file="$1"
  sed -e "s/namespace: gpu-pruner-system/namespace: ${NS}/" \
      -e "s/external-hostname: \"gpu-pruner-slack\"/external-hostname: \"gpu-pruner-slack-${NS}\"/g" \
    "${HACK}/${file}" | kubectl apply -f - --context "${CTX}"
}

echo "Applying gpu-pruner to namespace=${NS} context=${CTX}"

apply deployment.yaml
apply serviceaccount.yaml
apply service.yaml
apply slack-interactions-service-lb.yaml

echo
echo "Done. Next steps:"
echo "  1. Create slack webhook secret if needed:"
echo "     kubectl create secret generic gpu-pruner-slack-webhook -n ${NS} --context ${CTX} \\"
echo "       --from-literal=webhook-url='https://hooks.slack.com/services/...'"
echo "  2. Ask admin to bind ServiceAccount gpu-pruner in ${NS} to ClusterRoles gpu-pruner-cr and cluster-monitoring-view"
echo "  3. Wait for LoadBalancer DNS, then set Slack Request URL:"
echo "     kubectl get svc gpu-pruner-slack -n ${NS} --context ${CTX} \\"
echo "       -o jsonpath='http://{.status.conditions[?(@.type==\"ExternalRecords\")].message}/slack/interactions'"
echo
echo "  NOTE: Slack requires HTTPS. CoreWeave LoadBalancers expose HTTP by default."
echo "  If Slack URL verification fails, use ngrok locally or ask admin for TLS in front of the LB."
