use cerulean_core::artifacts::ArtifactObserver;
use cerulean_domain::{execution::*, workflow::*};
use std::fs;

struct Fixture {
    root: tempfile::TempDir,
    plan: PlanRevision,
    approval: Approval,
    attempt: Attempt,
    receipt: ExecutionReceipt,
}

#[test]
fn omp_controller_identity_is_bound_without_inventing_a_provider_turn() {
    assert_eq!(
        serde_json::to_string(&IdentityStrength::ProviderId).unwrap(),
        "\"provider-id\""
    );
    assert_eq!(
        serde_json::to_string(&IdentityStrength::UiObservation).unwrap(),
        "\"ui-observation\""
    );
    let mut f = Fixture::new();
    f.plan.execution[0].adapter_id = "omp".into();
    f.attempt.revision_digest = f.plan.approval_digest().unwrap();
    f.attempt.binding.adapter_id = "omp".into();
    f.attempt.binding.identity_strength =
        IdentityStrength::ControllerRequest(ControllerRequestIdentity {
            connection_id: "connection".into(),
            request_id: "request".into(),
        });
    f.receipt.binding = f.attempt.binding.clone();
    f.receipt.turn_id = None;
    f.receipt.validate(&f.plan, &f.attempt, None).unwrap();
    let wire = serde_json::to_value(&f.receipt).unwrap();
    assert_eq!(
        wire["binding"]["identityStrength"],
        serde_json::json!({"controller-request":{"connectionId":"connection","requestId":"request"}})
    );
    assert_eq!(
        serde_json::from_value::<ExecutionReceipt>(wire.clone()).unwrap(),
        f.receipt
    );
    for (field, value) in [
        ("connectionId", "other"),
        ("requestId", "other"),
        ("requestId", ""),
    ] {
        let mut forged = wire.clone();
        forged["binding"]["identityStrength"]["controller-request"][field] = value.into();
        let forged: ExecutionReceipt = serde_json::from_value(forged).unwrap();
        assert!(forged.validate(&f.plan, &f.attempt, None).is_err());
    }
    let mut extra = wire;
    extra["binding"]["identityStrength"]["controller-request"]["providerTurn"] = true.into();
    assert!(serde_json::from_value::<ExecutionReceipt>(extra).is_err());
    f.receipt.turn_id = Some("request".into());
    assert!(f
        .receipt
        .validate(&f.plan, &f.attempt, Some("request"))
        .is_err());
    f.receipt.turn_id = None;
    f.plan.execution[0].adapter_id = "codex".into();
    f.attempt.binding.adapter_id = "codex".into();
    f.receipt.binding = f.attempt.binding.clone();
    f.attempt.revision_digest = f.plan.approval_digest().unwrap();
    assert!(f.receipt.validate(&f.plan, &f.attempt, None).is_err());
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/result.txt"), "abc").unwrap();
        let plan = PlanRevision {
            management_setup_digest: None,
            worktree: None,
            snapshot_policy: None,
            schema_version: 1,
            project: Project {
                id: "project".into(),
                root: root.path().to_string_lossy().into(),
                timezone: "Asia/Shanghai".into(),
            },
            workflow: WorkflowPlan {
                id: "plan".into(),
                version: 1,
                tasks: vec![WorkflowTask {
                    id: "task".into(),
                    dependencies: vec![],
                    workspace_key: "project".into(),
                    goal: "Produce result".into(),
                    resource_keys: None,
                }],
            },
            policy: ExecutionPolicy {
                supervision: None,
                version: 1,
                tools: vec![
                    ToolPermission::ReadFiles,
                    ToolPermission::WriteFiles,
                    ToolPermission::RunCommands,
                ],
                max_requests: 3,
                max_seconds: 60,
                currency_budget: None,
                budget_usd_cents: Some(25),
            },
            execution: vec![TaskExecution {
                worker_directory: None,
                supervision_override: None,
                profile_id: None,
                desktop_worker: None,
                api_worker: None,
                worker: None,
                task_id: "task".into(),
                adapter_id: "codex".into(),
                artifact_paths: vec!["src/result.txt".into()],
                checks: vec![AcceptanceCheck {
                    expected_assertions: vec![],
                    name: None,
                    tool: None,
                    id: "check".into(),
                    program: "test-runner".into(),
                    args: vec!["--check".into()],
                    timeout_seconds: 10,
                }],
            }],
        };
        let approval = Approval {
            worker_account_setup: None,
            plan_id: "plan".into(),
            revision: 1,
            policy_version: 1,
            revision_digest: plan.approval_digest().unwrap(),
            user_id: "user".into(),
            approved_at: "2026-10-07T14:00:00Z".into(),
        };
        let binding = SessionBinding {
            adapter_id: "codex".into(),
            adapter_version: "fixture-1".into(),
            session_id: "session-1".into(),
            identity_strength: IdentityStrength::ProviderId,
            workspace: plan.project.root.clone(),
        };
        let attempt = Attempt {
            workspace_proof: None,
            id: "attempt".into(),
            task_id: "task".into(),
            worker_id: "worker".into(),
            revision_digest: approval.revision_digest.clone(),
            fence: 1,
            binding: binding.clone(),
        };
        let receipt = ExecutionReceipt {
            schema_version: 1,
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            fence: 1,
            binding,
            turn_id: Some("turn".into()),
            observed_at: "2026-10-07T14:01:00Z".into(),
            status: ReceiptStatus::Completed,
            evidence_sha256: "a".repeat(64),
        };
        Self {
            root,
            plan,
            approval,
            attempt,
            receipt,
        }
    }
    fn observer(&self) -> Result<ArtifactObserver<'_>, String> {
        ArtifactObserver::new(
            &self.plan,
            &self.approval,
            "user",
            1,
            &self.attempt,
            &self.receipt,
            Some("turn"),
        )
    }
}

