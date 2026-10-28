//! Same scheduler/worktrees/validators as the nonmonetary integration fixture.
//! Only provider count and generation transports are replaced; real journals.
use super::*;
use crate::currency_budget::Outcome;
use cerulean_adapters::{
    api_generation::scoped::{
        fixture_counted_api, fixture_scoped_api_messages, fixture_scoped_api_result,
        PreparedScopedApi, ScopedApiResult,
    },
    openai::scoped_pricing::{fixture_counted_scoped_decision, fixture_scoped_result},
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc, Mutex,
};

const ADD: &str = "export function add(a,b){return a+b;}\n";
const MULTIPLY: &str = "export function multiply(a,b){return a*b;}\n";
const BUNDLE: &str = "import {add} from './a.mjs'; import {multiply} from './b.mjs'; export function compute(a,b,c){return multiply(add(a,b),c);}\n";

fn owned(mode: &str) -> PathBuf {
    let parent = PathBuf::from(std::env::var_os("CERULEAN_SCOPED_FLOW_DIR").unwrap());
    let parent = fs::canonicalize(parent).unwrap();
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert!(parent.starts_with(&reports) && parent != reports);
    let path = tempfile::Builder::new()
        .prefix(mode)
        .tempdir_in(parent)
        .unwrap()
        .keep();
    println!("owned scoped flow: {}", path.display());
    path
}
fn configured(root: &Path, mode: &str) -> RuntimeHost {
    configured_with_limit(root, mode, None)
}
fn configured_with_limit(root: &Path, mode: &str, limit: Option<u64>) -> RuntimeHost {
    let (host, original) = project_planning::tests::fixture(root);
    drop(host);
    let data = root.join("data");
    crate::currency_api_guard::tests::reader10(&data);
    crate::currency_api_guard::tests::promote_fixture(&data);
    let mut host = RuntimeHost::open(&data).unwrap();
    let node = std::env::var("CERULEAN_TEST_NODE").expect("explicit existing Node required");
    for id in ["partial-code", "whole-code"] {
        host.register_validator(
            id.into(),
            "Independent Node tests".into(),
            node.clone(),
            vec![],
            Default::default(),
        )
        .unwrap();
    }
    let mut draft =
        api_project_tests::draft_for(original, ProjectWorkerAdapter::Openai, ApiTarget::OpenAi {});
    draft.requirement = mode.into();
    draft.artifact_paths = vec!["result.txt".into()];
    draft.worker_max_requests = 20;
    draft.management_max_requests = 20;
    draft.api_worker.as_mut().unwrap().max_rounds = 4;
    draft.validator_id = "partial-code".into();
    draft.check_args = vec!["--input-type=module".into(), "-e".into(), r#"import assert from 'node:assert/strict'; import {existsSync,readFileSync} from 'node:fs'; import {pathToFileURL} from 'node:url'; const a=existsSync('a.mjs'); const m=await import(pathToFileURL(process.cwd()+'/'+(a?'a.mjs':'b.mjs'))); assert.equal(a?m.add(2,3):m.multiply(2,3),a?5:6); assert.equal(a?m.add(-2,2):m.multiply(-2,2),a?0:-4); assert(['alpha','beta'].includes(readFileSync('result.txt','utf8'))); assert(!existsSync('bundle.mjs'));"#.into()];
    let mut worktree = dependency_host_fixture::configure(root);
    worktree.integration = Some(IntegrationDraft {
        task_id: "integration".into(),
        artifact_paths: vec!["bundle.mjs".into(), "a.mjs".into(), "b.mjs".into()],
        validator_id: "whole-code".into(),
        args: vec!["--input-type=module".into(), "-e".into(), r#"import assert from 'node:assert/strict'; import {readFileSync,existsSync} from 'node:fs'; import {pathToFileURL} from 'node:url'; const {compute}=await import(pathToFileURL(process.cwd()+'/bundle.mjs')); assert.equal(compute(2,3,4),20); assert.equal(compute(-2,2,9),0); assert.equal(compute(-3,-2,2),-10); assert.equal(readFileSync('base.txt','utf8'),'committed dependency base\n'); assert(!existsSync('.git'));"#.into()],
        timeout_seconds: 30,
    });
    draft.worktree = Some(worktree);
    let mut settings = management_currency_tests::scoped_decision::settings();
    if let Some(limit) = limit {
        settings.limit_usd_nanos = limit;
    }
    for quote in settings.scoped_quotes.as_mut().unwrap() {
        if quote.requested_model == "vendor/fixture-model" {
            quote.reported_models = vec!["fixture-worker-snapshot".into()];
            for band in &mut quote.bands {
                band.actual_model = "fixture-worker-snapshot".into();
            }
        }
    }
    draft.currency_budget = Some(settings);
    host.configure_project(draft).unwrap();
    host.set_openai_key(Some(
        secret::SecretInput::new("sk-synthetic-scoped-flow".into()).unwrap(),
    ))
    .unwrap();
    host
}
fn attach_manager(host: &mut RuntimeHost) {
    host.management_scoped_fixture = Some(management_scoped_host::ScopedFixture {
        count: Arc::new(|draft, quote| fixture_counted_scoped_decision(draft, quote, 100)),
        send: Arc::new(|intent, prepared| {
            let mut observed = manager(intent.clone())?;
            match &mut observed.envelope.decision {
                ManagementDecision::Plan { tasks, .. } => {
                    for task in tasks {
                        task.worker_id = "worker-openai".into();
                        task.validator_id = if task.id == "integration" {
                            "whole-code"
                        } else {
                            "partial-code"
                        }
                        .into();
                    }
                }
                ManagementDecision::Assign { worker_id, .. } => *worker_id = "worker-openai".into(),
                _ => {}
            }
            fixture_scoped_result(
                &prepared,
                json!({"id":observed.response_id,"model":intent.model,"status":"completed","service_tier":"default",
                "output":[{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":serde_json::to_string(&observed.envelope).unwrap()}]}],
                "usage":{"input_tokens":10,"output_tokens":20,"input_tokens_details":{"cached_tokens":2,"cache_write_tokens":1}}}),
            )
        }),
        fail_count_spawn: false,
        fail_generation_spawn: false,
    });
}
fn task(prepared: &PreparedScopedApi) -> &str {
    let ApiMessage::User(prompt) = &fixture_scoped_api_messages(prepared)[0] else {
        panic!("original prompt missing")
    };
    if prompt.contains("INTEGRATION_integration") {
        "integration"
    } else if prompt.contains("INTEGRATION_b") {
        "b"
    } else {
        "a"
    }
}
fn worker(prepared: &PreparedScopedApi, round: u32) -> Result<ScopedApiResult, ApiFailure> {
    let messages = fixture_scoped_api_messages(prepared);
    let ApiMessage::User(prompt) = &messages[0] else {
        panic!("original prompt missing")
    };
    let task = task(prepared);
    let call = if task == "integration" {
        if matches!(round, 2 | 3) {
            let ApiMessage::Tools(results) = messages.last().unwrap() else {
                panic!("actual read missing")
            };
            assert!(!results[0].is_error);
            let read: serde_json::Value = serde_json::from_str(&results[0].text).unwrap();
            assert_eq!(read["content"], if round == 2 { ADD } else { MULTIPLY });
        }
        match round {
            1 => Some(("cerulean_read_file", json!({"path":"a.mjs","offset":0}))),
            2 => Some(("cerulean_read_file", json!({"path":"b.mjs","offset":0}))),
            3 => Some((
                "cerulean_write_file",
                json!({"path":"bundle.mjs","content":if prompt.contains("bad-whole") {"export function compute(){return 99;}\n"} else {BUNDLE},"expectedSha256":"absent"}),
            )),
            _ => None,
        }
    } else {
        match round {
            1 => Some((
                "cerulean_write_file",
                json!({"path":"result.txt","content":if task=="b" && prompt.contains("conflict") {"beta"} else {"alpha"},"expectedSha256":"absent"}),
            )),
            2 => Some((
                "cerulean_write_file",
                json!({"path":if task=="b" {"b.mjs"} else {"a.mjs"},"content":if task=="b" {MULTIPLY} else {ADD},"expectedSha256":"absent"}),
            )),
            _ => None,
        }
    };
    let output = match call {
        Some((name, args)) => {
            json!([{"type":"function_call","id":format!("fc_{task}_{round}"),"call_id":format!("call_{task}_{round}"),"name":name,"status":"completed","arguments":args.to_string()}])
        }
        None => {
            json!([{"type":"message","id":format!("msg_{task}_{round}"),"role":"assistant","status":"completed","content":[{"type":"output_text","text":"Ready for independent validation"}]}])
        }
    };
    fixture_scoped_api_result(prepared,json!({"id":format!("resp_{task}_{round}"),"model":"fixture-worker-snapshot","status":"completed","service_tier":"default","output":output,
        "usage":{"input_tokens":10,"output_tokens":20,"input_tokens_details":{"cached_tokens":2,"cache_write_tokens":1}}})).map_err(|_|ApiFailure::InvalidResponse {})
}
fn attach_worker(host: &mut RuntimeHost) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    host.api_scoped_fixture = Some(crate::api_currency::Fixture {
        count: Arc::new(move |draft, quote| {
            counted.fetch_add(1, Ordering::SeqCst);
            fixture_counted_api(draft, quote, 100).map_err(|_| ApiFailure::InvalidResponse {})
        }),
        send: Arc::new(worker),
    });
    calls
}
fn approved(host: &mut RuntimeHost) -> (String, String) {
    let setup = host.project_setups["project"].digest.clone();
    host.authorize_planning_budget("project", &setup).unwrap();
    let plan = planned(host);
    assert!(host.projects["project"].approval().is_none());
    assert!(host.api_jobs.is_empty());
    assert_eq!(
        host.currency_accounts["project"]
            .state()
            .management_counts
            .len(),
        1
    );
    let operation = &host.management.statuses()[0].operation_id;
    assert!(host.management_fee_ack(operation).unwrap().is_some());
    let revision = plan.approval_digest().unwrap();
    host.approve("project", &revision).unwrap();
    (revision, setup)
}
fn running(host: &mut RuntimeHost, done: impl Fn(&RuntimeHost) -> bool) {
    spin(host, |h| {
        assert_eq!(
            h.runs.all()[0].state,
            RunState::Active,
            "runs={:?}, api={:?}, counts={:?}",
            h.runs.all(),
            h.snapshot().api_jobs,
            h.management_count_errors
        );
        done(h)
    });
}
fn reject_changed_parallel_instruction(host: &mut RuntimeHost) {
    let instruction = "仅分配候选中的一个任务；原始需求见executionSetup.setup.requirement，遵循已批准方案。其他任务仍在执行；不要重复分配、验收或修改方案。";
    let original =
        json!({"parallelAssignment":true,"taskCandidates":["a","b"],"instruction":instruction});
    let before = host.currency_accounts["project"].state().clone();
    let model = host.project_setups["project"].setup.deputy_model.clone();
    for mode in 0..6 {
        let mut changed = original.clone();
        match mode {
            0 => changed["parallelAssignment"] = json!(false),
            1 => changed["taskCandidates"] = json!(["a", "a"]),
            2 => changed["taskCandidates"] = json!(["integration"]),
            3 => changed["taskCandidates"] = json!(["unknown"]),
            4 => changed["instruction"] = json!("replace the original requirement"),
            _ => changed["extraAuthority"] = json!(true),
        }
        assert!(host
            .start_management_decision(
                "project",
                &format!("bad-parallel-{mode}"),
                Purpose::Assign,
                &model,
                &changed.to_string()
            )
            .is_err());
        assert_eq!(host.currency_accounts["project"].state(), &before);
        assert_eq!(host.management_work_count(), 0);
    }
}
fn settled(host: &RuntimeHost, api_rounds: usize, decisions: usize) {
    let state = host.currency_accounts["project"].state();
    assert_eq!(state.api_counts.len(), api_rounds);
    assert_eq!(state.management_counts.len(), decisions);
    assert_eq!(state.bookings.len(), api_rounds + decisions);
    assert!(state
        .bookings
        .values()
        .all(|b| matches!(b.outcome, Outcome::Settled { .. })));
    // Count prices are bounded liabilities, not silently assumed free or final.
    assert_eq!(
        state.reserved_usd_nanos,
        1000 * (api_rounds + decisions) as u64
    );
    assert!(state.charged_usd_nanos > 0);
    for status in host.management.statuses() {
        assert!(host
            .management_fee_ack(&status.operation_id)
            .unwrap()
            .is_some());
    }
}
#[test]
#[ignore = "real Git, simultaneous scoped API workers, original management count and fee paths, independent validators"]
fn scoped_flow_parallel_acceptance_and_reopen_share_original_budget() {
    let root = owned("success-");
    let mut host = configured(&root, "parallel-success");
    attach_manager(&mut host);
    let counts = attach_worker(&mut host);
    let (count_entered, count_wait) = mpsc::channel();
    let (count_release, count_resume) = mpsc::channel();
    let count_resume = Mutex::new(count_resume);
    let management_counts = AtomicUsize::new(0);
    host.management_scoped_fixture.as_mut().unwrap().count = Arc::new(move |draft, quote| {
        if management_counts.fetch_add(1, Ordering::SeqCst) == 2 {
            count_entered.send(()).unwrap();
            count_resume
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(60))
                .unwrap();
        }
        fixture_counted_scoped_decision(draft, quote, 100)
    });
    let (a_entered, a_wait) = mpsc::channel();
    let (a_release, a_resume) = mpsc::channel();
    let a_resume = Mutex::new(a_resume);
    let (second_entered, second_wait) = mpsc::channel();
    let (second_release, second_resume) = mpsc::channel();
    let second_resume = Mutex::new(second_resume);
    let (entered, wait) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let resume = Mutex::new(resume);
    host.api_scoped_fixture.as_mut().unwrap().send = Arc::new(move |p, n| {
        if task(p) == "a" && n == 1 {
            a_entered.send(()).unwrap();
            a_resume
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(60))
                .unwrap();
        }
        if task(p) == "b" && n == 1 {
            entered.send(()).unwrap();
            resume
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(60))
                .unwrap();
        }
        if task(p) == "a" && n == 2 {
            second_entered.send(()).unwrap();
            second_resume
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(60))
                .unwrap();
        }
        worker(p, n)
    });
    let (revision, setup) = approved(&mut host);
    reject_changed_parallel_instruction(&mut host);
    let api = host.api_scoped_fixture.take();
    assert!(host
        .start_project_run("project", &revision, &setup)
        .is_err());
    assert!(host.runs.all().is_empty());
    host.api_scoped_fixture = api;
    let manager = host.management_scoped_fixture.take();
    assert!(host
        .start_project_run("project", &revision, &setup)
        .is_err());
    assert!(host.runs.all().is_empty());
    host.management_scoped_fixture = manager;
    host.start_project_run("project", &revision, &setup)
        .unwrap();
    running(&mut host, |_| a_wait.try_recv().is_ok());
    running(&mut host, |_| count_wait.try_recv().is_ok());
    let original_head = host.currency_accounts["project"].head().to_owned();
    a_release.send(()).unwrap();
    running(&mut host, |_| second_wait.try_recv().is_ok());
    assert_ne!(host.currency_accounts["project"].head(), original_head);
    count_release.send(()).unwrap();
    running(&mut host, |_| wait.try_recv().is_ok());
    assert_eq!(host.api_active.len(), 2);
    assert_eq!(host.api_writers.len(), 1);
    assert!(host.jobs.is_empty());
    let gaps = host.currency_history_gaps().unwrap();
    let money = host.currency_account_views(Some(&gaps));
    let view = money.iter().find(|v| v.project_id == "project").unwrap();
    assert!(view.approval_linked);
    assert_eq!(
        view.available_usd_nanos,
        Some(
            host.currency_accounts["project"]
                .state()
                .available_usd_nanos(host.currency_accounts["project"].policy())
                .unwrap()
        )
    );
    second_release.send(()).unwrap();
    running(&mut host, |h| {
        h.snapshot().workflows[0].state.tasks["a"].state == TaskState::Accepted
    });
    assert_eq!(host.api_active.len(), 1);
    assert_eq!(host.api_writers.len(), 1);
    assert_eq!(host.jobs.len(), 1);
    assert_eq!(host.management.statuses().len(), 4);
    assert!(host.snapshot().workflows[0].state.tasks["a"].verified);
    let before = host.currency_accounts["project"].state().clone();
    for _ in 0..10 {
        host.poll_verifications();
    }
    assert_eq!(
        host.currency_accounts["project"].state(),
        &before,
        "waiting must not bill another decision"
    );
    release.send(()).unwrap();
    spin(&mut host, |h| {
        h.runs.all()[0].state != RunState::Active
            && h.api_active.is_empty()
            && h.management_work_count() == 0
            && h.active.is_empty()
    });
    assert_eq!(
        host.runs.all()[0].state,
        RunState::Completed,
        "{:?}",
        host.runs.all()
    );
    assert_eq!(host.jobs.len(), 3);
    assert!(host.projects["project"]
        .state()
        .tasks
        .values()
        .all(|t| t.state == TaskState::Accepted && t.verified));
    settled(&host, 10, 7);
    assert_eq!(counts.load(Ordering::SeqCst), 10);
    let path = dependency_host_fixture::task_path(&host, "integration");
    assert_eq!(fs::read_to_string(path.join("bundle.mjs")).unwrap(), BUNDLE);
    assert!(!root.join("project/bundle.mjs").exists());
    assert_eq!(
        fs::read_to_string(root.join("project/base.txt")).unwrap(),
        "uncommitted source change\n"
    );
    let money = host.currency_accounts["project"].state().clone();
    let history = host.projects["project"].history().to_vec();
    drop(host);
    let mut reopened = RuntimeHost::open(&root.join("data")).unwrap();
    reopened.poll_verifications();
    assert_eq!(reopened.runs.all()[0].state, RunState::Completed);
    assert_eq!(reopened.currency_accounts["project"].state(), &money);
    assert_eq!(reopened.projects["project"].history(), history);
    assert_eq!(counts.load(Ordering::SeqCst), 10);
    println!("scoped flow complete: 7 management decisions, 10 API rounds, 3 verified/accepted tasks, unchanged reopen");
}
#[test]
#[ignore = "real Git, scoped failure boundaries, independent integration verifier"]
fn scoped_flow_invalid_integration_or_merge_conflict_never_accepts() {
    for mode in ["bad-whole", "conflict"] {
        let root = owned(mode);
        let mut host = configured(&root, mode);
        attach_manager(&mut host);
        let counts = attach_worker(&mut host);
        let (revision, setup) = approved(&mut host);
        host.start_project_run("project", &revision, &setup)
            .unwrap();
        spin(&mut host, |h| {
            h.runs.all()[0].state != RunState::Active
                && h.api_active.is_empty()
                && h.management_work_count() == 0
                && h.active.is_empty()
        });
        assert_eq!(
            host.runs.all()[0].state,
            RunState::Attention,
            "{:?}",
            host.runs.all()
        );
        let state = host.projects["project"].state();
        assert_eq!(state.tasks["a"].state, TaskState::Accepted);
        assert_eq!(state.tasks["b"].state, TaskState::Accepted);
        assert!(!state.tasks["integration"].verified);
        assert_eq!(
            state.tasks["integration"].state,
            if mode == "conflict" {
                TaskState::Pending
            } else {
                TaskState::Submitted
            }
        );
        let rounds = if mode == "conflict" { 6 } else { 10 };
        settled(&host, rounds, 6);
        assert_eq!(counts.load(Ordering::SeqCst), rounds);
        let money = host.currency_accounts["project"].state().clone();
        let history = host.projects["project"].history().to_vec();
        drop(host);
        let mut reopened = RuntimeHost::open(&root.join("data")).unwrap();
        reopened.poll_verifications();
        assert_eq!(reopened.currency_accounts["project"].state(), &money);
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(reopened.runs.all()[0].state, RunState::Attention);
        assert_eq!(counts.load(Ordering::SeqCst), rounds);
    }
}

