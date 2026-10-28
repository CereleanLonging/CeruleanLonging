use super::publication_tests::seed;
use super::*;

#[cfg(windows)]
#[path = "management_review_crash_tests.rs"]
mod crash_tests;
use crate::management_review::{acknowledge, inspect, load};

#[test]
fn historical_source_pending_alias_matches_original_but_user_review_requires_publication() {
    let root = tempfile::tempdir().unwrap();
    let (sequence, _) = seed(root.path(), "discard");
    let original = DecisionJournal::open(root.path())
        .unwrap()
        .historical_source("request-1", true)
        .unwrap();
    let stem = root
        .path()
        .join(format!("management-decisions/request-1.{sequence}"));
    let saved = PathBuf::from(format!("{}.json", stem.display()));
    let pending = PathBuf::from(format!("{}.pending", stem.display()));
    fs::rename(&saved, &pending).unwrap();
    let bytes = fs::read(&pending).unwrap();
    let mut journal = DecisionJournal::open(root.path()).unwrap();
    assert!(journal.historical_source("request-1", true).is_err());
    assert_eq!(
        journal.historical_source("request-1", false).unwrap(),
        original
    );
    assert_eq!(fs::read(&pending).unwrap(), bytes);
    assert!(!saved.exists());
    journal.recover_publications().unwrap();
    assert_eq!(
        journal.historical_source("request-1", true).unwrap(),
        original
    );
}

#[test]
fn historical_source_reviewed_fragments_preserve_uncertainty_and_reject_changed_bytes() {
    use crate::currency_budget::HistoricalScope;
    for kind in ["intent", "response"] {
        let root = tempfile::tempdir().unwrap();
        let (sequence, relative, raw, _) = damaged(root.path(), kind, false);
        review(root.path(), &relative);
        let mut journal = DecisionJournal::open(root.path()).unwrap();
        if sequence == 1 {
            assert!(journal.historical_source("request-1", true).is_err());
            let head = journal.evidence_digest("request-1").unwrap().to_owned();
            journal
                .resolve(
                    "request-1",
                    &head,
                    UserResolution::Discard {
                        reason: "Close local unknown; preserve original fee uncertainty".into(),
                    },
                )
                .unwrap();
        }
        let source = journal.historical_source("request-1", true).unwrap();
        let HistoricalScope::Management {
            original_project_id,
            ..
        } = source.scope
        else {
            panic!("scope")
        };
        assert_eq!(original_project_id.is_none(), sequence == 0);
        assert_eq!(source.uncertain_outcomes, 1);
        assert!(source.references.contains(&relative));
        assert_eq!(fs::read(root.path().join(&relative)).unwrap(), raw);
        fs::write(root.path().join(&relative), b"{").unwrap();
        assert!(journal.historical_source("request-1", true).is_err());
    }
}

fn damaged(root: &Path, kind: &str, empty: bool) -> (u8, String, Vec<u8>, Record) {
    let (sequence, _) = seed(root, kind);
    let target = root.join(format!("management-decisions/request-1.{sequence}.json"));
    let (record, raw) = read(&target).unwrap();
    let partial = if empty {
        vec![]
    } else {
        raw[..raw.len() / 2].to_vec()
    };
    let relative = format!("management-decisions/request-1.{sequence}.pending");
    fs::rename(&target, root.join(&relative)).unwrap();
    fs::write(root.join(&relative), &partial).unwrap();
    (sequence, relative, partial, record)
}
fn review(root: &Path, relative: &str) {
    let evidence = inspect(root, relative).unwrap().0;
    acknowledge(
        root,
        relative,
        &evidence,
        "review original evidence",
        |_| Ok(()),
    )
    .unwrap();
}

#[test]
fn currency_history_reviewed_intent_or_response_remains_visible_without_invented_project() {
    use cerulean_domain::currency_budget::CurrencyHistoryReason;
    for kind in ["intent", "response", "apply"] {
        let root = tempfile::tempdir().unwrap();
        let (sequence, relative, raw, _) = damaged(root.path(), kind, false);
        review(root.path(), &relative);
        let journal = DecisionJournal::open(root.path()).unwrap();
        let gaps = journal.currency_history_gaps();
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].source_id, "request-1");
        if sequence <= 1 {
            assert_eq!(
                gaps[0].reason,
                CurrencyHistoryReason::ReviewedManagementRecord
            );
            assert_eq!(gaps[0].reference, relative);
            assert_eq!(gaps[0].project_id.is_none(), sequence == 0);
        } else {
            assert_eq!(
                gaps[0].reason,
                CurrencyHistoryReason::LegacyManagementRequest
            );
        }
        assert_eq!(fs::read(root.path().join(relative)).unwrap(), raw);
    }
}

