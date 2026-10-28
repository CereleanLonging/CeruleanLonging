#![cfg(windows)]
use cerulean_core::ipc::{Client, UserOperation};
use cerulean_domain::{Command, Request, PROTOCOL_VERSION};
use std::{path::Path, time::Duration};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_client_start_uses_one_resident_and_shutdown_releases_writer() {
    let dir = tempfile::tempdir().unwrap();
    let exe = Path::new(env!("CARGO_BIN_EXE_cerulean-core"));
    let (a, b) = tokio::join!(
        Client::connect_or_start(dir.path(), exe),
        Client::connect_or_start(dir.path(), exe)
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.server_pid, b.server_pid);
    for (i, client) in [a, b].into_iter().enumerate() {
        let result = client
            .call(UserOperation::Request {
                request: Request {
                    id: i.to_string(),
                    protocol_version: PROTOCOL_VERSION,
                    command: Command::Snapshot,
                },
            })
            .await
            .unwrap();
        assert!(result.response.error.is_none());
    }
    let stop = Client::connect(dir.path())
        .await
        .unwrap()
        .call(UserOperation::Shutdown {
            id: "stop".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(stop.stopping);
    let mut released = false;
    for _ in 0..100 {
        if cerulean_core::Store::open(dir.path()).is_ok() {
            released = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(released, "resident failed to release writer");
    // A losing concurrent starter must not take over after the selected service
    // has exited. Watch past the client's 50 ms startup polling interval.
    for _ in 0..25 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            Client::connect(dir.path()).await.is_err(),
            "late starter resurrected a stopped resident"
        );
        assert!(
            cerulean_core::Store::open(dir.path()).is_ok(),
            "late starter reacquired writer"
        );
    }
}
