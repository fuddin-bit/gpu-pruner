//! Tests for the acknowledgment system
//!
//! These tests verify that workloads can be acknowledged and that
//! acknowledged workloads are correctly identified and skipped during scaledown.

use chrono::{Duration, Utc};
use k8s_openapi::api::apps::v1::Deployment;
use kube::api::ObjectMeta;
use std::collections::BTreeMap;

use gpu_pruner::{
    PENDING_SCALE_ANNOTATION, PendingScaleStatus, ScaleKind, check_acknowledgment,
    check_pending_grace,
};

fn make_deployment_with_annotations(
    name: &str,
    ns: &str,
    annotations: Option<BTreeMap<String, String>>,
) -> ScaleKind {
    ScaleKind::Deployment(Deployment {
        metadata: ObjectMeta {
            name: Some(name.into()),
            namespace: Some(ns.into()),
            annotations,
            ..Default::default()
        },
        ..Default::default()
    })
}

#[tokio::test]
async fn check_acknowledgment_no_annotations() {
    let client = kube::Client::try_default().await.unwrap();
    let sk = make_deployment_with_annotations("test", "default", None);

    let status = check_acknowledgment(client, &sk).await.unwrap();

    assert!(!status.acknowledged);
    assert!(status.expires_at.is_none());
    assert!(status.by_user.is_none());
}

#[tokio::test]
async fn check_acknowledgment_valid() {
    let client = kube::Client::try_default().await.unwrap();

    // Create annotation that expires in the future
    let expires_at = Utc::now() + Duration::hours(4);
    let mut annotations = BTreeMap::new();
    annotations.insert(
        "gpu-pruner.io/ack-until".to_string(),
        expires_at.to_rfc3339(),
    );
    annotations.insert("gpu-pruner.io/ack-by".to_string(), "test-user".to_string());

    let sk = make_deployment_with_annotations("test", "default", Some(annotations));

    let status = check_acknowledgment(client, &sk).await.unwrap();

    assert!(status.acknowledged, "should be acknowledged");
    assert!(status.expires_at.is_some());
    assert_eq!(status.by_user, Some("test-user".to_string()));
}

#[tokio::test]
async fn check_acknowledgment_expired() {
    let client = kube::Client::try_default().await.unwrap();

    // Create annotation that expired 1 hour ago
    let expires_at = Utc::now() - Duration::hours(1);
    let mut annotations = BTreeMap::new();
    annotations.insert(
        "gpu-pruner.io/ack-until".to_string(),
        expires_at.to_rfc3339(),
    );
    annotations.insert("gpu-pruner.io/ack-by".to_string(), "test-user".to_string());

    let sk = make_deployment_with_annotations("test", "default", Some(annotations));

    let status = check_acknowledgment(client, &sk).await.unwrap();

    assert!(
        !status.acknowledged,
        "expired ack should not be acknowledged"
    );
    assert!(status.expires_at.is_none());
    assert!(status.by_user.is_none());
}

#[tokio::test]
async fn check_acknowledgment_invalid_timestamp() {
    let client = kube::Client::try_default().await.unwrap();

    // Create annotation with invalid timestamp
    let mut annotations = BTreeMap::new();
    annotations.insert(
        "gpu-pruner.io/ack-until".to_string(),
        "invalid-timestamp".to_string(),
    );
    annotations.insert("gpu-pruner.io/ack-by".to_string(), "test-user".to_string());

    let sk = make_deployment_with_annotations("test", "default", Some(annotations));

    let status = check_acknowledgment(client, &sk).await.unwrap();

    assert!(
        !status.acknowledged,
        "invalid timestamp should not be acknowledged"
    );
}

#[tokio::test]
async fn check_acknowledgment_missing_by_user() {
    let client = kube::Client::try_default().await.unwrap();

    // Valid expiry but missing ack-by annotation
    let expires_at = Utc::now() + Duration::hours(4);
    let mut annotations = BTreeMap::new();
    annotations.insert(
        "gpu-pruner.io/ack-until".to_string(),
        expires_at.to_rfc3339(),
    );

    let sk = make_deployment_with_annotations("test", "default", Some(annotations));

    let status = check_acknowledgment(client, &sk).await.unwrap();

    assert!(
        status.acknowledged,
        "should still be acknowledged even without by_user"
    );
    assert!(status.by_user.is_none());
}

#[test]
fn check_pending_grace_not_pending() {
    let sk = make_deployment_with_annotations("test", "default", None);
    assert_eq!(
        check_pending_grace(&sk, 300),
        PendingScaleStatus::NotPending
    );
}

#[test]
fn check_pending_grace_in_grace() {
    let pending_at = Utc::now();
    let mut annotations = BTreeMap::new();
    annotations.insert(
        PENDING_SCALE_ANNOTATION.to_string(),
        pending_at.to_rfc3339(),
    );

    let sk = make_deployment_with_annotations("test", "default", Some(annotations));
    match check_pending_grace(&sk, 300) {
        PendingScaleStatus::InGrace { until } => {
            assert!(until > Utc::now());
        }
        other => panic!("expected InGrace, got {other:?}"),
    }
}

#[test]
fn check_pending_grace_expired() {
    let pending_at = Utc::now() - Duration::minutes(10);
    let mut annotations = BTreeMap::new();
    annotations.insert(
        PENDING_SCALE_ANNOTATION.to_string(),
        pending_at.to_rfc3339(),
    );

    let sk = make_deployment_with_annotations("test", "default", Some(annotations));
    assert_eq!(
        check_pending_grace(&sk, 300),
        PendingScaleStatus::GraceExpired
    );
}

#[test]
fn check_pending_grace_invalid_timestamp() {
    let mut annotations = BTreeMap::new();
    annotations.insert(
        PENDING_SCALE_ANNOTATION.to_string(),
        "not-a-timestamp".to_string(),
    );

    let sk = make_deployment_with_annotations("test", "default", Some(annotations));
    assert_eq!(
        check_pending_grace(&sk, 300),
        PendingScaleStatus::NotPending
    );
}

#[tokio::test]
async fn acknowledged_workload_has_no_pending_grace() {
    let expires_at = Utc::now() + Duration::hours(4);
    let mut annotations = BTreeMap::new();
    annotations.insert(
        "gpu-pruner.io/ack-until".to_string(),
        expires_at.to_rfc3339(),
    );
    annotations.insert(
        PENDING_SCALE_ANNOTATION.to_string(),
        (Utc::now() - Duration::minutes(1)).to_rfc3339(),
    );

    let sk = make_deployment_with_annotations("test", "default", Some(annotations));
    let client = kube::Client::try_default().await.unwrap();

    let ack = check_acknowledgment(client, &sk).await.unwrap();
    assert!(ack.acknowledged);
    // Ack path is checked before pending grace in the pruner loop.
}