#[test]
fn all_management_stages_retain_partial_bytes_and_require_correct_followup() {
    for kind in [
        "intent",
        "response",
        "unknown",
        "apply",
        "applied",
        "discard",
        "reply",
        "abandon-application",
        "abandon-startup",
    ] {
        for empty in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (sequence, relative, raw, record) = damaged(root.path(), kind, empty);
            assert!(DecisionJournal::open(root.path()).is_err());
            review(root.path(), &relative);
            let mut journal = DecisionJournal::open(root.path()).unwrap();
            assert_eq!(journal.reviews.len(), 1);
            match sequence {
                0 => assert!(journal.begin(super::tests::intent()).is_err()),
                1 => {
                    assert_eq!(journal.statuses()[0].state, DecisionState::Unknown);
                    assert!(journal
                        .finish("request-1", Err("do not resend".into()))
                        .is_err());
                    let head = journal.evidence_digest("request-1").unwrap().to_owned();
                    journal
                        .resolve(
                            "request-1",
                            &head,
                            UserResolution::Discard {
                                reason: "fresh user discard".into(),
                            },
                        )
                        .unwrap();
                }
                2 => {
                    let i = journal.intent("request-1").unwrap().clone();
                    assert!(journal
                        .prepare_application("request-1", ApplicationTarget::Wait, &i.state_digest)
                        .is_err());
                    let head = journal.evidence_digest("request-1").unwrap().to_owned();
                    journal
                        .resolve(
                            "request-1",
                            &head,
                            UserResolution::Discard {
                                reason: "fresh user discard".into(),
                            },
                        )
                        .unwrap();
                }
                3 => journal.record_applied("request-1", None).unwrap(),
                4 => {
                    assert!(journal.ensure_dispatchable("request-1").is_err());
                    let Evidence::UserResolution { mut resolution } = record.body.evidence else {
                        panic!("resolution")
                    };
                    match &mut resolution {
                        UserResolution::Reply { answer } => *answer = "new explicit answer".into(),
                        UserResolution::Discard { reason }
                        | UserResolution::AbandonApplication { reason, .. }
                        | UserResolution::AbandonStartup { reason, .. } => {
                            *reason = "new explicit user decision".into()
                        }
                    }
                    let head = journal.evidence_digest("request-1").unwrap().to_owned();
                    journal.resolve("request-1", &head, resolution).unwrap();
                }
                _ => unreachable!(),
            }
            drop(journal);
            let mut journal = DecisionJournal::open(root.path()).unwrap();
            journal.recover_publications().unwrap();
            assert_eq!(fs::read(root.path().join(&relative)).unwrap(), raw);
            assert_eq!(journal.reviews[0].reason, "review original evidence");
            if sequence != 3 {
                assert!(journal.ensure_dispatchable("request-1").is_err());
            }
        }
    }
}

