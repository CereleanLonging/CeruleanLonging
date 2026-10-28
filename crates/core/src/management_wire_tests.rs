use super::*;
use crate::reader_generation::tests::snapshot;
use cerulean_adapters::api_generation::ApiUsage;
use cerulean_domain::credentials::CredentialProvider;

fn guarded() -> (tempfile::TempDir, PathBuf, DecisionJournal) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    drop(crate::Store::open(&root).unwrap());
    crate::currency_guard::tests::promote_fixture(&root);
    let journal = DecisionJournal::open(&root).unwrap();
    (temp, root, journal)
}
fn fingerprint(i: &DecisionIntent) -> ManagementRequestFingerprint {
    PreparedDecision::new(i.purpose, &i.model, &i.state_digest, &i.context)
        .unwrap()
        .fingerprint()
        .clone()
}
fn result(i: &DecisionIntent) -> DecisionResult {
    let observation = tests::response(i);
    DecisionResult {
        transport: Some(DecisionTransport {
            request: fingerprint(i),
            usage: ApiUsage {
                input_tokens: observation.input_tokens,
                output_tokens: observation.output_tokens,
                cache_read_tokens: Some(0),
                cache_write_tokens: Some(0),
            },
        }),
        observation,
    }
}

#[test]
fn management_wire_journal_replays_original_identity_usage_and_legacy_bytes() {
    let (_temp, root, mut journal) = guarded();
    let i = tests::intent();
    journal
        .begin_with_request(i.clone(), Some(fingerprint(&i)))
        .unwrap();
    let before = snapshot(&root);
    assert!(journal
        .begin_with_request(i.clone(), Some(fingerprint(&i)))
        .is_err());
    assert_eq!(snapshot(&root), before);
    journal
        .finish_result(&i.operation_id, Ok(result(&i)))
        .unwrap();
    let saved = snapshot(&root);
    drop(journal);
    let reopened = DecisionJournal::open(&root).unwrap();
    let op = &reopened.operations[&i.operation_id];
    assert_eq!(op.request.as_ref(), Some(&fingerprint(&i)));
    assert_eq!(
        op.transport.as_ref().unwrap().usage.cache_write_tokens,
        Some(0)
    );
    assert_eq!(reopened.statuses()[0].state, DecisionState::Ready);
    assert_eq!(snapshot(&root), saved);
    let legacy = tempfile::tempdir().unwrap();
    let mut journal = DecisionJournal::open(legacy.path()).unwrap();
    journal.begin(i.clone()).unwrap();
    journal
        .finish(&i.operation_id, Ok(tests::response(&i)))
        .unwrap();
    let bytes = fs::read(journal.root.join("request-1.0.json")).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value["body"]["evidence"].get("request").is_none());
    let saved = snapshot(legacy.path());
    drop(journal);
    DecisionJournal::open(legacy.path()).unwrap();
    assert_eq!(snapshot(legacy.path()), saved);
}

#[test]
fn management_wire_guard_and_prepared_body_required_before_any_intent() {
    let (_temp, root, mut journal) = guarded();
    let i = tests::intent();
    let mut wrong = fingerprint(&i);
    wrong.body_sha256 = "e".repeat(64);
    let before = snapshot(&root);
    assert!(journal.begin_with_request(i.clone(), Some(wrong)).is_err());
    assert_eq!(snapshot(&root), before);
    journal
        .begin_with_request(i.clone(), Some(fingerprint(&i)))
        .unwrap();
    drop(journal);
    let marker = root.join(crate::currency_guard::MARKER);
    let held = root.parent().unwrap().join("saved-reader4");
    fs::rename(&marker, &held).unwrap();
    let before = snapshot(&root);
    assert!(DecisionJournal::open(&root).is_err());
    assert!(crate::reader_generation::reject_unprotected(&root).is_err());
    assert_eq!(snapshot(&root), before);
    fs::rename(held, marker).unwrap();
    assert!(DecisionJournal::open(&root).is_ok());
    let legacy = tempfile::tempdir().unwrap();
    let mut journal = DecisionJournal::open(legacy.path()).unwrap();
    let before = snapshot(legacy.path());
    assert!(journal
        .begin_with_request(i.clone(), Some(fingerprint(&i)))
        .is_err());
    assert_eq!(snapshot(legacy.path()), before);
}

