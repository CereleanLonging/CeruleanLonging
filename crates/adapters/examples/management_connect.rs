//! Isolated connection harness. Does not apply workflow decisions, dispatch
//! workers, grant approval, or substitute for RuntimeHost integration.
use cerulean_adapters::{api_generation::ApiClient, deputy_management};
use cerulean_domain::{
    execution::{digest, require},
    management::Purpose,
    management_ingress::{ExternalChiefDecision, ManagementHandoff, ManagementSource},
    secret::SecretInput,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

fn read<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let file = File::open(path).map_err(|_| "handoff input unavailable")?;
    let mut bytes = Vec::new();
    file.take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "handoff input unreadable")?;
    require(bytes.len() <= 128 * 1024, "handoff input too large")?;
    serde_json::from_slice(&bytes).map_err(|_| "invalid handoff JSON".into())
}
fn save(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| "cannot serialize evidence")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "evidence already exists or unavailable; do not retry")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "evidence publication failed; do not retry".into())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Draft {
    operation_id: String,
    project_id: String,
    purpose: Purpose,
    source: ManagementSource,
    context: Value,
}
async fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    require(args.len() == 3 || args.len() == 4, "usage: prepare draft new-directory | chief directory envelope current-context | deputy-once directory current-context | provider-once directory current-context (key on stdin)")?;
    if args[0] == "prepare" && args.len() == 3 {
        let draft: Draft = read(Path::new(&args[1]))?;
        let request = ManagementHandoff {
            operation_id: draft.operation_id,
            project_id: draft.project_id,
            purpose: draft.purpose,
            source: draft.source,
            state_digest: digest(&draft.context)?,
            context: draft.context,
        };
        request.validate()?;
        let directory = Path::new(&args[2]);
        std::fs::create_dir(directory).map_err(|_| "request directory must be new")?;
        save(&directory.join("request.json"), &request)?;
        println!(
            "{}",
            json!({"prepared":true,"requestDigest":digest(&request)?,"stateDigest":request.state_digest})
        );
        return Ok(());
    }
    let directory = Path::new(&args[1]);
    let request: ManagementHandoff = read(&directory.join("request.json"))?;
    request.validate()?;
    require(
        !directory.join("intent.json").exists() && !directory.join("result.json").exists(),
        "request already consumed or uncertain; do not retry",
    )?;
    match args[0].as_str() {
        "chief" if args.len() == 4 => {
            let decision: ExternalChiefDecision = read(Path::new(&args[2]))?;
            let current: Value = read(Path::new(&args[3]))?;
            let session = std::env::var("CODEX_THREAD_ID")
                .map_err(|_| "current Codex session metadata missing")?;
            decision.validate_for(&request, &current, &session)?;
            save(
                &directory.join("intent.json"),
                &json!({"requestDigest":digest(&request)?,"kind":"local-chief-import","at":chrono::Utc::now().to_rfc3339()}),
            )?;
            save(
                &directory.join("result.json"),
                &json!({"status":"validated-local-decision","decision":decision,"identityAssurance":"local-session-correlation-only","workflowApplied":false,"userApproved":false,"apiUsage":null}),
            )?;
            println!("chief decision recorded; workflow not applied");
        }
        "deputy-once" | "provider-once" if args.len() == 3 => {
            let current: Value = read(Path::new(&args[2]))?;
            request.check_current(&current)?;
            require(
                if args[0] == "provider-once" {
                    matches!(request.source, ManagementSource::Provider { .. })
                } else {
                    matches!(request.source, ManagementSource::DeepSeek { .. })
                },
                "selected provider request required",
            )?;
            let mut secret = String::new();
            std::io::stdin()
                .take(513)
                .read_to_string(&mut secret)
                .map_err(|_| "credential input unavailable")?;
            let client = ApiClient::new(SecretInput::new(secret)?)?;
            let prepared = deputy_management::prepare(&client, request.clone())?;
            save(
                &directory.join("intent.json"),
                &json!({"requestDigest":digest(&request)?,"fingerprint":prepared.fingerprint(),"at":chrono::Utc::now().to_rfc3339(),"maximumHttpAttempts":1,"automaticRetry":false}),
            )?;
            let (sender, mut receiver) = tokio::sync::oneshot::channel();
            if args[0] == "provider-once" {
                let result =
                    deputy_management::send_evidenced(&client, prepared, &mut receiver).await;
                drop(sender);
                drop(client);
                result.validate(&result.request)?;
                let usable = result.envelope().is_some();
                save(
                    &directory.join("result.json"),
                    &json!({
                        "response":result,"decodeStage":result.decode_stage,
                        "workflowApplied":false,"userApproved":false,"credentialPersisted":false,
                        "automaticRetry":false
                    }),
                )?;
                println!("provider response recorded; workflow not applied; credential dropped");
                return if usable {
                    Ok(())
                } else {
                    Err(
                        "provider decision unusable; original diagnostic recorded; do not retry"
                            .into(),
                    )
                };
            }
            let result = deputy_management::send_once(&client, prepared, &mut receiver).await;
            drop(sender);
            drop(client);
            match result {
                Ok(decision) => {
                    save(
                        &directory.join("result.json"),
                        &json!({"status":"validated-provider-decision","decision":decision,"workflowApplied":false,"userApproved":false,"credentialPersisted":false}),
                    )?;
                    println!("deputy decision recorded; credential dropped; workflow not applied");
                }
                Err(error) => {
                    save(
                        &directory.join("result.json"),
                        &json!({"status":"failed-or-unknown","error":error,"automaticRetry":false,"credentialPersisted":false}),
                    )?;
                    return Err(
                        "deputy failed or unknown; stopped without retry; credential dropped"
                            .into(),
                    );
                }
            }
        }
        _ => return Err("unsupported management connection command".into()),
    }
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
