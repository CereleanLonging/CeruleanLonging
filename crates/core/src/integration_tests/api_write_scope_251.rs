use super::team_materials_250 as material;
use super::*;
fn outer() -> PathBuf {
    PathBuf::from(std::env::var_os("CERULEAN_TEAM_250_DIR").unwrap())
}
fn save(name: &str, v: &impl serde::Serialize) {
    crate::api_execution::publish(&outer(), name, v).unwrap();
}
fn worker(request: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    let messages = fixture_messages(request);
    let ApiMessage::User(prompt) = &messages[0] else {
        panic!("prompt missing")
    };
    let id = if prompt.contains("Fixed task: left.") {
        "left"
    } else if prompt.contains("Fixed task: right.") {
        "right"
    } else {
        "integrate"
    };
    if round <= 2 {
        let path = if id == "left" {
            "right.mjs"
        } else if id == "right" {
            "main.mjs"
        } else {
            "left.mjs"
        };
        let (name, args) = if round == 1 {
            ("cerulean_read_file", json!({"path":path,"offset":0}))
        } else {
            let ApiMessage::Tools(results) = messages.last().unwrap() else {
                panic!("actual read missing")
            };
            assert!(!results[0].is_error);
            let read: serde_json::Value = serde_json::from_str(&results[0].text).unwrap();
            (
                "cerulean_write_file",
                json!({"path":path,"content":"out of scope","expectedSha256":read["sha256"]}),
            )
        };
        return fixture_reply(request,json!({"id":format!("scope251-{id}-{round}"),"model":"synthetic-worker","choices":[{"index":0,"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"id":format!("scope-{id}-{round}"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]}}],"usage":{"prompt_tokens":10,"completion_tokens":10}})).map_err(|_|ApiFailure::InvalidResponse{});
    }
    if round == 3 {
        let ApiMessage::Tools(results) = messages.last().unwrap() else {
            panic!("denial missing")
        };
        assert!(results[0].is_error && results[0].text.contains("artifact write scope"));
    }
    material::fixture_worker(request, round - 2)
}
#[test]
#[ignore = "251 fresh material-bound Host; synthetic out-of-scope attempts plus real Git/files/Node checks"]
fn scoped_team_rejects_cross_branch_writes_and_accepts_original_allowed_work() {
    let (b, mut host) = material::configured("P01");
    let id = &b.plan.project.id;
    assert!(b
        .setup
        .setup
        .api_worker
        .as_ref()
        .unwrap()
        .write_scope
        .is_some());
    assert!(b
        .plan
        .execution
        .iter()
        .all(|t| t.api_worker.as_ref().unwrap().write_scope.is_some()));
    host.api_fixture = Some(worker);
    host.bind_team_experiment_materials(&b.request).unwrap();
    material::keys(&mut host, true);
    material::plan(&mut host, &b);
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
    material::keys(&mut host, false);
    save("terminal.json", &end);
    assert_eq!(end["accepted"], true);
    let history = host.projects[id].history().to_vec();
    drop(host);
    let report = crate::host::team_observation::inspect(
        Path::new(&b.request.material.data_root),
        id,
        &selected,
    )
    .unwrap();
    assert_eq!(report["materialContentBound"], true);
    assert_eq!(report["coreExport"]["apiReplies"], 14);
    assert_eq!(
        report["coreExport"]["verifications"]
            .as_array()
            .unwrap()
            .len(),
        3
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
        &json!({"accepted":true,"crossBranchToolDenials":3,"workerReplies":14,"managementOperations":7,"independentChecks":3,"reopensWithoutRedispatch":2,"realModelCalls":0}),
    );
}
