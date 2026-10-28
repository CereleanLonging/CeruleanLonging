// Frozen 328 serde DTO graph: selected management types only.
// Original derive/serde attributes and fields retained; only crate paths
// redirect to this isolated legacy graph. Query/result enums retain only
// their two management variants: this is not an old Host implementation.
// Provenance is recorded in review-330/q9-proposal/manifest.json.
mod legacy_328 {
    pub mod supervision {
        use serde::{Deserialize, Serialize};
        use ts_rs::TS;

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
    }
    pub mod project_setup {
        use serde::{Deserialize, Serialize};
        use ts_rs::TS;

        #[derive(Debug, Clone, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        pub struct ManagementView {
            #[serde(default)]
            pub supervision_advice: Option<super::supervision::SupervisionAdviceView>,
            pub operation_id: String,
            pub project_id: String,
            pub purpose: String,
            pub state: String,
            pub message: Option<String>,
            pub evidence_digest: String,
            pub wait_reason: Option<String>,
            pub user_reply: Option<String>,
            pub discard_reason: Option<String>,
            pub can_discard: bool,
            pub can_reply: bool,
            #[serde(default)]
            pub application_recovery: Option<ApplicationRecoveryReview>,
            #[serde(default)]
            pub advice_tracking_review: Option<AdviceTrackingReview>,
            #[serde(default)]
            pub advice_tracking_closure: Option<AdviceTrackingClosure>,
        }

        #[derive(Debug, Clone, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        pub struct AdviceTrackingReview {
            pub evidence_digest: String,
            pub workflow_sequence: u32,
        }

        #[derive(Debug, Clone, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        pub struct AdviceTrackingClosure {
            pub reason: String,
            pub at: String,
        }

        #[derive(Debug, Clone, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        pub struct ApplicationRecoveryReview {
            pub evidence_digest: String,
            pub target_kind: String,
            pub task_id: Option<String>,
            pub attempt_id: Option<String>,
            pub session_id: Option<String>,
            pub workflow_sequence: u32,
        }

        #[derive(Debug, Clone, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        pub struct ManagementSummary {
            pub operation_id: String,
            pub project_id: String,
            pub purpose: String,
            pub state: String,
            pub message: Option<String>,
            pub evidence_digest: String,
            pub can_discard: bool,
            pub can_reply: bool,
            pub has_wait: bool,
            pub has_reply: bool,
            pub has_discard: bool,
        }
    }
    pub mod workspace {
        use serde::{Deserialize, Serialize};
        use ts_rs::TS;

        #[derive(Clone, Debug, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct WorkspaceManagementDecisionPage {
            pub project_id: String,
            pub revision: String,
            pub offset: u32,
            pub next_offset: Option<u32>,
            pub total: u32,
            pub entries: Vec<super::project_setup::ManagementSummary>,
        }

        #[derive(Clone, Debug, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct WorkspaceManagementDecisionDetail {
            pub project_id: String,
            pub revision: String,
            pub record: super::project_setup::ManagementView,
        }

        #[derive(Clone, Debug, Serialize, Deserialize, TS)]
        #[serde(
            tag = "type",
            rename_all = "camelCase",
            rename_all_fields = "camelCase",
            deny_unknown_fields
        )]
        pub enum WorkspaceQuery {
            ManagementDecisions {
                project_id: String,
                offset: u32,
                revision: Option<String>,
            },
            ManagementDecision {
                project_id: String,
                operation_id: String,
                revision: String,
            },
        }

        #[derive(Clone, Debug, Serialize, Deserialize, TS)]
        #[serde(
            tag = "type",
            content = "value",
            rename_all = "camelCase",
            deny_unknown_fields
        )]
        pub enum WorkspaceResult {
            ManagementDecisions(WorkspaceManagementDecisionPage),
            ManagementDecision(Box<WorkspaceManagementDecisionDetail>),
        }
    }
}

use super::*;
use serde_json::{json, Value};

fn gateway_330_current_result(value: Value) -> Value {
    let current: WorkspaceResult = serde_json::from_value(value).unwrap();
    serde_json::to_value(current).unwrap()
}

fn gateway_330_detail(advice: Value) -> Value {
    json!({
        "type": "managementDecision",
        "value": {
            "projectId": "project-330", "revision": "a".repeat(64),
            "record": {
                "supervisionAdvice": advice,
                "operationId": "operation-330", "projectId": "project-330",
                "purpose": "advise", "state": "unknown", "message": "synthetic original record",
                "evidenceDigest": "b".repeat(64), "waitReason": "original wait",
                "userReply": "original user reply", "discardReason": null,
                "canDiscard": true, "canReply": false,
                "applicationRecovery": {"evidenceDigest":"c".repeat(64),"targetKind":"advise",
                    "taskId":"task-330","attemptId":"attempt-330","sessionId":"session-330","workflowSequence":7},
                "adviceTrackingReview": {"evidenceDigest":"d".repeat(64),"workflowSequence":8},
                "adviceTrackingClosure": {"reason":"synthetic closure","at":"2026-10-24T00:00:00Z"}
            }
        }
    })
}

