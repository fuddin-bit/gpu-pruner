use anyhow::Result;
use reqwest::Client;
use serde_json::json;

use crate::Meta;

#[derive(Clone, Debug)]
pub struct SlackNotifier {
    webhook_url: String,
    client: Client,
    channel: String,
}

impl SlackNotifier {
    pub fn new(webhook_url: String, channel: String) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build slack http client");

        Self {
            webhook_url,
            client,
            channel,
        }
    }

    #[tracing::instrument(skip(self, workload))]
    pub async fn send_notification<T: Meta + std::fmt::Debug>(
        &self,
        workload: &T,
        idle_duration_minutes: i64,
        ack_grace_period_secs: u64,
    ) -> Result<()> {
        let resource_type = workload.kind();
        let resource_name = workload.name();
        let namespace = workload
            .namespace()
            .unwrap_or_else(|| "default".to_string());

        let grace_minutes = ack_grace_period_secs / 60;
        let grace_label = if ack_grace_period_secs % 60 == 0 && grace_minutes > 0 {
            format!("{grace_minutes} minutes")
        } else {
            format!("{ack_grace_period_secs} seconds")
        };

        // Encode workload info in button values: kind:namespace:name:duration
        let button_value_4h = format!("{}:{}:{}:4", resource_type, namespace, resource_name);
        let button_value_8h = format!("{}:{}:{}:8", resource_type, namespace, resource_name);
        let button_value_24h = format!("{}:{}:{}:24", resource_type, namespace, resource_name);

        let payload = json!({
            "channel": self.channel,
            "attachments": [{
                "callback_id": "ack_idle_gpu",
                "color": "warning",
                "title": "Idle GPU Detected",
                "fields": [
                    {
                        "title": "Resource",
                        "value": format!("{}: {}", resource_type, resource_name),
                        "short": true
                    },
                    {
                        "title": "Namespace",
                        "value": namespace,
                        "short": true
                    },
                    {
                        "title": "Reason",
                        "value": format!("GPU idle for {} minutes", idle_duration_minutes),
                        "short": false
                    },
                    {
                        "title": "Action",
                        "value": format!("You have {grace_label} to acknowledge before scale-down"),
                        "short": false
                    }
                ],
                "actions": [
                    {
                        "name": "ack",
                        "text": "Keep 4h",
                        "type": "button",
                        "value": button_value_4h,
                        "style": "primary"
                    },
                    {
                        "name": "ack",
                        "text": "Keep 8h",
                        "type": "button",
                        "value": button_value_8h,
                        "style": "primary"
                    },
                    {
                        "name": "ack",
                        "text": "Keep 24h",
                        "type": "button",
                        "value": button_value_24h,
                        "style": "primary"
                    }
                ],
                "footer": "gpu-pruner",
                "ts": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            }]
        });

        tracing::debug!("Sending Slack notification payload: {:?}", payload);

        let response = self
            .client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::error!(
                status = %status,
                body = %body,
                "Slack webhook returned error status"
            );
            return Err(anyhow::anyhow!(
                "Slack webhook failed with status {}: {}",
                status,
                body
            ));
        }

        tracing::info!("Sent Slack notification for [{resource_type}] {namespace}:{resource_name}",);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slack_notifier_creation() {
        let notifier = SlackNotifier::new(
            "https://hooks.slack.com/services/TEST".to_string(),
            "#test-pruner".to_string(),
        );
        assert_eq!(
            notifier.webhook_url,
            "https://hooks.slack.com/services/TEST"
        );
        assert_eq!(notifier.channel, "#test-pruner");
    }

    // Note: Actual send_notification tests would require mocking the HTTP client
    // or using integration tests with a test webhook endpoint
}
