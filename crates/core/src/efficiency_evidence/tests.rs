use super::*;
use fs2::FileExt;
use std::fs::{self, OpenOptions};

#[test]
#[ignore = "242 original offline evidence and isolated corruption copies only"]
fn original_reports_and_rejection_cases() {
    let out = std::path::PathBuf::from(std::env::var_os("CERULEAN_EVIDENCE_242").unwrap());
    let inputs: Value =
        serde_json::from_slice(&fs::read(out.join("inputs.json")).unwrap()).unwrap();
    let mut results = Vec::new();
    for case in inputs["cases"].as_array().unwrap() {
        let data = Path::new(case["data"].as_str().unwrap());
        let revision = case["revision"].as_str().unwrap();
        let head = case["head"].as_str().unwrap();
        let report = read(data, "project", revision, head).unwrap();
        assert_eq!(report["apiIntentCount"], case["expectedRounds"]);
        assert_eq!(report["apiReplies"], case["expectedRounds"]);
        assert_eq!(report["realityAttested"], false);
        assert_eq!(report["actualCost"], Value::Null);
        assert_eq!(report["wallMs"], Value::Null);
        assert_eq!(
            report["verifications"].as_array().unwrap().len(),
            case["expectedChecks"].as_u64().unwrap() as usize
        );
        let failures = report["verifications"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["passed"] == false)
            .count();
        assert_eq!(failures, usize::from(case["negative"] == true));
        let name = case["name"].as_str().unwrap();
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(out.join(format!("{name}.export.json")))
            .unwrap();
        serde_json::to_writer_pretty(&mut file, &report).unwrap();
        results.push(json!({"name":name,"status":"passed"}));
    }
    let original = &inputs["cases"][0];
    let data = Path::new(original["data"].as_str().unwrap());
    let revision = original["revision"].as_str().unwrap();
    let head = original["head"].as_str().unwrap();
    assert!(read(data, "project", &"0".repeat(64), head).is_err());
    assert!(read(data, "project", revision, &"0".repeat(64)).is_err());
    assert!(read(data, "../project", revision, head).is_err());
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(data.join("workflows/project/writer.lock"))
        .unwrap();
    lock.try_lock_exclusive().unwrap();
    assert!(read(data, "project", revision, head)
        .unwrap_err()
        .contains("writer already active"));
    drop(lock);
    for case in inputs["corruptions"].as_array().unwrap() {
        let error = read(
            Path::new(case["data"].as_str().unwrap()),
            "project",
            revision,
            head,
        )
        .unwrap_err();
        results.push(json!({"name":case["name"],"status":"rejected","error":error}));
    }
    let unknown: Value =
        serde_json::from_slice(&fs::read(out.join("unknown-input.json")).unwrap()).unwrap();
    let unknown_report = read(
        Path::new(unknown["data"].as_str().unwrap()),
        "project",
        unknown["revision"].as_str().unwrap(),
        unknown["head"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(unknown_report["apiIntentCount"], 1);
    assert_eq!(unknown_report["apiReplies"], 0);
    assert_eq!(unknown_report["apiUncertainRemoteUsage"], 1);
    assert_eq!(unknown_report["apiCalls"][0]["usage"], Value::Null);
    assert_eq!(unknown_report["actualCost"], Value::Null);
    assert_eq!(unknown_report["taskStates"]["a"]["state"], "dispatching");
    results.push(json!({"name":"unknown-prefix","status":"preserved-as-unknown"}));
    let mut unknown_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(out.join("unknown.export.json"))
        .unwrap();
    serde_json::to_writer_pretty(&mut unknown_file, &unknown_report).unwrap();
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(out.join("test-result.json"))
        .unwrap();
    serde_json::to_writer_pretty(
        &mut file,
        &json!({
            "status":"passed", "originalReads":4, "scopeAndLockRejections":4,
            "corruptionRejections":inputs["corruptions"].as_array().unwrap().len(),
            "unknownPrefixesChecked":1,
            "realModelCalls":0,"newDispatches":0,"newVerifications":0,
            "releaseAccepted":false,"cases":results
        }),
    )
    .unwrap();
}
