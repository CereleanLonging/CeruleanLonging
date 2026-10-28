use super::omp_host::tests::{fixture, startup};
use super::*;
use cerulean_domain::omp_jobs::OmpJobState;

fn broken(root: &Path, bytes: &[u8]) -> (PathBuf, crate::startup_journal::StartupIntent, String) {
    let mut host = fixture(root, None);
    let intent = startup(&host, "attempt");
    host.omp_startups.begin(intent.clone()).unwrap();
    let head = host
        .omp_startups
        .get("project", "attempt")
        .unwrap()
        .intent_hash
        .clone();
    let path = crate::async_omp::cancel_path(&host.root, &intent)
        .unwrap()
        .with_extension("pending");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    (path, intent, head)
}
fn command(host: &mut RuntimeHost, cmd: Command) -> Response {
    host.handle(Request {
        id: "stop-review-test".into(),
        protocol_version: PROTOCOL_VERSION,
        command: cmd,
    })
}
#[test]
fn diagnostic_mode_blocks_mutations_and_acknowledgement_preserves_evidence_and_unknown_state() {
    for original in [b"".as_slice(), b"{\"version\":1,"] {
        let root = tempfile::tempdir().unwrap();
        let (path, intent, head) = broken(root.path(), original);
        let data = root.path().join("data");
        let mut host = RuntimeHost::open(&data).unwrap();
        let before = host.projects["project"].history().len();
        assert_eq!(host.omp_stop_issues.len(), 1);
        let evidence = host.omp_stop_issues[0].evidence_digest.clone().unwrap();
        for cmd in [
            Command::CreatePlan {
                title: "blocked".into(),
                tasks: vec!["blocked".into()],
            },
            Command::CheckOpenAi,
            Command::RecoverOmp {
                project_id: "project".into(),
                attempt_id: "attempt".into(),
            },
        ] {
            assert_eq!(
                command(&mut host, cmd).error.unwrap().code,
                "diagnosticMode"
            );
        }
        host.poll_verifications();
        host.poll_project_runs();
        assert_eq!(host.projects["project"].history().len(), before);
        assert!(host.management_active.is_empty() && host.omp_active.is_empty());
        assert!(command(&mut host, Command::Snapshot).error.is_none());
        assert!(host.can_shutdown());
        let acknowledge = |evidence: String, reason: &str| Command::AcknowledgeOmpStopIssue {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
            evidence_digest: evidence,
            reason: reason.into(),
        };
        assert!(command(&mut host, acknowledge("0".repeat(64), "stale"))
            .error
            .is_some());
        assert!(command(&mut host, acknowledge(evidence.clone(), ""))
            .error
            .is_some());
        assert!(!data.join("omp-stop-reviews").exists());
        assert!(command(
            &mut host,
            acknowledge(evidence.clone(), "reviewed; outcome still unknown")
        )
        .error
        .is_none());
        assert!(command(
            &mut host,
            acknowledge(evidence, "second click must not change first reason")
        )
        .error
        .is_none());
        assert!(host.omp_stop_issues[0].acknowledged);
        assert!(command(&mut host, Command::CheckOpenAi).error.is_some());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!path.with_extension("json").exists());
        assert_eq!(
            crate::omp_stop_review::accepted(&data, &intent, &head)
                .unwrap()
                .as_deref(),
            Some("reviewed; outcome still unknown")
        );
        drop(host);
        let host = RuntimeHost::open(&data).unwrap();
        assert!(host.omp_stop_issues.is_empty());
        assert_eq!(
            host.omp_jobs.values().next().unwrap().state,
            OmpJobState::Unknown
        );
        assert_eq!(host.projects["project"].history().len(), before);
        assert!(host.omp_active.is_empty());
        assert!(host.require_available("project").is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }
}
#[test]
fn stale_original_or_foreign_receipt_cannot_authorize_repair() {
    let root = tempfile::tempdir().unwrap();
    let (path, intent, head) = broken(root.path(), b"{");
    let data = root.path().join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    let evidence = host.omp_stop_issues[0].evidence_digest.clone().unwrap();
    fs::write(&path, b"{\"changed\":").unwrap();
    assert!(host
        .acknowledge_omp_stop_issue("project", "attempt", &evidence, "stale")
        .is_err());
    assert!(!data.join("omp-stop-reviews").exists());
    fs::write(&path, b"{").unwrap();
    host.acknowledge_omp_stop_issue("project", "attempt", &evidence, "review")
        .unwrap();
    fs::write(&path, b"{\"changed\":").unwrap();
    assert!(crate::omp_stop_review::accepted(&data, &intent, &head).is_err());
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(host.omp_stop_issues.len(), 1);
    assert!(host.omp_stop_issues[0].evidence_digest.is_none());
}
#[test]
fn complete_invalid_oversized_and_committed_records_are_diagnostic_without_acknowledgement() {
    for mode in ["invalid", "oversized", "committed", "linked"] {
        let root = tempfile::tempdir().unwrap();
        let (path, _, _) = broken(root.path(), b"{");
        match mode {
            "invalid" => fs::write(&path, b"{}").unwrap(),
            "oversized" => fs::write(&path, vec![b' '; 65537]).unwrap(),
            "committed" => fs::write(path.with_extension("json"), b"{}").unwrap(),
            "linked" => {
                fs::remove_file(&path).unwrap();
                fs::create_dir(&path).unwrap();
            }
            _ => unreachable!(),
        }
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.omp_stop_issues.len(), 1);
        assert!(host.omp_stop_issues[0].evidence_digest.is_none(), "{mode}");
        assert!(host.omp_active.is_empty());
    }
}
#[test]
fn interrupted_review_publication_requires_confirmation_until_final_name_exists() {
    for point in ["created", "partial", "synced", "published", "finished"] {
        let root = tempfile::tempdir().unwrap();
        let (path, intent, head) = broken(root.path(), b"{");
        let data = root.path().join("data");
        let host = RuntimeHost::open(&data).unwrap();
        let evidence = host.omp_stop_issues[0].evidence_digest.clone().unwrap();
        assert!(crate::omp_stop_review::acknowledge_observed(
            &data,
            &intent,
            &head,
            &evidence,
            "first review",
            |p| if p == point {
                Err("lost confirmation".into())
            } else {
                Ok(())
            }
        )
        .is_err());
        let published = matches!(point, "published" | "finished");
        assert_eq!(
            crate::omp_stop_review::accepted(&data, &intent, &head)
                .unwrap()
                .is_some(),
            published
        );
        drop(host);
        let mut host = RuntimeHost::open(&data).unwrap();
        assert_eq!(host.omp_stop_issues.is_empty(), published);
        if !published {
            host.acknowledge_omp_stop_issue("project", "attempt", &evidence, "explicit retry")
                .unwrap();
        }
        assert_eq!(fs::read(&path).unwrap(), b"{");
        assert!(!path.with_extension("json").exists());
    }
}
