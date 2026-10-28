//! Synthetic loopback-only transport tests. No production URL or credential is
//! used. Header observation is synchronized explicitly, never inferred by sleep.
use super::gateway::{GatewayHeaders, GatewayRequestId};
use super::*;
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const SYNTHETIC_KEY: &str = "synthetic-gateway-324-local-only";
const GATEWAY_ID: &str = "gateway-324:local.response-id";
const UNRELATED_HEADER: &str = "unrelated-header-must-not-be-retained";
const ERROR_BODY: &str = "raw-error-body-must-not-be-retained";
const TEST_DEADLINE: Duration = Duration::from_secs(6);

fn target() -> ApiTarget {
    ApiTarget::Provider {
        endpoint: ProviderEndpoint::A6Api {},
    }
}

fn client() -> ApiClient {
    ApiClient {
        http: ApiClient::http(false).unwrap(),
        key: SecretInput::new(SYNTHETIC_KEY.into()).unwrap(),
    }
}

async fn read_one_post(socket: &mut TcpStream) {
    let read = async {
        let mut bytes = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            let count = socket.read(&mut chunk).await.unwrap();
            assert!(count > 0, "request ended before its declared body");
            bytes.extend_from_slice(&chunk[..count]);
            assert!(
                bytes.len() <= 65536,
                "synthetic request exceeded test bound"
            );
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                assert!(bytes.starts_with(b"POST /gateway-fixture HTTP/1.1\r\n"));
                let header = std::str::from_utf8(&bytes[..end])
                    .unwrap()
                    .to_ascii_lowercase();
                let length: usize = header
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                assert!(length <= 32768);
                if bytes.len() >= end + 4 + length {
                    // No second request or pipelined bytes share this socket.
                    assert_eq!(bytes.len(), end + 4 + length);
                    break;
                }
            }
        }
    };
    tokio::time::timeout(TEST_DEADLINE, read).await.unwrap();
}

async fn accept_post(listener: &TcpListener) -> TcpStream {
    let (mut socket, _) = tokio::time::timeout(TEST_DEADLINE, listener.accept())
        .await
        .unwrap()
        .unwrap();
    read_one_post(&mut socket).await;
    socket
}

async fn no_retry(listener: TcpListener) {
    // The original client future has terminated before this check. Any retry
    // already queued by that attempt must be accepted here and fail the test.
    assert!(
        tokio::time::timeout(Duration::from_millis(75), listener.accept())
            .await
            .is_err(),
        "unexpected second connection from one attempt"
    );
}

fn response_headers(status: u16, content_length: usize) -> Vec<u8> {
    let reason = if status == 429 {
        "Too Many Requests"
    } else {
        "OK"
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {content_length}\r\nConnection: close\r\nX-Oneapi-Request-Id: {GATEWAY_ID}\r\nX-Unrelated-Private: {UNRELATED_HEADER}\r\n\r\n"
    ).into_bytes()
}

