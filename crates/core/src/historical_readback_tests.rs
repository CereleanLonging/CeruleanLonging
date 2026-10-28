//! Explicit local evidence inspection. Never run against live data by default.
use super::*;
use crate::scheduler::DurableWorkflow;
use serde_json::json;

#[test]
#[ignore = "requires an explicit frozen historical-readback plan and fresh report directory"]
fn original_histories_use_current_readers_without_recovery_or_publication() {
    let plan_path = std::env::var_os("CERULEAN_READBACK_PLAN").expect("explicit plan path");
    let output =
        PathBuf::from(std::env::var_os("CERULEAN_READBACK_OUTPUT").expect("fresh report path"));
    let plan: Value = serde_json::from_slice(&fs::read(plan_path).unwrap()).unwrap();
    assert_eq!(plan["schema"], 1);
    assert_eq!(plan["stage"], 309);
    assert_eq!(plan["cases"].as_array().unwrap().len(), 4);
    assert!(!output.exists(), "original attempt already has a result");
    let mut results = vec![];
    for case in plan["cases"].as_array().unwrap() {
        let root = Path::new(case["source"].as_str().unwrap());
        let project = case["project"].as_str().unwrap();
        identifier(project).unwrap();
        // Match Host ownership without opening a Host, creating a directory, or
        // taking recovery actions. The nested inspector owns the workflow lock.
        let _root_pin = crate::backup::open(root, true, false).unwrap();
        let _writer = crate::backup::open(&root.join("writer.lock"), false, true).unwrap();
        let format = crate::data_format::status(root).unwrap();
        assert_eq!(
            format.minimum_reader_generation,
            case["minimumReaderGeneration"].as_u64().unwrap() as u32
        );
        let journal = DecisionJournal::read_existing(root).unwrap();
        assert!(
            journal.pending_publications.is_empty(),
            "original publication needs separate review"
        );
        let mut decisions = journal
            .statuses()
            .into_iter()
            .filter(|s| s.project_id == project)
            .map(|s| {
                json!({
                    "operationId": s.operation_id,
                    "projectId": s.project_id,
                    "purpose": s.purpose,
                    "state": s.state,
                    "evidenceDigest": journal.evidence_digest(&s.operation_id).unwrap(),
                })
            })
            .collect::<Vec<_>>();
        decisions.sort_by_key(|value| value["operationId"].as_str().unwrap().to_owned());
        assert_eq!(Value::Array(decisions.clone()), case["managementDecisions"]);
        let workflow =
            DurableWorkflow::inspect_existing(&root.join("workflows").join(project), |store| {
                let value = json!({
                    "state": store.state(), "revision": store.revision(),
                    "approval": store.approval(), "sequence": store.history().len(),
                });
                require(
                    value == case["workflow"],
                    "historical workflow differs from frozen evidence",
                )?;
                require(
                    store.checkpoint_events() == 0,
                    "readonly inspector used checkpoint",
                )?;
                Ok(value)
            })
            .unwrap();
        results.push(json!({"stage": case["stage"], "project": project,
            "format": format, "workflow": workflow, "managementDecisions": decisions,
            "workflowAndManagementMatchFrozenEvidence": true,
            "scope": "current core read-only full replay; no Host/recovery/publication"}));
    }
    let result = json!({"stage":309, "result":"current-readers-match-four-frozen-histories",
        "cases":results,"modelCalls":0,"credentialsRead":false,"newAcceptOrVerification":false,
        "fullHostReadback":false,"originalDataUnchanged":"requires outer driver before/after hashes",
        "releaseAccepted":false});
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap();
    serde_json::to_writer_pretty(&mut file, &result).unwrap();
    file.write_all(b"\n").unwrap();
    file.sync_all().unwrap();
}
