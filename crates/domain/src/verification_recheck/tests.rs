use super::*;
use serde_json::json;

#[derive(Clone)]
struct Fixture {
    plan: PlanRevision,
    manifest: ArtifactManifest,
    job: VerificationJob,
    report: VerificationReport,
    request: RecheckRequest,
    head: String,
}

impl Fixture {
    fn new() -> Self {
        // All data is synthetic. These tests neither open files nor run tools.
        let exe = std::env::temp_dir().join("recheck-fixture.exe");
        let check = |id: &str| AcceptanceCheck {
            id: id.into(),
            name: Some(format!("Stage {id}")),
            program: "fixture".into(),
            args: vec![id.into()],
            timeout_seconds: 10,
            tool: Some(ValidationTool {
                executable: ValidationFile {
                    path: exe.to_string_lossy().into(),
                    sha256: "a".repeat(64),
                },
                inputs: vec![],
                environment: Default::default(),
            }),
        };
        let plan: PlanRevision = serde_json::from_value(json!({
            "schemaVersion": 1,
            "snapshotPolicy": {"exclude":[], "maxFiles":10, "maxBytes":1048576},
            "project": {"id":"project", "root":std::env::temp_dir(), "timezone":"Asia/Shanghai"},
            "workflow": {"id":"plan", "version":1, "tasks":[{
                "id":"task", "dependencies":[], "workspaceKey":"project", "goal":"Produce result"
            }]},
            "policy": {"version":1, "tools":["read-files","write-files","run-commands"],
                "maxRequests":3, "maxSeconds":60, "budgetUsdCents":25},
            "execution":[{"taskId":"task", "adapterId":"codex", "artifactPaths":["result.txt"],
                "checks":[check("syntax"),check("behavior")]}]
        }))
        .unwrap();
        let manifest = ArtifactManifest {
            schema_version: 1,
            revision_digest: plan.approval_digest().unwrap(),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            receipt_digest: "b".repeat(64),
            artifacts: vec![Artifact {
                path: "result.txt".into(),
                bytes: 3,
                sha256: "c".repeat(64),
            }],
        };
        let job = VerificationJob {
            project_id: "project".into(),
            operation_id: "original".into(),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            state: VerificationState::Failed,
            started_at: "2026-10-30T01:00:00Z".into(),
            updated_at: "2026-10-30T01:00:10Z".into(),
            message: None,
            reconciliation_digest: None,
        };
        let report = VerificationReport {
            schema_version: 1,
            verifier_id: "core".into(),
            observed_at: "2026-10-30T01:00:05Z".into(),
            manifest_digest: digest(&manifest).unwrap(),
            checks: plan.execution[0]
                .checks
                .iter()
                .map(|c| CheckObservation {
                    check_id: c.id.clone(),
                    command_digest: digest(c).unwrap(),
                    exit_code: Some(1),
                    timed_out: false,
                    stdout_sha256: "d".repeat(64),
                    stderr_sha256: "e".repeat(64),
                })
                .collect(),
            source_snapshot: Some(SourceSnapshot {
                operation_id: "original".into(),
                manifest_sha256: "f".repeat(64),
            }),
        };
        let mut replacement_checks = plan.execution[0].checks.clone();
        replacement_checks[1]
            .tool
            .as_mut()
            .unwrap()
            .executable
            .sha256 = "1".repeat(64);
        let request = RecheckRequest {
            project_id: "project".into(),
            original_operation_id: "original".into(),
            replacement_checks,
            reason: "Correct Windows file URI handling".into(),
        };
        Self {
            plan,
            manifest,
            job,
            report,
            request,
            head: "2".repeat(64),
        }
    }
    fn basis(&self) -> RecheckBasis<'_> {
        RecheckBasis {
            plan: &self.plan,
            manifest: &self.manifest,
            original_job: &self.job,
            original_report: &self.report,
            ledger_head: &self.head,
            trusted_verifier: "core",
        }
    }
    fn review(&self) -> RecheckReview {
        self.request.review(&self.basis()).unwrap()
    }
    fn result(&self, review: &RecheckReview) -> RecheckResult {
        let mut report = self.report.clone();
        report.observed_at = "2026-10-30T01:01:00Z".into();
        report.source_snapshot.as_mut().unwrap().operation_id = "recheck".into();
        for (observed, check) in report.checks.iter_mut().zip(&review.replacement_checks) {
            observed.command_digest = digest(check).unwrap();
            observed.exit_code = Some(0);
        }
        RecheckResult {
            schema_version: 1,
            operation_id: "recheck".into(),
            review_digest: digest(review).unwrap(),
            report,
        }
    }
    fn original_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&(&self.plan, &self.manifest, &self.job, &self.report)).unwrap()
    }
}

