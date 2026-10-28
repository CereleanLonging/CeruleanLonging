//! Exercise the production worker process owner without sending any prompt.
use cerulean_adapters::omp_transport::worker::host_tools::HostToolDefinition;
use cerulean_core::omp_process::WorkerProcess;
use cerulean_domain::{
    execution::{ValidationFile, WorkerSelection},
    secret::SecretInput,
};
use serde_json::json;
use std::{fs::OpenOptions, io::Write, path::PathBuf};
#[tokio::main]
async fn main() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let register = args.get(4).is_some_and(|a| a == "--register-host-tool");
    if !(args.len() == 4 || (args.len() == 5 && register)) {
        return Err(
            "usage: omp_owned_readonly_probe <exe> <approved-sha256> <fresh-profile> <new-report> [--register-host-tool]"
                .into(),
        );
    }
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[3])
        .map_err(|_| "report must be new")?;
    let worker = WorkerSelection {
        omp_runtime_parent: None,
        provider: "deepseek".into(),
        model: "deepseek-flash".into(),
        executable: ValidationFile {
            path: args[0].clone(),
            sha256: args[1].clone(),
        },
    };
    let placeholder = SecretInput::new("cerulean-offline-placeholder-not-a-real-key".into())?;
    let mut report = json!({"version":1,"mode":"owned-worker-readonly","status":"failed","modelPrompts":0,"credentialMode":"explicit-invalid-placeholder","realCredentialsRead":false});
    let result = async {
        let mut worker = WorkerProcess::launch(&worker, &PathBuf::from(&args[2]), &placeholder)?;
        report["pid"] = json!(worker.pid());
        let identity = worker.connect().await;
        let registration = async {
            if register {
                let identity = identity.as_ref().map_err(Clone::clone)?;
                let tool = HostToolDefinition {
                    name: "cerulean_read_file".into(),
                    description: "Read only an explicitly granted project file through the workbench host.".into(),
                    parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
                };
                let channel = worker.channel()?;
                channel.register_host_tools(identity, &[tool]).await?;
                report["registeredTools"] = json!(channel.registered_host_tools());
                report["hostToolCalls"] = json!(0);
            }
            Ok::<_, String>(())
        }.await;
        report["commandAttempts"] = json!(worker.command_attempts());
        report["exit"] = json!(worker.close().await?);
        registration?;
        report["identity"] = json!(identity?);
        Ok::<_, String>(())
    }
    .await;
    match &result {
        Ok(()) => report["status"] = json!("passed"),
        Err(e) => report["error"] = json!(e),
    }
    output
        .write_all(serde_json::to_string_pretty(&report).unwrap().as_bytes())
        .map_err(|_| "report write failed")?;
    output.sync_all().map_err(|_| "report sync failed")?;
    println!("{report}");
    result
}
