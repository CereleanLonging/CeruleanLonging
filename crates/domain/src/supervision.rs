//! Approved bounds and deterministic incident identities, without model authority.
use crate::{
    execution::{digest, require},
    worker_observation::*,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupervisionPolicy {
    pub schema_version: u32,
    pub repeated_failure_threshold: u32,
    pub max_advice_per_attempt: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub delivery_enabled: bool,
}
impl SupervisionPolicy {
    pub fn validate(&self) -> Result<(), String> {
        require(
            self.schema_version == 1
                && (2..=8).contains(&self.repeated_failure_threshold)
                && self.max_advice_per_attempt <= 2
                && (!self.delivery_enabled || self.max_advice_per_attempt > 0),
            "监督阈值须为 2–8 次，每次尝试建议上限须为 0–2 条；发送建议须有非零额度",
        )
    }
}
/// An absent override preserves the original plan-wide policy, including its
/// adapter restrictions. Disabling supervision is an explicit approved choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "camelCase", deny_unknown_fields)]
pub enum SupervisionOverride {
    Disabled {},
    Enabled { policy: SupervisionPolicy },
}
pub fn effective_policy<'a>(
    inherited: Option<&'a SupervisionPolicy>,
    selected: Option<&'a SupervisionOverride>,
    adapter: &str,
) -> Result<Option<&'a SupervisionPolicy>, String> {
    let policy = match selected {
        None => inherited,
        Some(SupervisionOverride::Disabled {}) => None,
        Some(SupervisionOverride::Enabled { policy }) => Some(policy),
    };
    require(
        adapter == "codex"
            || policy.is_none()
            || crate::api_worker::is_api_adapter(adapter)
            || adapter == "omp"
            || adapter == "qoder",
        "此执行器尚不支持当前监督模式",
    )?;
    if let Some(policy) = policy {
        policy.validate()?;
    }
    Ok(policy)
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupervisionIncident {
    pub id: String,
    pub first_sequence: u32,
    pub trigger_sequence: u32,
    pub action_digest: String,
    pub evidence_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SupervisionAdviceView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omp: Option<OmpAdviceReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<ApiAdviceReference>,
    pub incident_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub observation_digest: String,
    pub suggestion: Option<String>,
    pub rationale: Option<String>,
    pub current: bool,
    pub delivered: bool,
    #[serde(default)]
    pub delivery_state: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiAdviceReference {
    pub request_id: String,
    pub round: u32,
    pub tool_head: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OmpAdviceReference {
    pub request_id: String,
    pub tool_head: String,
}
/// Detect observed consecutive failures, never infer lack of progress or a loop.
/// A caller must separately gate loss, current ownership, approval and budgets.
pub fn repeated_failures(
    policy: &SupervisionPolicy,
    trace: &WorkerObservationTrace,
    scope: &str,
) -> Result<Vec<SupervisionIncident>, String> {
    policy.validate()?;
    require(trace.events.len() <= 128, "观察容量无效")?;
    if trace.thread_id.is_none() || trace.turn_id.is_none() {
        return Ok(vec![]);
    }
    let actions = trace
        .events
        .iter()
        .map(|event| {
            if event.kind == ObservedToolKind::Command && event.status == ObservedToolStatus::Failed
            {
                event
                    .action_digest
                    .as_deref()
                    .map(|action| (action, event.exit_code))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    repeated_failure_ranges(policy.repeated_failure_threshold, &actions)?
        .into_iter()
        .map(|(first, trigger)| {
            // Retain the original Codex incident hash and sequence semantics.
            let evidence = digest(&(
                scope,
                policy,
                &trace.thread_id,
                &trace.turn_id,
                &trace.events[..=trigger],
            ))?;
            Ok(SupervisionIncident {
                id: format!("incident-{}", &evidence[..48]),
                first_sequence: trace.events[first].sequence,
                trigger_sequence: trace.events[trigger].sequence,
                action_digest: trace.events[trigger].action_digest.clone().unwrap(),
                evidence_digest: evidence,
            })
        })
        .collect()
}

/// Consecutive equal failure keys; None resets a streak. Identity and authority
/// remain the caller's responsibility. One incident per uninterrupted streak.
pub fn repeated_failure_ranges<T: PartialEq>(
    threshold: u32,
    actions: &[Option<T>],
) -> Result<Vec<(usize, usize)>, String> {
    require(
        (2..=8).contains(&threshold) && actions.len() <= 128,
        "连续失败观察范围无效",
    )?;
    let mut ranges = Vec::new();
    let mut previous = None;
    let mut count = 0;
    let mut first = 0;
    for (index, action) in actions.iter().enumerate() {
        let Some(key) = action.as_ref() else {
            previous = None;
            count = 0;
            continue;
        };
        if previous == Some(key) {
            count += 1;
        } else {
            previous = Some(key);
            count = 1;
            first = index;
        }
        if count == threshold {
            ranges.push((first, index));
        }
    }
    Ok(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn api_delivery_requires_valid_approved_policy() {
        let mut policy = SupervisionPolicy {
            schema_version: 1,
            repeated_failure_threshold: 2,
            max_advice_per_attempt: 1,
            delivery_enabled: false,
        };
        for adapter in [
            "openai",
            "deepseek",
            "anthropic",
            "qwen",
            "kimi",
            "grok",
            "qoder",
        ] {
            assert_eq!(
                effective_policy(Some(&policy), None, adapter).unwrap(),
                Some(&policy)
            );
            policy.delivery_enabled = true;
            assert_eq!(
                effective_policy(Some(&policy), None, adapter).unwrap(),
                Some(&policy)
            );
            policy.delivery_enabled = false;
        }
        assert_eq!(
            effective_policy(Some(&policy), None, "omp").unwrap(),
            Some(&policy)
        );
        policy.delivery_enabled = true;
        assert!(effective_policy(Some(&policy), None, "omp").is_ok());
        policy.delivery_enabled = false;
        for adapter in ["workbuddy", "unknown"] {
            assert!(effective_policy(Some(&policy), None, adapter).is_err());
        }
    }
    fn event(n: u32, action: &str, status: ObservedToolStatus) -> ToolObservation {
        ToolObservation {
            sequence: n,
            received_at_ms: n as u64,
            item_digest: format!("{n:064x}"),
            kind: ObservedToolKind::Command,
            status,
            exit_code: Some(7),
            action_digest: Some(action.repeat(64)),
        }
    }
    #[test]
    fn overrides_preserve_inheritance_and_reject_unsupported_or_invalid_enabled_policy() {
        let inherited = SupervisionPolicy {
            schema_version: 1,
            repeated_failure_threshold: 2,
            max_advice_per_attempt: 1,
            delivery_enabled: true,
        };
        assert_eq!(
            effective_policy(Some(&inherited), None, "codex").unwrap(),
            Some(&inherited)
        );
        let disabled = SupervisionOverride::Disabled {};
        let enabled = SupervisionOverride::Enabled {
            policy: inherited.clone(),
        };
        assert_eq!(
            serde_json::to_string(&disabled).unwrap(),
            r#"{"mode":"disabled"}"#
        );
        assert_eq!(serde_json::to_value(&enabled).unwrap()["mode"], "enabled");
        for adapter in [
            "deepseek",
            "openai",
            "anthropic",
            "qwen",
            "kimi",
            "grok",
            "qoder",
        ] {
            assert_eq!(
                effective_policy(Some(&inherited), None, adapter).unwrap(),
                Some(&inherited)
            );
            assert_eq!(
                effective_policy(None, Some(&enabled), adapter).unwrap(),
                Some(&inherited)
            );
            assert!(effective_policy(Some(&inherited), Some(&disabled), adapter)
                .unwrap()
                .is_none());
        }
        for adapter in ["workbuddy", "unknown"] {
            assert!(effective_policy(Some(&inherited), None, adapter).is_err());
            assert!(effective_policy(None, Some(&enabled), adapter).is_err());
            assert!(effective_policy(Some(&inherited), Some(&disabled), adapter)
                .unwrap()
                .is_none());
        }
        for bad in [
            r#"{"mode":"disabled","policy":{}}"#,
            r#"{"mode":"enabled"}"#,
            r#"{"mode":"inherit"}"#,
        ] {
            assert!(
                serde_json::from_str::<SupervisionOverride>(bad).is_err(),
                "unexpected override accepted: {bad}"
            );
        }
        let mut invalid = inherited;
        invalid.max_advice_per_attempt = 0;
        assert!(effective_policy(
            None,
            Some(&SupervisionOverride::Enabled { policy: invalid }),
            "codex"
        )
        .is_err());
    }
    #[test]
    fn streaks_reset_and_incident_identity_survives_append_and_loss() {
        let p = SupervisionPolicy {
            schema_version: 1,
            repeated_failure_threshold: 2,
            max_advice_per_attempt: 2,
            delivery_enabled: false,
        };
        let mut t = WorkerObservationTrace {
            thread_id: Some("thread".into()),
            turn_id: Some("turn".into()),
            ..Default::default()
        };
        t.events = vec![
            event(1, "a", ObservedToolStatus::Failed),
            event(2, "a", ObservedToolStatus::Failed),
        ];
        let initial = repeated_failures(&p, &t, "scope").unwrap();
        assert_eq!(initial.len(), 1);
        t.events.push(event(3, "a", ObservedToolStatus::Failed));
        t.dropped = 1;
        assert_eq!(repeated_failures(&p, &t, "scope").unwrap(), initial);
        t.events.push(event(4, "a", ObservedToolStatus::Completed));
        t.events.push(event(5, "a", ObservedToolStatus::Failed));
        assert_eq!(repeated_failures(&p, &t, "scope").unwrap(), initial);
        t.events.push(event(6, "b", ObservedToolStatus::Failed));
        t.events.push(event(7, "b", ObservedToolStatus::Failed));
        let two = repeated_failures(&p, &t, "scope").unwrap();
        assert_eq!(two.len(), 2);
        assert_eq!(two[0], initial[0]);
        assert_ne!(two[0].id, two[1].id);
        assert_ne!(
            repeated_failures(&p, &t, "other-attempt").unwrap()[0].id,
            initial[0].id
        );
        t.events[1].exit_code = Some(8);
        assert_eq!(repeated_failures(&p, &t, "scope").unwrap().len(), 1);
    }
}
