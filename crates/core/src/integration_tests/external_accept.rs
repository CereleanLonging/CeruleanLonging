//! Real files, Git (parallel case), API tools and independent verifier processes.
//! Only model outputs and explicit user authorization are controlled fixtures.
use super::*;
use crate::management_execution::DecisionState;
use cerulean_domain::{
    management_ingress::{ExternalChiefDecision, ManagementHandoff},
    project_setup::{ChiefSource, DeputySource, ManagementSources},
};

pub(super) fn response(host: &RuntimeHost, request: &ManagementHandoff) -> ExternalChiefDecision {
    let intent = host
        .management
        .intent(&request.operation_id)
        .unwrap()
        .clone();
    ExternalChiefDecision {
        request_digest: digest(request).unwrap(),
        session_id: "current-session".into(),
        envelope: manager(intent).unwrap().envelope,
    }
}
pub(super) fn configured_source(
    root: &Path,
    parallel: bool,
    deepseek: bool,
    advice: bool,
) -> RuntimeHost {
    let mut selected = None;
    let host = configured_with(root, "external-accept", |draft| {
        if parallel {
            draft.worktree.as_mut().unwrap().integration = None;
        } else {
            draft.worktree = None;
        }
        selected = Some(draft.clone());
    });
    drop(host);
    crate::management_source_guard::tests::promote_fixture(&root.join("data"));
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    let mut draft = selected.unwrap();
    draft.management_sources = Some(ManagementSources {
        chief_reasoning_effort: None,
        deputy_reasoning_effort: None,
        chief: ChiefSource::CodexSession {
            session_id: "current-session".into(),
        },
        deputy: if deepseek {
            DeputySource::DeepSeek {}
        } else {
            DeputySource::OpenAi {}
        },
    });
    draft.chief_model = "external-codex-session".into();
    if deepseek {
        draft.deputy_model = "deepseek-flash".into();
    }
    if advice {
        draft.supervision = Some(cerulean_domain::supervision::SupervisionPolicy {
            schema_version: 1,
            repeated_failure_threshold: 2,
            max_advice_per_attempt: 1,
            delivery_enabled: true,
        });
    }

    host.configure_project(draft).unwrap();
    host.management_fixture = Some(manager);
    if deepseek {
        host.set_openai_key(None).unwrap();
        host.deputy_management_fixture = Some(super::deepseek_routing::deputy);
        host.management_fixture = Some(|_| panic!("DeepSeek route must not call OpenAI"));
    }

    host.api_fixture = Some(parallel::gated);
    let digest = host.project_setups["project"].digest.clone();
    host.request_chief_plan("project", &digest).unwrap();
    let id = host.management_decisions()[0].operation_id.clone();
    let request = host.read_external_chief_request(&id).unwrap();
    let mut reply = response(&host, &request);
    let ManagementDecision::Plan { tasks, .. } = &mut reply.envelope.decision else {
        panic!("plan")
    };
    tasks.retain(|t| t.id == "a" || (parallel && t.id == "b"));
    host.submit_external_chief_decision(&id, "current-session", reply)
        .unwrap();
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &revision).unwrap();
    host.start_project_run("project", &revision, &digest)
        .unwrap();
    host
}
pub(super) fn waiting(host: &mut RuntimeHost) -> ManagementHandoff {
    spin(host, |h| {
        assert_eq!(
            h.runs.all()[0].state,
            RunState::Active,
            "{:?}",
            h.runs.all()
        );
        h.management_decisions()
            .iter()
            .any(|s| s.purpose == Purpose::Accept && s.state == DecisionState::AwaitingExternal)
    });
    let id = host
        .management_decisions()
        .into_iter()
        .find(|s| s.purpose == Purpose::Accept && s.state == DecisionState::AwaitingExternal)
        .unwrap()
        .operation_id;
    let count = host.management_operation_total();
    for _ in 0..5 {
        host.poll_verifications();
    }
    assert_eq!(host.management_operation_total(), count);
    assert_eq!(host.runs.all()[0].state, RunState::Active);
    host.read_external_chief_request(&id).unwrap()
}

