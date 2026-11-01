use super::*;
use tokio::io::{duplex, split, AsyncReadExt, BufReader};

#[tokio::test]
async fn duplex_backpressure_drains_peer_frames_without_resending_or_reordering() {
    for (outbound, inbound) in [(16, 16), (32768, 16), (16, 32768), (32768, 32768)] {
        let (client, peer) = duplex(4096);
        let (read, write) = split(client);
        let mut transport = Transport::new(BufReader::new(read), write);
        let request =
            json!({"type":"host_tool_result","id":"original","body":"x".repeat(outbound)});
        let expected = request.clone();
        let peer = tokio::spawn(async move {
            let (read, mut write) = split(peer);
            let mut read = BufReader::new(read);
            for sequence in 0..3 {
                write_frame(
                    &mut write,
                    json!({"type":"message_update","sequence":sequence,"body":"d".repeat(inbound)}),
                )
                .await;
            }
            let mut line = String::new();
            read.read_line(&mut line).await.unwrap();
            assert_eq!(serde_json::from_str::<Value>(&line).unwrap(), expected);
            let mut tail = Vec::new();
            read.read_to_end(&mut tail).await.unwrap();
            assert!(tail.is_empty(), "one original send only");
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        transport.send(request, deadline).await.unwrap();
        for sequence in 0..3 {
            let frame = transport.read(deadline).await.unwrap();
            assert_eq!(frame["sequence"], sequence);
            assert_eq!(frame["body"].as_str().unwrap().len(), inbound);
        }
        assert!(transport.inbound.is_empty());
        assert_eq!(transport.inbound_bytes, 0);
        transport.close_input().await.unwrap();
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn duplex_backpressure_rejects_malformed_and_unbounded_peer_frames() {
    for malformed in [true, false] {
        let (client, peer) = duplex(4096);
        let (read, write) = split(client);
        let mut transport = Transport::new(BufReader::new(read), write);
        let peer = tokio::spawn(async move {
            let (_read, mut write) = split(peer);
            if malformed {
                write.write_all(b"not-json\n").await.unwrap();
            } else {
                for _ in 0..1025 {
                    write_frame(&mut write, json!({"type":"message_update"})).await;
                }
            }
            std::future::pending::<()>().await;
        });
        let error = transport
            .send(
                json!({"type":"host_tool_result","body":"x".repeat(32768)}),
                Instant::now() + Duration::from_secs(2),
            )
            .await
            .unwrap_err();
        assert!(
            error.contains(if malformed {
                "JSON invalid"
            } else {
                "buffered observations exceed limit"
            }),
            "{error}"
        );
        assert!(transport.inbound.len() <= 1024);
        assert!(transport.inbound_bytes <= REASSEMBLED_LIMIT);
        peer.abort();
        let _ = peer.await;
    }
}

fn chunks(value: &Value) -> Vec<Value> {
    let bytes = serde_json::to_vec(value).unwrap();
    let count = bytes.len().div_ceil(CHUNK_LIMIT);
    bytes
        .chunks(CHUNK_LIMIT)
        .enumerate()
        .map(|(index, part)| {
            json!({
                "type":"rpc_chunk", "chunkId":"fixture", "index":index,
                "count":count,"byteLength":bytes.len(),"data":STANDARD.encode(part)
            })
        })
        .collect()
}
fn large_value() -> Value {
    json!({"type":"response", "body":"中文🦀".repeat(FRAME_LIMIT / 9)})
}
fn decoder() -> Decoder {
    Decoder {
        version_two: true,
        ..Default::default()
    }
}
#[test]
fn reassembly_preserves_utf8_across_byte_boundaries() {
    let value = large_value();
    let mut decoder = decoder();
    let mut result = None;
    for chunk in chunks(&value) {
        assert!(serde_json::to_vec(&chunk).unwrap().len() < FRAME_LIMIT);
        assert!(result.is_none());
        result = decoder.push(chunk).unwrap();
    }
    assert_eq!(result.unwrap(), value);
}
#[test]
fn rejects_interrupted_reordered_changed_and_noncanonical_chunks() {
    let frames = chunks(&large_value());
    assert!(Decoder::default().push(frames[0].clone()).is_err());
    assert!(decoder().push(frames[1].clone()).is_err());
    for replacement in [
        json!({"type":"ready"}),
        frames[0].clone(),
        {
            let mut f = frames[1].clone();
            f["chunkId"] = json!("other");
            f
        },
        {
            let mut f = frames[1].clone();
            f["byteLength"] = json!(REASSEMBLED_LIMIT + 1);
            f
        },
        {
            let mut f = frames[1].clone();
            f["data"] = json!("Zh==");
            f
        },
        {
            let mut f = frames[1].clone();
            f["data"] = json!(STANDARD.encode(vec![0; CHUNK_LIMIT + 1]));
            f
        },
    ] {
        let mut d = decoder();
        assert!(d.push(frames[0].clone()).unwrap().is_none());
        assert!(d.push(replacement).is_err());
    }
    let mut d = decoder();
    let mut frames = frames;
    frames.last_mut().unwrap()["data"] = json!(STANDARD.encode(b"x"));
    for f in frames.iter().take(frames.len() - 1) {
        assert!(d.push(f.clone()).unwrap().is_none());
    }
    assert!(d.push(frames.last().unwrap().clone()).is_err());
}
fn ready() -> Value {
    json!({"type":"ready","protocolVersion":1,"supportedProtocolVersions":[1,2],
        "maxFrameBytes":FRAME_LIMIT,"maxReassembledFrameBytes":REASSEMBLED_LIMIT})
}
async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, frame: Value) {
    let mut bytes = serde_json::to_vec(&frame).unwrap();
    bytes.push(b'\n');
    writer.write_all(&bytes).await.unwrap();
}
async fn reply<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
    data: Value,
) {
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let request: Value = serde_json::from_str(&line).unwrap();
    write_frame(writer,json!({"type":"response","command":request["type"],"id":request["id"],"success":true,"data":data})).await;
}
#[tokio::test]
async fn negotiates_and_reads_correlated_chunked_state() {
    let (client, peer) = duplex(1024);
    let (read, write) = split(client);
    let mut transport = ReadOnlyTransport::new(BufReader::new(read), write);
    let task = tokio::spawn(async move {
        let (read, mut write) = split(peer);
        let mut read = BufReader::new(read);
        write_frame(&mut write, ready()).await;
        write_frame(
            &mut write,
            json!({"type":"available_commands_update","commands":[{"name":"ignored-command"}]}),
        )
        .await;
        write_frame(
            &mut write,
            json!({"type":"extension_ui_request","method":"setWidget","id":"display"}),
        )
        .await;
        reply(&mut read, &mut write, json!({"protocolVersion":2})).await;
        let mut line = String::new();
        read.read_line(&mut line).await.unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["type"], "get_state");
        let response = json!({"type":"response","command":"get_state","id":request["id"],"success":true,"data":large_value()});
        for frame in chunks(&response) {
            write_frame(&mut write, frame).await;
        }
        let mut tail = Vec::new();
        read.read_to_end(&mut tail).await.unwrap();
        assert!(tail.is_empty());
    });
    transport.connect(Duration::from_secs(5)).await.unwrap();
    assert_eq!(
        transport.get_state(Duration::from_secs(5)).await.unwrap(),
        large_value()
    );
    transport.close_input().await.unwrap();
    task.await.unwrap();
}
#[tokio::test]
async fn bad_response_host_requests_retry_and_eof_poison_connection() {
    for frame in [
        json!({"type":"response","id":"other","command":"get_state","success":true}),
        json!({"type":"response","id":"cerulean-omp-2","command":"prompt","success":true}),
        json!({"type":"response","id":"cerulean-omp-2","command":"get_state","success":false,"error":"secret-body"}),
        json!({"type":"host_tool_call","id":"tool"}),
        json!({"type":"auto_retry_start"}),
        Value::Null,
    ] {
        let (client, peer) = duplex(1024);
        let (read, write) = split(client);
        let mut transport = ReadOnlyTransport::new(BufReader::new(read), write);
        let task = tokio::spawn(async move {
            let (read, mut write) = split(peer);
            let mut read = BufReader::new(read);
            write_frame(&mut write, ready()).await;
            reply(&mut read, &mut write, json!({"protocolVersion":2})).await;
            let mut line = String::new();
            read.read_line(&mut line).await.unwrap();
            if !frame.is_null() {
                write_frame(&mut write, frame).await;
            }
        });
        transport.connect(Duration::from_secs(2)).await.unwrap();
        let error = transport
            .get_state(Duration::from_secs(2))
            .await
            .unwrap_err();
        assert!(!error.contains("secret-body"));
        assert!(transport
            .get_state(Duration::from_secs(2))
            .await
            .unwrap_err()
            .contains("unavailable"));
        task.await.unwrap();
    }
}
#[tokio::test]
async fn dropping_inflight_future_prevents_reuse_and_resend() {
    let (client, peer) = duplex(1024);
    let (read, write) = split(client);
    let mut transport = ReadOnlyTransport::new(BufReader::new(read), write);
    let (sent, received) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let (read, mut write) = split(peer);
        let mut read = BufReader::new(read);
        write_frame(&mut write, ready()).await;
        reply(&mut read, &mut write, json!({"protocolVersion":2})).await;
        let mut line = String::new();
        read.read_line(&mut line).await.unwrap();
        write.write_all(b"{\"type\":").await.unwrap();
        sent.send(()).unwrap();
        let mut tail = Vec::new();
        read.read_to_end(&mut tail).await.unwrap();
        assert!(tail.is_empty());
    });
    transport.connect(Duration::from_secs(2)).await.unwrap();
    tokio::select! {
        value=transport.get_state(Duration::from_secs(5)) => panic!("unexpected response: {value:?}"),
        _=received => {}
    }
    assert!(transport.get_state(Duration::from_secs(1)).await.is_err());
    transport.close_input().await.unwrap();
    task.await.unwrap();
}
#[tokio::test]
async fn physical_frame_and_timeout_bounds_are_enforced() {
    let mut transport = ReadOnlyTransport::new(
        std::io::Cursor::new(vec![b'x'; FRAME_LIMIT + 1]),
        tokio::io::sink(),
    );
    assert!(transport
        .connect(Duration::from_secs(2))
        .await
        .unwrap_err()
        .contains("limit"));
    let (client, _peer) = duplex(1024);
    let (read, write) = split(client);
    let mut transport = ReadOnlyTransport::new(BufReader::new(read), write);
    assert!(transport
        .connect(Duration::from_millis(10))
        .await
        .unwrap_err()
        .contains("timeout"));
    assert!(transport.connect(Duration::from_secs(1)).await.is_err());
}