#[test]
fn approval_covers_directory_budget_permissions_executor_and_checks() {
    let f = Fixture::new();
    f.approval.validate(&f.plan, "user", 1).unwrap();
    assert!(f.approval.validate(&f.plan, "worker", 1).is_err());
    assert!(f.approval.validate(&f.plan, "user", 2).is_err());
    let mut changed = Vec::new();
    let mut p = f.plan.clone();
    p.project.root.push_str("/other");
    changed.push(p);
    let mut p = f.plan.clone();
    p.policy.max_requests += 1;
    changed.push(p);
    let mut p = f.plan.clone();
    p.policy.budget_usd_cents = Some(100);
    changed.push(p);
    let mut p = f.plan.clone();
    p.policy.tools.push(ToolPermission::Network);
    changed.push(p);
    let mut p = f.plan.clone();
    p.execution[0].adapter_id = "omp".into();
    changed.push(p);
    let mut p = f.plan.clone();
    p.execution[0].checks[0].args.push("--skip".into());
    changed.push(p);
    let mut p = f.plan.clone();
    p.execution[0].artifact_paths.push("other.txt".into());
    changed.push(p);
    let mut p = f.plan.clone();
    p.workflow.version = 2;
    changed.push(p);
    for plan in changed {
        assert!(f.approval.validate(&plan, "user", 1).is_err());
    }
    assert_ne!(
        f.approval.revision_digest,
        workflow_digest(&f.plan.workflow).unwrap()
    );
}

