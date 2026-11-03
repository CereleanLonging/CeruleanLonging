use super::*;
use crate::reader_generation::tests::snapshot;
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
    propose(&mut host, &draft, "ui-plan-currency-1");
    (host, draft)
}
fn propose(host: &mut RuntimeHost, draft: &ProjectSetupDraft, id: &str) {
    host.configure_project(draft.clone()).unwrap();
    let mut proposal = project_planning::tests::proposal("tests");
    if let management::ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].worker_id = ProjectWorkerAdapter::Openai.worker_id();
    }
    project_planning::tests::complete_at(host, proposal, id);
}
fn approve(host: &mut RuntimeHost) -> String {
    let hash = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &hash).unwrap();
    hash
}
// Stop exactly between the two production records: the normal user approval
// is committed through DurableWorkflow, but its account link has not been made.
fn commit_approval_only(host: &mut RuntimeHost) {
    let anchor = host.prepare_currency_approval("project").unwrap();
    let store = host.projects.get_mut("project").unwrap();
    let revision = store.revision().unwrap();
    let approval = Approval {
        worker_account_setup: None,
        plan_id: revision.workflow.id.clone(),
        revision: revision.workflow.version,
        policy_version: revision.policy.version,
        revision_digest: revision.approval_digest().unwrap(),
        user_id: "user".into(),
        approved_at: chrono::Utc::now().to_rfc3339(),
    };
    store
        .command(
            "currency-interrupted-approval",
            "user",
            WorkflowCommand::Approve {
                digest: store.state().digest.clone().unwrap(),
            },
            CommandContext {
                currency_approval: anchor,
                approval_source: Some("authenticated local-user service request".into()),
                execution: Some(ExecutionEvidence::Approval { approval }),
                ..Default::default()
            },
            false,
        )
        .unwrap();
}
fn request_revision(host: &mut RuntimeHost) {
    let review = host
        .project_revision_candidates()
        .into_iter()
        .find(|v| v.project_id == "project")
        .unwrap();
    host.request_project_revision(
        "project",
        &review.evidence_digest,
        "Explicitly review revised currency policy",
    )
    .unwrap();
}
fn reserve(host: &mut RuntimeHost, id: &str) -> Reservation {
    let book = host.currency_accounts.get_mut("project").unwrap();
    let r = Reservation {
        pricing: None,
        request_id: id.into(),
        operation_id: "controlled-local-operation".into(),
        role: ChargeRole::Worker,
        quote_digest: digest(&book.policy().quotes[2]).unwrap(),
        request_sha256: digest(&id).unwrap(),
        ceiling: TokenCeiling {
            total_input: 100,
            output: 20,
            cache_read_allowed: false,
            cache_write_allowed: false,
        },
        ceiling_evidence_sha256: "a".repeat(64),
    };
    book.reserve(r.clone()).unwrap();
    r
}

#[test]
fn currency_host_approval_binds_original_event_and_duplicate_or_reopen_is_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, _) = fixture(temp.path());
    assert!(host.currency_accounts.is_empty());
    let approved = approve(&mut host);
    let book = &host.currency_accounts["project"];
    assert_eq!(book.policy().revision_digest, approved);
    let entry = host.projects["project"].history().last().unwrap();
    assert_eq!(
        book.state().approval_links[&book.state().current_policy_digest].approval_evidence_sha256,
        entry.hash
    );
    let data = temp.path().join("data");
    let before = snapshot(&data);
    approve(&mut host);
    assert_eq!(snapshot(&data), before);
    assert!(host.snapshot().currency_budgets[0].approval_linked);
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(snapshot(&data), before);
    assert_eq!(host.currency_accounts.len(), 1);
    assert_eq!(host.management.statuses().len(), 1);
}

#[test]
fn currency_host_recovers_only_original_approval_link_and_never_approves_preparation() {
    for approved in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, _) = fixture(temp.path());
        if approved {
            commit_approval_only(&mut host);
        } else {
            host.prepare_currency_approval("project").unwrap();
        }
        let old_workflow = host.projects["project"].history().to_vec();
        let old_head = host.currency_accounts["project"].head().to_owned();
        drop(host);
        let data = temp.path().join("data");
        let mut host = RuntimeHost::open(&data).unwrap();
        assert_eq!(host.projects["project"].history(), old_workflow);
        assert_eq!(host.projects["project"].approval().is_some(), approved);
        assert_eq!(
            host.currency_accounts["project"]
                .state()
                .approval_links
                .len(),
            usize::from(approved)
        );
        assert_eq!(
            host.currency_accounts["project"].head() == old_head,
            !approved
        );
        assert_eq!(
            host.snapshot().currency_budgets[0].approval_linked,
            approved
        );
        if !approved {
            approve(&mut host);
        }
        let before = snapshot(&data);
        drop(host);
        let host = RuntimeHost::open(&data).unwrap();
        assert_eq!(snapshot(&data), before);
        assert_eq!(host.management.statuses().len(), 1);
    }
}

