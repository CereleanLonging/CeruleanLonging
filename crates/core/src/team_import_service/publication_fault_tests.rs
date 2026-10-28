//! Import real pinned local materials, then fail the final outcome publication.
//! No credentials, planning, execution, model fixtures, or network are needed.
use super::*;
use crate::{diagnostic_service::ServiceHost, reader_generation::tests::snapshot, Store};
use cerulean_domain::{team_timing::TeamTimingAction, Command, Request, PROTOCOL_VERSION};
use sha2::Digest;
use std::{
    fs,
    time::{Duration, Instant},
};

#[test]
#[ignore = "266 fresh pinned local material fixture; import only, no provider calls"]
fn successful_import_with_failed_publication_preserves_binding_without_execution() {
    let area = PathBuf::from(std::env::var_os("CERULEAN_IMPORT_266_DIR").unwrap());
    let root = area.join("data");
    assert!(
        !root.exists(),
        "A previously attempted import must never be replayed"
    );
    drop(Store::open(&root).unwrap());
    let initial_backup = area.join("initial-backup");
    let initial_manifest = crate::backup::create(&root, &initial_backup)
        .unwrap()
        .manifest_digest;
    let review =
        crate::experiment_guard::review(&root, &initial_backup, &initial_manifest).unwrap();
    crate::experiment_guard::promote(&root, &initial_backup, &initial_manifest, &review.digest)
        .unwrap();
    let root = fs::canonicalize(root).unwrap();
    let file = fs::canonicalize(area.join("request.json")).unwrap();
    let sha = format!("{:x}", sha2::Sha256::digest(fs::read(&file).unwrap()));
    let mut host = Box::new(RuntimeHost::open(&root).unwrap());
    let mut job = begin(
        &mut host,
        "publication-fault-266",
        file.to_str().unwrap(),
        &sha,
    )
    .unwrap();
    let original_intent = fs::read(root.join(OPERATION)).unwrap();
    // The original import can complete, but Windows cannot hard-link over a directory.
    fs::create_dir(root.join(OUTCOME)).unwrap();
    job.launch(host);
    let started = Instant::now();
    let host = loop {
        if let Some(host) = job.take_finished() {
            break host;
        }
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "Original import not terminal; preserve and inspect, never retry"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let pending: Outcome = load(&root, &format!("{OUTCOME}.pending")).unwrap();
    assert!(
        pending.completed,
        "Import itself must succeed: {:?}",
        pending.message
    );
    assert!(pending.message.is_none());
    let binding = pending.binding.as_ref().unwrap();
    original_binding(&root, binding).unwrap();
    assert_eq!(pending.operation_digest, digest(&job.operation).unwrap());
    assert_eq!(fs::read(root.join(OPERATION)).unwrap(), original_intent);
    assert!(root.join(OUTCOME).is_dir());
    assert_eq!(view(&root).unwrap().state, "interrupted");
    assert!(
        RuntimeHost::open(&root).is_err(),
        "Original Host retains its writer lock"
    );
    let original = host.snapshot();
    assert_eq!(original.workflows.len(), 1);
    assert_eq!(original.team_timelines.len(), 1);
    assert_eq!(original.team_timelines[0].phase, "prepared");
    assert!(original.api_jobs.is_empty());
    assert!(original.project_runs.is_empty());
    drop(host);
    drop(job);
    let before = snapshot(&root);
    for _ in 0..2 {
        let mut service = ServiceHost::open(&root).unwrap();
        assert!(
            !service.is_diagnostic(),
            "Healthy registered project must remain readable"
        );
        let current = service.snapshot();
        assert_eq!(current.team_import.unwrap().state, "interrupted");
        assert_eq!(
            serde_json::to_value(current.workflows).unwrap(),
            serde_json::to_value(&original.workflows).unwrap()
        );
        assert_eq!(
            serde_json::to_value(current.team_timelines).unwrap(),
            serde_json::to_value(&original.team_timelines).unwrap()
        );
        assert!(current.api_jobs.is_empty());
        assert!(current.project_runs.is_empty());
        let b: crate::team_materials::TeamBlueprint = load(
            &root
                .join("team-material-bindings")
                .join(&binding.project_id),
            "binding.json",
        )
        .unwrap();
        for command in [
            Command::TeamTiming {
                action: TeamTimingAction::Import {
                    path: file.to_string_lossy().into(),
                    sha256: sha.clone(),
                },
            },
            Command::TeamTiming {
                action: TeamTimingAction::Begin {
                    project_id: binding.project_id.clone(),
                    setup_digest: b.setup.digest.clone(),
                },
            },
            Command::RequestChiefPlan {
                project_id: binding.project_id.clone(),
                setup_digest: b.setup.digest.clone(),
            },
            Command::StartProjectRun {
                project_id: binding.project_id.clone(),
                revision_digest: b.plan.approval_digest().unwrap(),
                setup_digest: b.setup.digest.clone(),
            },
            Command::CreatePlan {
                title: "blocked".into(),
                tasks: vec!["blocked".into()],
            },
        ] {
            assert!(service
                .handle_command(Request {
                    id: "blocked-266".into(),
                    protocol_version: PROTOCOL_VERSION,
                    command
                })
                .error
                .is_some());
        }
        assert!(!crate::upgrade::review(&mut service).unwrap().0.ready);
        assert!(service.can_shutdown());
        drop(service);
        assert_eq!(before, snapshot(&root));
        original_binding(&root, binding).unwrap();
    }
    // The existing cold-backup/disposition contract must preserve even a successful binding.
    let backup = area.join("fault-backup");
    let manifest = crate::backup::create(&root, &backup)
        .unwrap()
        .manifest_digest;
    let receipt = area.join("retention");
    let reason = "Preserve successful import with unpublished outcome";
    let review =
        crate::team_import_retention::review(&root, &backup, &manifest, &receipt, reason).unwrap();
    crate::team_import_retention::retain(
        &root,
        &backup,
        &manifest,
        &receipt,
        reason,
        &review.digest,
    )
    .unwrap();
    crate::team_import_retention::inspect(
        &root,
        &backup,
        &manifest,
        &receipt,
        reason,
        &review.digest,
    )
    .unwrap();
    assert_eq!(before, snapshot(&root));
    publish(&area, "result.json", &serde_json::json!({"passed":true,"importCompleted":true,"outcomePublished":false,"coldReopens":2,"blockedCommands":10,"bindingProjectId":binding.project_id,"bindingDigest":binding.digest,"retentionDigest":review.digest,"realModelCalls":0,"credentialsUsed":0,"releaseAccepted":false})).unwrap();
}
