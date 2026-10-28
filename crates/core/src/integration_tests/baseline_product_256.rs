use super::*;
use crate::ipc::{Client, UserOperation};
use cerulean_domain::baseline_product::{BaselineAction as Action, BaselineView};
use sha2::{Digest, Sha256};
fn outer() -> PathBuf {
    PathBuf::from(std::env::var_os("CERULEAN_BASELINE_256_DIR").unwrap())
}
fn request(task: &str) -> (PathBuf, String) {
    let p = outer().join(format!("fixtures/{task}-r1-single/request.json"));
    let d = format!("{:x}", Sha256::digest(fs::read(&p).unwrap()));
    (p, d)
}
fn command(a: Action) -> Command {
    Command::Baseline { action: a }
}
fn next() -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!(
        "baseline256-{}",
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
fn handle(h: &mut RuntimeHost, a: Action) -> Response {
    h.handle(Request {
        id: next(),
        protocol_version: PROTOCOL_VERSION,
        command: command(a),
    })
}
fn ok(h: &mut RuntimeHost, a: Action) {
    let r = handle(h, a);
    assert!(r.error.is_none(), "{:?}", r.error);
}
fn save(name: &str, v: &impl serde::Serialize) {
    crate::api_execution::publish(&outer(), name, v).unwrap();
}
fn wait_host(h: &mut RuntimeHost, phase: &str) -> BaselineView {
    let started = Instant::now();
    loop {
        h.poll_verifications();
        let v = h.snapshot().baselines[0].clone();
        if !v.busy {
            assert_eq!(v.phase, phase, "{:?}", v);
            return v;
        }
        assert!(started.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn prepared(task: &str) -> (RuntimeHost, String, String) {
    let root = outer().join(format!("host-{task}"));
    let mut h = RuntimeHost::open(&root).unwrap();
    h.baseline_fixture = true;
    let (path, sha) = request(task);
    ok(
        &mut h,
        Action::Import {
            id: task.into(),
            path: path.to_string_lossy().into(),
            sha256: sha,
        },
    );
    let binding = h.snapshot().baselines[0].binding_digest.clone().unwrap();
    ok(
        &mut h,
        Action::Prepare {
            id: task.into(),
            binding_digest: binding.clone(),
        },
    );
    let v = wait_host(&mut h, "review");
    (h, binding, v.timing.unwrap().start_digest.unwrap())
}
async fn call(root: &Path, c: Command) -> crate::ipc::ServiceReply {
    Client::connect(root)
        .await
        .unwrap()
        .call(UserOperation::Command {
            request: Request {
                id: next(),
                protocol_version: PROTOCOL_VERSION,
                command: c,
            },
        })
        .await
        .unwrap()
}
async fn ipc_ok(root: &Path, a: Action) {
    let r = call(root, command(a)).await;
    assert!(r.response.error.is_none(), "{:?}", r.response.error);
    assert!(r.command_receipt.is_some());
}
async fn wait_ipc(root: &Path, phase: &str) -> Snapshot {
    let started = Instant::now();
    loop {
        let s = Client::snapshot(root).await.unwrap();
        let v = &s.baselines[0];
        if !v.busy {
            assert_eq!(v.phase, phase, "{:?}", v);
            return s;
        }
        assert!(started.elapsed() < Duration::from_secs(120));
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

#[tokio::test]
#[ignore = "256 new synthetic baseline; real authenticated IPC, original file tools/checker, no network"]
async fn baseline_product_ipc_original_execution_and_two_reopens() {
    let root = outer().join("host-S05");
    let mut h = RuntimeHost::open(&root).unwrap();
    h.baseline_fixture = true;
    let owned = root.clone();
    let service = tokio::spawn(async move { crate::ipc::serve_host(&owned, h).await });
    let started = Instant::now();
    while Client::connect(&root).await.is_err() {
        assert!(started.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (path, sha) = request("S05");
    assert!(call(
        &root,
        command(Action::Import {
            id: "S05".into(),
            path: path.to_string_lossy().into(),
            sha256: "0".repeat(64)
        })
    )
    .await
    .response
    .error
    .is_some());
    assert!(!root.join("baseline-experiments").exists());
    ipc_ok(
        &root,
        Action::Import {
            id: "S05".into(),
            path: path.to_string_lossy().into(),
            sha256: sha.clone(),
        },
    )
    .await;
    assert!(call(
        &root,
        command(Action::Import {
            id: "s06".into(),
            path: path.to_string_lossy().into(),
            sha256: sha
        })
    )
    .await
    .response
    .error
    .is_some());
    let binding = Client::snapshot(&root).await.unwrap().baselines[0]
        .binding_digest
        .clone()
        .unwrap();
    let t = Instant::now();
    ipc_ok(
        &root,
        Action::Prepare {
            id: "S05".into(),
            binding_digest: binding.clone(),
        },
    )
    .await;
    assert!(t.elapsed() < Duration::from_secs(10));
    assert!(call(
        &root,
        command(Action::Prepare {
            id: "S05".into(),
            binding_digest: binding.clone()
        })
    )
    .await
    .response
    .error
    .is_some());
    let s = wait_ipc(&root, "review").await;
    let review = s.baseline_review.unwrap();
    let start = review.start_digest;
    let spec = review.spec_digest;
    let human = |active| Action::Human {
        id: "S05".into(),
        start_digest: start.clone(),
        active,
    };
    ipc_ok(&root, human(true)).await;
    assert!(call(
        &root,
        command(Action::Approve {
            id: "S05".into(),
            start_digest: start.clone(),
            spec_digest: spec.clone()
        })
    )
    .await
    .response
    .error
    .is_some());
    ipc_ok(&root, human(false)).await;
    assert!(call(
        &root,
        command(Action::Approve {
            id: "S05".into(),
            start_digest: start.clone(),
            spec_digest: "0".repeat(64)
        })
    )
    .await
    .response
    .error
    .is_some());
    ipc_ok(
        &root,
        Action::Approve {
            id: "S05".into(),
            start_digest: start.clone(),
            spec_digest: spec.clone(),
        },
    )
    .await;
    wait_ipc(&root, "approved").await;
    assert!(!root
        .join("baseline-experiments/S05/data/started.json")
        .exists());
    ipc_ok(
        &root,
        Action::Start {
            id: "S05".into(),
            start_digest: start.clone(),
            spec_digest: spec.clone(),
        },
    )
    .await;
    let stopped = Client::connect(&root)
        .await
        .unwrap()
        .call(UserOperation::Shutdown {
            id: next(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(!stopped.stopping);
    ipc_ok(&root, human(true)).await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    ipc_ok(&root, human(false)).await;
    wait_ipc(&root, "returned").await;
    assert!(call(
        &root,
        command(Action::Start {
            id: "S05".into(),
            start_digest: start.clone(),
            spec_digest: spec.clone()
        })
    )
    .await
    .response
    .error
    .is_some());
    ipc_ok(&root, human(true)).await;
    assert!(call(
        &root,
        command(Action::Finish {
            id: "S05".into(),
            start_digest: start.clone()
        })
    )
    .await
    .response
    .error
    .is_some());
    ipc_ok(&root, human(false)).await;
    ipc_ok(
        &root,
        Action::Finish {
            id: "S05".into(),
            start_digest: start.clone(),
        },
    )
    .await;
    let closed = wait_ipc(&root, "closed").await;
    assert_eq!(
        closed.baselines[0].timing.as_ref().unwrap().accepted,
        Some(true)
    );
    let stopped = Client::connect(&root)
        .await
        .unwrap()
        .call(UserOperation::Shutdown {
            id: next(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(stopped.stopping);
    tokio::time::timeout(Duration::from_secs(20), service)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let timeline = root.join("baseline-experiments/S05/timeline");
    let original = crate::baseline_runner::timing::inspect(&timeline, &start).unwrap();
    assert_eq!(
        original["sources"]["baseline"]["coreExport"]["apiReplies"],
        2
    );
    for _ in 0..2 {
        let mut h = RuntimeHost::open(&root).unwrap();
        assert_eq!(h.snapshot().baselines[0].phase, "needsRead");
        ok(
            &mut h,
            Action::Read {
                id: "S05".into(),
                binding_digest: binding.clone(),
            },
        );
        let v = wait_host(&mut h, "closed");
        assert_eq!(v.timing.as_ref().unwrap().accepted, Some(true));
        assert!(handle(
            &mut h,
            Action::Start {
                id: "S05".into(),
                start_digest: start.clone(),
                spec_digest: spec.clone()
            }
        )
        .error
        .is_some());
        assert!(h.can_shutdown());
    }
    save("ipc-original-inspection.json", &original);
    save("ipc-final-snapshot.json", &closed);
    save(
        "ipc-result.json",
        &json!({"syntheticWorkerReplies":2,"managementRequests":0,"reopensWithoutRedispatch":2,"realModelCalls":0,"authenticatedIpc":true,"releaseAccepted":false}),
    );
}
#[test]
#[ignore = "256 fresh preparation-only record; loss of clock cannot restore execution"]
fn baseline_product_reopen_closes_original_clock_loss() {
    let (h, binding, start) = prepared("S03");
    let root = h.root.clone();
    let spec = h.snapshot().baselines[0].spec_digest.clone().unwrap();
    drop(h);
    let mut h = RuntimeHost::open(&root).unwrap();
    ok(
        &mut h,
        Action::Read {
            id: "S03".into(),
            binding_digest: binding,
        },
    );
    let view = wait_host(&mut h, "clockLost");
    assert!(view.timing.as_ref().unwrap().elapsed_ms.is_none());
    assert!(handle(
        &mut h,
        Action::Approve {
            id: "S03".into(),
            start_digest: start.clone(),
            spec_digest: spec.clone()
        }
    )
    .error
    .is_some());
    assert!(handle(
        &mut h,
        Action::Start {
            id: "S03".into(),
            start_digest: start.clone(),
            spec_digest: spec
        }
    )
    .error
    .is_some());
    ok(
        &mut h,
        Action::CloseInterrupted {
            id: "S03".into(),
            start_digest: start.clone(),
        },
    );
    let v = wait_host(&mut h, "closed");
    assert_eq!(v.timing.unwrap().elapsed_ms, None);
    assert!(h.can_shutdown());
    drop(h);
    let report = crate::baseline_runner::timing::inspect(
        &root.join("baseline-experiments/S03/timeline"),
        &start,
    )
    .unwrap();
    assert_eq!(report["terminal"]["interrupted"], true);
    assert!(!root
        .join("baseline-experiments/S03/data/started.json")
        .exists());
    save("lost-clock-inspection.json", &report);
}
#[test]
#[ignore = "256 new cancellation-only synthetic attempt; native Started retained, no retry"]
fn baseline_product_cancel_owns_original_background_job() {
    let (mut h, _binding, start) = prepared("S01");
    let root = h.root.clone();
    let spec = h.snapshot().baselines[0].spec_digest.clone().unwrap();
    ok(
        &mut h,
        Action::Approve {
            id: "S01".into(),
            start_digest: start.clone(),
            spec_digest: spec.clone(),
        },
    );
    wait_host(&mut h, "approved");
    ok(
        &mut h,
        Action::Start {
            id: "S01".into(),
            start_digest: start.clone(),
            spec_digest: spec.clone(),
        },
    );
    assert!(!h.can_shutdown());
    let began = Instant::now();
    while !root
        .join("baseline-experiments/S01/data/started.json")
        .exists()
    {
        h.poll_verifications();
        assert!(h.snapshot().baselines[0].busy);
        assert!(began.elapsed() < Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(5));
    }
    ok(
        &mut h,
        Action::Cancel {
            id: "S01".into(),
            start_digest: start.clone(),
        },
    );
    assert!(handle(
        &mut h,
        Action::Cancel {
            id: "S01".into(),
            start_digest: start.clone()
        }
    )
    .error
    .is_some());
    let v = wait_host(&mut h, "returned");
    assert!(v.cancel_requested);
    ok(
        &mut h,
        Action::Finish {
            id: "S01".into(),
            start_digest: start.clone(),
        },
    );
    let v = wait_host(&mut h, "closed");
    assert_eq!(v.timing.unwrap().accepted, Some(false));
    assert!(h.can_shutdown());
    drop(h);
    let report = crate::baseline_runner::timing::inspect(
        &root.join("baseline-experiments/S01/timeline"),
        &start,
    )
    .unwrap();
    assert_eq!(report["sources"]["baseline"]["terminal"]["accepted"], false);
    save("cancel-inspection.json", &report);
}
#[test]
fn baseline_product_wire_is_user_only() {
    assert!(serde_json::from_value::<Action>(json!({"kind":"start","id":"x","startDigest":"a".repeat(64),"specDigest":"b".repeat(64),"transport":"synthetic"})).is_err());
    assert!(serde_json::from_value::<Action>(json!({"kind":"import","id":"x","path":"x","sha256":"a".repeat(64),"credentialRoot":"other"})).is_err());
    assert!(command(Action::Human {
        id: "x".into(),
        start_digest: "a".repeat(64),
        active: true
    })
    .supports_receipt());
}

#[test]
#[ignore = "256 original failed record read only; pending intent must remain unknown"]
fn baseline_product_original_failed_intent_is_not_retried() {
    let path = outer().join("host-S06/baseline-experiments/S06/timeline");
    let selected = crate::baseline_runner::timing::original_start_digest(&path).unwrap();
    let report = crate::baseline_runner::timing::inspect(&path, &selected);
    assert!(report.is_err());
    assert!(path.join("run-intent.json.pending").exists());
    assert!(!path.parent().unwrap().join("data/started.json").exists());
    save(
        "failed-original-inspection.json",
        &json!({"error":report.unwrap_err(),"nativeStarted":false,"realModelCalls":0,"retried":false}),
    );
}
