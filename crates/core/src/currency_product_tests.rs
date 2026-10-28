use super::*;
use cerulean_domain::{
    api_worker::ApiTarget, currency_budget::*, management::ManagementDecision,
    project_setup::ProjectWorkerAdapter,
};

pub(super) fn settings() -> BudgetSettings {
    BudgetSettings {
        scoped_quotes: None,
        schema: 1,
        limit_usd_nanos: 12_345_678_901,
        quotes: ["fixture-chief", "fixture-deputy", "vendor/fixture-model"]
            .into_iter()
            .map(|model| PriceQuote {
                count_price: None,
                schema: 1,
                target: ApiTarget::OpenAi {},
                requested_model: model.into(),
                reported_models: vec![model.into()],
                rates: Rates {
                    input: 1_234_567,
                    output: 2_500_000,
                    cache_read: None,
                    cache_write: Some(0),
                },
                valid_from: "2026-10-01T00:00:00Z".into(),
                valid_until: "2026-11-01T00:00:00Z".into(),
                source: "Controlled fixture, not supplier pricing".into(),
            })
            .collect(),
    }
}

#[test]
fn currency_product_saves_approves_reopens_without_enabling_requests() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, draft) = project_planning::tests::fixture(temp.path());
    let mut draft =
        api_project_tests::draft_for(draft, ProjectWorkerAdapter::Openai, ApiTarget::OpenAi {});
    host.configure_project(draft.clone()).unwrap();
    let legacy = serde_json::to_value(&host.project_setups["project"]).unwrap();
    assert!(legacy.pointer("/setup/policy/currencyBudget").is_none());
    let data = temp.path().join("data");
    draft.currency_budget = Some(settings());
    let before = crate::reader_generation::tests::snapshot(&data);
    assert!(host
        .configure_project(draft.clone())
        .unwrap_err()
        .contains("reader4"));
    assert_eq!(crate::reader_generation::tests::snapshot(&data), before);
    drop(host);
    crate::currency_guard::tests::promote_fixture(&data);
    let mut host = RuntimeHost::open(&data).unwrap();
    host.configure_project(draft.clone()).unwrap();
    let saved = host.project_setups["project"].clone();
    let before = crate::reader_generation::tests::snapshot(&data);
    assert!(host
        .request_chief_plan("project", &saved.digest)
        .unwrap_err()
        .contains("金额预算尚不可执行"));
    assert_eq!(crate::reader_generation::tests::snapshot(&data), before);
    let mut proposal = project_planning::tests::proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].worker_id = ProjectWorkerAdapter::Openai.worker_id();
    }
    project_planning::tests::complete(&mut host, proposal);
    let revision = host.projects["project"].revision().unwrap().clone();
    assert_eq!(revision.policy.currency_budget, draft.currency_budget);
    let approved = revision.approval_digest().unwrap();
    host.approve("project", &approved).unwrap();
    let policy = settings().bind("project", &approved).unwrap();
    assert_eq!(policy.revision_digest, approved);
    for mode in 0..6 {
        let mut changed = revision.clone();
        let budget = changed.policy.currency_budget.as_mut().unwrap();
        match mode {
            0 => budget.limit_usd_nanos += 1,
            1 => budget.quotes[0].rates.input += 1,
            2 => budget.quotes[0].rates.cache_read = Some(0),
            3 => budget.quotes[0]
                .reported_models
                .push("another-model".into()),
            4 => budget.quotes[0].valid_until = "2026-12-01T00:00:00Z".into(),
            _ => budget.quotes[0].source.push_str(" revised"),
        }
        assert_ne!(changed.approval_digest().unwrap(), approved);
    }
    let before = crate::reader_generation::tests::snapshot(&data);
    assert!(host
        .start_project_run("project", &approved, &saved.digest)
        .unwrap_err()
        .contains("金额预算尚不可执行"));
    assert!(host.runs.all().is_empty());
    assert_eq!(crate::reader_generation::tests::snapshot(&data), before);
    // Existing pending work already prevents changing the saved setup.
    draft.currency_budget = None;
    assert!(host
        .configure_project(draft)
        .unwrap_err()
        .contains("已有批准"));
    // The execution gate independently checks the approved plan without a setup.
    let setup = host.project_setups.remove("project").unwrap();
    assert!(host
        .require_currency_execution_available("project")
        .is_err());
    host.project_setups.insert("project".into(), setup);
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        host.projects["project"].approval().unwrap().revision_digest,
        approved
    );
    assert_eq!(
        host.projects["project"]
            .revision()
            .unwrap()
            .policy
            .currency_budget,
        Some(settings())
    );
}