#[test]
fn malformed_contracts_and_unsafe_windows_paths_are_rejected() {
    for path in [
        "../x",
        "/x",
        "C:/x",
        "a\\b",
        "x:stream",
        "a/./b",
        "a//b",
        "NUL.txt",
        "CON .txt",
        "COM1",
        "LPT².log",
        "a/dir.",
        "a/dir ",
        "a\0b",
    ] {
        assert!(artifact_path(path).is_err(), "{path}");
    }
    artifact_path("src/有效文件.test.ts").unwrap();
    let f = Fixture::new();
    let mut p = f.plan.clone();
    p.execution[0].artifact_paths.push("SRC/RESULT.TXT".into());
    assert!(p.validate().is_err());
    let mut p = f.plan.clone();
    p.execution[0].checks.clear();
    assert!(p.validate().is_err());
    let mut p = f.plan.clone();
    p.execution[0].checks[0].timeout_seconds = 0;
    assert!(p.validate().is_err());
    let mut p = f.plan.clone();
    p.execution[0].task_id = "other".into();
    assert!(p.validate().is_err());
    let mut approval = f.approval.clone();
    approval.approved_at = "yesterday".into();
    assert!(approval.validate(&f.plan, "user", 1).is_err());
    let mut value = serde_json::to_value(&f.receipt).unwrap();
    value["assumeApproved"] = true.into();
    assert!(serde_json::from_value::<ExecutionReceipt>(value).is_err());
}

#[test]
fn receipt_cannot_cross_sessions_turns_attempts_or_policy_revisions() {
    let f = Fixture::new();
    f.receipt
        .validate(&f.plan, &f.attempt, Some("turn"))
        .unwrap();
    let mut bad = Vec::new();
    let mut r = f.receipt.clone();
    r.binding.session_id = "other".into();
    bad.push(r);
    let mut r = f.receipt.clone();
    r.binding.adapter_version = "other".into();
    bad.push(r);
    let mut r = f.receipt.clone();
    r.turn_id = Some("other".into());
    bad.push(r);
    let mut r = f.receipt.clone();
    r.turn_id = None;
    bad.push(r);
    let mut r = f.receipt.clone();
    r.attempt_id = "other".into();
    bad.push(r);
    let mut r = f.receipt.clone();
    r.fence = 2;
    bad.push(r);
    let mut r = f.receipt.clone();
    r.evidence_sha256 = "not-a-hash".into();
    bad.push(r);
    for receipt in bad {
        assert!(receipt.validate(&f.plan, &f.attempt, Some("turn")).is_err());
    }
    let mut attempt = f.attempt.clone();
    attempt.revision_digest = "a".repeat(64);
    assert!(f.receipt.validate(&f.plan, &attempt, Some("turn")).is_err());
    for status in [
        ReceiptStatus::Running,
        ReceiptStatus::Unknown,
        ReceiptStatus::Cancelled,
        ReceiptStatus::Failed,
    ] {
        let receipt = ExecutionReceipt {
            status,
            ..f.receipt.clone()
        };
        assert!(ArtifactObserver::new(
            &f.plan,
            &f.approval,
            "user",
            1,
            &f.attempt,
            &receipt,
            Some("turn")
        )
        .is_err());
    }
}

#[test]
fn real_file_hash_recheck_rejects_same_size_changes_deletions_and_forged_manifests() {
    let f = Fixture::new();
    let observer = f.observer().unwrap();
    let manifest = observer.capture().unwrap();
    assert_eq!(manifest.artifacts[0].bytes, 3);
    assert_eq!(
        manifest.artifacts[0].sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    observer.require_unchanged(&manifest).unwrap();
    let mut forged = manifest.clone();
    forged.attempt_id = "other".into();
    assert!(observer.require_unchanged(&forged).is_err());
    let mut forged = manifest.clone();
    forged.artifacts.clear();
    assert!(observer.require_unchanged(&forged).is_err());
    fs::write(f.root.path().join("src/result.txt"), "xyz").unwrap();
    assert!(observer.require_unchanged(&manifest).is_err());
    fs::remove_file(f.root.path().join("src/result.txt")).unwrap();
    assert!(observer.require_unchanged(&manifest).is_err());
}

#[cfg(windows)]
#[test]
fn junction_inside_artifact_path_is_rejected_without_reading_target() {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    let mut f = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("result.txt"), "outside sentinel").unwrap();
    let link = f.root.path().join("linked");
    let output=Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",
        "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:CERULEAN_LINK_TEST -Target $env:CERULEAN_TARGET_TEST | Out-Null"])
        .env("CERULEAN_LINK_TEST",&link).env("CERULEAN_TARGET_TEST",outside.path())
        .creation_flags(0x08000000).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    f.plan.execution[0].artifact_paths = vec!["linked/result.txt".into()];
    f.approval.revision_digest = f.plan.approval_digest().unwrap();
    f.attempt.revision_digest = f.approval.revision_digest.clone();
    let rejection = f.observer().unwrap().capture().unwrap_err();
    assert!(
        matches!(
            rejection.as_str(),
            "linked artifact path" | "reparse artifact path"
        ),
        "{rejection}"
    );
    // Remove this test-owned junction itself, never traverse or remove its target.
    fs::remove_dir(&link).unwrap();
    assert_eq!(
        fs::read_to_string(outside.path().join("result.txt")).unwrap(),
        "outside sentinel"
    );
}