#[test]
fn currency_host_missing_or_relocated_account_never_creates_a_replacement() {
    for approved in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, _) = fixture(temp.path());
        if approved {
            approve(&mut host);
        } else {
            commit_approval_only(&mut host);
        }
        drop(host);
        let data = temp.path().join("data");
        let account = data.join("currency-budgets/project");
        let preserved = temp.path().join("preserved-original-account");
        fs::rename(&account, &preserved).unwrap();
        let before = snapshot(&data);
        assert!(RuntimeHost::open(&data).err().unwrap().contains("missing"));
        assert!(!account.exists());
        assert_eq!(snapshot(&data), before);
        fs::rename(&preserved, &account).unwrap();
        drop(RuntimeHost::open(&data).unwrap());
    }
}

#[test]
fn currency_host_new_plan_retains_costs_unknowns_and_reconciles_interrupted_authorization() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, mut draft) = fixture(temp.path());
    approve(&mut host);
    let r = reserve(&mut host, "settled-request");
    let quote = host.currency_accounts["project"].policy().quotes[2].clone();
    host.currency_accounts
        .get_mut("project")
        .unwrap()
        .settle(Settlement {
            actual_service: None,
            request_id: r.request_id,
            response_id: "controlled-response".into(),
            request_sha256: r.request_sha256,
            actual_model: quote.reported_models[0].clone(),
            usage: Usage {
                input: 10,
                output: 3,
                cache_read: 0,
                cache_write: 0,
            },
            evidence_sha256: "c".repeat(64),
        })
        .unwrap();
    let r = reserve(&mut host, "unknown-request");
    host.currency_accounts
        .get_mut("project")
        .unwrap()
        .unknown(&r.request_id, &"d".repeat(64))
        .unwrap();
    let old = host.currency_accounts["project"].state().clone();
    request_revision(&mut host);
    draft.currency_budget.as_mut().unwrap().limit_usd_nanos += 1_000_000_000;
    propose(&mut host, &draft, "ui-plan-currency-2");
    commit_approval_only(&mut host);
    let workflow = host.projects["project"].history().to_vec();
    drop(host);
    let host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    let now = host.currency_accounts["project"].state();
    assert_eq!(host.projects["project"].history(), workflow);
    assert_eq!(now.charged_usd_nanos, old.charged_usd_nanos);
    assert_eq!(now.reserved_usd_nanos, old.reserved_usd_nanos);
    assert_eq!(now.bookings, old.bookings);
    assert_eq!(now.approval_links.len(), 2);
    assert_eq!(host.snapshot().currency_budgets[0].unknown_requests, 1);
    assert_eq!(
        fs::read_dir(temp.path().join("data/currency-budgets"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn currency_host_rejects_lower_limit_and_budget_removal_before_user_approval() {
    for remove in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = fixture(temp.path());
        approve(&mut host);
        reserve(&mut host, "held-request");
        request_revision(&mut host);
        if remove {
            draft.currency_budget = None;
        } else {
            draft.currency_budget.as_mut().unwrap().limit_usd_nanos = 1;
        }
        propose(&mut host, &draft, "ui-plan-currency-2");
        let before = snapshot(&temp.path().join("data"));
        let hash = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        assert!(host.approve("project", &hash).is_err());
        assert!(host.projects["project"].approval().is_none());
        assert_eq!(snapshot(&temp.path().join("data")), before);
        assert!(host
            .require_currency_execution_available("project")
            .is_err());
    }
}

#[test]
fn currency_host_failed_link_publication_preserves_approval_and_prevents_availability() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, _) = fixture(temp.path());
    host.prepare_currency_approval("project").unwrap();
    let pending = temp
        .path()
        .join("data/currency-budgets/project/00000001.pending");
    fs::write(&pending, b"preserved-incomplete-record").unwrap();
    let hash = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    assert!(host.approve("project", &hash).is_err());
    assert!(host.projects["project"].approval().is_some());
    let view = &host.snapshot().currency_budgets[0];
    assert!(!view.healthy);
    assert!(!view.approval_linked);
    assert_eq!(view.available_usd_nanos, None);
    let before = snapshot(&temp.path().join("data"));
    assert!(host.approve("project", &hash).is_err());
    assert_eq!(snapshot(&temp.path().join("data")), before);
    drop(host);
    assert!(RuntimeHost::open(&temp.path().join("data")).is_err());
    assert_eq!(snapshot(&temp.path().join("data")), before);
}

#[test]
fn currency_host_refuses_account_authorization_without_matching_original_user_event() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, mut draft) = fixture(temp.path());
    approve(&mut host);
    request_revision(&mut host);
    draft.currency_budget.as_mut().unwrap().limit_usd_nanos += 1;
    propose(&mut host, &draft, "ui-plan-currency-2");
    let revision = host.projects["project"].revision().unwrap();
    let policy = revision
        .policy
        .currency_budget
        .as_ref()
        .unwrap()
        .bind("project", &revision.approval_digest().unwrap())
        .unwrap();
    let book = host.currency_accounts.get_mut("project").unwrap();
    book.authorize(crate::currency_budget::PolicyAuthorization {
        previous_policy_digest: digest(book.policy()).unwrap(),
        previous_ledger_head: book.head().into(),
        policy,
        approval_evidence_sha256: "f".repeat(64),
    })
    .unwrap();
    drop(host);
    let before = snapshot(&temp.path().join("data"));
    assert!(RuntimeHost::open(&temp.path().join("data"))
        .err()
        .unwrap()
        .contains("original user approval"));
    assert_eq!(snapshot(&temp.path().join("data")), before);
}
