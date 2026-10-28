//! Terminate the whole original Host process at two live transport boundaries.
//! Model responses remain synthetic; no historical sender is reconstructed.
use super::*;
use std::{
    io::Write,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

static REPLAYS: AtomicUsize = AtomicUsize::new(0);

fn stop_at(point: &str) {
    if std::env::var("CERULEAN_ADVICE_CRASH_POINT").unwrap() == point {
        let root = PathBuf::from(std::env::var_os("CERULEAN_ADVICE_CRASH_CASE").unwrap());
        // This is a test observation marker, not an execution journal entry.
        // The parent reads it only after observing this process exit.
        let mut marker = fs::File::create_new(root.join("boundary.json")).unwrap();
        marker
            .write_all(
                &serde_json::to_vec(
                    &json!({"point":point,"pid":std::process::id(),"realProviderCalls":0}),
                )
                .unwrap(),
            )
            .unwrap();
        marker.sync_all().unwrap();
        // Exits all threads without unwinding Host/worker destructors.
        std::process::exit(91);
    }
}

fn manager_at_boundary(
    intent: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    if intent.purpose == Purpose::Advise {
        stop_at("management-inflight");
    }
    supervised_manager(intent)
}

fn worker_at_boundary(prepared: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    if round == 2 {
        assert_eq!(
            fixture_messages(prepared)
                .iter()
                .filter(|m| matches!(m, ApiMessage::User(s) if s.contains(ADVICE)))
                .count(),
            1
        );
        stop_at("next-request-inflight");
    }
    supervised_worker(prepared, round)
}

#[test]
#[ignore = "owned subprocess; explicit isolated root and crash boundary required"]
fn api_supervision_crash_child() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_ADVICE_CRASH_CASE").unwrap());
    let mut host = configured(
        &root,
        ProjectWorkerAdapter::Openai,
        ApiTarget::OpenAi {},
        "SUPERVISION_SUCCESS",
        8,
    );
    host.management_fixture = Some(manager_at_boundary);
    host.api_fixture = Some(worker_at_boundary);
    approved(&mut host);
    start(&mut host);
    spin(&mut host, |_| false);
}

fn durable_evidence(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, output: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, output);
            } else {
                let relative = path.strip_prefix(root).unwrap();
                let parts = relative
                    .iter()
                    .map(|s| s.to_string_lossy())
                    .collect::<Vec<_>>();
                let immutable = parts
                    .iter()
                    .any(|s| matches!(s.as_ref(), "management-decisions" | "events" | "rounds"))
                    || path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("advice-");
                if immutable && path.extension().is_some_and(|s| s == "json") {
                    output.insert(relative.to_path_buf(), fs::read(&path).unwrap());
                }
            }
        }
    }
    let mut output = BTreeMap::new();
    visit(root, root, &mut output);
    output
}

