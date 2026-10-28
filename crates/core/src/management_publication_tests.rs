use super::tests::{intent, prepared_assignment, response};
use super::*;

#[cfg(windows)]
#[path = "management_publication_crash_tests.rs"]
mod crash_tests;

const KINDS: [&str; 9] = [
    "intent",
    "response",
    "unknown",
    "apply",
    "applied",
    "discard",
    "reply",
    "abandon-application",
    "abandon-startup",
];

pub(super) fn seed(root: &Path, kind: &str) -> (u8, DecisionState) {
    let mut journal = DecisionJournal::open(root).unwrap();
    let i = intent();
    if kind.starts_with("abandon-") {
        prepared_assignment(&mut journal);
        let resolution = if kind == "abandon-application" {
            UserResolution::AbandonApplication {
                reason: "fixture close unassigned application".into(),
                workflow_sequence: 0,
                workflow_hash: None,
            }
        } else {
            // Construct an assignment response with no application yet.
            fs::remove_file(journal.root.join("request-1.2.json")).unwrap();
            drop(journal);
            journal = DecisionJournal::open(root).unwrap();
            UserResolution::AbandonStartup {
                reason: "fixture close unassigned startup".into(),
                attempt_id: "attempt".into(),
                startup_hash: "c".repeat(64),
            }
        };
        let head = journal.evidence_digest("request-1").unwrap().to_owned();
        journal.resolve("request-1", &head, resolution).unwrap();
        return (
            4,
            if kind == "abandon-application" {
                DecisionState::Abandoned
            } else {
                DecisionState::StartupClosed
            },
        );
    }
    journal.begin(i.clone()).unwrap();
    if kind == "intent" {
        return (0, DecisionState::Unknown);
    }
    journal
        .finish(
            &i.operation_id,
            if kind == "unknown" {
                Err("fixture".into())
            } else {
                Ok(response(&i))
            },
        )
        .unwrap();
    if kind == "response" {
        return (1, DecisionState::Ready);
    }
    if kind == "unknown" {
        return (1, DecisionState::Unknown);
    }
    if kind == "discard" {
        let head = journal.evidence_digest(&i.operation_id).unwrap().to_owned();
        journal
            .resolve(
                &i.operation_id,
                &head,
                UserResolution::Discard {
                    reason: "fixture discard".into(),
                },
            )
            .unwrap();
        return (4, DecisionState::Discarded);
    }
    journal
        .prepare_application(&i.operation_id, ApplicationTarget::Wait, &i.state_digest)
        .unwrap();
    if kind == "apply" {
        return (2, DecisionState::Applying);
    }
    journal.record_applied(&i.operation_id, None).unwrap();
    if kind == "applied" {
        return (3, DecisionState::Applied);
    }
    assert_eq!(kind, "reply");
    let head = journal.evidence_digest(&i.operation_id).unwrap().to_owned();
    journal
        .resolve(
            &i.operation_id,
            &head,
            UserResolution::Reply {
                answer: "fixture reply".into(),
            },
        )
        .unwrap();
    (4, DecisionState::Applied)
}

fn paths(root: &Path, sequence: u8) -> (PathBuf, PathBuf) {
    let dir = root.join("management-decisions");
    (
        dir.join(format!("request-1.{sequence}.json")),
        dir.join(format!("request-1.{sequence}.pending")),
    )
}

fn inventory(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root.join("management-decisions"))
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().into_string().unwrap(),
                fs::read(e.path()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn all_record_kinds_are_read_only_until_recovery_and_keep_exact_original_bytes() {
    for kind in KINDS {
        for duplicate in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (sequence, expected) = seed(root.path(), kind);
            let (target, pending) = paths(root.path(), sequence);
            let original = fs::read(&target).unwrap();
            if duplicate {
                fs::copy(&target, &pending).unwrap();
            } else {
                fs::rename(&target, &pending).unwrap();
            }
            let before = inventory(root.path());
            let mut journal = DecisionJournal::open(root.path()).unwrap();
            assert_eq!(journal.statuses()[0].state, expected, "{kind}");
            assert_eq!(inventory(root.path()), before);
            assert_eq!(
                journal
                    .publish("fresh", 0, "genesis".into(), Evidence::Unknown)
                    .unwrap_err(),
                "management publication recovery required before writing"
            );
            journal.recover_publications().unwrap();
            assert!(!pending.exists());
            assert_eq!(fs::read(&target).unwrap(), original);
            assert!(journal.healthy());
            let recovered = inventory(root.path());
            drop(journal);
            let mut journal = DecisionJournal::open(root.path()).unwrap();
            journal.recover_publications().unwrap();
            assert_eq!(journal.statuses()[0].state, expected);
            assert_eq!(inventory(root.path()), recovered);
        }
    }
}