#[test]
fn currency_product_rejects_missing_quotes_unsupported_workers_and_invalid_amounts() {
    let temp = tempfile::tempdir().unwrap();
    let (host, original) = project_planning::tests::fixture(temp.path());
    let mut draft = api_project_tests::draft_for(
        original.clone(),
        ProjectWorkerAdapter::Openai,
        ApiTarget::OpenAi {},
    );
    draft.currency_budget = Some(settings());
    assert!(draft.validate().is_ok());
    for mode in 0..5 {
        let mut invalid = draft.clone();
        let budget = invalid.currency_budget.as_mut().unwrap();
        match mode {
            0 => {
                budget.quotes.remove(0);
            }
            1 => budget.limit_usd_nanos = MAX_USD_NANOS + 1,
            2 => budget.quotes[0].rates.input = u64::MAX,
            3 => budget.quotes[0].valid_until = budget.quotes[0].valid_from.clone(),
            _ => budget.quotes[0].reported_models.clear(),
        }
        assert!(invalid.validate().is_err());
    }
    let mut unsupported = original;
    unsupported.currency_budget = Some(settings());
    assert!(unsupported.validate().is_err());
    assert!(host.management.statuses().is_empty());
}

#[test]
fn currency_product_scoped_quotes_survive_user_approvals_and_reopen_without_send() {
    use cerulean_domain::currency_budget::scoped::{
        BillingBasis, ContextMetric, InputRange, PriceBand, RequestedService, ScopedPriceQuote,
    };
    let temp = tempfile::tempdir().unwrap();
    let (mut host, draft) = project_planning::tests::fixture(temp.path());
    let mut draft =
        api_project_tests::draft_for(draft, ProjectWorkerAdapter::Openai, ApiTarget::OpenAi {});
    host.configure_project(draft.clone()).unwrap();
    let legacy_setup = host.project_setups["project"].clone();
    let data = temp.path().join("data");
    let legacy_path = data.join("project-setups/project.00000001.json");
    let legacy_bytes = fs::read(&legacy_path).unwrap();
    let original = settings();
    let scoped = BudgetSettings {
        schema: 2,
        limit_usd_nanos: original.limit_usd_nanos,
        quotes: vec![],
        scoped_quotes: Some(
            original
                .quotes
                .into_iter()
                .enumerate()
                .map(|(index, quote)| {
                    let mut bands = vec![];
                    for service in ["default", "priority"] {
                        for (from, through) in [(0, 100), (101, MAX_TOKENS)] {
                            bands.push(PriceBand {
                                actual_model: quote.reported_models[0].clone(),
                                actual_service: service.into(),
                                input: InputRange { from, through },
                                rates: Rates {
                                    input: quote.rates.input + from,
                                    output: quote.rates.output + index as u64,
                                    ..quote.rates.clone()
                                },
                            });
                        }
                    }
                    ScopedPriceQuote {
                        schema: 1,
                        count_price: (index < 2).then(|| ManagementCountPrice {
                            schema: 1,
                            maximum_usd_nanos: index as u64,
                            maximum_body_bytes: 4096,
                            source: "Synthetic explicitly reviewed count liability".into(),
                        }),
                        target: quote.target,
                        requested_model: quote.requested_model,
                        requested_service: match index {
                            0 => RequestedService::Explicit {
                                tier: "default".into(),
                            },
                            1 => RequestedService::Explicit {
                                tier: "auto".into(),
                            },
                            _ => RequestedService::Omitted,
                        },
                        reported_models: quote.reported_models,
                        reported_services: vec!["default".into(), "priority".into()],
                        billing_basis: BillingBasis::PerRequest,
                        context_metric: ContextMetric::TotalInputIncludingCache,
                        bands,
                        valid_from: quote.valid_from,
                        valid_until: quote.valid_until,
                        source: quote.source,
                    }
                })
                .collect(),
        ),
    };
    scoped.validate().unwrap();
    draft.currency_budget = Some(scoped.clone());
    let before = crate::reader_generation::tests::snapshot(&data);
    assert!(host
        .configure_project(draft.clone())
        .unwrap_err()
        .contains("reader8"));
    assert_eq!(crate::reader_generation::tests::snapshot(&data), before);
    assert_eq!(
        serde_json::to_value(&host.project_setups["project"]).unwrap(),
        serde_json::to_value(&legacy_setup).unwrap()
    );
    drop(host);
    crate::currency_guard::tests::promote_fixture(&data);
    crate::currency_execution_guard::tests::promote_fixture(&data);
    crate::currency_count_guard::tests::promote_fixture(&data);
    crate::currency_count_resolution_guard::tests::promote_fixture(&data);
    crate::currency_scoped_guard::tests::promote_fixture(&data);
    let mut host = RuntimeHost::open(&data).unwrap();
    host.configure_project(draft).unwrap();
    let saved = host.project_setups["project"].clone();
    assert_eq!(saved.setup.policy.currency_budget, Some(scoped.clone()));
    assert_eq!(fs::read(&legacy_path).unwrap(), legacy_bytes);

    host.authorize_planning_budget("project", &saved.digest)
        .unwrap();
    let planning_policy = scoped
        .bind_planning("project", &saved.digest, &saved.setup.chief_model)
        .unwrap();
    assert_eq!(host.currency_accounts["project"].policy(), &planning_policy);
    let before = crate::reader_generation::tests::snapshot(&data);
    assert!(host
        .request_chief_plan("project", &saved.digest)
        .unwrap_err()
        .contains("金额预算尚不可执行"));
    assert_eq!(crate::reader_generation::tests::snapshot(&data), before);
    assert!(host.management.statuses().is_empty());

    // Reuse the established synthetic completed decision; this creates no
    // network send and does not exercise the still-disabled money transport.
    let mut proposal = project_planning::tests::proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].worker_id = ProjectWorkerAdapter::Openai.worker_id();
    }
    project_planning::tests::complete(&mut host, proposal);
    let revision = host.projects["project"].revision().unwrap().clone();
    assert_eq!(revision.policy.currency_budget, Some(scoped.clone()));
    let approved = revision.approval_digest().unwrap();
    for mutation in 0..6 {
        let mut changed = revision.clone();
        let quote = &mut changed
            .policy
            .currency_budget
            .as_mut()
            .unwrap()
            .scoped_quotes
            .as_mut()
            .unwrap()[0];
        match mutation {
            0 => quote.requested_service = RequestedService::Omitted,
            1 => quote.bands[0].input.through -= 1,
            2 => quote.bands[0].rates.input += 1,
            3 => quote.source.push_str(" revised"),
            4 => quote.count_price.as_mut().unwrap().maximum_usd_nanos += 1,
            _ => quote.count_price.as_mut().unwrap().maximum_body_bytes += 1,
        }
        assert_ne!(changed.approval_digest().unwrap(), approved);
    }
    host.approve("project", &approved).unwrap();
    let execution_policy = scoped.bind("project", &approved).unwrap();
    let book = &host.currency_accounts["project"];
    assert_eq!(book.policy(), &execution_policy);
    assert_eq!(
        book.state()
            .policies
            .get(&digest(&planning_policy).unwrap()),
        Some(&planning_policy)
    );
    assert_eq!(book.state().approval_links.len(), 2);
    assert!(book.state().bookings.is_empty());
    assert!(book.state().management_counts.is_empty());
    let account_state = book.state().clone();
    let workflow_history = serde_json::to_vec(host.projects["project"].history()).unwrap();
    let before = crate::reader_generation::tests::snapshot(&data);
    assert!(host
        .start_project_run("project", &approved, &saved.digest)
        .unwrap_err()
        .contains("金额预算尚不可执行"));
    assert_eq!(crate::reader_generation::tests::snapshot(&data), before);
    assert!(host.codex_jobs.is_empty());
    assert!(host.jobs.is_empty());
    assert!(host.runs.all().is_empty());
    drop(host);

    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        serde_json::to_value(&host.project_setups["project"]).unwrap(),
        serde_json::to_value(&saved).unwrap()
    );
    assert_eq!(
        host.projects["project"].approval().unwrap().revision_digest,
        approved
    );
    assert_eq!(
        host.projects["project"]
            .revision()
            .unwrap()
            .policy
            .currency_budget,
        Some(scoped)
    );
    assert_eq!(
        host.currency_accounts["project"].policy(),
        &execution_policy
    );
    assert_eq!(host.currency_accounts["project"].state(), &account_state);
    assert_eq!(
        serde_json::to_vec(host.projects["project"].history()).unwrap(),
        workflow_history
    );
    assert_eq!(fs::read(&legacy_path).unwrap(), legacy_bytes);
    assert!(host.codex_jobs.is_empty());
    assert!(host.jobs.is_empty());
    assert!(host.runs.all().is_empty());
}
