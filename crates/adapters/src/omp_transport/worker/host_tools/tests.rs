use super::*;
use crate::omp_transport::worker::tests::{eof, intent, pair, read, respond, setup, state, PROMPT};
use tokio::sync::oneshot;

fn tool() -> HostToolDefinition {
    HostToolDefinition {
        name: "cerulean_read_file".into(),
        description: "Read an explicitly granted project file.".into(),
        parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
    }
}
fn identity() -> SessionIdentity {
    intent().validate_metadata().unwrap().0
}

#[tokio::test]
async fn registration_matches_ack_and_live_schema_and_does_not_allow_toolless_dispatch() {
    let (mut client, peer) = pair();
    let peer = tokio::spawn(async move {
        let (mut r, mut w) = setup(peer, true).await;
        let q = read(&mut r).await;
        assert_eq!(q["type"], "set_host_tools");
        assert_eq!(q["tools"], json!([tool()]));
        respond(&mut w, &q, json!({"toolNames":[tool().name]})).await;
        let q = read(&mut r).await;
        let mut active = state();
        active["dumpTools"] = json!([tool()]);
        respond(&mut w, &q, active.clone()).await;
        // Existing execution route deliberately remains no-tool until the host
        // handler path is implemented; registering metadata cannot grant it.
        let q = read(&mut r).await;
        assert_eq!(q["type"], "get_state");
        respond(&mut w, &q, active).await;
        eof(&mut r).await;
    });
    client.connect(Duration::from_secs(2)).await.unwrap();
    client
        .register_host_tools(&identity(), &[tool()])
        .await
        .unwrap();
    assert_eq!(client.registered_host_tools(), Some([tool()].as_slice()));
    assert!(client
        .register_host_tools(&identity(), &[tool()])
        .await
        .is_err());
    let (_cancel, mut rx) = oneshot::channel();
    assert!(client
        .execute(
            intent(),
            PROMPT,
            Duration::from_secs(2),
            &mut rx,
            |_| panic!("no dispatch intent allowed")
        )
        .await
        .is_err());
    assert!(!client.command_attempts().contains_key("prompt"));
    client.close_input().await.unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn changed_ack_schema_identity_or_native_tools_poison_registration() {
    for mode in ["duplicate-ack", "extra-ack", "schema", "identity", "native"] {
        let (mut client, peer) = pair();
        let peer = tokio::spawn(async move {
            let (mut r, mut w) = setup(peer, false).await;
            let q = read(&mut r).await;
            assert_eq!(q["type"], "get_state");
            let mut initial = state();
            if mode == "native" {
                initial["dumpTools"] = json!([{"name":"bash"}]);
            }
            respond(&mut w, &q, initial).await;
            if mode != "native" {
                let q = read(&mut r).await;
                assert_eq!(q["type"], "set_host_tools");
                let names = match mode {
                    "duplicate-ack" => json!([tool().name, tool().name]),
                    "extra-ack" => json!(["unapproved"]),
                    _ => json!([tool().name]),
                };
                respond(&mut w, &q, json!({"toolNames":names})).await;
                if !matches!(mode, "duplicate-ack" | "extra-ack") {
                    let q = read(&mut r).await;
                    let mut active = state();
                    active["dumpTools"] = json!([tool()]);
                    if mode == "schema" {
                        active["dumpTools"][0]["parameters"]["additionalProperties"] = json!(true);
                    }
                    if mode == "identity" {
                        active["sessionId"] = json!("other-session");
                    }
                    respond(&mut w, &q, active).await;
                }
            }
            eof(&mut r).await;
        });
        client.connect(Duration::from_secs(2)).await.unwrap();
        assert!(
            client
                .register_host_tools(&identity(), &[tool()])
                .await
                .is_err(),
            "{mode}"
        );
        assert!(client
            .register_host_tools(&identity(), &[tool()])
            .await
            .is_err());
        assert!(client.registered_host_tools().is_none());
        assert!(!client.command_attempts().contains_key("prompt"));
        client.close_input().await.unwrap();
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn invalid_definitions_never_send_registration_bytes() {
    let (mut client, mut peer) = pair();
    let mut changed = tool();
    changed.parameters["additionalProperties"] = json!(true);
    assert!(changed.validate().is_err());
    changed = tool();
    changed.name = "bash".into();
    assert!(changed.validate().is_err());
    // Even a fresh unconnected stream must refuse before RPC or journal effects.
    assert!(client
        .register_host_tools(&identity(), &[changed])
        .await
        .is_err());
    assert!(client.command_attempts().is_empty());
    client.close_input().await.unwrap();
    use tokio::io::AsyncReadExt;
    let mut wire = Vec::new();
    peer.read_to_end(&mut wire).await.unwrap();
    assert!(wire.is_empty());
}

#[tokio::test]
async fn connected_invalid_schema_and_duplicates_preserve_channel_without_wire() {
    let (mut client, peer) = pair();
    let peer = tokio::spawn(async move {
        let (mut r, _w) = setup(peer, false).await;
        eof(&mut r).await;
    });
    client.connect(Duration::from_secs(2)).await.unwrap();
    let before = client.command_attempts().clone();
    let mut invalid = tool();
    invalid.parameters["additionalProperties"] = json!(true);
    assert!(client
        .register_host_tools(&identity(), &[invalid])
        .await
        .is_err());
    assert!(client
        .register_host_tools(&identity(), &[tool(), tool()])
        .await
        .is_err());
    assert!(client.register_host_tools(&identity(), &[]).await.is_err());
    assert_eq!(&before, client.command_attempts());
    assert!(!client.rpc.broken);
    client.close_input().await.unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn interrupted_registration_never_retries_and_never_dispatches() {
    let (mut client, peer) = pair();
    let (notice, received) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (mut r, _w) = setup(peer, true).await;
        let q = read(&mut r).await;
        assert_eq!(q["type"], "set_host_tools");
        notice.send(()).unwrap();
        eof(&mut r).await;
    });
    client.connect(Duration::from_secs(2)).await.unwrap();
    {
        let tools = [tool()];
        let scope = identity();
        let registration = client.register_host_tools(&scope, &tools);
        tokio::pin!(registration);
        tokio::select! {r=&mut registration=>panic!("registration unexpectedly ended: {r:?}"),_=received=>{}}
    }
    assert!(client
        .register_host_tools(&identity(), &[tool()])
        .await
        .is_err());
    let (_cancel, mut rx) = oneshot::channel();
    assert!(client
        .execute(
            intent(),
            PROMPT,
            Duration::from_secs(2),
            &mut rx,
            |_| panic!("no intent after failed registration")
        )
        .await
        .is_err());
    assert_eq!(client.command_attempts().get("set_host_tools"), Some(&1));
    assert!(!client.command_attempts().contains_key("prompt"));
    client.close_input().await.unwrap();
    peer.await.unwrap();
}