#[test]
fn malformed_conflicting_and_semantically_invalid_pending_never_publish() {
    for mode in [
        "empty",
        "partial",
        "conflict",
        "hash",
        "wrong-name",
        "orphan",
        "oversized",
        "extra-field",
        "bad-link",
    ] {
        let root = tempfile::tempdir().unwrap();
        seed(root.path(), "response");
        let (target, mut pending) = paths(root.path(), 1);
        let original = fs::read(&target).unwrap();
        if mode == "conflict" {
            fs::copy(&target, &pending).unwrap();
        } else {
            fs::rename(&target, &pending).unwrap();
        }
        let mut value: Value = serde_json::from_slice(&original).unwrap();
        match mode {
            "empty" => fs::write(&pending, b"").unwrap(),
            "partial" => fs::write(&pending, &original[..original.len() / 2]).unwrap(),
            "oversized" => fs::write(&pending, vec![b' '; MAX_BYTES + 1]).unwrap(),
            "wrong-name" => {
                let wrong = pending.with_file_name("wrong.1.pending");
                fs::rename(&pending, &wrong).unwrap();
                pending = wrong;
            }
            "orphan" => {
                fs::remove_file(paths(root.path(), 0).0).unwrap();
            }
            "extra-field" => {
                value["unexpected"] = true.into();
                fs::write(&pending, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            "hash" => {
                value["hash"] = "f".repeat(64).into();
                fs::write(&pending, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            "conflict" => {
                let mut changed = original.clone();
                changed.push(b' ');
                fs::write(&pending, changed).unwrap();
            }
            "bad-link" => {
                value["body"]["previous"] = "f".repeat(64).into();
                let body: Body = serde_json::from_value(value["body"].clone()).unwrap();
                value["hash"] = digest(&body).unwrap().into();
                fs::write(&pending, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            _ => unreachable!(),
        }
        let before = inventory(root.path());
        assert!(DecisionJournal::open(root.path()).is_err(), "{mode}");
        assert_eq!(inventory(root.path()), before);
        assert!(pending.exists());
        assert_eq!(target.exists(), mode == "conflict");
    }
}

#[test]
fn changed_publications_and_partial_batch_recovery_require_reopen() {
    for mode in [
        "pending-changed",
        "target-changed",
        "target-appeared",
        "synced",
        "linked",
        "removed",
    ] {
        let root = tempfile::tempdir().unwrap();
        seed(root.path(), "response");
        let (target0, pending0) = paths(root.path(), 0);
        let (target1, pending1) = paths(root.path(), 1);
        let original0 = fs::read(&target0).unwrap();
        let original1 = fs::read(&target1).unwrap();
        fs::rename(&target0, &pending0).unwrap();
        if mode == "target-changed" {
            fs::copy(&target1, &pending1).unwrap();
        } else {
            fs::rename(&target1, &pending1).unwrap();
        }
        let mut journal = DecisionJournal::open(root.path()).unwrap();
        match mode {
            "pending-changed" => {
                let mut changed = original1.clone();
                changed.push(b' ');
                fs::write(&pending1, changed).unwrap();
            }
            "target-changed" => fs::write(&target1, b"changed").unwrap(),
            "target-appeared" => fs::write(&target1, b"foreign").unwrap(),
            _ => {}
        }
        let before = inventory(root.path());
        let point = match mode {
            "synced" => RecoveryPoint::Synced,
            "linked" => RecoveryPoint::FinalNamePresent,
            _ => RecoveryPoint::PendingRemoved,
        };
        assert!(journal
            .recover_publications_observed(|p, i| {
                if p == point && i == 1 {
                    Err("injected interruption".into())
                } else {
                    Ok(())
                }
            })
            .is_err());
        assert!(!journal.healthy());
        assert!(journal
            .publish("fresh", 0, "genesis".into(), Evidence::Unknown)
            .is_err());
        if matches!(
            mode,
            "pending-changed" | "target-changed" | "target-appeared"
        ) {
            assert_eq!(inventory(root.path()), before);
            assert!(!target0.exists());
        } else {
            drop(journal);
            let mut journal = DecisionJournal::open(root.path()).unwrap();
            journal.recover_publications().unwrap();
            assert_eq!(fs::read(target0).unwrap(), original0);
            assert_eq!(fs::read(target1).unwrap(), original1);
            assert!(!pending0.exists() && !pending1.exists());
        }
    }
}

#[test]
fn publishing_never_overwrites_a_target_created_after_the_initial_check() {
    let root = tempfile::tempdir().unwrap();
    let mut journal = DecisionJournal::open(root.path()).unwrap();
    let (target, pending) = paths(root.path(), 0);
    let i = intent();
    let error = journal.publish_observed(
        &i.operation_id,
        0,
        "genesis".into(),
        Evidence::Intent {
            intent: i.clone(),
            request: None,
            scoped: None,
        },
        |p| {
            if p == PublishPoint::TempSynced {
                fs::write(&target, b"foreign bytes").unwrap();
            }
            Ok(())
        },
    );
    assert!(error.is_err());
    assert!(!journal.healthy());
    assert_eq!(fs::read(&target).unwrap(), b"foreign bytes");
    assert!(read(&pending).is_ok());
    assert!(DecisionJournal::open(root.path()).is_err());
}
