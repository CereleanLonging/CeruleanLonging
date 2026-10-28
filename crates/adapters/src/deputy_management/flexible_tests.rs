use super::*;
use cerulean_domain::{
    credentials::{CredentialProvider, ProviderEndpoint as E},
    management_ingress::ManagementRole,
    secret::SecretInput,
};
#[test]
fn provider_wait_284_prompt_matches_strict_schema_for_every_management_purpose() {
    for purpose in [
        Purpose::Plan,
        Purpose::Assign,
        Purpose::Accept,
        Purpose::Advise,
    ] {
        let context = json!({"purpose":"wait contract without prerequisites"});
        let h = ManagementHandoff {
            operation_id: "wait284".into(),
            project_id: "project".into(),
            purpose,
            source: ManagementSource::Provider {
                endpoint: E::A6Api {},
                model: "fixture-model".into(),
                role: ManagementRole::for_purpose(purpose),
            },
            state_digest: digest(&context).unwrap(),
            context,
        };
        let request = request(&h).unwrap();
        let example = request.instructions.split_once("Wait shape: ").unwrap().1;
        let wait: DecisionEnvelope = serde_json::from_str(example).unwrap();
        wait.validate(purpose, &h.state_digest).unwrap();
        assert!(matches!(
            wait.decision,
            cerulean_domain::management::ManagementDecision::Wait { .. }
        ));
        assert!(request.tools.is_empty());
        let client =
            ApiClient::new(SecretInput::new("invalid-fixture-only".into()).unwrap()).unwrap();
        let p = prepare(&client, h).unwrap();
        for valid in [true, false] {
            let content = if valid {
                serde_json::to_value(&wait).unwrap()
            } else {
                json!({"stateDigest":wait.state_digest,"decision":{"action":"wait","rationale":"wrong field"}})
            };
            let result = fixture_result(
                &p,
                json!({"id":"wait-response","model":"fixture-model","usage":{"prompt_tokens":1,"completion_tokens":1},"choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":content.to_string()}}]}),
            );
            assert_eq!(result.envelope().is_some(), valid);
            assert_eq!(
                result.decode_stage,
                Some(if valid {
                    DecodeStage::Accepted
                } else {
                    DecodeStage::SchemaReason
                })
            );
        }
    }
}
fn decision(purpose: Purpose) -> serde_json::Value {
    match purpose {
        Purpose::Plan => {
            json!({"action":"plan","tasks":[{"id":"t","goal":"bounded edit","dependencies":[],"workerId":"worker-a6api","validatorId":"tests"}],"rationale":"bounded plan"})
        }
        Purpose::Assign => {
            json!({"action":"assign","taskId":"t","workerId":"worker-a6api","rationale":"ready"})
        }
        Purpose::Accept => {
            json!({"action":"accept","taskId":"t","attemptId":"a","rationale":"verified"})
        }
        Purpose::Advise => {
            json!({"action":"advise","taskId":"t","suggestion":"review failure","rationale":"bounded suggestion"})
        }
    }
}
#[test]
fn flexible_role_provider_matrix_binds_tool_free_wire_and_keeps_usage_on_rejected_decisions() {
    for endpoint in [
        E::DeepSeek {},
        E::Anthropic {},
        E::KimiChina {},
        E::Grok {},
        E::A6Api {},
        E::QwenBeijing {
            workspace_id: "test-workspace".into(),
        },
        E::QwenSingapore {},
        E::QwenHongKong {},
    ] {
        for purpose in [
            Purpose::Plan,
            Purpose::Assign,
            Purpose::Accept,
            Purpose::Advise,
        ] {
            let context = json!({"supervision":if purpose==Purpose::Advise {json!({"taskId":"t"})} else {serde_json::Value::Null}});
            let handoff = ManagementHandoff {
                operation_id: "op".into(),
                project_id: "project".into(),
                purpose,
                source: ManagementSource::Provider {
                    endpoint: endpoint.clone(),
                    model: "fixture-model".into(),
                    role: ManagementRole::for_purpose(purpose),
                },
                state_digest: digest(&context).unwrap(),
                context,
            };
            let client =
                ApiClient::new(SecretInput::new("invalid-local-test-only".into()).unwrap())
                    .unwrap();
            let p = prepare(&client, handoff.clone()).unwrap();
            p.request().validate().unwrap();
            assert_eq!(p.fingerprint().target.provider(), endpoint.provider());
            assert_eq!(
                p.fingerprint().tool_catalog_sha256,
                digest(&Vec::<serde_json::Value>::new()).unwrap()
            );
            let anthropic = endpoint.provider() == CredentialProvider::Anthropic;
            for case in 0..6 {
                let mut content =
                    json!({"stateDigest":handoff.state_digest,"decision":decision(purpose)});
                match case {
                    1 => content["stateDigest"] = json!("0".repeat(64)),
                    2 => {
                        content["decision"] = decision(if purpose == Purpose::Plan {
                            Purpose::Accept
                        } else {
                            Purpose::Plan
                        })
                    }
                    3 => content["userApproved"] = json!(true),
                    _ => {}
                }
                let mut value = if anthropic {
                    json!({"id":"reply","model":"fixture-model","type":"message","role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":content.to_string()}],"usage":{"input_tokens":12,"output_tokens":8,"cache_read_input_tokens":2,"cache_creation_input_tokens":1}})
                } else {
                    json!({"id":"reply","model":"fixture-model","usage":{"prompt_tokens":12,"completion_tokens":8},"choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":content.to_string(),"reasoning_content":"private-must-not-persist"}}]})
                };
                if case == 4 {
                    if anthropic {
                        value["content"] =
                            json!([{"type":"tool_use","id":"c","name":"write_file","input":{}}]);
                    } else {
                        value["choices"][0]["message"]["tool_calls"] = json!([{"id":"c","type":"function","function":{"name":"write_file","arguments":"{}"}}]);
                    }
                }
                if case == 5 {
                    value["usage"] = json!(null);
                }
                let result = fixture_result(&p, value);
                result.validate(&p.request()).unwrap();
                assert_eq!(result.envelope().is_some(), case == 0);
                assert_eq!(result.billing().is_some(), case != 5);
                assert!(!serde_json::to_string(&result)
                    .unwrap()
                    .contains("private-must-not-persist"));
            }
            let mut wrong = p.request();
            wrong.wire.target = ApiTarget::OpenAi {};
            assert!(wrong.validate().is_err());
            let mut wrong = handoff;
            if let ManagementSource::Provider { role, .. } = &mut wrong.source {
                *role = if *role == ManagementRole::Chief {
                    ManagementRole::Deputy
                } else {
                    ManagementRole::Chief
                };
            }
            assert!(prepare(&client, wrong).is_err());
        }
    }
}