fn assert_observation(observed: &[GatewayHeaders], status: u16) {
    assert_eq!(
        observed.len(),
        1,
        "each response supplies exactly one callback"
    );
    assert_eq!(
        observed[0],
        GatewayHeaders {
            status,
            request_id: GatewayRequestId::Accepted {
                value: GATEWAY_ID.into()
            },
        }
    );
    observed[0].validate().unwrap();
    let serialized = serde_json::to_string(&observed[0]).unwrap();
    assert!(!serialized.contains(UNRELATED_HEADER));
    assert!(!serialized.contains(ERROR_BODY));
    assert!(!serialized.contains(SYNTHETIC_KEY));
    let debug = format!("{:?}", observed[0]);
    assert!(debug.contains("[SAFE_GATEWAY_ID]"));
    for private in [GATEWAY_ID, UNRELATED_HEADER, ERROR_BODY, SYNTHETIC_KEY] {
        assert!(!debug.contains(private));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SocketCase {
    Json,
    Http,
    BodyDisconnect,
    BodyTimeout,
    InvalidJson,
    Oversized,
    RequestTimeout,
}

async fn socket_case(case: SocketCase) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/gateway-fixture", listener.local_addr().unwrap());
    let (observed_tx, observed_rx) = oneshot::channel();
    let (done_tx, done_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let mut socket = accept_post(&listener).await;
        if case != SocketCase::RequestTimeout {
            let (status, length) = match case {
                SocketCase::Json => (200, 2),
                SocketCase::Http => (429, ERROR_BODY.len()),
                SocketCase::BodyDisconnect | SocketCase::BodyTimeout => (200, 100),
                SocketCase::InvalidJson => (200, ERROR_BODY.len()),
                SocketCase::Oversized => (200, MAX_RESPONSE + 1),
                SocketCase::RequestTimeout => unreachable!(),
            };
            socket
                .write_all(&response_headers(status, length))
                .await
                .unwrap();
            // This signal proves observation ran before any body bytes arrived.
            tokio::time::timeout(TEST_DEADLINE, observed_rx)
                .await
                .unwrap()
                .unwrap();
            match case {
                SocketCase::Json => socket.write_all(b"{}").await.unwrap(),
                SocketCase::InvalidJson => socket.write_all(ERROR_BODY.as_bytes()).await.unwrap(),
                SocketCase::BodyDisconnect => {
                    socket.write_all(b"{").await.unwrap();
                    socket.shutdown().await.unwrap();
                }
                SocketCase::BodyTimeout => socket.write_all(b"{").await.unwrap(),
                SocketCase::Http | SocketCase::Oversized => (),
                SocketCase::RequestTimeout => unreachable!(),
            }
        }
        // HTTP errors/oversized lengths must complete without waiting for the
        // advertised body. Timeout cases keep their socket open, not sleep.
        tokio::time::timeout(TEST_DEADLINE, done_rx)
            .await
            .unwrap()
            .unwrap();
        if case == SocketCase::Http {
            // Deliberately release the error body only after the caller already
            // returned Http; success of this write is immaterial after close.
            let _ = socket.write_all(ERROR_BODY.as_bytes()).await;
        }
        drop(socket);
        no_retry(listener).await;
    });
    let client = client();
    let (cancel_tx, mut cancel) = oneshot::channel();
    let mut observations = Vec::new();
    let mut observed_tx = Some(observed_tx);
    let mut stage = None;
    let result = {
        let mut observer = |headers: &GatewayHeaders| {
            observations.push(headers.clone());
            observed_tx
                .take()
                .expect("callback invoked more than once")
                .send(())
                .unwrap();
        };
        tokio::time::timeout(
            TEST_DEADLINE,
            client.send_json_captured(
                &target(),
                &json!({"request":"synthetic-local"}),
                1,
                &mut cancel,
                &url,
                &mut stage,
                &mut observer,
            ),
        )
        .await
        .unwrap()
    };
    done_tx.send(()).unwrap();
    server.await.unwrap();
    drop(cancel_tx);
    if case == SocketCase::RequestTimeout {
        assert!(observations.is_empty());
    } else {
        assert_observation(
            &observations,
            if case == SocketCase::Http { 429 } else { 200 },
        );
    }
    let expected_stage = match case {
        SocketCase::BodyDisconnect => Some(TransportStage::BodyRead),
        SocketCase::BodyTimeout => Some(TransportStage::BodyTimeout),
        SocketCase::RequestTimeout => Some(TransportStage::RequestTimeout),
        _ => None,
    };
    assert_eq!(stage, expected_stage, "{case:?}");
    let expected = match case {
        SocketCase::Json => Ok(json!({})),
        SocketCase::Http => Err(ApiFailure::Http { status: 429 }),
        SocketCase::InvalidJson => Err(ApiFailure::ResponseRejected {
            reason: ResponseRejection::InvalidJson,
        }),
        SocketCase::Oversized => Err(ApiFailure::Oversized {}),
        SocketCase::BodyDisconnect | SocketCase::BodyTimeout | SocketCase::RequestTimeout => {
            Err(ApiFailure::Transport {})
        }
    };
    assert_eq!(result, expected, "{case:?}");
    if let Err(error) = result {
        let serialized = serde_json::to_string(&error).unwrap();
        let debug = format!("{error:?}");
        for raw in [ERROR_BODY, UNRELATED_HEADER, SYNTHETIC_KEY, GATEWAY_ID] {
            assert!(!serialized.contains(raw));
            assert!(!debug.contains(raw));
        }
    }
}