#[test]
fn correction_preserves_original_and_cannot_masquerade_as_original_pass() {
    let f = Fixture::new();
    let before = f.original_bytes();
    let review = f.review();
    review.require_current(&f.basis()).unwrap();
    let result = f.result(&review);
    assert_eq!(result.validate(&review, &f.basis()), Ok(true));
    assert!(result
        .report
        .require_pass(&f.plan, &f.manifest, "core")
        .is_err());
    assert!(f.report.require_pass(&f.plan, &f.manifest, "core").is_err());
    assert_eq!(before, f.original_bytes());
}

#[test]
fn complete_failure_and_partial_timeout_remain_failed_results() {
    let f = Fixture::new();
    let review = f.review();
    let mut result = f.result(&review);
    result.report.checks[1].exit_code = Some(1);
    assert_eq!(result.validate(&review, &f.basis()), Ok(false));
    result.report.checks.truncate(1);
    result.report.checks[0].timed_out = true;
    assert_eq!(result.validate(&review, &f.basis()), Ok(false));
    result.report.checks[0].timed_out = false;
    assert!(result.validate(&review, &f.basis()).is_err());
}

#[test]
fn original_must_be_observed_terminal_failure_not_unknown_or_timeout() {
    let mutations: &[fn(&mut Fixture)] = &[
        |f| f.plan.snapshot_policy = None,
        |f| f.job.state = VerificationState::Unknown,
        |f| f.job.state = VerificationState::Running,
        |f| f.job.state = VerificationState::Succeeded,
        |f| {
            f.report
                .checks
                .iter_mut()
                .for_each(|c| c.exit_code = Some(0))
        },
        |f| f.report.checks[0].exit_code = None,
        |f| f.report.checks[1].timed_out = true,
        |f| {
            f.report.checks.pop();
        },
        |f| f.report.checks.clear(),
        |f| f.report.source_snapshot = None,
        |f| f.report.source_snapshot.as_mut().unwrap().operation_id = "elsewhere".into(),
        |f| f.report.source_snapshot.as_mut().unwrap().manifest_sha256 = "invalid".into(),
        |f| f.report.observed_at = "2026-10-30T00:59:59Z".into(),
        |f| f.report.observed_at = "2026-10-30T01:00:11Z".into(),
        |f| f.job.updated_at = "invalid".into(),
    ];
    for (index, change) in mutations.iter().enumerate() {
        let mut f = Fixture::new();
        change(&mut f);
        assert!(f.request.review(&f.basis()).is_err(), "mutation {index}");
    }
}

