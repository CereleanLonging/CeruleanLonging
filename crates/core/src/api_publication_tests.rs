//! Production publication writers interrupted by termination of their owning process.
use super::*;
use crate::{
    api_publication,
    api_startup::{self, ApiStartup},
    diagnostic_service::ServiceHost,
};
use cerulean_domain::{
    api_jobs::ApiJobState, api_worker::ApiTarget, project_setup::ProjectWorkerAdapter,
};
use std::{
    os::windows::process::CommandExt,
    process::{Command as Process, Stdio},
    time::{Duration, Instant},
};
fn fixture(root: &Path, cancel: bool) -> (RuntimeHost, ApiStartup) {
    let mut host = api_host_tests::fixture(
        root,
        ProjectWorkerAdapter::Deepseek,
        ApiTarget::Provider {
            endpoint: credentials::ProviderEndpoint::DeepSeek {},
        },
        true,
    );
    host.api_fixture =
        Some(|_, _| Err(cerulean_adapters::api_generation::ApiFailure::Http { status: 401 }));
    host.start_managed_api("deputy-api", "attempt").unwrap();
    if !cancel {
        let crate::async_api::Update::Assignment(crate::managed_assignment::Update::Prepare {
            reply,
            ..
        }) = host.api_active[&("project".into(), "attempt".into())]
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
        else {
            panic!("expected preparation")
        };
        reply
            .send(Err("seed contains no assignment".into()))
            .unwrap();
    }
    api_host_tests::drain(&mut host);
    let startup = host.api_startups.values().next().unwrap().clone();
    (host, startup)
}
fn relative(startup: &ApiStartup, cancel: bool, extension: &str) -> String {
    format!(
        "{}/{}.{extension}",
        if cancel {
            "api-cancellations"
        } else {
            "api-startups"
        },
        digest(&startup.key()).unwrap()
    )
}
fn events(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(root.join("data/workflows/project/events"))
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            let raw = fs::read(&p).unwrap();
            (p, raw)
        })
        .collect()
}
fn review(service: &mut ServiceHost, file: &DiagnosticFile, hash: &str) -> Response {
    service.handle(Request {
        id: "review-api".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::ReviewApiPublication {
            relative_path: file.relative_path.clone(),
            evidence_digest: hash.into(),
            reason: "保留原文，不重发请求，不确认远端取消".into(),
        },
    })
}
#[test]
#[ignore = "owned API publication crash child; parent supplies an isolated fixture and boundary"]
fn api_publication_child() {
    let root =
        PathBuf::from(std::env::var_os("CERULEAN_API_PUBLICATION_ROOT").expect("isolated root"));
    let point = std::env::var("CERULEAN_API_PUBLICATION_POINT").unwrap();
    let cancel = std::env::var("CERULEAN_API_PUBLICATION_KIND").unwrap() == "cancel";
    let data = root.join("data");
    let _writer = crate::Store::open(&data).unwrap();
    let startup: ApiStartup =
        serde_json::from_slice(&fs::read(root.join("publication-seed.json")).unwrap()).unwrap();
    let stop = |phase: &str| {
        if phase == point {
            std::process::exit(91);
        }
        Ok(())
    };
    if cancel {
        api_startup::cancel_observed(&data, &startup, "synthetic stop", stop).unwrap();
    } else {
        api_startup::publish_observed(&data, &startup, stop).unwrap();
    }
    panic!("boundary not reached");
}
#[test]
fn ten_real_api_publication_interruptions_preserve_bytes_and_never_replay_requests() {
    for cancel in [false, true] {
        for point in ["created", "partial", "synced", "published", "finished"] {
            let root = tempfile::tempdir().unwrap();
            let data = root.path().join("data");
            let (host, startup) = fixture(root.path(), cancel);
            fs::write(
                root.path().join("publication-seed.json"),
                serde_json::to_vec(&startup).unwrap(),
            )
            .unwrap();
            drop(host);
            if !cancel {
                fs::remove_file(data.join(relative(&startup, false, "json"))).unwrap();
            }
            let before = events(root.path());
            let mut child = Process::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "host::api_publication_tests::api_publication_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("CERULEAN_API_PUBLICATION_ROOT", root.path())
                .env("CERULEAN_API_PUBLICATION_POINT", point)
                .env(
                    "CERULEAN_API_PUBLICATION_KIND",
                    if cancel { "cancel" } else { "startup" },
                )
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("owned publication child timed out");
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            assert_eq!(status.code(), Some(91));
            let pending = data.join(relative(&startup, cancel, "pending"));
            let final_path = data.join(relative(&startup, cancel, "json"));
            let pending_before = fs::read(&pending).ok();
            let final_before = fs::read(&final_path).ok();
            let partial = matches!(point, "created" | "partial");
            let mut service = ServiceHost::open(&data).unwrap();
            assert_eq!(service.is_diagnostic(), partial);
            if partial {
                let file = service
                    .snapshot()
                    .core_diagnostic
                    .unwrap()
                    .api_files
                    .into_iter()
                    .find(|f| f.relative_path == relative(&startup, cancel, "pending"))
                    .unwrap();
                assert_eq!(file.review_kind.as_deref(), Some("api-publication"));
                let hash = file.review_digest.clone().unwrap();
                assert!(review(&mut service, &file, &"0".repeat(64)).error.is_some());
                assert!(review(&mut service, &file, &hash).error.is_none());
                assert!(
                    service.is_diagnostic(),
                    "review alone must not activate scheduling"
                );
            }
            drop(service);
            let mut host = RuntimeHost::open(&data).unwrap();
            assert!(host.api_active.is_empty());
            assert!(host.management_active.is_empty());
            assert_eq!(events(root.path()), before);
            assert_eq!(fs::read(&pending).ok(), pending_before);
            assert_eq!(fs::read(&final_path).ok(), final_before);
            assert_eq!(
                host.projects["project"].state().tasks["task"].state,
                if cancel {
                    TaskState::Unknown
                } else {
                    TaskState::Pending
                }
            );
            if partial && !cancel {
                assert!(host.api_jobs.is_empty());
                assert!(host
                    .start_managed_api("deputy-api", "attempt")
                    .unwrap_err()
                    .contains("禁止复用"));
                assert!(
                    host.api_fault.is_none(),
                    "a known reserved name is not a new I/O fault"
                );
            } else {
                assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Unknown);
            }
            assert_eq!(
                host.snapshot().api_publication_reviews.len(),
                usize::from(partial)
            );
            drop(host);
            let host = RuntimeHost::open(&data).unwrap();
            assert!(host.api_active.is_empty());
            assert_eq!(events(root.path()), before);
            println!("verified API {} owner termination at {point}: preserved bytes, no model request replay",if cancel{"cancel"}else{"startup"});
        }
    }
}
#[test]
fn api_publication_rejects_conflicts_foreign_binding_aliases_and_changed_review() {
    for mode in [
        "conflict",
        "wrong-anchor",
        "wrong-management",
        "wrong-key",
        "invalid-json",
        "changed-reviewed",
        "orphan-cancel",
    ] {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let (host, startup) = fixture(root.path(), false);
        drop(host);
        let path = data.join(relative(&startup, false, "json"));
        let pending = path.with_extension("pending");
        let raw = fs::read(&path).unwrap();
        let before = events(root.path());
        match mode {
            "conflict" => {
                fs::write(&pending, b"{}").unwrap();
            }
            "wrong-anchor" | "wrong-management" => {
                let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
                value["intent"][if mode == "wrong-anchor" {
                    "workflowHash"
                } else {
                    "managementHash"
                }] = serde_json::json!("a".repeat(64));
                value["hash"] = serde_json::json!(digest(
                    &serde_json::from_value::<ApiStartup>(value["intent"].clone()).unwrap()
                )
                .unwrap());
                fs::remove_file(&path).unwrap();
                fs::write(&pending, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            "wrong-key" => {
                fs::remove_file(&path).unwrap();
                fs::write(
                    data.join("api-startups")
                        .join(format!("{}.pending", "a".repeat(64))),
                    &raw,
                )
                .unwrap();
            }
            "invalid-json" => {
                fs::remove_file(&path).unwrap();
                fs::write(&pending, b"garbage").unwrap();
                assert!(
                    api_publication::inspect(&data, &relative(&startup, false, "pending")).is_err()
                );
            }
            "changed-reviewed" => {
                fs::remove_file(&path).unwrap();
                fs::write(&pending, b"{").unwrap();
                let rel = relative(&startup, false, "pending");
                let hash = api_publication::inspect(&data, &rel).unwrap().0;
                api_publication::acknowledge(&data, &rel, &hash, "review", |_| Ok(())).unwrap();
                fs::write(&pending, b"{\"other\":").unwrap();
            }
            "orphan-cancel" => {
                fs::write(
                    data.join("api-cancellations")
                        .join(format!("{}.pending", "b".repeat(64))),
                    b"{",
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(RuntimeHost::open(&data).is_err(), "{mode}");
        assert_eq!(events(root.path()), before);
        assert!(api_publication::inspect(&data, "../api-startups/x.pending").is_err());
        assert!(api_publication::inspect(
            &data,
            &relative(&startup, false, "pending").to_uppercase()
        )
        .is_err());
    }
}
#[test]
fn interrupted_api_review_requires_explicit_confirmation_and_retains_original() {
    for point in ["created", "partial", "synced", "published", "finished"] {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let (host, startup) = fixture(root.path(), false);
        drop(host);
        fs::remove_file(data.join(relative(&startup, false, "json"))).unwrap();
        let rel = relative(&startup, false, "pending");
        let raw = b"{\"private-not-in-diagnostic\":\"unfinished";
        fs::write(data.join(&rel), raw).unwrap();
        let hash = api_publication::inspect(&data, &rel).unwrap().0;
        assert!(
            api_publication::acknowledge(&data, &rel, &hash, "first", |phase| if phase == point {
                Err("interrupt".into())
            } else {
                Ok(())
            })
            .is_err()
        );
        assert_eq!(
            api_publication::reviews(&data).unwrap().len(),
            usize::from(matches!(point, "published" | "finished"))
        );
        let service = ServiceHost::open(&data).unwrap();
        assert!(!serde_json::to_string(&service.snapshot())
            .unwrap()
            .contains("private-not-in-diagnostic"));
        drop(service);
        api_publication::acknowledge(&data, &rel, &hash, "explicit retry", |_| Ok(())).unwrap();
        assert_eq!(fs::read(data.join(&rel)).unwrap(), raw);
        assert_eq!(api_publication::reviews(&data).unwrap().len(), 1);
        let host = RuntimeHost::open(&data).unwrap();
        assert!(host.api_jobs.is_empty());
    }
}

#[test]
#[ignore = "seed an isolated native API publication review fixture; no provider request"]
fn native_api_publication_fixture() {
    let root = PathBuf::from(
        std::env::var_os("CERULEAN_API_PUBLICATION_NATIVE_DIR").expect("isolated native directory"),
    );
    assert!(root.is_absolute());
    assert!(!root.join("data").exists());
    let cancel = std::env::var("CERULEAN_API_PUBLICATION_KIND").unwrap() == "cancel";
    let (host, startup) = fixture(&root, cancel);
    drop(host);
    let data = root.join("data");
    if !cancel {
        fs::remove_file(data.join(relative(&startup, false, "json"))).unwrap();
    }
    let rel = relative(&startup, cancel, "pending");
    fs::write(
        data.join(&rel),
        b"{\"private-api-publication-marker\":\"unfinished",
    )
    .unwrap();
    fs::write(
        root.join("fixture.json"),
        serde_json::to_vec(&serde_json::json!({"data":data,"relativePath":rel,"cancel":cancel}))
            .unwrap(),
    )
    .unwrap();
}
