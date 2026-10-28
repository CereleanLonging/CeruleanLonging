use super::*;
use cerulean_domain::historical_money::HistoricalAccountingState as State;

fn request(host: &mut RuntimeHost, command: Command) -> Response {
    host.handle(Request {
        id: "history-paging".into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    })
}
fn lookup(r: &HistoricalReview) -> Command {
    Command::ReadHistoricalConfirmation {
        project_id: r.project_id.clone(),
        scope: r.source.scope.clone(),
        review_digest: r.digest().unwrap(),
    }
}

#[test]
fn currency_historical_paging_all_original_records_control_availability_and_restart_cursor() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let mut original = Vec::new();
    for i in 0..21 {
        let scope = closed(&mut host, &format!("paged-{i:02}"));
        let r = host
            .review_historical_charge("project", &scope, 17, &"b".repeat(8000), &"e".repeat(8000))
            .unwrap();
        host.confirm_historical_charge(&r, &r.digest().unwrap())
            .unwrap();
        original.push(r);
    }
    let original_files = snapshot(&data);
    crate::management_execution::read_metrics::take();
    let check_start = std::time::Instant::now();
    assert_eq!(host.historical_confirmations(true).unwrap().len(), 21);
    let counts = crate::management_execution::read_metrics::take();
    assert_eq!(counts.full_reads, 2);
    assert!(original_files == snapshot(&data));
    println!("21 original confirmed records: historical_confirmations_us={} counts={counts:?}; original files unchanged", check_start.elapsed().as_micros());
    let snapshot_view = host.snapshot();
    assert!(snapshot_view.currency_accounted.is_none()); // never a misleading partial array
    assert!(snapshot_view.currency_history.is_none());
    let first = snapshot_view.historical_accounting.unwrap();
    let sources = snapshot_view.historical_sources.unwrap();
    assert_eq!(
        (first.total, first.entries.len(), first.next_offset),
        (21, 20, Some(20))
    );
    assert_eq!(sources.pending_total, Some(0));
    assert_eq!(snapshot_view.currency_budgets[0].charged_usd_nanos, 21 * 17);
    assert!(snapshot_view.currency_budgets[0]
        .available_usd_nanos
        .is_some());
    assert!(serde_json::to_vec(&host.snapshot()).unwrap().len() < 512 * 1024);
    let before = snapshot(&data);
    let second = request(
        &mut host,
        Command::ReadHistoricalAccounting {
            offset: 20,
            revision: Some(first.revision.clone()),
        },
    )
    .result
    .unwrap()
    .historical_accounting
    .unwrap();
    assert_eq!(second.entries.len(), 1);
    assert_eq!(second.next_offset, None);
    let all = first
        .entries
        .iter()
        .chain(second.entries.iter())
        .map(|r| r.scope.key().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(all.len(), 21);
    let detail = request(&mut host, lookup(&original[0]))
        .result
        .unwrap()
        .historical_confirmation
        .unwrap()
        .record
        .unwrap();
    assert_eq!(detail.review, original[0].view().unwrap());
    assert_eq!(detail.state, State::Accounted);
    assert!(snapshot(&data) == before);
    for (offset, revision) in [
        (20, None),
        (20, Some("a".repeat(64))),
        (22, Some(first.revision.clone())),
    ] {
        assert!(request(
            &mut host,
            Command::ReadHistoricalAccounting { offset, revision }
        )
        .error
        .is_some());
    }
    assert!(snapshot(&data) == before);
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        request(
            &mut host,
            Command::ReadHistoricalAccounting {
                offset: 20,
                revision: Some(first.revision)
            }
        )
        .result
        .unwrap()
        .historical_accounting
        .unwrap(),
        second
    );
    assert!(snapshot(&data) == before);
    closed(&mut host, "paged-last-unaccounted");
    assert!(host.snapshot().currency_budgets[0]
        .available_usd_nanos
        .is_none());
    assert_eq!(
        host.snapshot().historical_sources.unwrap().pending_total,
        Some(1)
    );
    assert!(request(
        &mut host,
        Command::ReadHistoricalSources {
            offset: 20,
            revision: Some(sources.revision)
        }
    )
    .error
    .is_some());
}

#[test]
fn currency_historical_paging_pending_lookup_is_readonly_and_changes_page_revision_after_publication(
) {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let scope = closed(&mut host, "pending-read");
    let r = review(&host, &scope);
    let missing = request(&mut host, lookup(&r))
        .result
        .unwrap()
        .historical_confirmation
        .unwrap();
    assert!(missing.record.is_none());
    commit(&mut host, &r, "user", context(&r)).unwrap();
    let before = snapshot(&data);
    let result = request(&mut host, lookup(&r)).result.unwrap();
    let old = result.historical_accounting.unwrap();
    assert_eq!(
        result
            .historical_confirmation
            .unwrap()
            .record
            .unwrap()
            .state,
        State::ConfirmedPending
    );
    assert_eq!(result.historical_sources.unwrap().pending_total, Some(1));
    assert!(result.currency_budgets[0].available_usd_nanos.is_none());
    assert!(snapshot(&data) == before); // read did not repair or append the pending amount
    host.reconcile_currency_account("project").unwrap();
    assert!(request(
        &mut host,
        Command::ReadHistoricalAccounting {
            offset: 0,
            revision: Some(old.revision)
        }
    )
    .error
    .is_some());
    assert_eq!(
        request(&mut host, lookup(&r))
            .result
            .unwrap()
            .historical_confirmation
            .unwrap()
            .record
            .unwrap()
            .state,
        State::Accounted
    );
}

#[test]
fn currency_historical_paging_lookup_rejects_changed_source_and_mismatched_identity_without_writes()
{
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let scope = closed(&mut host, "lookup-original");
    let r = review(&host, &scope);
    host.confirm_historical_charge(&r, &r.digest().unwrap())
        .unwrap();
    let before = snapshot(&data);
    for change in ["project", "scope", "digest"] {
        let mut command = lookup(&r);
        if let Command::ReadHistoricalConfirmation {
            project_id,
            scope,
            review_digest,
        } = &mut command
        {
            match change {
                "project" => *project_id = "other".into(),
                "scope" => {
                    *scope = HistoricalScope::Management {
                        operation_id: "lookup-original".into(),
                        original_project_id: None,
                    }
                }
                _ => *review_digest = "a".repeat(64),
            }
        }
        assert!(request(&mut host, command).error.is_some(), "{change}");
    }
    assert!(snapshot(&data) == before);
    let file = data.join("management-decisions/lookup-original.0.json");
    let mut bytes = fs::read(&file).unwrap();
    bytes.push(b' ');
    fs::write(&file, bytes).unwrap();
    let changed = snapshot(&data);
    assert!(request(&mut host, lookup(&r)).error.is_some());
    assert!(snapshot(&data) == changed);
}
