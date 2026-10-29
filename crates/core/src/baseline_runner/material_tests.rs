use super::*;
use crate::baseline_materials::{compose, verify, ComposeRequest};
use cerulean_adapters::api_generation::fixture_reply;
use cerulean_domain::secret::SecretInput;
use sha2::{Digest, Sha256};

fn root() -> PathBuf {
    PathBuf::from(std::env::var_os("CERULEAN_MATERIAL_248_DIR").unwrap())
}
fn request(task: &str) -> ComposeRequest {
    serde_json::from_slice(
        &fs::read(root().join(format!("fixtures/{task}-r1-single/request.json"))).unwrap(),
    )
    .unwrap()
}
fn selected(task: &str, name: &str) -> RunSpec {
    let mut req = request(task);
    req.data_root = root().join(name).to_string_lossy().into();
    compose(&req).unwrap().0
}
fn authorization(spec: &RunSpec) -> RunApproval {
    RunApproval {
        spec_digest: digest(spec).unwrap(),
        approval: Approval {
            plan_id: spec.plan.workflow.id.clone(),
            revision: 1,
            policy_version: 1,
            revision_digest: spec.plan.approval_digest().unwrap(),
            user_id: "user".into(),
            approved_at: chrono::Utc::now().to_rfc3339(),
        },
    }
}

#[test]
#[ignore = "explicit 248 isolated materials and pinned Git/Node"]
fn all_twelve_fixed_workloads_compose_without_dispatch() {
    let mut results = vec![];
    for name in [
        "S01", "S02", "S03", "S04", "S05", "S06", "P01", "P02", "P03", "P04", "P05", "P06",
    ] {
        let (spec, proof) = compose(&request(name)).unwrap();
        assert_eq!(proof.declared_execution_kind, "synthetic");
        assert_eq!(
            spec.plan.execution[0].artifact_paths.len(),
            if name.starts_with('P') { 3 } else { 1 }
        );
        assert!(!Path::new(&spec.data_root).exists());
        results.push(json!({"spec":spec,"proof":proof}));
    }
    publish(&root(), "twelve-composed.json", &results).unwrap();
}

#[test]
#[ignore = "explicit 248 isolated materials and pinned Git/Node"]
fn whole_spec_reconstruction_rejects_plan_drift() {
    let spec = selected("S02", "drift-data");
    let mut checked = vec![];
    for change in [
        "goal",
        "model",
        "limit",
        "tool",
        "check",
        "artifact",
        "seed",
        "data",
        "manifest",
        "configuration",
        "workspace",
        "snapshot",
    ] {
        let mut changed = spec.clone();
        match change {
            "goal" => changed.plan.workflow.tasks[0].goal.push_str(" changed"),
            "model" => {
                changed.plan.execution[0].api_worker.as_mut().unwrap().model =
                    "another-model".into()
            }
            "limit" => changed.plan.policy.max_requests += 1,
            "tool" => changed.plan.policy.tools.push(ToolPermission::Network),
            "check" => changed.plan.execution[0].checks[0].timeout_seconds += 1,
            "artifact" => changed.plan.execution[0]
                .artifact_paths
                .push("extra.mjs".into()),
            "seed" => changed.seed.files[0].sha256 = "0".repeat(64),
            "data" => changed.data_root = root().join("other-data").to_string_lossy().into(),
            "manifest" => changed.material_manifest_sha256 = "0".repeat(64),
            "configuration" => changed.configuration_sha256 = "0".repeat(64),
            "workspace" => changed.plan.workflow.tasks[0].workspace_key = "wrong".into(),
            "snapshot" => changed.plan.snapshot_policy.as_mut().unwrap().max_bytes += 1,
            _ => unreachable!(),
        }
        assert!(verify(&changed).is_err(), "{change}");
        checked.push(change);
    }
    publish(&root(), "plan-drift-result.json", &checked).unwrap();
}

