use super::*;
use cerulean_adapters::api_generation::gateway::{GatewayRequestId, GatewayRequestIdRejection};

fn binding() -> Binding {
    Binding {
        lane: Lane::Worker,
        project_id: "project".into(),
        local_id: "local-request".into(),
        request_digest: "a".repeat(64),
        intent_hash: "b".repeat(64),
    }
}
fn headers() -> GatewayHeaders {
    GatewayHeaders {
        status: 200,
        request_id: GatewayRequestId::Accepted {
            value: "gateway-different-324".into(),
        },
    }
}
fn source() -> CompletionSource {
    CompletionSource {
        record_hash: "c".repeat(64),
        outcome_digest: "d".repeat(64),
        outcome: Outcome::Reply {},
        body_response_id: Some("body-different-324".into()),
    }
}

#[test]
fn gateway_324_header_only_and_completion_keep_three_independent_ids() {
    let root = tempfile::tempdir().unwrap();
    let original = binding();
    let mut attempt = GatewayAttempt::new(root.path(), original.clone());
    attempt.headers(&headers());
    let first = read(root.path(), &original, None).unwrap();
    assert_eq!(first.status, "headerOnly");
    assert_eq!(
        first.gateway_request_id.as_deref(),
        Some("gateway-different-324")
    );
    assert!(first.body_response_id.is_none());
    let original_source = source();
    attempt.finish(original_source.clone()).unwrap();
    let final_read = read(root.path(), &original, Some(&original_source)).unwrap();
    assert_eq!(final_read.status, "linked");
    assert_eq!(final_read.local_id, "local-request");
    assert_eq!(
        final_read.body_response_id.as_deref(),
        Some("body-different-324")
    );
    assert!(
        read(root.path(), &original, None).is_err(),
        "sidecar cannot create an original outcome"
    );
    let mut different = original_source;
    different.record_hash = "e".repeat(64);
    assert!(read(root.path(), &original, Some(&different)).is_err());
}

#[test]
fn gateway_324_missing_rejected_and_no_headers_are_distinct_without_raw_bytes() {
    for request_id in [
        GatewayRequestId::Missing {},
        GatewayRequestId::Rejected {
            reason: GatewayRequestIdRejection::RepeatedHeader,
        },
    ] {
        let root = tempfile::tempdir().unwrap();
        let b = binding();
        let mut attempt = GatewayAttempt::new(root.path(), b.clone());
        attempt.headers(&GatewayHeaders {
            status: 429,
            request_id: request_id.clone(),
        });
        let s = CompletionSource {
            outcome: Outcome::Http { status: 429 },
            body_response_id: None,
            ..source()
        };
        attempt.finish(s.clone()).unwrap();
        let review = read(root.path(), &b, Some(&s)).unwrap();
        assert_eq!(review.status, "linked");
        assert_eq!(review.headers.unwrap().request_id, request_id);
        assert!(review.gateway_request_id.is_none());
    }
    let root = tempfile::tempdir().unwrap();
    let b = binding();
    assert_eq!(read(root.path(), &b, None).unwrap().status, "missing");
    assert!(
        !root.path().join(DIRECTORY).exists(),
        "read must not initialize"
    );
    let attempt = GatewayAttempt::new(root.path(), b.clone());
    let s = CompletionSource {
        outcome: Outcome::Transport {},
        body_response_id: None,
        ..source()
    };
    attempt.finish(s.clone()).unwrap();
    assert_eq!(
        read(root.path(), &b, Some(&s)).unwrap().status,
        "noHeadersObserved"
    );
}

#[test]
fn gateway_324_partial_conflicting_and_tampered_records_are_never_repaired() {
    for case in [
        "pending",
        "changed-binding",
        "changed-id",
        "missing-header",
        "oversized",
    ] {
        let root = tempfile::tempdir().unwrap();
        let b = binding();
        let mut attempt = GatewayAttempt::new(root.path(), b.clone());
        attempt.headers(&headers());
        let s = source();
        attempt.finish(s.clone()).unwrap();
        let file = path(root.path(), &b, "headers").unwrap();
        match case {
            "pending" => fs::write(file.with_extension("pending"), b"partial").unwrap(),
            "missing-header" => fs::remove_file(&file).unwrap(),
            "oversized" => fs::write(&file, vec![b'x'; LIMIT as usize + 1]).unwrap(),
            _ => {
                let mut r: Record<HeaderRecord> =
                    serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
                if case == "changed-binding" {
                    r.body.binding.intent_hash = "f".repeat(64);
                } else {
                    r.body.observation.request_id = GatewayRequestId::Accepted {
                        value: "other".into(),
                    };
                }
                r.hash = digest(&r.body).unwrap();
                fs::write(&file, serde_json::to_vec(&r).unwrap()).unwrap();
            }
        }
        let before = fs::read(&file).ok();
        assert!(read(root.path(), &b, Some(&s)).is_err(), "{case}");
        assert_eq!(fs::read(&file).ok(), before, "read must not repair {case}");
    }
}

#[test]
fn gateway_324_write_failures_and_second_callback_do_not_overwrite_original_headers() {
    let root = tempfile::tempdir().unwrap();
    let b = binding();
    let mut attempt = GatewayAttempt::new(root.path(), b.clone());
    attempt.headers(&headers());
    let file = path(root.path(), &b, "headers").unwrap();
    let before = fs::read(&file).unwrap();
    let mut changed = headers();
    changed.request_id = GatewayRequestId::Accepted {
        value: "second".into(),
    };
    attempt.headers(&changed);
    let s = source();
    attempt.finish(s.clone()).unwrap();
    assert_eq!(fs::read(file).unwrap(), before);
    let r = read(root.path(), &b, Some(&s)).unwrap();
    assert_eq!(r.status, "unavailable");
    assert!(r.gateway_request_id.is_none());

    let blocked = tempfile::tempdir().unwrap();
    fs::write(
        blocked.path().join(DIRECTORY),
        b"ordinary file prevents directory",
    )
    .unwrap();
    let mut capture = GatewayAttempt::new(blocked.path(), b.clone());
    capture.headers(&headers()); // no panic or propagated operation error
    assert!(capture.finish(s).is_err());
    assert!(read(blocked.path(), &b, None).is_err());
}

#[cfg(windows)]
#[test]
fn gateway_324_backup_inventory_captures_auxiliary_files_without_reader_promotion() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    drop(crate::Store::open(&data).unwrap());
    crate::relay_guard::tests::promote_fixture(&data, &root.path().join("before16"));
    let before = crate::data_format::status(&data)
        .unwrap()
        .minimum_reader_generation;
    let base = data.join("workflows/project/api-attempt-fixture");
    fs::create_dir_all(&base).unwrap();
    let mut capture = GatewayAttempt::new(&base, binding());
    capture.headers(&headers());
    capture.finish(source()).unwrap();
    // Inventory reads ordinary subtrees recursively; no task replay or remote call.
    let inventory = crate::backup::inventory(&data, false).unwrap();
    assert_eq!(
        inventory
            .nodes
            .iter()
            .filter(|n| !n.entry.directory && n.entry.path.contains("/gateway-response-evidence/"))
            .count(),
        2
    );
    drop(inventory);
    let backup = root.path().join("with-gateway");
    crate::backup::create(&data, &backup).unwrap();
    assert_eq!(
        crate::data_format::status(&data)
            .unwrap()
            .minimum_reader_generation,
        before
    );
    assert_eq!(before, 16);
    assert_eq!(
        read(&base, &binding(), Some(&source())).unwrap().status,
        "linked"
    );
}