#[test]
#[ignore = "explicit retained crash root; terminates two owned Host processes"]
fn api_supervision_process_exit_preserves_unknown_and_never_replays() {
    let root = PathBuf::from(std::env::var_os("API_SUPERVISION_CRASH_ROOT").unwrap());
    assert!(root.is_absolute());
    fs::create_dir(&root).unwrap();
    let mut results = Vec::new();
    for point in ["management-inflight", "next-request-inflight"] {
        let case = root.join(point);
        fs::create_dir(&case).unwrap();
        let stdout = fs::File::create_new(case.join("child.stdout")).unwrap();
        let stderr = fs::File::create_new(case.join("child.stderr")).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "host::api_run_tests::supervision_pipeline::crash::api_supervision_crash_child",
                "--ignored",
                "--nocapture",
            ])
            .env("CERULEAN_ADVICE_CRASH_CASE", &case)
            .env("CERULEAN_ADVICE_CRASH_POINT", point)
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .unwrap();
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(60);
        let exit = loop {
            if let Some(exit) = child.try_wait().unwrap() {
                break exit;
            }
            if Instant::now() > deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("owned crash child timed out at {point}");
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(exit.code(), Some(91));
        let boundary: serde_json::Value =
            serde_json::from_slice(&fs::read(case.join("boundary.json")).unwrap()).unwrap();
        assert_eq!(boundary["point"], point);
        assert_eq!(boundary["pid"], pid);
        let data = case.join("data");
        let before = durable_evidence(&data);
        assert!(!before.is_empty());
        let mut states = Vec::new();
        let mut first_recovery = None;
        for _ in 0..2 {
            let mut host = RuntimeHost::open(&data).unwrap();
            host.management_fixture = Some(|_| {
                REPLAYS.fetch_add(1, Ordering::SeqCst);
                Err("unexpected management replay".into())
            });
            host.api_fixture = Some(|_, _| {
                REPLAYS.fetch_add(1, Ordering::SeqCst);
                Err(ApiFailure::InvalidResponse {})
            });
            for _ in 0..5 {
                host.poll_verifications();
            }
            assert!(host.api_active.is_empty() && host.management_active.is_empty());
            assert_eq!(REPLAYS.load(Ordering::SeqCst), 0);
            assert_eq!(host.management.statuses().len(), 3);
            assert_eq!(host.api_jobs.len(), 1);
            assert!(host.jobs.is_empty());
            let statuses = host.management.statuses();
            let advice = statuses
                .iter()
                .find(|s| s.purpose == Purpose::Advise)
                .unwrap();
            let intent = host.management.intent(&advice.operation_id).unwrap();
            let delivery = host.api_advice_delivery_state(intent).unwrap();
            assert_eq!(
                delivery,
                if point == "management-inflight" {
                    "awaitingApplication"
                } else {
                    "requestOutcomeUnknown"
                }
            );
            let store = &host.projects["project"];
            assert_eq!(store.state().tasks["a"].state, TaskState::Unknown);
            let attempt = store.state().tasks["a"].attempt.clone().unwrap();
            assert_eq!(store.state().tasks["b"].state, TaskState::Pending);
            assert!(!store.history().iter().any(|e| matches!(
                e.body.event.command,
                WorkflowCommand::Accept { .. } | WorkflowCommand::Submit { .. }
            )));
            assert_eq!(
                store
                    .history()
                    .iter()
                    .filter(|e| matches!(e.body.event.command, WorkflowCommand::Advise { .. }))
                    .count(),
                usize::from(point == "next-request-inflight")
            );
            assert_eq!(
                crate::api_execution::prior_requests(store).unwrap(),
                host.project_setups["project"]
                    .setup
                    .api_worker
                    .as_ref()
                    .unwrap()
                    .max_rounds
            );
            // Unsealed attempts retain the whole approved reservation. Count
            // actual journaled requests separately; unknown must not refund it.
            assert_eq!(
                before
                    .keys()
                    .filter(|p| p.file_name().is_some_and(|s| s == "00.json")
                        && p.iter().any(|s| s == "rounds"))
                    .count(),
                if point == "management-inflight" { 1 } else { 2 }
            );
            states.push(json!({"delivery":delivery,"run":host.runs.all()[0].state,
                "workflowDigest":digest(&store.history()).unwrap()}));
            host.set_openai_key(None).unwrap();
            drop(host);
            let after = durable_evidence(&data);
            for (path, bytes) in &before {
                assert!(
                    after.get(path) == Some(bytes),
                    "original evidence changed: {}",
                    path.display()
                );
            }
            let added = after
                .iter()
                .filter(|(p, _)| !before.contains_key(*p))
                .collect::<Vec<_>>();
            assert_eq!(
                added.len(),
                1,
                "only original RecoveryUnknown may be appended"
            );
            let recovery: serde_json::Value = serde_json::from_slice(added[0].1).unwrap();
            assert_eq!(recovery["body"]["event"]["principal"], "core");
            assert_eq!(
                recovery["body"]["event"]["command"],
                json!({"kind":"recovery-unknown","task":"a","attempt":attempt})
            );
            if let Some(previous) = &first_recovery {
                assert!(previous == &after, "second recovery must append nothing");
            } else {
                first_recovery = Some(after);
            }
        }
        assert_eq!(states[0], states[1]);
        let result = json!({"point":point,"childPid":pid,"exit":91,
            "originalEvidenceFiles":before.len(),"recoveryUnknownEvents":1,"reopens":states,"replayedRequests":0,
            "realProviderCalls":0,"scope":"original Host process exit, synthetic transports"});
        crate::api_execution::publish(&case, "result.json", &result).unwrap();
        results.push(result);
    }
    crate::api_execution::publish(&root, "result.json", &json!({"cases":results})).unwrap();
}