#[test]
#[ignore = "explicit 248 isolated materials and pinned Git/Node"]
fn material_and_repository_changes_are_rejected() {
    let req = request("S03");
    let mut outcomes = vec![];
    for name in [
        "manifest.json",
        "configuration.json",
        "inputs/control/task.txt",
        "inputs/control/verify.json",
        "inputs/project/main.mjs",
    ] {
        let path = Path::new(&req.bundle).join(name);
        let original = fs::read(&path).unwrap();
        let mut changed = original.clone();
        changed.push(b' ');
        fs::write(&path, changed).unwrap();
        let result = compose(&req);
        fs::write(&path, original).unwrap();
        assert!(result.is_err(), "{name}");
        outcomes.push(name.to_owned());
    }
    let extra = Path::new(&req.bundle).join("extra.txt");
    fs::write(&extra, "unexpected").unwrap();
    let result = compose(&req);
    fs::remove_file(extra).unwrap();
    assert!(result.is_err());
    outcomes.push("extra entry".into());
    for field in ["commit", "git", "node", "pool", "project"] {
        let mut changed = req.clone();
        match field {
            "commit" => changed.worktree.base_commit = "0".repeat(40),
            "git" => changed.worktree.git.sha256 = "0".repeat(64),
            "node" => changed.node.sha256 = "0".repeat(64),
            "pool" => changed.worktree.pool_root = req.project_root.clone(),
            "project" => changed.project_root = req.bundle.clone(),
            _ => unreachable!(),
        }
        assert!(compose(&changed).is_err(), "{field}");
        outcomes.push(field.into());
    }
    let path = Path::new(&req.project_root).join("main.mjs");
    let original = fs::read(&path).unwrap();
    fs::write(&path, "changed").unwrap();
    let result = compose(&req);
    fs::write(path, original).unwrap();
    assert!(result.is_err());
    outcomes.push("project seed".into());
    publish(&root(), "material-drift-result.json", &outcomes).unwrap();
}

#[tokio::test]
#[ignore = "explicit 248 isolated materials and pinned Git/Node"]
async fn material_change_after_prepare_stops_before_credentials_and_intent() {
    let spec = selected("S04", "changed-material-data");
    let data = Path::new(&spec.data_root);
    prepare(data, spec.clone()).unwrap();
    let file = Path::new(&spec.material.as_ref().unwrap().bundle).join("inputs/control/task.txt");
    let original = fs::read(&file).unwrap();
    fs::write(&file, "changed").unwrap();
    let (_tx, mut rx) = oneshot::channel();
    let result = execute(
        data,
        &digest(&spec).unwrap(),
        authorization(&spec),
        "synthetic",
        |_| panic!("must not load credentials"),
        &mut async |_: &ApiClient, _, _: &mut oneshot::Receiver<()>, _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| panic!("must not send"),
        &mut rx,
        &AtomicBool::new(false),
    )
    .await;
    // Historical inspection deliberately does not require external inputs to remain online.
    let history = inspect(data, &digest(&spec).unwrap());
    fs::write(file, original).unwrap();
    assert!(result.unwrap_err().contains("material"));
    assert!(!data.join("started.json").exists());
    assert_eq!(history.unwrap()["status"], "prepared");
}

#[tokio::test]
#[ignore = "explicit 248 isolated materials and pinned Git/Node"]
async fn synthetic_and_legacy_specs_cannot_enter_production() {
    let bound = selected("S05", "production-denied-data");
    let temp = tempfile::tempdir().unwrap();
    // Reuse the legacy fixture, without starting its task.
    let mut legacy = bound.clone();
    legacy.schema = 1;
    legacy.material = None;
    legacy.data_root = temp.path().join("legacy-data").to_string_lossy().into();
    for spec in [bound, legacy] {
        let data = Path::new(&spec.data_root);
        prepare(data, spec.clone()).unwrap();
        let (_tx, mut rx) = oneshot::channel();
        let result =
            execute(
                data,
                &digest(&spec).unwrap(),
                authorization(&spec),
                "production-api",
                |_| panic!("must not read credentials"),
                &mut async |_: &ApiClient,
                            _,
                            _: &mut oneshot::Receiver<()>,
                            _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<
                    '_,
                >| panic!("must not send"),
                &mut rx,
                &AtomicBool::new(false),
            )
            .await;
        assert!(result.unwrap_err().contains("bound real-attempt"));
        assert!(!data.join("started.json").exists());
    }
}

