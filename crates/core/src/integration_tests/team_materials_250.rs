use super::*;
use crate::team_materials::{self, TeamBlueprint, TeamRequest};
use sha2::{Digest, Sha256};

fn outer() -> PathBuf {
    PathBuf::from(std::env::var_os("CERULEAN_TEAM_250_DIR").unwrap())
}
fn request(task: &str) -> TeamRequest {
    serde_json::from_slice(
        &fs::read(outer().join(format!("fixtures/{task}-r1-team/request.json"))).unwrap(),
    )
    .unwrap()
}
fn blueprint(task: &str) -> TeamBlueprint {
    team_materials::compose(&request(task)).unwrap()
}
fn save(name: &str, value: &impl serde::Serialize) {
    crate::api_execution::publish(&outer(), name, value).unwrap();
}
fn fixture_manager(
    intent: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let selected = intent
        .project_id
        .strip_prefix("eff-")
        .unwrap()
        .split('-')
        .next()
        .unwrap();
    let expected = blueprint(selected);
    let mut result = manager(intent)?;
    if let ManagementDecision::Plan { tasks, .. } = &mut result.envelope.decision {
        *tasks = expected.tasks;
    }
    Ok(result)
}
pub(super) fn configured(task: &str) -> (TeamBlueprint, RuntimeHost) {
    let b = blueprint(task);
    let mut host = RuntimeHost::open(Path::new(&b.request.material.data_root)).unwrap();
    host.register(b.plan.project.clone()).unwrap();
    let s = &b.setup.setup;
    let mut validators = vec![(s.validator_id.clone(), s.check.tool.clone().unwrap())];
    for p in &s.profiles {
        let stage = &p.acceptance.stages[0];
        validators.push((
            stage.validator_id.clone(),
            stage.check.tool.clone().unwrap(),
        ));
    }
    for (id, tool) in validators {
        host.register_validator(
            id.clone(),
            id,
            tool.executable.path,
            tool.inputs.into_iter().map(|f| f.path).collect(),
            tool.environment,
        )
        .unwrap();
    }
    host.configure_project(b.configuration_draft()).unwrap();
    let id = &b.plan.project.id;
    let before = host.project_setups[id].digest.clone();
    host.prepare_team_experiment_inputs(id, &before).unwrap();
    assert_eq!(host.project_setups[id].digest, b.setup.digest);
    host.management_fixture = Some(fixture_manager);
    host.api_fixture = Some(fixture_worker);
    (b, host)
}
pub(super) fn keys(host: &mut RuntimeHost, set: bool) {
    host.set_openai_key(
        set.then(|| secret::SecretInput::new("sk-synthetic-team250-manager".into()).unwrap()),
    )
    .unwrap();
    host.set_provider_key(
        CredentialProvider::DeepSeek,
        set.then(|| secret::SecretInput::new("sk-synthetic-team250-worker".into()).unwrap()),
    )
    .unwrap();
}
pub(super) fn plan(host: &mut RuntimeHost, b: &TeamBlueprint) {
    let id = &b.plan.project.id;
    host.request_chief_plan(id, &b.setup.digest).unwrap();
    spin(host, |h| h.projects[id].revision().is_some());
    assert_eq!(host.projects[id].revision(), Some(&b.plan));
}
const LEFT: &str = "export function normalize(events){if(!Array.isArray(events))throw new TypeError();const m=new Map();for(const e of events){if(!e||typeof e.id!=='string'||!e.id.length||!Number.isSafeInteger(e.seq)||e.seq<0||!['queued','running','done','failed'].includes(e.state))throw new TypeError();if(!m.has(e.id)||m.get(e.id).seq<=e.seq)m.set(e.id,{id:e.id,seq:e.seq,state:e.state});}return [...m.values()].sort((a,b)=>a.id<b.id?-1:a.id>b.id?1:0);}\n";
const RIGHT: &str = "export function summarize(rows){const counts={queued:0,running:0,done:0,failed:0};const completedIds=[];for(const r of rows){counts[r.state]++;if(r.state==='done')completedIds.push(r.id);}completedIds.sort();return {counts,completedIds};}\n";
const MAIN: &str = "import {normalize} from './left.mjs';import {summarize} from './right.mjs';export function run(events){return summarize(normalize(events));}\n";
pub(super) fn fixture_worker(
    request: &PreparedRequest,
    round: u32,
) -> Result<ApiReply, ApiFailure> {
    let messages = fixture_messages(request);
    let ApiMessage::User(prompt) = &messages[0] else {
        panic!("prompt missing")
    };
    let (id, name, content) = if prompt.contains("Fixed task: left.") {
        ("left", "left.mjs", LEFT)
    } else if prompt.contains("Fixed task: right.") {
        ("right", "right.mjs", RIGHT)
    } else {
        assert!(prompt.contains("Fixed task: integrate."));
        ("integrate", "main.mjs", MAIN)
    };
    if round > 1 {
        let ApiMessage::Tools(results) = messages.last().unwrap() else {
            panic!("tool evidence missing")
        };
        assert!(!results[0].is_error);
        if id == "integrate" && round <= 3 {
            let v: serde_json::Value = serde_json::from_str(&results[0].text).unwrap();
            assert_eq!(v["content"], if round == 2 { LEFT } else { RIGHT });
        }
    }
    let write_round = if id == "integrate" { 3 } else { 1 };
    let tool = if id == "integrate" && round < 3 {
        Some((
            "cerulean_read_file",
            json!({"path":if round==1 {"left.mjs"} else {"right.mjs"},"offset":0}),
        ))
    } else if round == write_round {
        let seed = fs::read(
            Path::new(&request_for_worker().material.bundle)
                .join("inputs/project")
                .join(name),
        )
        .unwrap();
        Some((
            "cerulean_write_file",
            json!({"path":name,"content":content,"expectedSha256":format!("{:x}",Sha256::digest(seed))}),
        ))
    } else {
        None
    };
    let message = match &tool {
        Some((name, args)) => {
            json!({"role":"assistant","content":null,"tool_calls":[{"id":format!("call-{id}-{round}"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]})
        }
        None => {
            json!({"role":"assistant","content":"Synthetic branch complete; independent acceptance required."})
        }
    };
    fixture_reply(request,json!({"id":format!("team250-{id}-{round}"),"model":"synthetic-worker","choices":[{"index":0,"finish_reason":if tool.is_some(){"tool_calls"}else{"stop"},"message":message}],"usage":{"prompt_tokens":10,"completion_tokens":10}})).map_err(|_| ApiFailure::InvalidResponse {})
}
fn request_for_worker() -> TeamRequest {
    request("P01")
}

#[test]
#[ignore = "250 fixed seed-only materials, pinned Git/Node; no dispatch"]
fn compose_all_workloads_and_reject_forged_fields() {
    let mut all = vec![];
    for kind in ["S", "P"] {
        for n in 1..=6 {
            let b = blueprint(&format!("{kind}{n:02}"));
            assert_eq!(b.tasks.len(), if kind == "S" { 1 } else { 3 });
            assert!(!Path::new(&b.request.material.data_root).exists());
            assert!(crate::baseline_materials::compose(&b.request.material).is_err());
            all.push(b);
        }
    }
    save("twelve-blueprints.json", &all);
    let original = &all[6];
    let fields = [
        "goal",
        "dependencies",
        "worker",
        "budget",
        "chief",
        "deputy",
        "check",
        "files",
        "seed",
        "proof",
        "snapshot",
        "requirement",
    ];
    for field in fields {
        let mut b = original.clone();
        match field {
            "goal" => b.plan.workflow.tasks[0].goal.push('x'),
            "dependencies" => b.plan.workflow.tasks[2].dependencies.clear(),
            "worker" => b.plan.execution[0]
                .api_worker
                .as_mut()
                .unwrap()
                .model
                .push('x'),
            "budget" => b.setup.setup.management_max_requests += 1,
            "chief" => b.setup.setup.chief_model.push('x'),
            "deputy" => b.setup.setup.deputy_model.push('x'),
            "check" => b.plan.execution[0].checks[0].args.clear(),
            "files" => b.plan.execution[2].artifact_paths.push("left.mjs".into()),
            "seed" => b.seed.files[0].sha256 = "0".repeat(64),
            "proof" => b.proof.configuration_sha256 = "0".repeat(64),
            "snapshot" => b
                .setup
                .setup
                .snapshot_policy
                .exclude
                .push("node_modules".into()),
            "requirement" => b.setup.setup.requirement.push('x'),
            _ => unreachable!(),
        }
        b.setup.digest = digest(&b.setup.setup).unwrap();
        assert!(team_materials::verify(&b).is_err(), "{field}");
    }
    // Keep the previous single-executor contract while sharing parsing code.
    let old: crate::baseline_materials::ComposeRequest = serde_json::from_slice(
        &fs::read(
            outer()
                .parent()
                .unwrap()
                .join("material-binding-248/fixtures/S02-r1-single/request.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(crate::baseline_materials::compose(&old).is_ok());
    save(
        "composition-result.json",
        &json!({"workloads":12,"forgedFieldsRejected":fields,"singleModeStillWorks":true,"realModelCalls":0}),
    );
}

#[test]
#[ignore = "250 exact branch assertion projection and input pins; no dispatch"]
fn reject_changed_branch_content_and_control_inventory() {
    let req = request("P03");
    let mut outcomes = vec![];
    for field in [
        "branch-content",
        "branch-missing",
        "metadata",
        "session",
        "seed",
    ] {
        let mut r = req.clone();
        let mut restore = None;
        match field {
            "branch-content" => {
                let p = PathBuf::from(&r.branch_specs["left"].path);
                let before = fs::read(&p).unwrap();
                let mut v: serde_json::Value = serde_json::from_slice(&before).unwrap();
                v["cases"][0]["expected"] = json!("forged");
                let data = serde_json::to_vec(&v).unwrap();
                fs::write(&p, &data).unwrap();
                r.branch_specs.get_mut("left").unwrap().sha256 =
                    format!("{:x}", Sha256::digest(&data));
                restore = Some((p, before));
            }
            "branch-missing" => {
                r.branch_specs.remove("right");
            }
            "metadata" => r.workloads = r.material.schedule.clone(),
            "session" => r.chief_session_id = Some("unselected-session".into()),
            "seed" => {
                let p = Path::new(&r.material.project_root).join("main.mjs");
                let before = fs::read(&p).unwrap();
                fs::write(&p, "changed").unwrap();
                restore = Some((p, before));
            }
            _ => unreachable!(),
        }
        let result = team_materials::compose(&r);
        if let Some((p, bytes)) = restore {
            fs::write(p, bytes).unwrap();
        }
        assert!(result.is_err(), "{field}");
        outcomes.push(field);
    }
    assert!(team_materials::compose(&req).is_ok());
    save("material-negatives.json", &outcomes);
}

#[test]
#[ignore = "250 preplanning and prelaunch guards; synthetic planning only"]
fn binding_and_launch_guards_precede_dispatch() {
    let (b, mut host) = configured("P02");
    let id = &b.plan.project.id;
    let original = host.project_setups[id].clone();
    host.project_setups.get_mut(id).unwrap().digest = "0".repeat(64);
    assert!(host.bind_team_experiment_materials(&b.request).is_err());
    host.project_setups.insert(id.clone(), original);
    let binding = host.bind_team_experiment_materials(&b.request).unwrap();
    assert_eq!(binding, digest(&b).unwrap());
    assert!(host.bind_team_experiment_materials(&b.request).is_err());
    keys(&mut host, true);
    plan(&mut host, &b);
    keys(&mut host, false);
    assert!(host.bind_team_experiment_materials(&b.request).is_err());
    let revision = b.plan.approval_digest().unwrap();
    host.approve(id, &revision).unwrap();
    let file = Path::new(&b.request.material.bundle).join("inputs/control/task.txt");
    let original = fs::read(&file).unwrap();
    fs::write(&file, "changed task").unwrap();
    let failed = host.start_observed_team_run(id, &revision, &b.setup.digest);
    fs::write(file, original).unwrap();
    assert!(failed.is_err());
    assert!(host.runs.all().is_empty() && !host.root.join("team-observations").exists());
    // The binding is checked even through the original unqualified observation API.
    let mut setup = host.project_setups[id].clone();
    setup.setup.deputy_model.push('x');
    host.project_setups.insert(id.clone(), setup);
    assert!(host
        .start_observed_team_run(id, &revision, &b.setup.digest)
        .is_err());
    host.project_setups.insert(id.clone(), b.setup.clone());
    host.api_fixture = None;
    host.management_fixture = None;
    assert!(host
        .start_observed_team_run(id, &revision, &b.setup.digest)
        .err()
        .unwrap()
        .contains("synthetic"));
    assert!(host.runs.all().is_empty() && !host.root.join("team-observations").exists());
    save(
        "guards-result.json",
        &json!({"beforePlanningBinding":true,"changedMaterialAndSetupRejected":true,"productionSyntheticRejected":true,"workerCalls":0,"realModelCalls":0}),
    );
}

#[test]
#[ignore = "250 original Host, worktrees, file tools and independent Node checks; synthetic models"]
fn bound_parallel_team_accepts_and_reopens_without_redispatch() {
    let (b, mut host) = configured("P01");
    let id = &b.plan.project.id;
    host.bind_team_experiment_materials(&b.request).unwrap();
    keys(&mut host, true);
    plan(&mut host, &b);
    let revision = b.plan.approval_digest().unwrap();
    host.approve(id, &revision).unwrap();
    let observation = host
        .start_observed_team_run(id, &revision, &b.setup.digest)
        .unwrap();
    assert!(observation.launch_error().is_none());
    let selected = observation.start_digest().to_owned();
    save(
        "selection.json",
        &json!({"dataRoot":host.root,"project":id,"startDigest":selected}),
    );
    spin(&mut host, |h| {
        h.runs.all()[0].state != RunState::Active
            && h.api_active.is_empty()
            && h.active.is_empty()
            && h.management_work_count() == 0
    });
    let end = observation.finish(&host).unwrap();
    keys(&mut host, false);
    save("terminal.json", &end);
    assert_eq!(end["accepted"], true);
    assert_eq!(b.plan.execution[2].artifact_paths, vec!["main.mjs"]);
    let history = host.projects[id].history().to_vec();
    drop(host);
    let report = crate::host::team_observation::inspect(
        Path::new(&b.request.material.data_root),
        id,
        &selected,
    )
    .unwrap();
    assert_eq!(report["materialContentBound"], true);
    assert_eq!(report["coreExport"]["apiReplies"], 8);
    assert_eq!(
        report["coreExport"]["management"]["operations"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    save("inspection.json", &report);
    for _ in 0..2 {
        let mut h = RuntimeHost::open(Path::new(&b.request.material.data_root)).unwrap();
        h.poll_verifications();
        assert_eq!(h.projects[id].history(), history);
        assert!(h
            .start_observed_team_run(id, &revision, &b.setup.digest)
            .is_err());
    }
    save(
        "accepted-result.json",
        &json!({"accepted":true,"workerReplies":8,"managementOperations":7,"reopensWithoutRedispatch":2,"realModelCalls":0}),
    );
}