#[test]
fn workspace_management_gateway_330_pinned_328_dto_decodes_current_page_and_detail() {
    let page = gateway_330_current_result(json!({
        "type": "managementDecisions",
        "value": {"projectId":"project-330","revision":"a".repeat(64),"offset":0,"nextOffset":null,"total":1,
            "entries":[{"operationId":"operation-330","projectId":"project-330","purpose":"advise","state":"unknown",
                "message":"synthetic original record","evidenceDigest":"b".repeat(64),
                "canDiscard":true,"canReply":false,"hasWait":true,"hasReply":true,"hasDiscard":false}]}
    }));
    let old: legacy_328::workspace::WorkspaceResult = serde_json::from_value(page.clone()).unwrap();
    assert_eq!(serde_json::to_value(old).unwrap(), page);
    let advice = json!({
        "incidentId":"incident-330","taskId":"task-330","attemptId":"attempt-330",
        "threadId":"thread-330","turnId":"turn-330","observationDigest":"e".repeat(64),
        "suggestion":"synthetic suggestion","rationale":"synthetic rationale",
        "current":false,"delivered":false,"deliveryState":"unknown"
    });
    let mut omp = advice.clone();
    omp["omp"] = json!({"requestId":"omp-request-330","toolHead":"f".repeat(64)});
    let mut api = advice.clone();
    api["api"] = json!({"requestId":"api-request-330","round":2,"toolHead":"f".repeat(64)});
    for advice in [Value::Null, advice, omp, api] {
        let current = gateway_330_current_result(gateway_330_detail(advice));
        let old: legacy_328::workspace::WorkspaceResult =
            serde_json::from_value(current.clone()).unwrap();
        assert_eq!(serde_json::to_value(old).unwrap(), current);

        // The pinned outer detail is strict. This is why 329 must remain a
        // separate result instead of extending managementDecision in place.
        let mut widened = current;
        widened["value"]["observation"] = Value::Null;
        assert!(serde_json::from_value::<legacy_328::workspace::WorkspaceResult>(widened).is_err());
    }
    // Exercise the pinned nested DTO rather than hiding the old record in Value.
    let mut malformed = gateway_330_current_result(gateway_330_detail(Value::Null));
    malformed["value"]["record"]["canDiscard"] = json!("true");
    assert!(serde_json::from_value::<legacy_328::workspace::WorkspaceResult>(malformed).is_err());
}

#[test]
fn workspace_management_gateway_330_pinned_328_query_rejects_new_optional_variant() {
    let old_queries = [
        WorkspaceQuery::ManagementDecisions {
            project_id: "project-330".into(),
            offset: 0,
            revision: None,
        },
        WorkspaceQuery::ManagementDecision {
            project_id: "project-330".into(),
            operation_id: "operation-330".into(),
            revision: "a".repeat(64),
        },
    ];
    let old_values = old_queries
        .into_iter()
        .map(|query| serde_json::to_value(query).unwrap())
        .collect::<Vec<_>>();
    for value in &old_values {
        let old: legacy_328::workspace::WorkspaceQuery =
            serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(old).unwrap(), *value);
        let mut unauthorized_field = value.clone();
        unauthorized_field["approve"] = json!(true);
        assert!(
            serde_json::from_value::<legacy_328::workspace::WorkspaceQuery>(unauthorized_field)
                .is_err()
        );
    }

    let new_query = WorkspaceQuery::ManagementGatewayEvidence {
        project_id: "project-330".into(),
        operation_id: "operation-330".into(),
        revision: "a".repeat(64),
    };
    let new_value = serde_json::to_value(new_query).unwrap();
    assert_eq!(new_value["type"], "managementGatewayEvidence");
    let error = serde_json::from_value::<legacy_328::workspace::WorkspaceQuery>(new_value)
        .expect_err("the frozen 328 management query has no gateway variant");
    assert!(error.to_string().contains("unknown variant"));
    assert!(error.to_string().contains("managementGatewayEvidence"));

    // Only a serialization boundary is exercised. No IPC, Host, dispatch or
    // fallback action runs here; old-server lifecycle remains outside this test.
    for value in old_values {
        let old: legacy_328::workspace::WorkspaceQuery =
            serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(old).unwrap(), value);
    }
}