#[test]
#[ignore = "real Git and original concurrent manager/API accounting with bounded exhaustion"]
fn scoped_flow_shared_budget_exhaustion_retains_late_fees_and_stops_new_work() {
    for settle_before_count in [false, true] {
        let root = owned(if settle_before_count {
            "budget-after-api-"
        } else {
            "budget-before-api-"
        });
        // Enough for initial planning and assignment, but not the next deputy
        // generation while an API reservation and the count fees are retained.
        let limit = 11_800_000;
        let mut host = configured_with_limit(&root, "budget", Some(limit));
        attach_manager(&mut host);
        let counts = attach_worker(&mut host);
        let management_sends = Arc::new(AtomicUsize::new(0));
        let sent = management_sends.clone();
        let original_send = host
            .management_scoped_fixture
            .as_ref()
            .unwrap()
            .send
            .clone();
        host.management_scoped_fixture.as_mut().unwrap().send = Arc::new(move |i, p| {
            sent.fetch_add(1, Ordering::SeqCst);
            original_send(i, p)
        });
        let (count_entered, count_wait) = mpsc::channel();
        let (count_release, count_resume) = mpsc::channel();
        let count_resume = Mutex::new(count_resume);
        let manager_counts = AtomicUsize::new(0);
        host.management_scoped_fixture.as_mut().unwrap().count = Arc::new(move |draft, quote| {
            if manager_counts.fetch_add(1, Ordering::SeqCst) == 2 {
                count_entered.send(()).unwrap();
                count_resume
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(60))
                    .unwrap();
            }
            fixture_counted_scoped_decision(draft, quote, 100)
        });
        let (first_entered, first_wait) = mpsc::channel();
        let (first_release, first_resume) = mpsc::channel();
        let first_resume = Mutex::new(first_resume);
        let (second_entered, second_wait) = mpsc::channel();
        let (second_release, second_resume) = mpsc::channel();
        let second_resume = Mutex::new(second_resume);
        let api_sends = Arc::new(AtomicUsize::new(0));
        let sent = api_sends.clone();
        host.api_scoped_fixture.as_mut().unwrap().send = Arc::new(move |p, round| {
            assert_eq!(task(p), "a", "budget refusal must precede B dispatch");
            sent.fetch_add(1, Ordering::SeqCst);
            match round {
                1 => {
                    first_entered.send(()).unwrap();
                    first_resume
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(60))
                        .unwrap();
                }
                2 if settle_before_count => {
                    second_entered.send(()).unwrap();
                    second_resume
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(60))
                        .unwrap();
                }
                _ => panic!("no further API generation after shared-budget exhaustion"),
            }
            worker(p, round)
        });
        let (revision, setup) = approved(&mut host);
        host.start_project_run("project", &revision, &setup)
            .unwrap();
        running(&mut host, |_| first_wait.try_recv().is_ok());
        running(&mut host, |_| count_wait.try_recv().is_ok());
        let head_before_api = host.currency_accounts["project"].head().to_owned();
        if settle_before_count {
            first_release.send(()).unwrap();
            running(&mut host, |_| second_wait.try_recv().is_ok());
            assert_ne!(host.currency_accounts["project"].head(), head_before_api);
        }
        assert_eq!(host.api_active.len(), 1);
        assert_eq!(host.management_work_count(), 1);
        let book = &host.currency_accounts["project"];
        let available = book.state().available_usd_nanos(book.policy()).unwrap();
        let quote = book
            .policy()
            .scoped_quotes
            .as_ref()
            .unwrap()
            .iter()
            .find(|q| q.requested_model == "fixture-deputy")
            .unwrap();
        let ceiling = quote
            .reserve(
                &cerulean_domain::currency_budget::scoped::PricingRequest {
                    target: quote.target.clone(),
                    requested_model: quote.requested_model.clone(),
                    requested_service: quote.requested_service.clone(),
                    input: cerulean_domain::currency_budget::scoped::InputRange {
                        from: 0,
                        through: 100,
                    },
                    output_ceiling: 4096,
                    cache_read_allowed: true,
                    cache_write_allowed: true,
                },
                &chrono::Utc::now().to_rfc3339(),
            )
            .unwrap();
        assert!(available < ceiling);
        count_release.send(()).unwrap();
        spin(&mut host, |h| {
            h.runs.all()[0].state == RunState::Attention && h.management_work_count() == 0
        });
        let error = host.management_count_errors.get("project").unwrap();
        assert!(
            !error.contains("ledger changed"),
            "legitimate API billing is not a stale management authority: {error}"
        );
        assert_eq!(management_sends.load(Ordering::SeqCst), 2);
        assert_eq!(
            host.currency_accounts["project"]
                .state()
                .management_counts
                .len(),
            3
        );
        assert!(host.currency_accounts["project"]
            .state()
            .management_counts
            .values()
            .all(|c| matches!(
                c.outcome,
                crate::currency_budget::CountOutcome::ScopedCounted { .. }
            )));
        if settle_before_count {
            second_release.send(()).unwrap();
        } else {
            first_release.send(()).unwrap();
        }
        spin(&mut host, |h| {
            h.api_active.is_empty() && h.management_work_count() == 0
        });
        let rounds = if settle_before_count { 2 } else { 1 };
        assert_eq!(counts.load(Ordering::SeqCst), rounds);
        assert_eq!(api_sends.load(Ordering::SeqCst), rounds);
        let money = host.currency_accounts["project"].state().clone();
        assert_eq!(money.bookings.len(), 2 + rounds);
        assert!(money
            .bookings
            .values()
            .all(|b| matches!(b.outcome, Outcome::Settled { .. })));
        assert_eq!(money.reserved_usd_nanos, (3 + rounds as u64) * 1000);
        assert!(
            money.charged_usd_nanos > 0
                && money.charged_usd_nanos + money.reserved_usd_nanos <= limit
        );
        assert_eq!(money.api_counts.len(), rounds);
        let state = host.projects["project"].state();
        assert_eq!(state.tasks["b"].state, TaskState::Pending);
        assert_eq!(state.tasks["integration"].state, TaskState::Pending);
        assert!(state.tasks.values().all(|t| t.state != TaskState::Accepted));
        assert!(host.jobs.is_empty());
        let path = dependency_host_fixture::task_path(&host, "a");
        assert_eq!(path.join("result.txt").exists(), settle_before_count);
        assert!(!path.join("a.mjs").exists());
        for _ in 0..10 {
            host.poll_verifications();
        }
        assert_eq!(host.currency_accounts["project"].state(), &money);
        let history = host.projects["project"].history().to_vec();
        drop(host);
        let mut reopened = RuntimeHost::open(&root.join("data")).unwrap();
        reopened.poll_verifications();
        assert_eq!(reopened.runs.all()[0].state, RunState::Attention);
        assert_eq!(reopened.currency_accounts["project"].state(), &money);
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(counts.load(Ordering::SeqCst), rounds);
        assert_eq!(management_sends.load(Ordering::SeqCst), 2);
        fs::write(root.join("budget-proof.json"), serde_json::to_vec_pretty(&json!({
            "scope":"Synthetic transport; original scheduling/accounting, real Git and file tools",
            "settleBeforeCount":settle_before_count,"budgetUsdNanos":limit,
            "availableBeforeCountReturn":available,"deputyGenerationCeiling":ceiling,
            "managementGenerations":2,"managementCounts":3,"apiRounds":rounds,
            "chargedUsdNanos":money.charged_usd_nanos,"reservedUsdNanos":money.reserved_usd_nanos,
            "reopenedWithoutNewRequests":true,"runState":"attention","acceptedTasks":0,
            "partialResultExists":settle_before_count,"implementationFileExists":false,
            "ledgerHead":reopened.currency_accounts["project"].head(),
        })).unwrap()).unwrap();
        println!("shared budget exhausted: settle_before_count={settle_before_count}, API rounds={rounds}, charged={}, reserved={}", money.charged_usd_nanos, money.reserved_usd_nanos);
    }
}
