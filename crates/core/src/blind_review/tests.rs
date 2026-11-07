use super::*;
use cerulean_domain::api_worker::ApiTarget;

fn fixture(root: &Path) -> (Ledger, String) {
    let mut l = Ledger::open(root).unwrap();
    let material = ReviewMaterial {
        independent_check_evidence: None,
        protocol: PROTOCOL.into(),
        requirement: "Add integers".into(),
        criteria: vec!["Return their sum".into()],
        fragments: vec![Fragment {
            id: "f1".into(),
            file: "file1".into(),
            part: 1,
            content: "return a + b".into(),
        }],
        objective_checks: 1,
    };
    let binding = ReviewBinding {
        project_id: "p1".into(),
        revision: "a".repeat(64),
        setup: "b".repeat(64),
        delivery: "c".repeat(64),
        evidence: "d".repeat(64),
        material: digest(&material).unwrap(),
        policy: "e".repeat(64),
    };
    let seats = [
        ("chief", "chief"),
        ("deputy", "deputy"),
        ("worker-1", "worker"),
    ]
    .into_iter()
    .map(|(id, role)| ReviewSeat {
        max_output_tokens: None,
        id: id.into(),
        role: role.into(),
        target: Some(ApiTarget::OpenAi {}),
        transport: None,
        model: "model-a".into(),
        allowed_models: vec!["model-a".into()],
    })
    .collect();
    let id = l
        .prepare(
            binding,
            material,
            seats,
            ReviewCoverage {
                files: vec!["main.py".into()],
                excluded: vec![],
                integration_task: None,
            },
        )
        .unwrap();
    (l, id)
}
fn ballot(vote: Vote) -> Ballot {
    Ballot {
        assessments: vec![],
        vote,
        citations: vec!["f1".into()],
        reason: "Verified sum semantics".into(),
    }
}
#[test]
fn api_review_limit_is_approved_and_requires_reader25_on_reopen() {
    let original_root = tempfile::tempdir().unwrap();
    let (original, id) = fixture(original_root.path());
    let round = original.rounds[&id].clone();
    let mut seats = round.seats.clone();
    seats[2].max_output_tokens = Some(393216);
    assert_ne!(review_digest(&round.binding, &seats).unwrap(), round.digest);
    let root = tempfile::tempdir().unwrap();
    drop(crate::Store::open(root.path()).unwrap());
    crate::review_material_guard::enable_api_limits(root.path()).unwrap();
    let mut ledger = Ledger::open(root.path()).unwrap();
    let id = ledger
        .prepare(round.binding, round.material, seats, round.coverage)
        .unwrap();
    assert_eq!(ledger.rounds[&id].view().max_output_tokens, 393216);
    drop(ledger);
    assert!(Ledger::open(root.path()).is_ok());
    fs::remove_file(
        root.path()
            .join("events")
            .join(crate::review_material_guard::API_LIMITS_FILE_NAME),
    )
    .unwrap();
    assert!(Ledger::open(root.path()).is_err());
}
#[test]
fn blind_review_crash_child() {
    let Some(root) = std::env::var_os("BLIND_REVIEW_TEST_CRASH_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let (mut l, id) = fixture(&root);
    start(&mut l, &id);
    l.reserve(&id, 101).unwrap();
    std::process::exit(74);
}
#[test]
fn real_process_crash_before_publication_and_after_reservation() {
    for partial in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "blind_review::tests::blind_review_crash_child",
                "--nocapture",
            ])
            .env("BLIND_REVIEW_TEST_CRASH_ROOT", root.path())
            .env_remove("BLIND_REVIEW_TEST_CRASH_PENDING");
        if partial {
            command.env("BLIND_REVIEW_TEST_CRASH_PENDING", "1");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(if partial { 73 } else { 74 }));
        let reopened = Ledger::open(root.path());
        if partial {
            assert!(reopened.is_err());
        } else {
            let l = reopened.unwrap();
            let r = l.latest("p1").unwrap();
            assert_eq!(r.phase, ReviewPhase::Incomplete);
            assert!(r.view().ballots.is_none());
        }
    }
}
fn start(l: &mut Ledger, id: &str) {
    let key = l.rounds[id].digest.clone();
    l.start("p1", id, &key, 100).unwrap();
}
fn submit(l: &mut Ledger, id: &str, vote: Vote) -> Pending {
    let (p, _, _) = l.reserve(id, 101).unwrap().unwrap();
    l.complete(id, &p, "model-a", ballot(vote), 102).unwrap();
    p
}
#[test]
fn weighted_round_keeps_scores_sealed_and_blocks_functional_test_on_missing_criteria() {
    use cerulean_domain::{
        independent_checks::{CheckArchive, CheckFact, CheckRecord},
        review_weights::CriterionAssessment,
    };
    for complete in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (_, old) = fixture(root.path());
        let mut original = Ledger::open(root.path()).unwrap().rounds[&old].clone();
        let fresh = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::open(fresh.path()).unwrap();
        let record = CheckRecord {
            schema_version: 1,
            project_id: "p1".into(),
            revision: original.binding.revision.clone(),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            operation_id: "verification".into(),
            source_digest: "f".repeat(64),
            manifest_digest: "e".repeat(64),
            report_digest: "d".repeat(64),
            observed_at: "2026-11-05T01:00:00Z".into(),
            checks: vec![CheckFact {
                id: "check".into(),
                command_digest: "c".repeat(64),
                tool_digest: "b".repeat(64),
                passed: true,
                reason: "real fixture assertion".into(),
            }],
        };
        let archive = CheckArchive {
            assembled: None,
            schema_version: 1,
            project_id: "p1".into(),
            revision: original.binding.revision.clone(),
            final_task: "task".into(),
            final_source: record.source_digest.clone(),
            records: vec![record],
        };
        if complete {
            drop(crate::Store::open(fresh.path()).unwrap());
            crate::review_material_guard::enable(fresh.path()).unwrap();
            original.material.independent_check_evidence = Some(
                cerulean_domain::independent_verifier::summarize_checks(
                    &original.material,
                    &archive,
                )
                .unwrap(),
            );
            original.binding.material = digest(&original.material).unwrap();
        }
        let id = ledger
            .prepare_with_checks(
                original.binding,
                original.material,
                original.seats,
                original.coverage,
                Some(archive),
            )
            .unwrap();
        start(&mut ledger, &id);
        for index in 0..4 {
            let (pending, seat, _) = ledger.reserve(&id, 101).unwrap().unwrap();
            let mut value = ballot(Vote::Pass);
            if complete || index != 2 {
                value.assessments = vec![CriterionAssessment {
                    criterion: 1,
                    aligned: true,
                    citations: vec!["f1".into()],
                }];
            }
            ledger
                .complete_review(
                    &id,
                    &pending,
                    &seat.model,
                    Some(value),
                    ReviewToolAudit {
                        status: ReviewToolStatus::Clean,
                        source: "api-response".into(),
                        request: pending.request.clone(),
                        evidence_digest: "a".repeat(64),
                        tools: vec![],
                        reason: "complete synthetic protocol".into(),
                    },
                    102,
                )
                .unwrap();
            if index < 3 {
                assert!(ledger.rounds[&id].view().weighted_result.is_none());
            }
        }
        let round = ledger.rounds[&id].clone();
        assert_eq!(
            round.phase,
            if complete {
                ReviewPhase::AwaitingFunctionalTest
            } else {
                ReviewPhase::Rejected
            }
        );
        assert_eq!(round.weighted_result.as_ref().unwrap().passed, complete);
        assert_eq!(round.view().weighted_result.unwrap().seats.len(), 3);
        if !complete {
            assert!(ledger
                .start_functional(&id, &round.digest, &round.binding, 103, 30)
                .is_err());
            ledger.reserve_analysis(&id, 103).unwrap();
        }
        let reopened = Ledger::open(fresh.path()).unwrap();
        assert_eq!(reopened.rounds[&id].weighted_result, round.weighted_result);
        if complete {
            let mut forged = round.clone();
            forged
                .material
                .independent_check_evidence
                .as_mut()
                .unwrap()
                .assembled_delivery = true;
            forged.binding.material = digest(&forged.material).unwrap();
            forged.digest = review_digest(&forged.binding, &forged.seats).unwrap();
            assert!(forged.validate().is_err());
            fs::remove_file(
                fresh
                    .path()
                    .join("events")
                    .join(crate::review_material_guard::FILE_NAME),
            )
            .unwrap();
            assert!(Ledger::open(fresh.path()).is_err());
        }
        let mut forged = round;
        forged
            .weighted_result
            .as_mut()
            .unwrap()
            .overall_basis_points = 9999;
        assert!(forged.validate().is_err());
    }
}
#[test]
fn deputy_ready_is_not_a_vote_and_all_votes_are_sealed() {
    let root = tempfile::tempdir().unwrap();
    let (mut l, id) = fixture(root.path());
    start(&mut l, &id);
    submit(&mut l, &id, Vote::Pass);
    assert!(l.rounds[&id].ballots.is_empty());
    submit(&mut l, &id, Vote::Pass);
    assert!(l.rounds[&id].view().ballots.is_none());
    submit(&mut l, &id, Vote::Fail);
    assert!(l.rounds[&id].view().ballots.is_none());
    submit(&mut l, &id, Vote::Pass);
    assert_eq!(l.rounds[&id].phase, ReviewPhase::Rejected);
    assert_eq!(l.rounds[&id].view().ballots.unwrap().len(), 3);
    let r = l.rounds[&id].clone();
    assert!(l.confirm("p1", &id, &r.digest, &r.binding).is_err());
}
fn analysis() -> FailureAnalysis {
    FailureAnalysis {
        omissions: vec!["Missing boundary case".into()],
        disputed_criteria: vec![],
        chief_revision_request: "Clarify negative inputs with the user and revise the plan".into(),
    }
}
#[test]
fn rejected_tool_audit_counts_as_fail_and_keeps_functional_testing_locked() {
    for status in [
        ReviewToolStatus::ToolCallObserved,
        ReviewToolStatus::Unverifiable,
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut ledger, id) = fixture(root.path());
        start(&mut ledger, &id);
        submit(&mut ledger, &id, Vote::Pass);
        let (pending, seat, _) = ledger.reserve(&id, 101).unwrap().unwrap();
        let audit = ReviewToolAudit {
            tools: if status == ReviewToolStatus::ToolCallObserved {
                vec!["web_search".into()]
            } else {
                vec![]
            },
            status,
            source: "codex-history".into(),
            request: pending.request.clone(),
            evidence_digest: "f".repeat(64),
            reason: "Synthetic scoring audit".into(),
        };
        ledger
            .complete_review(
                &id,
                &pending,
                &seat.model,
                Some(ballot(Vote::Pass)),
                audit.clone(),
                102,
            )
            .unwrap();
        assert!(ledger.rounds[&id].view().ballots.is_none());
        submit(&mut ledger, &id, Vote::Pass);
        submit(&mut ledger, &id, Vote::Pass);
        let round = ledger.rounds[&id].clone();
        assert_eq!(round.phase, ReviewPhase::Rejected);
        assert_eq!(round.ballots.len(), 3);
        assert_eq!(round.ballots[&seat.id].ballot.vote, Vote::Fail);
        assert_eq!(
            round.ballots[&seat.id].original_ballot,
            Some(ballot(Vote::Pass))
        );
        assert_eq!(round.ballots[&seat.id].tool_audit, Some(audit));
        assert!(ledger
            .start_functional(&id, &round.digest, &round.binding, 103, 90)
            .is_err());
        drop(ledger);
        let reopened = Ledger::open(root.path()).unwrap();
        assert_eq!(reopened.rounds[&id].phase, ReviewPhase::Rejected);
        assert_eq!(reopened.rounds[&id].ballots, round.ballots);
    }
}
#[test]
fn revealed_failure_analysis_is_single_use_and_never_changes_votes() {
    let root = tempfile::tempdir().unwrap();
    let (mut l, id) = fixture(root.path());
    start(&mut l, &id);
    assert!(l.reserve_analysis(&id, 101).is_err());
    for vote in [Vote::Pass, Vote::Pass, Vote::Fail, Vote::Pass] {
        submit(&mut l, &id, vote);
    }
    let before = l.rounds[&id].ballots.clone();
    let (p, seat, _, votes) = l.reserve_analysis(&id, 103).unwrap();
    assert_eq!(seat.role, "deputy");
    assert_eq!(votes.len(), 3);
    assert!(l.reserve_analysis(&id, 103).is_err());
    assert!(l.end(&id, ReviewPhase::ChangesRequested, "change").is_err());
    assert!(l
        .complete_analysis(&id, &p, "wrong-model", analysis(), 104)
        .is_err());
    l.complete_analysis(&id, &p, "model-a", analysis(), 104)
        .unwrap();
    assert_eq!(l.rounds[&id].ballots, before);
    assert_eq!(l.rounds[&id].view().deputy_analysis, Some(analysis()));
    assert!(l
        .complete_analysis(&id, &p, "model-a", analysis(), 105)
        .is_err());
    assert!(l.reserve_analysis(&id, 105).is_err());
    l.end(&id, ReviewPhase::ChangesRequested, "Clarify inputs")
        .unwrap();
    drop(l);
    let reopened = Ledger::open(root.path()).unwrap();
    assert_eq!(
        reopened.rounds[&id].view().deputy_analysis,
        Some(analysis())
    );
}
#[cfg(windows)]
#[test]
fn cold_backup_restores_sealed_votes_guard_and_revokes_old_dispatch() {
    for failure_analysis in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        drop(crate::Store::open(&data).unwrap());
        crate::blind_review_guard::enable(&data).unwrap();
        let (mut l, id) = fixture(&data);
        start(&mut l, &id);
        submit(&mut l, &id, Vote::Pass);
        submit(&mut l, &id, Vote::Pass);
        let pending = if failure_analysis {
            submit(&mut l, &id, Vote::Fail);
            submit(&mut l, &id, Vote::Pass);
            l.reserve_analysis(&id, 103).unwrap().0
        } else {
            l.reserve(&id, 103).unwrap().unwrap().0
        };
        let before = fs::read(
            data.join("blind-review")
                .join(format!("{:08}.json", l.sequence)),
        )
        .unwrap();
        let sequence = l.sequence;
        drop(l);
        let archive = root.path().join("backup");
        let review = crate::backup::create(&data, &archive).unwrap();
        let owner = crate::ipc::current_sid().unwrap();
        for p in [
            &archive,
            &archive.join("payload"),
            &archive.join("payload/blind-review"),
        ] {
            crate::windows_worker_account::check_acl(p, &owner, None, false).unwrap();
        }
        fs::rename(&data, root.path().join("preserved")).unwrap();
        crate::backup::restore(&archive, &data, &review.manifest_digest).unwrap();
        crate::windows_worker_account::check_acl(&data.join("blind-review"), &owner, None, false)
            .unwrap();
        assert_eq!(
            before,
            fs::read(
                data.join("blind-review")
                    .join(format!("{sequence:08}.json"))
            )
            .unwrap()
        );
        assert_eq!(
            crate::data_format::status(&data)
                .unwrap()
                .minimum_reader_generation,
            19
        );
        let mut restored = Ledger::open(&data).unwrap();
        if failure_analysis {
            assert_eq!(restored.rounds[&id].phase, ReviewPhase::Rejected);
            assert!(restored.rounds[&id].view().ballots.is_some());
            assert!(restored
                .complete_analysis(&id, &pending, "model-a", analysis(), 104)
                .is_err());
            assert!(restored.reserve_analysis(&id, 104).is_err());
        } else {
            assert_eq!(restored.rounds[&id].phase, ReviewPhase::Incomplete);
            assert!(restored.rounds[&id].view().ballots.is_none());
            assert!(restored
                .complete(&id, &pending, "model-a", ballot(Vote::Pass), 104)
                .is_err());
            assert!(restored.reserve(&id, 104).is_err());
        }
        assert_eq!(
            crate::backup::verify(&archive).unwrap().manifest_digest,
            review.manifest_digest
        );
    }
}
#[test]
fn confirmation_requires_same_delivery_and_round() {
    let root = tempfile::tempdir().unwrap();
    let (mut l, id) = fixture(root.path());
    start(&mut l, &id);
    for _ in 0..4 {
        submit(&mut l, &id, Vote::Pass);
    }
    let r = l.rounds[&id].clone();
    assert_eq!(r.phase, ReviewPhase::AwaitingFunctionalTest);
    assert!(l.confirm("p1", &id, &r.digest, &r.binding).is_err());
    let (p, seat) = l
        .start_functional(&id, &r.digest, &r.binding, 103, 60)
        .unwrap();
    assert_eq!(seat.role, "chief");
    l.finish_functional(&id, &p, functional_outcome(&id), 104)
        .unwrap();
    let r = l.rounds[&id].clone();
    let key = r.confirmation_digest().unwrap();
    let mut changed = r.binding.clone();
    changed.delivery = "f".repeat(64);
    assert!(l.confirm("p1", &id, &r.digest, &changed).is_err());
    assert!(l.confirm("other", &id, &r.digest, &r.binding).is_err());
    assert!(l.confirm("p1", &id, &r.digest, &r.binding).is_err());
    l.confirm("p1", &id, &key, &r.binding).unwrap();
    assert!(l.confirm("p1", &id, &r.digest, &r.binding).is_err());
    drop(l);
    let restored = Ledger::open(root.path()).unwrap();
    assert_eq!(restored.rounds[&id].phase, ReviewPhase::Accepted);
}
fn functional_outcome(id: &str) -> FunctionalOutcome {
    use cerulean_domain::execution::{CheckObservation, SourceSnapshot, VerificationReport};
    FunctionalOutcome {
        model: "model-a".into(),
        authorization: FunctionalDecision {
            vote: Vote::Pass,
            reason: "Approved functional suite covers the requirements".into(),
        },
        report: Some(VerificationReport {
            schema_version: 1,
            verifier_id: "core".into(),
            observed_at: "2026-11-04T00:00:00Z".into(),
            manifest_digest: "a".repeat(64),
            source_snapshot: Some(SourceSnapshot {
                operation_id: format!("functional-{id}"),
                manifest_sha256: "b".repeat(64),
            }),
            checks: vec![CheckObservation {
                check_id: "check".into(),
                command_digest: "c".repeat(64),
                exit_code: Some(0),
                timed_out: false,
                assertions: vec![],
                assertion_error: None,
                stdout_sha256: "d".repeat(64),
                stderr_sha256: "e".repeat(64),
            }],
        }),
        decision: FunctionalDecision {
            vote: Vote::Pass,
            reason: "Actual functional checks passed".into(),
        },
    }
}
#[test]
fn functional_permission_is_one_shot_chief_only_and_bound_to_the_current_delivery() {
    let root = tempfile::tempdir().unwrap();
    let (mut l, id) = fixture(root.path());
    let r = l.rounds[&id].clone();
    assert!(l
        .start_functional(&id, &r.digest, &r.binding, 100, 60)
        .is_err());
    start(&mut l, &id);
    for _ in 0..4 {
        submit(&mut l, &id, Vote::Pass);
    }
    let r = l.rounds[&id].clone();
    let mut changed = r.binding.clone();
    changed.delivery = "f".repeat(64);
    assert!(l
        .start_functional(&id, &r.digest, &changed, 103, 60)
        .is_err());
    let (p, _) = l
        .start_functional(&id, &r.digest, &r.binding, 103, 60)
        .unwrap();
    assert!(l
        .start_functional(&id, &r.digest, &r.binding, 104, 60)
        .is_err());
    let mut wrong = p.clone();
    wrong.seat = "deputy".into();
    assert!(l
        .finish_functional(&id, &wrong, functional_outcome(&id), 104)
        .is_err());
    let mut wrong = functional_outcome(&id);
    wrong.model = "fallback".into();
    assert!(l.finish_functional(&id, &p, wrong, 104).is_err());
    let mut missing = functional_outcome(&id);
    missing.report = None;
    assert!(l.finish_functional(&id, &p, missing, 104).is_err());
    assert!(l
        .finish_functional(&id, &p, functional_outcome(&id), 1000)
        .is_err());
    l.finish_functional(&id, &p, functional_outcome(&id), 104)
        .unwrap();
    assert!(l
        .finish_functional(&id, &p, functional_outcome(&id), 105)
        .is_err());
}
#[test]
fn functional_cancellation_restart_and_failure_never_unlock_user_confirmation() {
    for mode in ["cancel", "restart", "fail"] {
        let root = tempfile::tempdir().unwrap();
        let (mut l, id) = fixture(root.path());
        start(&mut l, &id);
        for _ in 0..4 {
            submit(&mut l, &id, Vote::Pass);
        }
        let r = l.rounds[&id].clone();
        let (p, _) = l
            .start_functional(&id, &r.digest, &r.binding, 103, 60)
            .unwrap();
        match mode {
            "cancel" => l
                .end(&id, ReviewPhase::Cancelled, "User cancelled")
                .unwrap(),
            "restart" => {
                drop(l);
                l = Ledger::open(root.path()).unwrap();
            }
            _ => {
                let mut failed = functional_outcome(&id);
                failed.decision.vote = Vote::Fail;
                failed.report.as_mut().unwrap().checks[0].exit_code = Some(1);
                l.finish_functional(&id, &p, failed, 104).unwrap();
            }
        }
        assert!(l
            .finish_functional(&id, &p, functional_outcome(&id), 105)
            .is_err());
        assert!(l.confirm("p1", &id, &r.digest, &r.binding).is_err());
        assert!(l
            .start_functional(&id, &r.digest, &r.binding, 106, 60)
            .is_err());
        assert_eq!(l.rounds[&id].view().ballots.unwrap().len(), 3);
        assert!(l.rounds[&id].confirmation_digest().is_none());
    }
}
#[test]
fn cancellation_after_reveal_preserves_public_ballots_without_final_acceptance() {
    let root = tempfile::tempdir().unwrap();
    let (mut l, id) = fixture(root.path());
    start(&mut l, &id);
    for _ in 0..4 {
        submit(&mut l, &id, Vote::Pass);
    }
    let before = l.rounds[&id].view().ballots;
    l.end(
        &id,
        ReviewPhase::Cancelled,
        "User cancelled final confirmation",
    )
    .unwrap();
    assert_eq!(l.rounds[&id].view().ballots, before);
    let r = &l.rounds[&id];
    assert!(l
        .confirm("p1", &id, &r.digest.clone(), &r.binding.clone())
        .is_err());
}
#[test]
fn restart_revokes_requests_and_keeps_partial_votes_private() {
    let root = tempfile::tempdir().unwrap();
    let (mut l, id) = fixture(root.path());
    start(&mut l, &id);
    submit(&mut l, &id, Vote::Pass);
    submit(&mut l, &id, Vote::Pass);
    let (p, _, _) = l.reserve(&id, 103).unwrap().unwrap();
    drop(l);
    let mut reopened = Ledger::open(root.path()).unwrap();
    assert_eq!(reopened.rounds[&id].phase, ReviewPhase::Incomplete);
    assert!(reopened.rounds[&id].view().ballots.is_none());
    assert!(reopened
        .complete(&id, &p, "model-a", ballot(Vote::Pass), 104)
        .is_err());
    assert!(reopened.reserve(&id, 105).is_err());
}
#[test]
fn late_wrong_model_duplicate_and_forged_request_never_count() {
    let root = tempfile::tempdir().unwrap();
    let (mut l, id) = fixture(root.path());
    start(&mut l, &id);
    let (p, _, _) = l.reserve(&id, 101).unwrap().unwrap();
    assert!(l
        .complete(&id, &p, "fallback", ballot(Vote::Pass), 102)
        .is_err());
    let mut foreign = p.clone();
    foreign.epoch = "foreign".into();
    assert!(l
        .complete(&id, &foreign, "model-a", ballot(Vote::Pass), 102)
        .is_err());
    assert!(l
        .complete(&id, &p, "model-a", ballot(Vote::Pass), 1000)
        .is_err());
    l.complete(&id, &p, "model-a", ballot(Vote::Pass), 102)
        .unwrap();
    assert!(l
        .complete(&id, &p, "model-a", ballot(Vote::Pass), 103)
        .is_err());
    l.end(&id, ReviewPhase::Cancelled, "cancel").unwrap();
    assert!(l.reserve(&id, 104).is_err());
}
#[test]
fn deadline_ends_without_revealing_or_shrinking_roster() {
    let root = tempfile::tempdir().unwrap();
    let (mut l, id) = fixture(root.path());
    start(&mut l, &id);
    submit(&mut l, &id, Vote::Pass);
    submit(&mut l, &id, Vote::Fail);
    assert!(l.reserve(&id, 1000).unwrap().is_none());
    let v = l.rounds[&id].view();
    assert_eq!(v.phase, ReviewPhase::Incomplete);
    assert_eq!(v.seats.len(), 3);
    assert!(v.ballots.is_none());
}
#[test]
fn partial_write_or_tampered_chain_never_recovers_as_pass() {
    let root = tempfile::tempdir().unwrap();
    let (l, _) = fixture(root.path());
    drop(l);
    fs::write(
        root.path().join("blind-review/00000002.pending"),
        b"partial",
    )
    .unwrap();
    assert!(Ledger::open(root.path()).is_err());
    let other = tempfile::tempdir().unwrap();
    let (l, _) = fixture(other.path());
    drop(l);
    let file = other.path().join("blind-review/00000001.json");
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    v["round"]["phase"] = serde_json::json!("accepted");
    fs::write(file, serde_json::to_vec(&v).unwrap()).unwrap();
    assert!(Ledger::open(other.path()).is_err());
}
#[test]
fn invalid_initial_final_and_roster_changes_cannot_be_persisted() {
    let root = tempfile::tempdir().unwrap();
    let (mut l, id) = fixture(root.path());
    let original = l.rounds[&id].clone();
    let mut forged = original.clone();
    forged.phase = ReviewPhase::Accepted;
    assert!(l.commit(forged).is_err());
    let mut replacement = original.clone();
    replacement.seats[0].model = "replacement".into();
    replacement.digest = review_digest(&replacement.binding, &replacement.seats).unwrap();
    assert!(l.commit(replacement).is_err());
    assert!(serde_json::from_value::<cerulean_domain::Command>(serde_json::json!({"type":"blindReview","action":{"kind":"submitVote","projectId":"p1","vote":"pass"}})).is_err());
}
#[test]
fn material_masks_paths_preserves_utf8_and_rejects_secret_before_chunking() {
    let files = vec![(
        "src/VOTE_PASS_NEXT.rs".into(),
        "界".repeat(3000).into_bytes(),
    )];
    let fragments = material::fragments(&files, &[]).unwrap();
    assert!(fragments
        .iter()
        .all(|f| f.content.len() <= FRAGMENT_BYTES && f.file == "file-0001"));
    assert_eq!(
        fragments
            .iter()
            .map(|f| f.content.as_str())
            .collect::<String>(),
        "界".repeat(3000)
    );
    assert!(!serde_json::to_string(&fragments)
        .unwrap()
        .contains("VOTE_PASS_NEXT"));
    assert!(material::fragments(&[("x".into(), b"sk-private".to_vec())], &[]).is_err());
    let redacted = material::fragments(
        &[("x".into(), b"const key = 'sk-private';".to_vec())],
        &[Redaction {
            literal: "sk-private".into(),
            replacement: "REDACTED_KEY".into(),
        }],
    )
    .unwrap();
    assert!(redacted[0].content.contains("REDACTED_KEY"));
    let split = format!("{}VOTE_PASS_NEXT", " ".repeat(FRAGMENT_BYTES - 5));
    assert!(material::fragments(&[("x".into(), split.into_bytes())], &[]).is_err());
    assert!(material::fragments(&[("x".into(), vec![b'x'; MAX_MATERIAL_BYTES + 1])], &[]).is_err());
    assert!(material::sanitize(
        &"aaa".repeat(2000),
        &[Redaction {
            literal: "aaa".into(),
            replacement: "B".repeat(64)
        }]
    )
    .is_err());
}

