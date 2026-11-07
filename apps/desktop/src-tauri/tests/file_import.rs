#![cfg(windows)]
#[path = "../src/file_import_process.rs"]
mod file_import_process;
#[path = "../src/file_import_service.rs"]
mod file_import_service;

#[test]
fn actual_standard_child_cancel_and_owner_binding() {
    use file_import_service::{Request, Service};
    let root = tempfile::tempdir().unwrap();
    let imports = Service::new(root.path().join("imports"));
    let opened = imports.run("main", Request::Start).unwrap();
    let id = opened["session"].as_str().unwrap().to_owned();
    assert!(opened["pid"].as_u64().unwrap() > 0);
    assert!(imports
        .run(
            "other-window",
            Request::Poll {
                session: id.clone()
            }
        )
        .is_err());
    assert!(imports.run("main", Request::Start).is_err());
    assert!(imports
        .run(
            "main",
            Request::Finish {
                session: id.clone()
            }
        )
        .is_err());
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert_eq!(
        imports
            .run(
                "main",
                Request::Poll {
                    session: id.clone()
                }
            )
            .unwrap()["status"],
        "waiting"
    );
    imports
        .run("main", Request::Cancel { session: id })
        .unwrap();
    assert_eq!(
        std::fs::read_dir(root.path().join("imports"))
            .unwrap()
            .count(),
        0
    );
    let _ = imports.run("main", Request::Start).unwrap();
    imports.close_owner("main").unwrap();
    assert_eq!(
        std::fs::read_dir(root.path().join("imports"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
#[ignore = "The native UI driver selects a synthetic file in the real standard-privilege picker"]
fn actual_picker_selection() {
    use file_import_service::{Request, Service};
    let report =
        std::path::PathBuf::from(std::env::var_os("CERULEAN_IMPORT_PROBE_REPORT").unwrap());
    let root = tempfile::tempdir().unwrap();
    let selected = root.path().join("要求.md");
    let data = format!(
        "# 独立窗口测试\n{}",
        "保留中文和 367 项检查。\n".repeat(6000)
    );
    std::fs::write(&selected, data.as_bytes()).unwrap();
    let imports = Service::new(root.path().join("imports"));
    let opened = imports.run("main", Request::Start).unwrap();
    std::fs::write(
        &report,
        serde_json::to_vec(&serde_json::json!({
            "pid":opened["pid"],"selected":selected,"hostPid":std::process::id(),
            "ownerSid":file_import_process::current_owner().unwrap()
        }))
        .unwrap(),
    )
    .unwrap();
    let id = opened["session"].as_str().unwrap().to_owned();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let status = loop {
        let status = imports
            .run(
                "main",
                Request::Poll {
                    session: id.clone(),
                },
            )
            .unwrap();
        if status["status"] == "ready" {
            break status;
        }
        assert_eq!(status["status"], "waiting");
        assert!(
            std::time::Instant::now() < deadline,
            "native file selection timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    assert_eq!(status["name"], "要求.md");
    let mut received = Vec::new();
    loop {
        let chunk = imports
            .run(
                "main",
                Request::Read {
                    session: id.clone(),
                    offset: received.len() as u64,
                },
            )
            .unwrap();
        received.extend(serde_json::from_value::<Vec<u8>>(chunk["bytes"].clone()).unwrap());
        if chunk["done"] == true {
            break;
        }
    }
    assert_eq!(received, data.as_bytes());
    imports
        .run("main", Request::Finish { session: id })
        .unwrap();
    assert_eq!(
        std::fs::read_dir(root.path().join("imports"))
            .unwrap()
            .count(),
        0
    );
    println!(
        "Native file picker returned {} verified bytes and cleaned its session",
        received.len()
    );
}