#[tokio::test]
async fn gateway_324_success_observes_header_before_body_without_retry() {
    socket_case(SocketCase::Json).await;
}

#[tokio::test]
async fn gateway_324_http_429_observes_header_and_does_not_wait_for_error_body() {
    socket_case(SocketCase::Http).await;
}

#[tokio::test]
async fn gateway_324_body_disconnect_retains_preceding_header_observation() {
    socket_case(SocketCase::BodyDisconnect).await;
}

#[tokio::test]
async fn gateway_324_body_timeout_retains_preceding_header_observation() {
    socket_case(SocketCase::BodyTimeout).await;
}

#[tokio::test]
async fn gateway_324_invalid_json_keeps_header_and_discards_raw_body() {
    socket_case(SocketCase::InvalidJson).await;
}

#[tokio::test]
async fn gateway_324_oversized_content_length_observes_header_without_reading_body() {
    socket_case(SocketCase::Oversized).await;
}

#[tokio::test]
async fn gateway_324_request_timeout_has_no_observed_headers_and_no_retry() {
    socket_case(SocketCase::RequestTimeout).await;
}

#[tokio::test]
async fn gateway_324_cancellation_after_observer_signal_preserves_header() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/gateway-fixture", listener.local_addr().unwrap());
    let (done_tx, done_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let mut socket = accept_post(&listener).await;
        socket.write_all(&response_headers(200, 100)).await.unwrap();
        // Never send body bytes: the observer explicitly triggers cancellation.
        tokio::time::timeout(TEST_DEADLINE, done_rx)
            .await
            .unwrap()
            .unwrap();
        drop(socket);
        no_retry(listener).await;
    });
    let client = client();
    let (cancel_tx, mut cancel) = oneshot::channel();
    let (observed_tx, observed_rx) = oneshot::channel();
    let mut observed_tx = Some(observed_tx);
    let mut observations = Vec::new();
    let mut stage = None;
    let result = {
        let mut observer = |headers: &GatewayHeaders| {
            observations.push(headers.clone());
            observed_tx
                .take()
                .expect("callback invoked more than once")
                .send(())
                .unwrap();
        };
        let local_target = target();
        let body = json!({"request":"synthetic-cancellation"});
        let request = client.send_json_captured(
            &local_target,
            &body,
            3,
            &mut cancel,
            &url,
            &mut stage,
            &mut observer,
        );
        let cancel_after_header = async move {
            observed_rx.await.unwrap();
            cancel_tx.send(()).unwrap();
        };
        tokio::time::timeout(TEST_DEADLINE, async {
            let (result, ()) = tokio::join!(request, cancel_after_header);
            result
        })
        .await
        .unwrap()
    };
    done_tx.send(()).unwrap();
    server.await.unwrap();
    assert_eq!(result.unwrap_err(), ApiFailure::LocalCancellation {});
    assert!(stage.is_none());
    assert_observation(&observations, 200);
}