#[test]
#[ignore = "real independent verifier and Git; external-chief serial/parallel acceptance with controlled model outputs"]
fn external_accept_serial_parallel_wait_pause_reopen_and_scoped_reply() {
    for parallel in [false, true] {
        let root = temporary();
        let mut host = configured_source(root.path(), parallel, false, false);
        let (a_wait, a_release) = parallel::gate("a");
        let b_gate = parallel.then(|| parallel::gate("b"));
        spin(&mut host, |_| a_wait.try_recv().is_ok());
        if let Some((b_wait, _)) = &b_gate {
            spin(&mut host, |_| b_wait.try_recv().is_ok());
            assert_eq!(host.api_active.len(), 2);
        }
        // Already assigned workers are gated above. The following chief request
        // must bypass the old OpenAI connection-flag precondition.
        host.openai_connection.key_configured = false;
        a_release.send(()).unwrap();
        let request = waiting(&mut host);
        assert_eq!(request.purpose, Purpose::Accept);
        let before = host.management_operation_total();
        let good = response(&host, &request);
        let mut wrong = good.clone();
        let ManagementDecision::Accept { attempt_id, .. } = &mut wrong.envelope.decision else {
            panic!("accept")
        };
        *attempt_id = "unverified-other-attempt".into();
        assert!(host
            .submit_external_chief_decision(&request.operation_id, "current-session", wrong)
            .is_err());
        assert!(host
            .submit_external_chief_decision(&request.operation_id, "wrong-session", good.clone())
            .is_err());
        assert!(host
            .management
            .response_head(&request.operation_id)
            .is_err());
        if parallel {
            // B's authoritative Submit occurs after A's request. It must not
            // invalidate A's unchanged verified target or permit another send.
            b_gate.as_ref().unwrap().1.send(()).unwrap();
            spin(&mut host, |h| h.api_active.is_empty());
            assert_ne!(
                digest(
                    &host
                        .management_context("project", "external-accept")
                        .unwrap()
                )
                .unwrap(),
                request.state_digest
            );
            let saved = host.project_setups["project"].clone();
            host.project_setups
                .get_mut("project")
                .unwrap()
                .setup
                .deputy_model = "different-deputy".into();
            assert!(host
                .submit_external_chief_decision(
                    &request.operation_id,
                    "current-session",
                    good.clone()
                )
                .is_err());
            host.project_setups.insert("project".into(), saved);
        } else {
            let id = host.runs.all()[0].id.clone();
            host.pause_project_run(&id).unwrap();
            assert!(host
                .submit_external_chief_decision(
                    &request.operation_id,
                    "current-session",
                    good.clone()
                )
                .is_err());
            drop(host);
            host = RuntimeHost::open(&root.path().join("data")).unwrap();
            assert_eq!(host.runs.all()[0].state, RunState::Paused);
            assert_eq!(
                digest(
                    &host
                        .read_external_chief_request(&request.operation_id)
                        .unwrap()
                )
                .unwrap(),
                digest(&request).unwrap()
            );
            assert_eq!(host.management_operation_total(), before);
            let run = host.runs.all()[0].clone();
            host.start_project_run("project", &run.revision_digest, &run.setup_digest)
                .unwrap();
            host.management_fixture = Some(manager);
            host.api_fixture = Some(worker);
        }
        host.submit_external_chief_decision(&request.operation_id, "current-session", good.clone())
            .unwrap();
        assert_eq!(host.management_operation_total(), before);
        assert!(host
            .submit_external_chief_decision(&request.operation_id, "current-session", good)
            .is_err());
        if parallel {
            let second = waiting(&mut host);
            assert_ne!(second.operation_id, request.operation_id);
            let reply = response(&host, &second);
            host.submit_external_chief_decision(&second.operation_id, "current-session", reply)
                .unwrap();
        }
        spin(&mut host, |h| h.runs.all()[0].state == RunState::Completed);
        let decisions = host.management_operation_total();
        let history = host.projects["project"].history().to_vec();
        let accepts = history
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::Accept { .. }))
            .count();
        assert_eq!(accepts, if parallel { 2 } else { 1 });
        assert!(
            host.management_active.is_empty()
                && host.api_active.is_empty()
                && host.active.is_empty()
        );
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        host.poll_verifications();
        assert_eq!(host.runs.all()[0].state, RunState::Completed);
        assert_eq!(host.management_operation_total(), decisions);
        assert_eq!(host.projects["project"].history(), history);
        println!("external accept parallel={parallel}: verified tasks={accepts}, same-request wait/pause/reopen, wrong replies rejected, no duplicate submission, realProviderCalls=0");
    }
}