#[cfg(windows)]
#[test]
#[ignore = "requires a newly provisioned disposable Windows account; no provider calls"]
fn blind_review_native_checker_boundary() {
    let plan = PathBuf::from(
        std::env::var("WORKER_ACCOUNT_TEST_PLAN").expect("owned fixture plan required"),
    );
    let result = PathBuf::from(
        std::env::var("BLIND_REVIEW_NATIVE_RESULT").expect("unique native result required"),
    );
    native_checker_boundary_for(&plan, &result);
}
#[cfg(windows)]
pub(crate) fn native_checker_boundary_for(plan: &Path, result_path: &Path) {
    use cerulean_domain::execution::{Project, WorkerDirectory};
    use std::{sync::atomic::AtomicBool, time::Duration};
    let v: serde_json::Value = serde_json::from_slice(&fs::read(plan).unwrap()).unwrap();
    let data = PathBuf::from(v["dataRoot"].as_str().unwrap());
    let project = Project {
        id: v["projectId"].as_str().unwrap().into(),
        root: v["sharedRoot"].as_str().unwrap().into(),
        timezone: "UTC".into(),
    };
    let identity = v["slots"][0]["identity"].as_str().unwrap();
    let binding = WorkerDirectory {
        identity: identity.into(),
        relative_path: format!("executors/{identity}"),
    };
    let account = crate::windows_worker_account::load_binding(&data, &project, &binding).unwrap();
    let label = result_path.file_stem().unwrap().to_str().unwrap();
    assert!(
        label.len() <= 80
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    );
    assert!(!result_path.exists());
    // A new diagnostic after a fixture correction uses its own work/vault and
    // output paths. Never overwrite or rerun an earlier boundary transaction.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let pipe = crate::ipc::endpoint(&data).unwrap();
    let _listener = {
        let _entered = runtime.enter();
        crate::ipc::private_listener_for_test(&pipe).unwrap()
    };
    let vault = data.join(format!("{label}-private-vault"));
    crate::windows_worker_account::private_directory(&vault, &account.record.owner_sid).unwrap();
    fs::write(vault.join("ballots.json"), b"sealed synthetic ballot").unwrap();
    let operation = data.join(format!("{label}-verification"));
    fs::create_dir(&operation).unwrap();
    let work = operation.join("work");
    let temp = operation.join("temp");
    fs::create_dir(&work).unwrap();
    fs::create_dir(&temp).unwrap();
    fs::write(work.join("input.txt"), b"approved input").unwrap();
    let script = work.join("check.ps1");
    fs::write(work.join("child.ps1"),r#"param([string]$Vault,[string]$ExpectedSid)
$ErrorActionPreference='Stop'
if ([Security.Principal.WindowsIdentity]::GetCurrent().User.Value -cne $ExpectedSid) {throw 'child identity mismatch'}
try { [void][IO.File]::ReadAllText((Join-Path $Vault 'ballots.json')); exit 8 } catch [UnauthorizedAccessException] { Write-Output 'CHILD-DENIED'; exit 0 }
"#).unwrap();
    let source = r#"param([string]$Vault,[string]$InputRoot,[string]$PipeName)
$ErrorActionPreference='Stop'
if ([IO.File]::ReadAllText((Join-Path $InputRoot 'input.txt')) -ne 'approved input') {throw 'input mismatch'}
$denied=$false
try {[void][IO.File]::ReadAllText((Join-Path $Vault 'ballots.json'))} catch [UnauthorizedAccessException] {$denied=$true}
if (-not $denied) {throw 'private vote read was not denied'}
$writeDenied=$false
try {[IO.File]::WriteAllText((Join-Path $Vault 'forged.json'),'forged')} catch [UnauthorizedAccessException] {$writeDenied=$true}
if (-not $writeDenied) {throw 'private vote write was not denied'}
$pipeDenied=$false
$client=[IO.Pipes.NamedPipeClientStream]::new('.', $PipeName, [IO.Pipes.PipeDirection]::InOut)
try { $client.Connect(3000) } catch [UnauthorizedAccessException] { $pipeDenied=$true } finally { $client.Dispose() }
if (-not $pipeDenied) {throw 'core IPC was not denied'}
$childInfo=New-Object Diagnostics.ProcessStartInfo
$childInfo.FileName=[IO.Path]::Combine($env:WINDIR,'System32\WindowsPowerShell\v1.0\powershell.exe')
$childInfo.UseShellExecute=$false
$childInfo.CreateNoWindow=$true
$childInfo.RedirectStandardOutput=$true
$childInfo.RedirectStandardError=$true
$childInfo.WorkingDirectory=[Environment]::CurrentDirectory
$childInfo.Arguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+[IO.Path]::Combine($InputRoot,'child.ps1')+'" -Vault "'+$Vault+'" -ExpectedSid '+[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$child=[Diagnostics.Process]::Start($childInfo)
try {
    $childOut=$child.StandardOutput.ReadToEndAsync()
    $childErr=$child.StandardError.ReadToEndAsync()
    if (-not $child.WaitForExit(10000)) {throw 'child did not exit within bound'}
    $childText=$childOut.GetAwaiter().GetResult()
    $childError=$childErr.GetAwaiter().GetResult()
    if ($child.ExitCode -ne 0 -or $childText.Trim() -cne 'CHILD-DENIED' -or $childError.Length -ne 0) {throw ('child denial check failed: '+$child.ExitCode+' '+$childText+' '+$childError)}
    [Console]::WriteLine($childText.Trim())
} finally { $child.Dispose() }
[IO.File]::WriteAllText((Join-Path $InputRoot 'own-output.txt'),'allowed')
Write-Output 'ISOLATED-CHECK-PASS'
"#;
    fs::write(&script, source).unwrap();
    // The script is created after workspace protection, and inherits only this account's scope.
    let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
    let exe = system.join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let args = vec![
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-File".into(),
        crate::windows_worker_account::dos_path(&script)
            .unwrap()
            .to_string_lossy()
            .into(),
        "-Vault".into(),
        crate::windows_worker_account::dos_path(&vault)
            .unwrap()
            .to_string_lossy()
            .into(),
        "-InputRoot".into(),
        crate::windows_worker_account::dos_path(&work)
            .unwrap()
            .to_string_lossy()
            .into(),
        "-PipeName".into(),
        pipe.trim_start_matches(r"\\.\pipe\").into(),
    ];
    let outcome = crate::windows_worker_account::with_check_workspaces(
        &[work.as_path(), temp.as_path()],
        &account,
        || {
            crate::verifier::process::run_isolated(
                &exe,
                &args,
                &work,
                &temp,
                Duration::from_secs(30),
                &AtomicBool::new(false),
                &BTreeMap::new(),
                None,
                &account,
            )
        },
    )
    .unwrap();
    crate::windows_worker_account::with_identity(&account, || {
        assert_eq!(
            fs::read(work.join("input.txt")).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        Ok(())
    })
    .unwrap();
    let result = serde_json::json!({"exitCode":outcome.exit_code,"timedOut":outcome.timed_out,"failure":outcome.failure,
        "stdout":String::from_utf8_lossy(&outcome.stdout),"stderr":String::from_utf8_lossy(&outcome.stderr),"sid":account.record.sid,
        "privateVoteUnchanged":fs::read(vault.join("ballots.json")).unwrap()==b"sealed synthetic ballot", "forgedVoteExists":vault.join("forged.json").exists()});
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(result_path)
        .unwrap();
    output
        .write_all(serde_json::to_string_pretty(&result).unwrap().as_bytes())
        .unwrap();
    output.sync_all().unwrap();
    assert_eq!(outcome.exit_code, 0, "{result}");
    assert!(!outcome.timed_out);
    assert!(outcome.failure.is_none());
    assert!(String::from_utf8_lossy(&outcome.stdout).contains("ISOLATED-CHECK-PASS"));
    assert!(!vault.join("forged.json").exists());
    assert_eq!(fs::read(work.join("own-output.txt")).unwrap(), b"allowed");
}

#[cfg(windows)]
#[test]
#[ignore = "read-only reopening of the existing accepted native fixture; no UAC, models, or check rerun"]
fn native_accepted_project_reopens_without_dispatch() {
    let root = PathBuf::from(std::env::var_os("BLIND_REVIEW_HOST_NATIVE_ROOT").unwrap());
    let output = root.join("accepted-reopen-771.json");
    let host = crate::host::RuntimeHost::open(&root.join("data")).unwrap();
    let snapshot = host.snapshot();
    assert_eq!(snapshot.blind_reviews.len(), 1);
    let review = &snapshot.blind_reviews[0];
    assert!(review.final_accepted && review.is_current);
    let value = serde_json::json!({"finalAccepted":review.final_accepted,"round":review.round_id,"digest":review.digest,"isCurrent":review.is_current,"liveProviderRequests":0});
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)
        .unwrap()
        .write_all(serde_json::to_string_pretty(&value).unwrap().as_bytes())
        .unwrap();
}