async fn decoded_case(valid: bool) {
    let request = super::tests::request(target());
    let body = if valid {
        super::tests::reply(&request, false)
    } else {
        // Valid JSON, but not a valid provider reply or accepted workflow result.
        json!({"failure": ERROR_BODY})
    };
    let bytes = serde_json::to_vec(&body).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/gateway-fixture", listener.local_addr().unwrap());
    let (observed_tx, observed_rx) = oneshot::channel();
    let (done_tx, done_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let mut socket = accept_post(&listener).await;
        socket
            .write_all(&response_headers(200, bytes.len()))
            .await
            .unwrap();
        tokio::time::timeout(TEST_DEADLINE, observed_rx)
            .await
            .unwrap()
            .unwrap();
        socket.write_all(&bytes).await.unwrap();
        tokio::time::timeout(TEST_DEADLINE, done_rx)
            .await
            .unwrap()
            .unwrap();
        drop(socket);
        no_retry(listener).await;
    });
    let client = client();
    let prepared = client.prepare(request).unwrap();
    let (cancel_tx, mut cancel) = oneshot::channel();
    let mut observed_tx = Some(observed_tx);
    let mut observations = Vec::new();
    let result = {
        let mut observer = |headers: &GatewayHeaders| {
            observations.push(headers.clone());
            observed_tx
                .take()
                .expect("callback invoked more than once")
                .send(())
                .unwrap();
        };
        tokio::time::timeout(
            TEST_DEADLINE,
            client.send_url_with_gateway(prepared, &mut cancel, &url, &mut observer),
        )
        .await
        .unwrap()
    };
    done_tx.send(()).unwrap();
    server.await.unwrap();
    drop(cancel_tx);
    assert_observation(&observations, 200);
    if valid {
        let reply = result.unwrap();
        assert_eq!(reply.observation().response_id, "chat-1");
        assert_ne!(reply.observation().response_id, GATEWAY_ID);
        // The existing authoritative observation wire shape remains unchanged.
        let serialized = serde_json::to_string(reply.observation()).unwrap();
        assert!(!serialized.contains(GATEWAY_ID));
        assert!(!serialized.contains("gateway"));
        assert!(!serialized.contains(UNRELATED_HEADER));
    } else {
        let failure = result.unwrap_err();
        assert_eq!(
            failure,
            ApiFailure::ResponseRejected {
                reason: ResponseRejection::UnsupportedSchema
            }
        );
        assert!(!serde_json::to_string(&failure)
            .unwrap()
            .contains(ERROR_BODY));
        assert!(!format!("{failure:?}").contains(ERROR_BODY));
    }
}

#[tokio::test]
async fn gateway_324_decoded_reply_keeps_body_id_distinct_and_legacy_wire_unchanged() {
    decoded_case(true).await;
}

#[tokio::test]
async fn gateway_324_protocol_rejection_does_not_erase_observed_header() {
    decoded_case(false).await;
}

#[test]
fn gateway_324_original_and_observed_futures_remain_send_without_polling() {
    fn assert_send<T: Send>(_: T) {}
    let client = client();
    let (_keep, mut cancel) = oneshot::channel();
    let prepared = client.prepare(super::tests::request(target())).unwrap();
    assert_send(client.send(prepared, &mut cancel));
    let prepared = client.prepare(super::tests::request(target())).unwrap();
    let mut observer = |_: &GatewayHeaders| {};
    assert_send(client.send_with_gateway(prepared, &mut cancel, &mut observer));
    let prepared = client.prepare(super::tests::request(target())).unwrap();
    assert_send(client.send_evidence_json(&prepared, &mut cancel));
    assert_send(client.send_evidence_json_with_gateway(&prepared, &mut cancel, &mut observer));
    let context = json!({"taskId":"synthetic"});
    let handoff = cerulean_domain::management_ingress::ManagementHandoff {
        operation_id: "send-check".into(),
        project_id: "project".into(),
        purpose: cerulean_domain::management::Purpose::Advise,
        source: cerulean_domain::management_ingress::ManagementSource::DeepSeek {
            model: "synthetic-model".into(),
        },
        state_digest: cerulean_domain::execution::digest(&context).unwrap(),
        context,
    };
    let prepared = crate::deputy_management::prepare(&client, handoff.clone()).unwrap();
    assert_send(crate::deputy_management::send_evidenced(
        &client,
        prepared,
        &mut cancel,
    ));
    let prepared = crate::deputy_management::prepare(&client, handoff).unwrap();
    assert_send(crate::deputy_management::send_evidenced_with_gateway(
        &client,
        prepared,
        &mut cancel,
        &mut observer,
    ));
    // Each future was dropped unpolled: no HTTP request, socket, or task was started.
}