#[test]
fn acceptance_requires_all_approved_checks_and_current_artifact_content() {
    let f = Fixture::new();
    let observer = f.observer().unwrap();
    let manifest = observer.capture().unwrap();
    // These are protocol fixtures, not a claim that a production test runner exists.
    let report = VerificationReport {
        source_snapshot: None,
        schema_version: 1,
        verifier_id: "core-verifier".into(),
        observed_at: "2026-10-07T14:02:00Z".into(),
        manifest_digest: digest(&manifest).unwrap(),
        checks: vec![CheckObservation {
            assertion_error: None,
            assertions: vec![],
            check_id: "check".into(),
            command_digest: digest(&f.plan.execution[0].checks[0]).unwrap(),
            exit_code: Some(0),
            timed_out: false,
            stdout_sha256: "a".repeat(64),
            stderr_sha256: "b".repeat(64),
        }],
    };
    observer
        .require_verified(&manifest, &report, "core-verifier")
        .unwrap();
    let mut bad = Vec::new();
    let mut r = report.clone();
    r.checks.clear();
    bad.push(r);
    let mut r = report.clone();
    r.checks[0].command_digest = "c".repeat(64);
    bad.push(r);
    let mut r = report.clone();
    r.checks[0].timed_out = true;
    bad.push(r);
    let mut r = report.clone();
    r.checks[0].exit_code = Some(1);
    bad.push(r);
    let mut r = report.clone();
    r.verifier_id = "worker".into();
    bad.push(r);
    let mut r = report.clone();
    r.manifest_digest = "c".repeat(64);
    bad.push(r);
    for r in bad {
        assert!(observer
            .require_verified(&manifest, &r, "core-verifier")
            .is_err());
    }
    fs::write(f.root.path().join("src/result.txt"), "changed after tests").unwrap();
    assert!(observer
        .require_verified(&manifest, &report, "core-verifier")
        .is_err());
    let updated = observer.capture().unwrap();
    assert!(observer
        .require_verified(&updated, &report, "core-verifier")
        .is_err());
}

#[test]
fn catalog_remains_unverified_and_capability_claims_require_versioned_evidence() {
    let catalog = cerulean_adapters::catalog();
    assert_eq!(catalog.len(), 10);
    for adapter in catalog {
        assert_eq!(adapter.capabilities.len(), 10);
        for capability in adapter.capabilities {
            assert_eq!(capability.state, CapabilityState::Unverified);
            capability.validate().unwrap();
        }
    }
    let mut capability = AdapterCapability {
        capability: Capability::Tools,
        state: CapabilityState::Supported,
        tested_version: None,
        evidence_sha256: None,
    };
    assert!(capability.validate().is_err());
    capability.tested_version = Some("test-version".into());
    assert!(capability.validate().is_err());
    capability.evidence_sha256 = Some("a".repeat(64));
    capability.validate().unwrap();
}
