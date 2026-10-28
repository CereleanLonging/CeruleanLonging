//! Owned background preparation, cancellation and immutable startup binding.
use super::*;

#[test]
#[ignore = "real Git fixture; deterministic preparation pause proves responsive host and cancellation"]
fn preparation_is_owned_responsive_cancellable_and_reopens_without_retry() {
    let temp = temporary();
    let mut host = host(temp.path(), true);
    let (entered, started) = std::sync::mpsc::channel();
    let (_release, wait) = std::sync::mpsc::channel();
    host.api_prepare_gate = Some(crate::async_api::PreparationGate {
        entered,
        release: wait,
    });
    let began = Instant::now();
    host.start_managed_api("deputy-api", "attempt").unwrap();
    assert!(began.elapsed() < Duration::from_secs(5));
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        host.api_writers["project"]
            .owner
            .snapshot()
            .unwrap()
            .state
            .tasks["task"]
            .state,
        TaskState::Pending
    );
    let key = ("project".into(), "attempt".into());
    assert!(host.api_startups[&key].prepare_workspace);
    assert!(host.api_startups[&key].attempt.workspace_proof.is_none());
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Starting);
    assert!(!host.can_shutdown());
    assert!(host.require_available("project").is_err());
    assert!(host.start_managed_api("deputy-api", "another").is_err());
    assert!(host.management.application("deputy-api").is_none());
    assert!(!temp.path().join("pool/records").exists());
    let began = Instant::now();
    host.cancel_api("project", "attempt", "stop directory preparation")
        .unwrap();
    assert!(began.elapsed() < Duration::from_secs(5));
    drain(&mut host);
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Unknown);
    assert!(!temp.path().join("pool/records").exists());
    assert!(host.projects["project"]
        .attempt_record("task", "attempt")
        .is_none());
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    reopened.poll_verifications();
    assert!(reopened.api_active.is_empty());
    assert!(reopened.start_managed_api("deputy-api", "attempt").is_err());
    let review = reopened.api_summaries()[0]
        .closure_digest
        .clone()
        .expect("unassigned preparation can be reviewed");
    reopened
        .abandon_api_startup(
            "project",
            "attempt",
            &review,
            "close unassigned preparation",
        )
        .unwrap();
    assert_eq!(
        reopened.snapshot().api_jobs[0].state,
        ApiJobState::Abandoned
    );
    drop(reopened);
    let reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert_eq!(
        reopened.snapshot().api_jobs[0].state,
        ApiJobState::Abandoned
    );
}

#[test]
#[ignore = "real Git fixture; cancellation spanning binding publication retains immutable reservation"]
fn cancellation_before_workspace_ack_survives_completed_binding() {
    let temp = temporary();
    let mut host = host(temp.path(), true);
    host.start_managed_api("deputy-api", "attempt").unwrap();
    let key = ("project".into(), "attempt".into());
    let pending = host.api_startups[&key].clone();
    let update = host.api_active[&("project".into(), "attempt".into())]
        .receiver
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    let crate::async_api::Update::WorkspaceReady { startup, reply } = update else {
        panic!("workspace binding must precede assignment")
    };
    let prepared = crate::api_startup::read_prepared(&host.root, &pending).unwrap();
    assert_eq!(prepared, *startup);
    assert_ne!(digest(&pending).unwrap(), digest(&prepared).unwrap());
    assert_eq!(
        pending.reservation_digest().unwrap(),
        prepared.reservation_digest().unwrap()
    );
    let original = crate::api_startup::original(&host.root, &digest(&key).unwrap()).unwrap();
    assert_eq!(pending, original);
    host.cancel_api(
        "project",
        "attempt",
        "stop before directory acknowledgement",
    )
    .unwrap();
    drop(reply); // No permission to proceed to the two assignment acknowledgements.
    drain(&mut host);
    assert!(host.api_startups[&key].attempt.workspace_proof.is_some());
    assert!(host.management.application("deputy-api").is_none());
    assert!(!Path::new(&prepared.attempt.binding.workspace)
        .join("result.txt")
        .exists());
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert_eq!(reopened.snapshot().api_jobs[0].state, ApiJobState::Unknown);
    assert!(reopened.api_active.is_empty());
    let review = reopened.api_summaries()[0].closure_digest.clone().unwrap();
    reopened
        .abandon_api_startup(
            "project",
            "attempt",
            &review,
            "close prepared but undispatched startup",
        )
        .unwrap();
    assert!(Path::new(&prepared.attempt.binding.workspace)
        .join("base.txt")
        .exists());
    drop(reopened);
    RuntimeHost::open(&temp.path().join("data")).unwrap();
}

#[test]
#[ignore = "real Git fixture; partial workspace binding requires explicit byte-preserving review"]
fn partial_workspace_binding_blocks_reopen_until_review_without_reconstruction() {
    let temp = temporary();
    let mut host = host(temp.path(), true);
    host.start_managed_api("deputy-api", "attempt").unwrap();
    let update = host.api_active[&("project".into(), "attempt".into())]
        .receiver
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    let crate::async_api::Update::WorkspaceReady { startup, reply } = update else {
        panic!("expected binding")
    };
    drop(reply);
    drain(&mut host);
    let stem = digest(&startup.key()).unwrap();
    let folder = host.root.join("api-workspace-bindings");
    let file = folder.join(format!("{stem}.json"));
    let raw = fs::read(&file).unwrap();
    drop(host);
    fs::rename(&file, temp.path().join("original-binding.json")).unwrap();
    let relative = format!("api-workspace-bindings/{stem}.pending");
    let data = temp.path().join("data");
    let partial = &raw[..raw.len() / 2];
    fs::write(data.join(&relative), partial).unwrap();
    assert!(RuntimeHost::open(&data).is_err());
    let (head, done) = crate::api_publication::inspect(&data, &relative).unwrap();
    assert!(!done);
    crate::api_publication::acknowledge(
        &data,
        &relative,
        &head,
        "review simulated incomplete binding",
        |_| Ok(()),
    )
    .unwrap();
    let mut reopened = RuntimeHost::open(&data).unwrap();
    assert!(reopened.api_startups[&startup.key()]
        .attempt
        .workspace_proof
        .is_none());
    reopened.poll_verifications();
    assert!(reopened.api_active.is_empty());
    assert!(!file.exists());
    assert_eq!(fs::read(data.join(relative)).unwrap(), partial);
    assert!(Path::new(&startup.attempt.binding.workspace)
        .join("base.txt")
        .exists());
    assert!(reopened.start_managed_api("deputy-api", "another").is_err());
}
