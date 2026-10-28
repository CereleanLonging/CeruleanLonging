//! Explicit opt-in local executable probe. Never invokes a model.
use cerulean_adapters::omp_transport::ProbeClient;
use serde_json::json;
use std::{fs::OpenOptions, io::Write, path::PathBuf};

#[tokio::main]
async fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err("usage: omp_readonly_probe <exe> <approved-sha256> <fresh-isolated-dir> <new-report.json>".into());
    }
    let report_path = PathBuf::from(&args[3]);
    let mut report_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report_path)
        .map_err(|_| "probe report must be a new writable file")?;
    let mut report = json!({"version":1,"status":"failed","mode":"isolated-readonly-omp","modelPrompts":0,"allowedCommands":["negotiate_protocol","get_state"],"credentialMode":"placeholder-not-authenticated","realCredentialsInherited":false});
    let result = async {
        let digest = args[1].to_str().ok_or("invalid digest argument")?;
        let mut client =
            ProbeClient::launch(&PathBuf::from(&args[0]), digest, &PathBuf::from(&args[2]))?;
        report["pid"] = json!(client.process_id());
        report["executableSha256"] = json!(digest);
        let inspected = client.inspect().await;
        report["commandAttempts"] = json!(client.command_attempts());
        let closed = client.close().await;
        match closed {
            Ok(evidence) => {
                report["ownedProcessExited"] = json!(true);
                report["close"] = json!(evidence);
            }
            Err(error) => {
                report["cleanupError"] = json!(error);
                return Err(error);
            }
        }
        report["state"] = json!(inspected?);
        Ok::<_, String>(())
    }
    .await;
    match &result {
        Ok(()) => report["status"] = json!("passed"),
        Err(error) => report["error"] = json!(error),
    }
    report_file
        .write_all(serde_json::to_string_pretty(&report).unwrap().as_bytes())
        .map_err(|_| "report write failed")?;
    report_file.sync_all().map_err(|_| "report sync failed")?;
    println!("{report}");
    result
}
