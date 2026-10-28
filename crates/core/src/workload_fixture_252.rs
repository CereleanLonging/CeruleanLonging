//! Synthetic transport only; implementations are outside prepared model inputs.
use cerulean_adapters::api_generation::{
    fixture_messages, fixture_reply, ApiFailure, ApiMessage, ApiReply, PreparedRequest,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

pub(crate) fn outer() -> PathBuf {
    PathBuf::from(std::env::var_os("CERULEAN_COVERAGE_252_DIR").unwrap())
}
pub(crate) fn slot() -> String {
    std::env::var("CERULEAN_COVERAGE_252_SLOT").unwrap()
}
pub(crate) fn area() -> PathBuf {
    outer().join("fixtures").join(slot())
}
pub(crate) fn save(name: &str, v: &impl serde::Serialize) {
    crate::api_execution::publish(&area(), name, v).unwrap();
}
pub(crate) fn worker(request: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    let slot = slot();
    let task = &slot[..3];
    let solutions: Value =
        serde_json::from_slice(&fs::read(outer().join("responses.json")).unwrap()).unwrap();
    let payload = &solutions[task];
    let messages = fixture_messages(request);
    let ApiMessage::User(prompt) = &messages[0] else {
        panic!("missing prompt")
    };
    let branch = if slot.ends_with("single") {
        "single"
    } else if task.starts_with('S') {
        "serial"
    } else if prompt.contains("Fixed task: left.") {
        "left"
    } else if prompt.contains("Fixed task: right.") {
        "right"
    } else {
        assert!(prompt.contains("Fixed task: integrate."));
        "integrate"
    };
    if round > 1 {
        let ApiMessage::Tools(results) = messages.last().unwrap() else {
            panic!("missing tool result")
        };
        assert_eq!(results.len(), 1);
        assert!(!results[0].is_error, "{}", results[0].text);
        if branch == "integrate" && round <= 3 {
            let v: Value = serde_json::from_str(&results[0].text).unwrap();
            assert_eq!(
                v["content"],
                payload[if round == 2 { "left.mjs" } else { "right.mjs" }]
            );
        }
    }
    let files: &[&str] = match branch {
        "left" => &["left.mjs"],
        "right" => &["right.mjs"],
        "single" if task.starts_with('P') => &["left.mjs", "right.mjs", "main.mjs"],
        _ => &["main.mjs"],
    };
    let offset = if branch == "integrate" { 2 } else { 0 };
    let tool = if branch == "integrate" && round <= 2 {
        Some((
            "cerulean_read_file",
            json!({"path":if round==1 {"left.mjs"} else {"right.mjs"},"offset":0}),
        ))
    } else if let Some(path) = files.get((round - offset - 1) as usize) {
        let seed = fs::read(area().join("bundle/inputs/project").join(path)).unwrap();
        Some((
            "cerulean_write_file",
            json!({"path":path,"content":payload[*path],
            "expectedSha256":format!("{:x}",Sha256::digest(seed))}),
        ))
    } else {
        None
    };
    let message = match &tool {
        Some((name, args)) => json!({"role":"assistant","content":null,"tool_calls":[{
            "id":format!("call-{slot}-{branch}-{round}"),"type":"function",
            "function":{"name":name,"arguments":args.to_string()}}]}),
        None => {
            json!({"role":"assistant","content":"Synthetic task complete; independent checks required."})
        }
    };
    fixture_reply(request, json!({"id":format!("synthetic252-{slot}-{branch}-{round}"),"model":"synthetic-worker",
        "choices":[{"index":0,"finish_reason":if tool.is_some() {"tool_calls"} else {"stop"},"message":message}],
        "usage":{"prompt_tokens":10,"completion_tokens":10}}))
        .map_err(|_| ApiFailure::InvalidResponse {})
}