#[test]
fn cross_project_attempt_manifest_command_and_log_mismatches_are_rejected() {
    let mutations: &[fn(&mut Fixture)] = &[
        |f| f.request.project_id = "other".into(),
        |f| f.request.original_operation_id = "other".into(),
        |f| f.job.project_id = "other".into(),
        |f| f.job.task_id = "other".into(),
        |f| f.job.attempt_id = "other".into(),
        |f| f.manifest.schema_version = 2,
        |f| f.manifest.revision_digest = "0".repeat(64),
        |f| f.manifest.receipt_digest = "bad".into(),
        |f| f.manifest.artifacts.push(f.manifest.artifacts[0].clone()),
        |f| f.manifest.artifacts[0].path = "other.txt".into(),
        |f| f.manifest.artifacts[0].sha256 = "bad".into(),
        |f| f.report.verifier_id = "worker".into(),
        |f| f.report.manifest_digest = "0".repeat(64),
        |f| f.report.checks.swap(0, 1),
        |f| f.report.checks[0].command_digest = "0".repeat(64),
        |f| f.report.checks[0].stdout_sha256 = "bad".into(),
        |f| f.report.checks[0].stderr_sha256 = "bad".into(),
        |f| f.head = "bad".into(),
    ];
    for (index, change) in mutations.iter().enumerate() {
        let mut f = Fixture::new();
        change(&mut f);
        assert!(f.request.review(&f.basis()).is_err(), "mutation {index}");
    }
}

#[test]
fn replacement_cannot_remove_reorder_rename_or_bypass_original_policy() {
    let mutations: &[fn(&mut Fixture)] = &[
        |f| f.request.replacement_checks = f.plan.execution[0].checks.clone(),
        |f| {
            f.request.replacement_checks.pop();
        },
        |f| f.request.replacement_checks.swap(0, 1),
        |f| f.request.replacement_checks[0].id = "other".into(),
        |f| f.request.replacement_checks[0].name = Some("Other".into()),
        |f| f.request.replacement_checks[0].tool = None,
        |f| {
            f.request.replacement_checks[0]
                .tool
                .as_mut()
                .unwrap()
                .executable
                .sha256 = "bad".into()
        },
        |f| {
            f.request.replacement_checks[0]
                .tool
                .as_mut()
                .unwrap()
                .environment
                .insert("PATH".into(), "other".into());
        },
        |f| f.request.replacement_checks[0].timeout_seconds = 61,
        |f| f.request.replacement_checks[0].timeout_seconds = 0,
        |f| f.request.replacement_checks[0].args.push("bad\0arg".into()),
        |f| f.request.reason.clear(),
        |f| f.request.reason = " ".into(),
        |f| f.request.reason = "a".repeat(2001),
        |f| f.request.reason = "bad\0reason".into(),
    ];
    for (index, change) in mutations.iter().enumerate() {
        let mut f = Fixture::new();
        change(&mut f);
        assert!(f.request.review(&f.basis()).is_err(), "mutation {index}");
    }
}

#[test]
fn changed_host_facts_invalidate_review_even_if_individually_valid() {
    let mutations: &[fn(&mut Fixture)] = &[
        |f| f.head = "3".repeat(64),
        |f| f.report.checks[0].stdout_sha256 = "3".repeat(64),
        |f| f.report.source_snapshot.as_mut().unwrap().manifest_sha256 = "3".repeat(64),
        |f| {
            f.manifest.artifacts[0].sha256 = "3".repeat(64);
            f.report.manifest_digest = digest(&f.manifest).unwrap();
        },
        |f| {
            f.plan.execution[0].checks[0].args.push("--changed".into());
            f.manifest.revision_digest = f.plan.approval_digest().unwrap();
            f.report.manifest_digest = digest(&f.manifest).unwrap();
            f.report.checks[0].command_digest = digest(&f.plan.execution[0].checks[0]).unwrap();
        },
    ];
    for (index, change) in mutations.iter().enumerate() {
        let mut f = Fixture::new();
        let review = f.review();
        change(&mut f);
        assert!(
            review.require_current(&f.basis()).is_err(),
            "mutation {index}"
        );
    }
}

