use super::*;
use cerulean_domain::management::Purpose;
use cerulean_domain::project_setup::{AcceptanceDraft, AcceptanceStageDraft, ProjectRun, RunState};

#[test]
fn worker_boundaries_live_guard_blocks_legacy_dispatch_but_preserves_reopen() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, draft) = project_planning::tests::fixture(root.path());
    host.configure_project(draft).unwrap();
    project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &revision).unwrap();
    let history = host.projects["project"].history().len();
    assert!(host
        .require_execution_boundaries("project")
        .unwrap_err()
        .contains("正常执行必须"));
    assert!(host
        .start_codex("project", "task", "new-attempt", &revision)
        .unwrap_err()
        .contains("正常执行必须"));
    let run = ProjectRun {
        id: "run".into(),
        project_id: "project".into(),
        revision_digest: revision,
        setup_digest: host.project_setups["project"].digest.clone(),
        state: RunState::Active,
        message: String::new(),
        acknowledged_decisions: 0,
    };
    host.run_scope(&run).unwrap(); // Historical identity checks remain readable.
    assert!(host
        .start_project_run("project", &run.revision_digest, &run.setup_digest)
        .unwrap_err()
        .contains("正常执行必须"));
    assert!(host.codex_jobs.is_empty());
    assert!(host.startups.all().is_empty());
    assert_eq!(host.projects["project"].history().len(), history);
    drop(host);
    let reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(reopened.projects["project"].history().len(), history);
}

fn isolated(root: &Path) -> RuntimeHost {
    let (mut host, mut draft) = project_planning::tests::fixture(root);
    draft.chief_directory = Some(root.join("chief").to_string_lossy().into());
    draft.acceptance = Some(AcceptanceDraft {
        schema_version: 1,
        stages: vec![AcceptanceStageDraft {
            id: "tests".into(),
            name: "behavior".into(),
            validator_id: "tests".into(),
            args: vec![],
            timeout_seconds: 10,
            expected_assertions: vec!["behavior".into()],
        }],
    });
    draft.check_args.clear();
    host.configure_project(draft).unwrap();
    project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
    host
}

#[test]
fn worker_boundaries_managers_receive_roots_ownership_and_fragment_only_review() {
    let root = tempfile::tempdir().unwrap();
    let host = isolated(root.path());
    host.require_task_resources("project", "task", "fresh")
        .unwrap();
    let raw = host
        .management_context("project", "Implement a sum function")
        .unwrap();
    let context = crate::worker_boundaries::annotate(raw.clone(), Purpose::Assign).unwrap();
    let row = &context["executionBoundaries"]["tasks"][0];
    let plan = host.projects["project"].revision().unwrap();
    assert_eq!(
        row["workingDirectory"],
        plan.execution_root("task", "fresh").unwrap()
    );
    assert_eq!(row["ownedSourcePaths"], serde_json::json!(["sum.mjs"]));
    assert_eq!(row["normalAccess"], "assigned-directory-only");
    assert_eq!(row["reviewAccess"], "approved-read-only-fragments-only");
    assert_eq!(context["workflow"], raw["workflow"]);
    let mut intent = crate::management_execution::DecisionIntent {
        operation_id: "boundary".into(),
        project_id: "project".into(),
        purpose: Purpose::Assign,
        model: "fixture".into(),
        state_digest: digest(&context).unwrap(),
        context: context.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    assert_eq!(
        management_assignments::rebuild(&intent, raw.clone()).unwrap(),
        context
    );
    intent.context["executionBoundaries"]["tasks"][0]["workingDirectory"] = "D:\\shared".into();
    assert!(management_assignments::rebuild(&intent, raw.clone())
        .unwrap_err()
        .contains("执行边界已变化"));
    intent.context = raw.clone();
    assert_eq!(
        management_assignments::rebuild(&intent, raw.clone()).unwrap(),
        raw
    );
}

#[test]
fn worker_boundaries_conflicting_proposal_does_not_mutate_workflow() {
    let root = tempfile::tempdir().unwrap();
    let mut host = isolated(root.path());
    let mut plan = host.projects["project"].revision().unwrap().clone();
    plan.workflow.version += 1;
    let mut other = plan.execution[0].clone();
    other.task_id = "other".into();
    other.worker.as_mut().unwrap().model = "second-model".into();
    other.worker_directory = Some(WorkerDirectory::for_task(&other).unwrap());
    plan.execution.push(other);
    let mut task = plan.workflow.tasks[0].clone();
    task.id = "other".into();
    plan.workflow.tasks.push(task);
    let count = host.projects["project"].history().len();
    assert!(host
        .propose("project", "overlap-proposal", plan)
        .unwrap_err()
        .contains("源码分工冲突"));
    assert_eq!(host.projects["project"].history().len(), count);
}
