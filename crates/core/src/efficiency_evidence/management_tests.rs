use super::*;
use fs2::FileExt;
use std::fs::{self, OpenOptions};

#[test]
#[ignore = "243 offline original source/usage binding, no new model calls"]
fn original_sources_usage_and_cross_links() {
    let out = std::path::PathBuf::from(std::env::var_os("CERULEAN_MANAGEMENT_243").unwrap());
    let inputs: Value =
        serde_json::from_slice(&fs::read(out.join("inputs.json")).unwrap()).unwrap();
    let mut results = Vec::new();
    let inspect = |case: &Value| {
        read_with_management(
            Path::new(case["data"].as_str().unwrap()),
            case["project"].as_str().unwrap(),
            case["revision"].as_str().unwrap(),
            case["head"].as_str().unwrap(),
        )
    };
    for case in inputs["originals"].as_array().unwrap() {
        let report = inspect(case).unwrap();
        let operations = report["management"]["operations"].as_array().unwrap();
        assert_eq!(operations.len(), 3);
        assert_eq!(report["management"]["linkedWorkflowEvents"], 3);
        assert_eq!(report["realityAttested"], false);
        assert_eq!(report["actualCost"], Value::Null);
        if case["name"] == "real218" {
            assert_eq!(
                operations
                    .iter()
                    .filter(|o| o["source"] == "codex-session")
                    .count(),
                2
            );
            for op in operations.iter().filter(|o| o["source"] == "codex-session") {
                assert_eq!(op["usage"], Value::Null);
                assert_eq!(op["productApiIntent"], false);
            }
            let deputy = operations
                .iter()
                .find(|o| o["source"] == "deepseek")
                .unwrap();
            assert!(deputy["usage"]["inputTokens"].as_u64().unwrap() > 0);
        } else {
            assert!(operations
                .iter()
                .all(|o| o["source"] == "openai" && o["usage"]["inputTokens"] == 1));
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out.join(format!("{}.export.json", case["name"].as_str().unwrap())))
            .unwrap();
        serde_json::to_writer_pretty(&mut file, &report).unwrap();
        results.push(json!({"name":case["name"],"status":"passed"}));
    }
    for case in inputs["corruptions"].as_array().unwrap() {
        let error = inspect(case).unwrap_err();
        results.push(json!({"name":case["name"],"status":"rejected","error":error}));
    }
    let case = &inputs["retained"];
    let report = inspect(case).unwrap();
    let ops = report["management"]["operations"].as_array().unwrap();
    assert_eq!(ops.len(), 5);
    let discarded = ops
        .iter()
        .find(|o| o["operationId"] == "extra-discarded")
        .unwrap();
    assert_eq!(discarded["state"], "discarded");
    assert_eq!(discarded["usage"]["inputTokens"], 17);
    assert_eq!(discarded["workflowEventHash"], Value::Null);
    let pending = ops
        .iter()
        .find(|o| o["operationId"] == "extra-unknown")
        .unwrap();
    assert_eq!(pending["usage"], Value::Null);
    assert_eq!(pending["state"], "unknown");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out.join("retained.export.json"))
        .unwrap();
    serde_json::to_writer_pretty(&mut file, &report).unwrap();
    let original = &inputs["originals"][0];
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(Path::new(original["data"].as_str().unwrap()).join("writer.lock"))
        .unwrap();
    lock.try_lock_exclusive().unwrap();
    assert!(inspect(original)
        .unwrap_err()
        .contains("data writer is active"));
    drop(lock);
    results.push(json!({"name":"retained-discard-and-unknown","status":"passed"}));
    results.push(json!({"name":"active-root-writer","status":"rejected"}));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out.join("test-result.json"))
        .unwrap();
    serde_json::to_writer_pretty(&mut file, &json!({
        "status":"passed","originalsRead":2,"corruptionRejections":inputs["corruptions"].as_array().unwrap().len(),
        "discardedUsageRetained":true,"externalSessionUsageUnknown":true,
        "activeWriterRejected":true,"realModelCalls":0,"newDispatches":0,"cases":results,
        "releaseAccepted":false
    })).unwrap();
}