#[test]
fn management_review_rejects_changed_prefix_source_stale_or_complete_evidence() {
    for mode in [
        "raw-changed",
        "prefix-changed",
        "missing",
        "complete",
        "future",
        "receipt-corrupt",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (_, relative, raw, _) = damaged(root.path(), "response", false);
        let evidence = inspect(root.path(), &relative).unwrap().0;
        assert!(acknowledge(root.path(), &relative, &"f".repeat(64), "stale", |_| Ok(())).is_err());
        if mode == "complete" {
            fs::write(root.path().join(&relative), b"{}").unwrap();
            assert!(inspect(root.path(), &relative).is_err());
            continue;
        }
        if mode == "future" {
            fs::write(
                root.path().join("management-decisions/request-1.2.pending"),
                b"{",
            )
            .unwrap();
            assert!(inspect(root.path(), &relative).is_err());
            continue;
        }
        review(root.path(), &relative);
        acknowledge(
            root.path(),
            &relative,
            &evidence,
            "must not replace first reason",
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(
            load(root.path()).unwrap()[0].reason,
            "review original evidence"
        );
        match mode {
            "raw-changed" => fs::write(root.path().join(&relative), b"{").unwrap(),
            "prefix-changed" => {
                let path = root.path().join("management-decisions/request-1.0.json");
                let mut prefix = fs::read(&path).unwrap();
                prefix.push(b' ');
                fs::write(path, prefix).unwrap();
            }
            "missing" => fs::remove_file(root.path().join(&relative)).unwrap(),
            "receipt-corrupt" => {
                let path = root.path().join(format!(
                    "management-record-reviews/{}.json",
                    digest(&relative).unwrap()
                ));
                fs::write(path, b"{}").unwrap();
            }
            _ => unreachable!(),
        }
        assert!(DecisionJournal::open(root.path()).is_err(), "{mode}");
        if mode != "missing" && mode != "raw-changed" {
            assert_eq!(fs::read(root.path().join(&relative)).unwrap(), raw);
        }
    }
}

#[test]
fn reviewed_receipt_and_user_resolution_retries_preserve_every_partial_write() {
    for kind in ["applied", "discard"] {
        for point in [
            PublishPoint::TempCreated,
            PublishPoint::PartialWritten,
            PublishPoint::TempSynced,
            PublishPoint::Published,
            PublishPoint::PendingRemoved,
        ] {
            let root = tempfile::tempdir().unwrap();
            let (sequence, relative, raw, record) = damaged(root.path(), kind, false);
            review(root.path(), &relative);
            let mut journal = DecisionJournal::open(root.path()).unwrap();
            let previous = journal.evidence_digest("request-1").unwrap().to_owned();
            assert!(journal
                .publish_observed(
                    "request-1",
                    sequence,
                    previous,
                    record.body.evidence.clone(),
                    |p| if p == point {
                        Err("interrupted retry".into())
                    } else {
                        Ok(())
                    }
                )
                .is_err());
            assert!(!journal.healthy());
            drop(journal);
            if matches!(
                point,
                PublishPoint::TempCreated | PublishPoint::PartialWritten
            ) {
                assert!(DecisionJournal::open(root.path()).is_err());
                let retry = fs::read_dir(root.path().join("management-decisions"))
                    .unwrap()
                    .map(|e| e.unwrap().file_name().into_string().unwrap())
                    .find(|n| {
                        n.starts_with(&format!("request-1.{sequence}."))
                            && n.ends_with(".pending")
                            && n.split('.').count() == 4
                    })
                    .unwrap();
                review(root.path(), &format!("management-decisions/{retry}"));
                let mut journal = DecisionJournal::open(root.path()).unwrap();
                if sequence == 3 {
                    journal.record_applied("request-1", None).unwrap();
                } else {
                    let head = journal.evidence_digest("request-1").unwrap().to_owned();
                    journal
                        .resolve(
                            "request-1",
                            &head,
                            UserResolution::Discard {
                                reason: "third explicit attempt".into(),
                            },
                        )
                        .unwrap();
                }
            }
            let mut journal = DecisionJournal::open(root.path()).unwrap();
            journal.recover_publications().unwrap();
            assert_eq!(fs::read(root.path().join(&relative)).unwrap(), raw);
            assert_eq!(
                journal.reviews.len(),
                if matches!(
                    point,
                    PublishPoint::TempCreated | PublishPoint::PartialWritten
                ) {
                    2
                } else {
                    1
                }
            );
            assert!(root
                .path()
                .join(format!("management-decisions/request-1.{sequence}.json"))
                .exists());
        }
    }
}

#[test]
fn review_publication_requires_new_confirmation_until_the_final_name_exists() {
    for point in ["created", "partial", "synced", "published", "finished"] {
        let root = tempfile::tempdir().unwrap();
        let (_, relative, raw, _) = damaged(root.path(), "intent", false);
        let evidence = inspect(root.path(), &relative).unwrap().0;
        assert!(
            acknowledge(root.path(), &relative, &evidence, "first", |p| {
                if p == point {
                    Err("interrupted review".into())
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        let published = matches!(point, "published" | "finished");
        assert_eq!(load(root.path()).unwrap().len(), usize::from(published));
        assert_eq!(DecisionJournal::open(root.path()).is_ok(), published);
        acknowledge(
            root.path(),
            &relative,
            &evidence,
            "new explicit confirmation",
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(
            load(root.path()).unwrap()[0].reason,
            if published {
                "first"
            } else {
                "new explicit confirmation"
            }
        );
        assert_eq!(fs::read(root.path().join(relative)).unwrap(), raw);
    }
}