#[test]
fn management_wire_wrong_or_missing_response_identity_becomes_unknown_without_resend() {
    for mode in 0..5 {
        let (_temp, root, mut journal) = guarded();
        let i = tests::intent();
        journal
            .begin_with_request(i.clone(), Some(fingerprint(&i)))
            .unwrap();
        let mut r = result(&i);
        match mode {
            0 => r.transport = None,
            1 => r.transport.as_mut().unwrap().request.body_sha256 = "b".repeat(64),
            2 => r.transport.as_mut().unwrap().request.purpose = Purpose::Advise,
            3 => r.transport.as_mut().unwrap().usage.input_tokens += 1,
            _ => r.transport.as_mut().unwrap().usage.cache_write_tokens = Some(u64::MAX),
        }
        journal.finish_result(&i.operation_id, Ok(r)).unwrap();
        assert_eq!(journal.statuses()[0].state, DecisionState::Unknown);
        let before = snapshot(&root);
        assert!(journal
            .finish_result(&i.operation_id, Ok(result(&i)))
            .is_err());
        drop(journal);
        let mut reopened = DecisionJournal::open(&root).unwrap();
        assert!(reopened
            .begin_with_request(i.clone(), Some(fingerprint(&i)))
            .is_err());
        assert_eq!(snapshot(&root), before);
    }
}

#[test]
fn management_wire_rehashed_but_mismatched_response_is_rejected_on_reopen() {
    let (_temp, root, mut journal) = guarded();
    let i = tests::intent();
    journal
        .begin_with_request(i.clone(), Some(fingerprint(&i)))
        .unwrap();
    journal
        .finish_result(&i.operation_id, Ok(result(&i)))
        .unwrap();
    let file = journal.root.join("request-1.1.json");
    drop(journal);
    let mut record: Record = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    if let Evidence::Response {
        transport: Some(t), ..
    } = &mut record.body.evidence
    {
        t.request.body_sha256 = "d".repeat(64);
    } else {
        panic!("missing original wire evidence");
    }
    record.hash = digest(&record.body).unwrap();
    fs::write(&file, serde_json::to_vec(&record).unwrap()).unwrap();
    let before = snapshot(&root);
    assert!(DecisionJournal::open(&root).is_err());
    assert_eq!(snapshot(&root), before);
}

#[test]
fn management_wire_incomplete_usage_survives_discard_without_becoming_zero_cost() {
    let (_temp, root, mut journal) = guarded();
    let i = tests::intent();
    journal
        .begin_with_request(i.clone(), Some(fingerprint(&i)))
        .unwrap();
    let mut r = result(&i);
    r.transport.as_mut().unwrap().usage.cache_write_tokens = None;
    journal.finish_result(&i.operation_id, Ok(r)).unwrap();
    let head = journal.response_head(&i.operation_id).unwrap().to_owned();
    journal
        .resolve(
            &i.operation_id,
            &head,
            UserResolution::Discard {
                reason: "Close local decision, not billing".into(),
            },
        )
        .unwrap();
    let before = snapshot(&root);
    drop(journal);
    let reopened = DecisionJournal::open(&root).unwrap();
    let op = &reopened.operations[&i.operation_id];
    assert!(op
        .transport
        .as_ref()
        .unwrap()
        .usage
        .billable_usage(CredentialProvider::OpenAi)
        .is_err());
    assert_eq!(reopened.response_head(&i.operation_id).unwrap(), head);
    assert_eq!(reopened.statuses()[0].state, DecisionState::Discarded);
    assert_eq!(snapshot(&root), before);
}
