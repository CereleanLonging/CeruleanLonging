use super::*;
use crate::{currency_approval::PlanningApproval, reader_generation::tests::snapshot};
use cerulean_domain::{
    api_worker::ApiTarget,
    currency_budget::*,
    project_setup::{ProjectSetupDraft, ProjectWorkerAdapter},
};
fn fixture(root: &Path) -> (RuntimeHost, ProjectSetupDraft) {
    let (host, original) = project_planning::tests::fixture(root);
    drop(host);
    crate::currency_guard::tests::promote_fixture(&root.join("data"));
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    let mut draft =
        api_project_tests::draft_for(original, ProjectWorkerAdapter::Openai, ApiTarget::OpenAi {});
    draft.currency_budget = Some(currency_product_tests::settings());
    host.configure_project(draft.clone()).unwrap();
    (host, draft)
}
fn permit(host: &mut RuntimeHost) -> String {
    let saved = host.project_setups["project"].digest.clone();
    host.authorize_planning_budget("project", &saved).unwrap();
    saved
}
fn prepared(host: &mut RuntimeHost) -> PlanningApproval {
    let saved = &host.project_setups["project"];
    let policy = saved
        .setup
        .policy
        .currency_budget
        .as_ref()
        .unwrap()
        .bind_planning("project", &saved.digest, &saved.setup.chief_model)
        .unwrap();
    let anchor = host
        .prepare_currency_policy("project", policy.clone())
        .unwrap();
    PlanningApproval { policy, anchor }
}
fn commit_only(
    host: &mut RuntimeHost,
    permission: PlanningApproval,
    principal: &str,
) -> Result<(), String> {
    let saved = host.project_setups["project"].digest.clone();
    host.projects
        .get_mut("project")
        .unwrap()
        .command(
            "planning-permission-interrupted",
            principal,
            WorkflowCommand::AuthorizePlanningBudget {
                setup_digest: saved,
            },
            CommandContext {
                planning_budget: Some(permission),
                approval_source: Some("authenticated local-user planning-budget approval".into()),
                ..Default::default()
            },
            false,
        )
        .map(|_| ())
}
fn reservation(book: &crate::currency_budget::BudgetLedger, id: &str) -> Reservation {
    Reservation {
        pricing: None,
        request_id: id.into(),
        operation_id: "controlled-planning".into(),
        role: ChargeRole::Chief,
        quote_digest: digest(&book.policy().quotes[0]).unwrap(),
        request_sha256: digest(&id).unwrap(),
        ceiling: TokenCeiling {
            total_input: 100,
            output: 10,
            cache_read_allowed: false,
            cache_write_allowed: false,
        },
        ceiling_evidence_sha256: "a".repeat(64),
    }
}
#[test]
fn planning_budget_permission_precedes_any_plan_is_user_only_and_never_sends() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, _) = fixture(temp.path());
    let data = temp.path().join("data");
    let before = snapshot(&data);
    assert!(host
        .authorize_planning_budget("project", &"0".repeat(64))
        .is_err());
    assert_eq!(snapshot(&data), before);
    let prepared = prepared(&mut host);
    let before = snapshot(&data);
    assert!(commit_only(&mut host, prepared, "chief").is_err());
    assert_eq!(snapshot(&data), before);
    let saved = permit(&mut host);
    assert!(host.projects["project"].revision().is_none());
    assert!(host.projects["project"].approval().is_none());
    assert!(host.management.statuses().is_empty());
    let view = &host.snapshot().currency_budgets[0];
    assert_eq!(view.planning_setup_digest.as_deref(), Some(saved.as_str()));
    assert!(view.approval_linked);
    let before = snapshot(&data);
    permit(&mut host);
    assert_eq!(snapshot(&data), before);
    assert!(host
        .request_chief_plan("project", &saved)
        .unwrap_err()
        .contains("金额预算尚不可执行"));
    assert_eq!(snapshot(&data), before);
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(snapshot(&data), before);
    assert!(host.management.statuses().is_empty());
}
#[test]
fn planning_budget_permission_cannot_spend_as_worker_deputy_or_another_chief_model() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, _) = fixture(temp.path());
    permit(&mut host);
    let book = host.currency_accounts.get_mut("project").unwrap();
    let before = book.head().to_owned();
    for role in [ChargeRole::Worker, ChargeRole::Deputy] {
        let mut r = reservation(book, "wrong-role");
        r.role = role;
        assert!(book.reserve(r).is_err());
    }
    let mut wrong_model = reservation(book, "wrong-model");
    wrong_model.quote_digest = digest(&book.policy().quotes[1]).unwrap();
    assert!(book.reserve(wrong_model).is_err());
    assert_eq!(book.head(), before);
    let r = reservation(book, "chief-planning");
    assert!(book.reserve(r).unwrap() > 0);
}
#[test]
fn planning_budget_charges_and_unknowns_flow_into_execution_approval_in_same_account() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, _) = fixture(temp.path());
    permit(&mut host);
    let book = host.currency_accounts.get_mut("project").unwrap();
    let r = reservation(book, "planning-complete");
    book.reserve(r.clone()).unwrap();
    book.settle(Settlement {
        actual_service: None,
        request_id: r.request_id,
        response_id: "planning-response".into(),
        request_sha256: r.request_sha256,
        actual_model: book.policy().planning.as_ref().unwrap().chief_model.clone(),
        usage: Usage {
            input: 12,
            output: 3,
            cache_read: 0,
            cache_write: 0,
        },
        evidence_sha256: "b".repeat(64),
    })
    .unwrap();
    let unknown = reservation(book, "planning-unknown");
    book.reserve(unknown.clone()).unwrap();
    book.unknown(&unknown.request_id, &"c".repeat(64)).unwrap();
    let before = book.state().clone();
    let mut proposal = project_planning::tests::proposal("tests");
    if let management::ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].worker_id = ProjectWorkerAdapter::Openai.worker_id();
    }
    project_planning::tests::complete(&mut host, proposal);
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &revision).unwrap();
    let book = &host.currency_accounts["project"];
    assert!(book.policy().planning.is_none());
    assert_eq!(book.state().bookings, before.bookings);
    assert_eq!(book.state().charged_usd_nanos, before.charged_usd_nanos);
    assert_eq!(book.state().reserved_usd_nanos, before.reserved_usd_nanos);
    assert_eq!(book.state().approval_links.len(), 2);
    let data = temp.path().join("data");
    let before = snapshot(&data);
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(snapshot(&data), before);
    assert_eq!(host.snapshot().currency_budgets[0].unknown_requests, 1);
    assert_eq!(
        fs::read_dir(data.join("currency-budgets")).unwrap().count(),
        1
    );
}
#[test]
fn planning_budget_reopen_uses_original_permission_and_never_promotes_preparation() {
    for commit in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, _) = fixture(temp.path());
        let p = prepared(&mut host);
        if commit {
            commit_only(&mut host, p, "user").unwrap();
        }
        let history = host.projects["project"].history().to_vec();
        drop(host);
        let data = temp.path().join("data");
        let host = RuntimeHost::open(&data).unwrap();
        assert_eq!(host.projects["project"].history(), history);
        assert!(host.projects["project"].revision().is_none());
        assert_eq!(host.snapshot().currency_budgets[0].approval_linked, commit);
        assert_eq!(
            host.currency_accounts["project"]
                .state()
                .approval_links
                .len(),
            usize::from(commit)
        );
        let before = snapshot(&data);
        drop(host);
        drop(RuntimeHost::open(&data).unwrap());
        assert_eq!(snapshot(&data), before);
    }
}
#[test]
fn planning_budget_changed_setup_requires_new_permission_without_erasing_reservations() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, mut draft) = fixture(temp.path());
    let original = permit(&mut host);
    let book = host.currency_accounts.get_mut("project").unwrap();
    let r = reservation(book, "unanswered-plan");
    book.reserve(r.clone()).unwrap();
    book.unknown(&r.request_id, &"a".repeat(64)).unwrap();
    let held = book.state().reserved_usd_nanos;
    draft.requirement.push_str(" with changed scope");
    host.configure_project(draft.clone()).unwrap();
    assert!(!host.snapshot().currency_budgets[0].approval_linked);
    let before = snapshot(&temp.path().join("data"));
    assert!(host
        .authorize_planning_budget("project", &original)
        .is_err());
    assert_eq!(snapshot(&temp.path().join("data")), before);
    let changed = permit(&mut host);
    assert_ne!(changed, original);
    assert_eq!(
        host.currency_accounts["project"].state().reserved_usd_nanos,
        held
    );
    assert_eq!(
        host.currency_accounts["project"]
            .state()
            .approval_links
            .len(),
        2
    );
    draft.currency_budget.as_mut().unwrap().limit_usd_nanos = 1;
    host.configure_project(draft).unwrap();
    let latest = host.project_setups["project"].digest.clone();
    let before = snapshot(&temp.path().join("data"));
    assert!(host.authorize_planning_budget("project", &latest).is_err());
    assert_eq!(snapshot(&temp.path().join("data")), before);
    drop(host);
    let host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert_eq!(
        host.currency_accounts["project"].state().reserved_usd_nanos,
        held
    );
    assert!(!host.snapshot().currency_budgets[0].approval_linked);
}
#[test]
fn planning_budget_rejects_missing_account_and_missing_original_setup_without_reconstruction() {
    for account in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, _) = fixture(temp.path());
        permit(&mut host);
        drop(host);
        let data = temp.path().join("data");
        let path = if account {
            data.join("currency-budgets/project")
        } else {
            data.join("project-setups/project.00000001.json")
        };
        let saved = temp.path().join("preserved");
        fs::rename(&path, &saved).unwrap();
        let before = snapshot(&data);
        assert!(RuntimeHost::open(&data).is_err());
        assert_eq!(snapshot(&data), before);
        assert!(!path.exists());
        fs::rename(saved, path).unwrap();
        drop(RuntimeHost::open(&data).unwrap());
    }
}