#[test]
fn result_must_bind_distinct_operation_and_exact_approved_review() {
    let mutations: &[fn(&mut RecheckResult)] = &[
        |r| r.report.source_snapshot.as_mut().unwrap().manifest_sha256 = "0".repeat(64),
        |r| r.schema_version = 2,
        |r| r.operation_id = "original".into(),
        |r| r.review_digest = "0".repeat(64),
        |r| r.report.verifier_id = "worker".into(),
        |r| r.report.schema_version = 2,
        |r| r.report.source_snapshot = None,
        |r| r.report.source_snapshot.as_mut().unwrap().operation_id = "original".into(),
        |r| r.report.checks[0].exit_code = None,
        |r| r.report.checks[0].timed_out = true,
        |r| r.report.checks.swap(0, 1),
        |r| r.report.checks.clear(),
        |r| r.report.checks.push(r.report.checks[0].clone()),
        |r| r.report.observed_at = "2026-10-30T01:00:04Z".into(),
    ];
    let f = Fixture::new();
    let review = f.review();
    for (index, change) in mutations.iter().enumerate() {
        let mut result = f.result(&review);
        change(&mut result);
        assert!(
            result.validate(&review, &f.basis()).is_err(),
            "mutation {index}"
        );
    }
    let result = f.result(&review);
    let mut changed = review.clone();
    changed.reason.push_str(" revised");
    assert!(result.validate(&changed, &f.basis()).is_err());
    let mut changed = review.clone();
    changed.replacement_checks[0].args.push("--changed".into());
    assert!(result.validate(&changed, &f.basis()).is_err());
}

#[test]
fn serialized_review_does_not_accept_unknown_authority_fields() {
    fn roundtrip_strict<T>(value: &T)
    where
        T: Serialize + serde::de::DeserializeOwned,
    {
        let mut json = serde_json::to_value(value).unwrap();
        assert!(serde_json::from_value::<T>(json.clone()).is_ok());
        json["autoAccept"] = json!(true);
        assert!(serde_json::from_value::<T>(json).is_err());
    }
    let f = Fixture::new();
    let review = f.review();
    roundtrip_strict(&f.request);
    roundtrip_strict(&review);
    roundtrip_strict(&f.result(&review));
    roundtrip_strict(&RecheckApproval {
        schema_version: 1,
        operation_id: "recheck".into(),
        review,
        original_job: f.job.clone(),
    });
}

#[test]
fn approval_binds_original_job_and_distinct_single_operation() {
    let f = Fixture::new();
    let approval = RecheckApproval {
        schema_version: 1,
        operation_id: "recheck".into(),
        review: f.review(),
        original_job: f.job.clone(),
    };
    approval.validate(&f.basis()).unwrap();
    let mutations: &[fn(&mut RecheckApproval)] = &[
        |a| a.schema_version = 2,
        |a| a.operation_id = "original".into(),
        |a| a.operation_id = "../other".into(),
        |a| a.original_job.operation_id = "other".into(),
        |a| a.original_job.state = VerificationState::Succeeded,
        |a| a.original_job.updated_at = "2026-10-30T01:01:00Z".into(),
        |a| a.review.ledger_head = "3".repeat(64),
        |a| a.original_job.message = Some("x".repeat(192 * 1024)),
    ];
    for (i, mutate) in mutations.iter().enumerate() {
        let mut a = approval.clone();
        mutate(&mut a);
        // The host-captured job is an input to validation, never request authority.
        assert!(a.validate(&f.basis()).is_err(), "mutation {i}");
    }
}

#[test]
fn review_and_approval_have_independent_serialized_size_limits() {
    let mut f = Fixture::new();
    let mut a = RecheckApproval {
        schema_version: 1,
        operation_id: "recheck".into(),
        review: f.review(),
        original_job: f.job.clone(),
    };
    f.job.message = Some("x".repeat(192 * 1024));
    a.original_job = f.job.clone();
    assert!(a
        .validate(&f.basis())
        .unwrap_err()
        .contains("evidence limit"));
    // Each argument remains within the original plan policy, but the approval
    // envelope cannot grow without bound via many otherwise valid arguments.
    f.request.replacement_checks[0].args = vec!["x".repeat(4000); 40];
    assert!(f
        .request
        .review(&f.basis())
        .unwrap_err()
        .contains("review exceeds"));
}