#[tokio::test]
#[ignore = "explicit 248 isolated materials; two synthetic replies, real Git/tools/verifier"]
async fn bound_run_accepts_fixed_checker_and_reopens_without_redispatch() {
    let spec = selected("S01", "retained-data");
    let data = Path::new(&spec.data_root);
    prepare(data, spec.clone()).unwrap();
    let (_tx, mut rx) = oneshot::channel();
    let mut sends = 0;
    // Derived from the task contract, never loaded from the suite's reference implementation.
    let solution = r#"export function run(value) {
  if (typeof value === 'number') {
    if (Number.isSafeInteger(value) && value >= 0 && value <= 3600000) return value;
    throw new TypeError('duration');
  }
  if (typeof value !== 'string') throw new TypeError('duration');
  const m = /^(\d+(?:\.\d+)?)(ms|s|m)$/.exec(value.trim());
  if (!m) throw new TypeError('duration');
  const result = Number(m[1]) * ({ms:1,s:1000,m:60000}[m[2]]);
  if (!Number.isSafeInteger(result) || result < 0 || result > 3600000) throw new TypeError('duration');
  return result;
}
"#;
    let end=execute(data,&digest(&spec).unwrap(),authorization(&spec),"synthetic-material-fixture",
        |_|ApiClient::new(SecretInput::new("synthetic-no-user-key".into()).unwrap()),
        &mut async |_:&ApiClient,prepared,_:&mut oneshot::Receiver<()>, _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| {
            sends+=1;
            let message=if sends==1 {json!({"role":"assistant","content":null,"tool_calls":[{"id":"write-main","type":"function","function":{
                "name":"cerulean_write_file","arguments":json!({"path":"main.mjs","content":solution,"expectedSha256":spec.seed.files[0].sha256}).to_string()
            }}]})} else {json!({"role":"assistant","content":"Done."})};
            fixture_reply(&prepared,json!({"id":format!("synthetic-248-{sends}"),"model":"synthetic-worker","choices":[{"index":0,
                "finish_reason":if sends==1{"tool_calls"}else{"stop"},"message":message}],"usage":{"prompt_tokens":10,"completion_tokens":10}}))
                .map_err(|_|ApiFailure::InvalidResponse {})
        },&mut rx,&AtomicBool::new(false)).await.unwrap();
    publish(&root(), "synthetic-terminal.json", &end).unwrap();
    assert!(end.accepted, "{:?}", end.error);
    assert_eq!(sends, 2);
    let terminal_bytes = fs::read(data.join("terminal.json")).unwrap();
    for _ in 0..2 {
        let report = inspect(data, &digest(&spec).unwrap()).unwrap();
        assert_eq!(report["taskState"], "accepted");
        assert_eq!(report["managementModelRequests"], 0);
        assert_eq!(report["coreExport"]["apiReplies"], 2);
        let (_tx, mut cancel) = oneshot::channel();
        let denied =
            execute(
                data,
                &digest(&spec).unwrap(),
                authorization(&spec),
                "synthetic",
                |_| panic!("no credentials on repeat"),
                &mut async |_: &ApiClient,
                            _,
                            _: &mut oneshot::Receiver<()>,
                            _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<
                    '_,
                >| panic!("no repeat send"),
                &mut cancel,
                &AtomicBool::new(false),
            )
            .await;
        assert!(denied.is_err());
    }
    assert_eq!(
        fs::read(data.join("terminal.json")).unwrap(),
        terminal_bytes
    );
    publish(
        &root(),
        "synthetic-inspection.json",
        &inspect(data, &digest(&spec).unwrap()).unwrap(),
    )
    .unwrap();
    publish(&root(),"synthetic-selection.json",&json!({"dataRoot":data,"specDigest":digest(&spec).unwrap(),
        "terminalSha256":format!("{:x}",Sha256::digest(terminal_bytes)),"sends":sends,"repeatsDenied":2})).unwrap();
}
