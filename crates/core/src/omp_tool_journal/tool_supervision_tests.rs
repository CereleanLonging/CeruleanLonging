use super::*;
use cerulean_domain::supervision::SupervisionPolicy;

fn policy() -> SupervisionPolicy {
    SupervisionPolicy {
        schema_version: 1,
        repeated_failure_threshold: 2,
        max_advice_per_attempt: 2,
        delivery_enabled: false,
    }
}
fn start(
    journal: &mut OmpToolJournal,
    scope: &DispatchIntent,
    id: &str,
    tool: &str,
    argument: char,
) {
    journal
        .record(
            scope,
            &HostEvent::Dispatch {
                id: id.into(),
                tool_call_id: format!("call-{id}"),
                tool_name: tool.into(),
                arguments_sha256: argument.to_string().repeat(64),
            },
        )
        .unwrap();
}
fn finish(journal: &mut OmpToolJournal, scope: &DispatchIntent, id: &str, failed: bool) {
    journal
        .record(
            scope,
            &HostEvent::Result {
                id: id.into(),
                result_sha256: "c".repeat(64),
                is_error: failed,
            },
        )
        .unwrap();
}
fn files(path: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(path)
        .unwrap()
        .filter(|entry| entry.as_ref().unwrap().file_name() != "writer.lock")
        .map(|e| {
            let p = e.unwrap().path();
            let bytes = fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect()
}

#[test]
fn tool_supervision_uses_original_completion_order_and_reopens_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tools");
    let scope = intent();
    let mut journal = OmpToolJournal::create(&path, &scope).unwrap();
    for id in ["10", "2", "1"] {
        start(&mut journal, &scope, id, "cerulean_read_file", 'b');
    }
    finish(&mut journal, &scope, "10", true);
    finish(&mut journal, &scope, "2", true);
    assert!(journal.supervision_incidents(&policy()).is_err());
    assert!(journal.checkpoint().is_err());
    finish(&mut journal, &scope, "1", false);
    let events = journal.supervision_incidents(&policy()).unwrap();
    assert_eq!(
        journal
            .checkpoint()
            .unwrap()
            .supervision_incidents(&policy())
            .unwrap(),
        events
    );
    assert_eq!(events.len(), 1);
    assert_eq!(
        (events[0].first_sequence, events[0].trigger_sequence),
        (4, 5)
    );
    assert_eq!(
        journal.observation().calls["1"].result_is_error,
        Some(false)
    );
    assert_eq!(
        journal.observation().calls["10"].result_is_error,
        Some(true)
    );
    let before = files(&path);
    let head = journal.head().to_owned();
    for _ in 0..3 {
        assert_eq!(journal.supervision_incidents(&policy()).unwrap(), events);
    }
    assert_eq!(files(&path), before);
    drop(journal);
    let mut original = OmpToolJournal::open(&path, &scope).unwrap();
    assert_eq!(original.head(), head);
    assert_eq!(original.supervision_incidents(&policy()).unwrap(), events);
    assert!(original.record(&scope, &dispatch()).is_err());
    assert_eq!(files(&path), before);
}

#[test]
fn tool_supervision_success_cancellation_and_changed_action_reset_streaks() {
    let temp = tempfile::tempdir().unwrap();
    let scope = intent();
    let mut journal = OmpToolJournal::create(&temp.path().join("tools"), &scope).unwrap();
    // The same result text hash must not hide the original is_error flag.
    let steps = [
        (true, false, "cerulean_read_file", 'a'),
        (false, false, "cerulean_read_file", 'a'),
        (true, false, "cerulean_read_file", 'a'),
        (true, true, "cerulean_read_file", 'a'),
        (true, false, "cerulean_read_file", 'a'),
        (true, false, "cerulean_write_file", 'a'),
        (true, false, "cerulean_write_file", 'b'),
        (true, false, "cerulean_write_file", 'b'),
        (true, false, "cerulean_write_file", 'b'),
    ];
    for (i, (failed, cancelled, tool, argument)) in steps.into_iter().enumerate() {
        let id = (i + 1).to_string();
        start(&mut journal, &scope, &id, tool, argument);
        if cancelled {
            journal
                .record(&scope, &HostEvent::Cancel { id: id.clone() })
                .unwrap();
        }
        finish(&mut journal, &scope, &id, failed);
        assert_eq!(
            journal.supervision_incidents(&policy()).unwrap().len(),
            usize::from(i >= 7)
        );
    }
    let incident = journal.supervision_incidents(&policy()).unwrap()[0].clone();
    start(&mut journal, &scope, "later", "cerulean_read_file", 'c');
    assert!(journal.supervision_incidents(&policy()).is_err());
    finish(&mut journal, &scope, "later", false);
    assert_eq!(
        journal.supervision_incidents(&policy()).unwrap(),
        vec![incident]
    );
}

#[test]
fn tool_supervision_scope_and_incomplete_publication_cannot_supply_an_incident() {
    let temp = tempfile::tempdir().unwrap();
    let mut ids = Vec::new();
    for fence in [1, 2] {
        let mut scope = intent();
        scope.fence = fence;
        let mut journal =
            OmpToolJournal::create(&temp.path().join(fence.to_string()), &scope).unwrap();
        for id in ["1", "2"] {
            start(&mut journal, &scope, id, "cerulean_read_file", 'b');
            finish(&mut journal, &scope, id, true);
        }
        ids.push(
            journal.supervision_incidents(&policy()).unwrap()[0]
                .id
                .clone(),
        );
        assert!(journal
            .publish(
                &HostEvent::Dispatch {
                    id: "3".into(),
                    tool_call_id: "call-3".into(),
                    tool_name: "cerulean_read_file".into(),
                    arguments_sha256: "b".repeat(64),
                },
                |_| Err("controlled publication stop".into())
            )
            .is_err());
        assert!(journal.supervision_incidents(&policy()).is_err());
    }
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn historical_prefix_uses_original_parser_and_never_reconstructs_a_live_owner() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tools");
    let scope = intent();
    let mut owner = OmpToolJournal::create(&path, &scope).unwrap();
    start(&mut owner, &scope, "1", "cerulean_read_file", 'a');
    let unresolved = owner.head().to_owned();
    finish(&mut owner, &scope, "1", true);
    let first = owner.checkpoint().unwrap().evidence().clone();
    assert!(first.verify_original(&path, &scope).is_err());
    start(&mut owner, &scope, "2", "cerulean_read_file", 'a');
    finish(&mut owner, &scope, "2", true);
    let second = owner.checkpoint().unwrap().evidence().clone();
    let expected_incidents = owner.supervision_incidents(&policy()).unwrap();
    assert_eq!(expected_incidents.len(), 1);
    drop(owner);
    let before = files(&path);
    let restored: ToolEvidence =
        serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();
    restored.verify_original(&path, &scope).unwrap();
    assert_eq!(
        OmpToolJournal::read_evidence(&path, &scope, first.head()).unwrap(),
        first
    );
    assert!(first.incidents(&policy()).unwrap().is_empty());
    second.verify_original(&path, &scope).unwrap();
    assert_eq!(second.incidents(&policy()).unwrap(), expected_incidents);
    assert!(OmpToolJournal::read_evidence(&path, &scope, &unresolved).is_err());
    assert!(OmpToolJournal::read_evidence(&path, &scope, &"0".repeat(64)).is_err());
    let mut wrong_scope = scope.clone();
    wrong_scope.fence += 1;
    assert!(first.verify_original(&path, &wrong_scope).is_err());
    let mut altered = serde_json::to_value(&first).unwrap();
    altered["observation"]["calls"]["1"]["result_is_error"] = false.into();
    let altered: ToolEvidence = serde_json::from_value(altered).unwrap();
    assert!(altered.verify_original(&path, &scope).is_err());
    assert_eq!(files(&path), before);
    let reopened = OmpToolJournal::open(&path, &scope).unwrap();
    assert!(reopened.checkpoint().is_err());
    drop(reopened);
    // A valid early prefix must not hide a partial later publication.
    fs::write(path.join("004.pending"), b"incomplete later record").unwrap();
    let pending = files(&path);
    assert!(first.verify_original(&path, &scope).is_err());
    assert_eq!(files(&path), pending);
    // An inspection must not repair a missing lock file either.
    fs::remove_file(path.join("writer.lock")).unwrap();
    assert!(first.verify_original(&path, &scope).is_err());
    assert!(!path.join("writer.lock").exists());
    assert_eq!(files(&path), pending);
}
