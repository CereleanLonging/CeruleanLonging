//! Fresh local imports at the post-registration boundary; never replay prior fixtures.
use super::*;
use crate::{diagnostic_service::ServiceHost, reader_generation::tests::snapshot, Store};
use cerulean_domain::{team_timing::TeamTimingAction, Command, Request, PROTOCOL_VERSION};
use sha2::Digest;
use std::{
    fs,
    os::windows::process::CommandExt,
    time::{Duration, Instant},
};

fn area(case: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("CERULEAN_IMPORT_267_DIR").unwrap()).join(case)
}
fn start(area: &Path) -> (Box<RuntimeHost>, Importing, PathBuf, String) {
    let root = area.join("data");
    assert!(!root.exists(), "Preserve existing attempt; do not replay");
    drop(Store::open(&root).unwrap());
    let archive = area.join("initial-backup");
    let manifest = crate::backup::create(&root, &archive)
        .unwrap()
        .manifest_digest;
    let review = crate::experiment_guard::review(&root, &archive, &manifest).unwrap();
    crate::experiment_guard::promote(&root, &archive, &manifest, &review.digest).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let file = fs::canonicalize(area.join("request.json")).unwrap();
    let sha = format!("{:x}", sha2::Sha256::digest(fs::read(&file).unwrap()));
    let mut host = Box::new(RuntimeHost::open(&root).unwrap());
    let job = begin(
        &mut host,
        "partial-registration-267",
        file.to_str().unwrap(),
        &sha,
    )
    .unwrap();
    (host, job, file, sha)
}
fn finish(job: &mut Importing) -> Box<RuntimeHost> {
    let started = Instant::now();
    loop {
        if let Some(host) = job.take_finished() {
            return host;
        }
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "Inspect original process; never restart on timeout"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

// Test-only hook in the original worker, after synchronous project registration.
// An explicit child-only environment binding excludes every normal import/test.
pub(crate) fn after_registration(host: &RuntimeHost) {
    let Some(selected) = std::env::var_os("CERULEAN_IMPORT_267_EXIT_ROOT") else {
        return;
    };
    let selected = fs::canonicalize(selected).unwrap();
    if selected != host.directory() {
        return;
    }
    assert_eq!(std::thread::current().name(), Some("cerulean-team-import"));
    let current = host.snapshot();
    assert_eq!(current.workflows.len(), 1);
    assert!(current.validators.is_empty());
    assert!(current.team_timelines.is_empty());
    assert!(current.api_jobs.is_empty() && current.project_runs.is_empty());
    assert!(selected.join(OPERATION).is_file());
    assert!(selected.join("team-material-import.json").is_file());
    assert!(!selected.join(OUTCOME).exists());
    assert!(!selected.join("team-material-bindings").exists());
    publish(
        selected.parent().unwrap(),
        "exit-boundary.json",
        &serde_json::json!({
            "boundary":"after-project-registration-before-validator-registration", "exitCode":74,
            "workerThread":std::thread::current().name(), "snapshot":current
        }),
    )
    .unwrap();
    // Exit only this fresh test process. No user process/model job is running here.
    std::process::exit(74);
}

fn inspect_and_retain(area: &Path, state: &str, diagnostic: bool) {
    let root = fs::canonicalize(area.join("data")).unwrap();
    let file = fs::canonicalize(area.join("request.json")).unwrap();
    let sha = format!("{:x}", sha2::Sha256::digest(fs::read(&file).unwrap()));
    let before = snapshot(&root);
    let mut codes = vec![];
    for _ in 0..2 {
        let mut host = ServiceHost::open(&root).unwrap();
        assert_eq!(host.is_diagnostic(), diagnostic);
        let current = host.snapshot();
        assert_eq!(current.team_import.unwrap().state, state);
        assert!(current.api_jobs.is_empty() && current.project_runs.is_empty());
        assert!(current.team_timelines.is_empty());
        if !diagnostic {
            assert_eq!(current.workflows.len(), 1);
        }
        for command in [
            Command::TeamTiming {
                action: TeamTimingAction::Import {
                    path: file.to_string_lossy().into(),
                    sha256: sha.clone(),
                },
            },
            Command::CreatePlan {
                title: "blocked".into(),
                tasks: vec!["blocked".into()],
            },
            Command::RequestChiefPlan {
                project_id: "eff-S01-r1-team".into(),
                setup_digest: "a".repeat(64),
            },
            Command::StartProjectRun {
                project_id: "eff-S01-r1-team".into(),
                revision_digest: "a".repeat(64),
                setup_digest: "a".repeat(64),
            },
        ] {
            let reimport = matches!(command, Command::TeamTiming { .. });
            let error = host
                .handle_command(Request {
                    id: "blocked-267".into(),
                    protocol_version: PROTOCOL_VERSION,
                    command,
                })
                .error
                .unwrap();
            let expected = if diagnostic {
                "diagnosticReadOnly"
            } else if reimport {
                "teamImportRejected"
            } else {
                "teamImportUnresolved"
            };
            assert_eq!(error.code, expected);
            codes.push(error.code);
        }
        assert!(!crate::upgrade::review(&mut host).unwrap().0.ready);
        assert!(host.can_shutdown());
        drop(host);
        assert_eq!(before, snapshot(&root));
    }
    let archive = area.join("fault-backup");
    let manifest = crate::backup::create(&root, &archive)
        .unwrap()
        .manifest_digest;
    let receipt = area.join("retention");
    let reason = "Preserve original partial project registration";
    let review =
        crate::team_import_retention::review(&root, &archive, &manifest, &receipt, reason).unwrap();
    crate::team_import_retention::retain(
        &root,
        &archive,
        &manifest,
        &receipt,
        reason,
        &review.digest,
    )
    .unwrap();
    crate::team_import_retention::inspect(
        &root,
        &archive,
        &manifest,
        &receipt,
        reason,
        &review.digest,
    )
    .unwrap();
    assert_eq!(before, snapshot(&root));
    publish(area, "result.json", &serde_json::json!({"passed":true,"state":state,"diagnostic":diagnostic,"coldReopens":2,"blockedCommandCodes":codes,"retentionDigest":review.digest,"realModelCalls":0,"credentialsUsed":0,"releaseAccepted":false})).unwrap();
}

#[test]
#[ignore = "267 fresh local partial-registration fixture, no provider calls"]
fn validator_conflict_after_registration_preserves_failed_import() {
    let area = area("guard");
    let (host, mut job, _, _) = start(&area);
    let root = host.directory().to_owned();
    // Real catalog conflict, after the Host was opened and before import mutation.
    fs::create_dir(root.join("validators/eff-full.json")).unwrap();
    job.launch(host);
    let host = finish(&mut job);
    let outcome: Outcome = load(&root, OUTCOME).unwrap();
    assert!(!outcome.completed && outcome.binding.is_none());
    assert!(outcome
        .message
        .unwrap()
        .contains("validator registration already on disk"));
    assert_eq!(host.snapshot().workflows.len(), 1);
    assert!(host.snapshot().validators.is_empty());
    assert!(root.join("team-material-import.json").is_file());
    assert!(!root.join("team-material-bindings").exists());
    assert!(RuntimeHost::open(&root).is_err());
    drop(host);
    drop(job);
    inspect_and_retain(&area, "failed", true);
}

#[test]
#[ignore = "child-only 267 exit fixture; parent owns isolated root and original process"]
fn registered_worker_child() {
    let area = area("exit");
    let (host, mut job, _, _) = start(&area);
    job.launch(host);
    let _host = finish(&mut job);
    panic!("Original worker did not exit at the registered boundary");
}

#[test]
#[ignore = "267 isolated worker process exit at actual registration boundary"]
fn worker_exit_after_registration_reopens_without_replay() {
    let area = area("exit");
    assert!(!area.join("data").exists());
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "team_import_service::partial_fault_tests::registered_worker_child",
            "--ignored",
            "--nocapture",
        ])
        .env("CERULEAN_IMPORT_267_EXIT_ROOT", area.join("data"))
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(74),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let proof: serde_json::Value = load(&area, "exit-boundary.json").unwrap();
    assert_eq!(proof["workerThread"], "cerulean-team-import");
    assert_eq!(proof["snapshot"]["workflows"].as_array().unwrap().len(), 1);
    let root = area.join("data");
    assert!(!root.join(OUTCOME).exists());
    assert!(!root.join("team-material-bindings").exists());
    publish(&area, "child-result.json", &serde_json::json!({"exitCode":output.status.code(),"stdout":String::from_utf8_lossy(&output.stdout),"stderr":String::from_utf8_lossy(&output.stderr),"originalProcessAndLogsAwaited":true,"externalKill":false})).unwrap();
    inspect_and_retain(&area, "interrupted", false);
}
