use cerulean_core::Store;
#[cfg(windows)]
mod workbench_cli;
use cerulean_domain::{ApiError, Request, Response};
use std::{
    io::{self, BufRead, Read, Write},
    path::PathBuf,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["--build-info"] {
        serde_json::to_writer(
            std::io::stdout().lock(),
            &serde_json::json!({
                "capabilitySchema": 1,
                "product": cerulean_domain::PRODUCT_NAME,
                "version": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "architecture": std::env::consts::ARCH,
                "environment": if cfg!(target_env = "msvc") { "msvc" } else { "unsupported" },
                "protocolVersion": cerulean_domain::PROTOCOL_VERSION,
                "upgradeHoldSchema": if cfg!(windows) { 1 } else { 0 },
                "backupSchema": 1,
                "binaryUpdateSchema": 2,
                "dataSchema": 1,
                "dataFormatGuardSchema": 1,
                "dataReaderGeneration": cerulean_core::DATA_READER_GENERATION,
                "maintenanceInterlockSchema": 1,
                "dataRecoverySchema": 2,
                "readableDataSchemas": cerulean_core::data_format::READABLE_SCHEMAS,
                "backupWriterVersions": cerulean_core::BACKUP_WRITER_VERSIONS
            }),
        )?;
        println!();
        return Ok(());
    }
    #[cfg(windows)]
    if args.first().is_some_and(|arg| arg == "--workbench") {
        std::process::exit(i32::from(workbench_cli::run(&args[1..])));
    }
    if args.len() == 5
        && matches!(args[0].as_str(), "--codex-check" | "--codex-bind-check")
        && args[1] == "--executable"
        && args[3] == "--workspace"
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let result = runtime
            .block_on(async {
                let mut client = cerulean_adapters::codex::CodexClient::launch(
                    std::path::Path::new(&args[2]),
                    std::path::Path::new(&args[4]),
                )?;
                let result = async {
                    let connection=client.connect().await?;
                    let binding=if args[0]=="--codex-bind-check" {
                        let model=connection.models.iter().find(|model|model.is_default).ok_or("no default Codex model")?;
                        Some(client.create_worker(&model.model).await?)
                    } else {None};
                    Ok::<_,String>(serde_json::json!({"connection":connection,"workerBinding":binding,"modelCalls":0}))
                }.await;
                let cleanup = client.close().await;
                cleanup?;
                result
            })
            .map_err(io::Error::other)?;
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }
    #[cfg(windows)]
    if args.len() == 3
        && args[1] == "--data-dir"
        && matches!(
            args[0].as_str(),
            "--serve" | "--request" | "--stop" | "--endpoint"
        )
    {
        let directory = PathBuf::from(&args[2]);
        if args[0] == "--endpoint" {
            println!("{}", cerulean_core::ipc::endpoint(&directory)?);
            return Ok(());
        }
        if args[0] != "--serve" {
            // Match the workbench CLI's bounded, joined client stack. Full
            // snapshot decoding exceeds Windows' 1 MiB main stack in debug
            // builds, including through this legacy request entry point.
            let stop = args[0] == "--stop";
            let client = std::thread::Builder::new()
                .name("resident-cli".into())
                .stack_size(8 * 1024 * 1024)
                .spawn(move || resident_client(directory, stop))?;
            client
                .join()
                .map_err(|_| {
                    io::Error::other(
                        "resident CLI thread failed; result unknown, request not resent",
                    )
                })?
                .map_err(io::Error::other)?;
            return Ok(());
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        runtime
            .block_on(cerulean_core::ipc::serve(&directory))
            .map_err(io::Error::other)?;
        return Ok(());
    }
    if args.len() != 3 || args[0] != "--stdio" || args[1] != "--data-dir" {
        return Err("Usage: cerulean-core --stdio --data-dir <directory>".into());
    }
    let mut store = Store::open(&PathBuf::from(&args[2]))?;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        let mut bytes = Vec::new();
        // Bound memory before deserialization; oversized protocol frames terminate the channel.
        let count = std::io::Read::by_ref(&mut input)
            .take(65537)
            .read_until(b'\n', &mut bytes)?;
        if count == 0 {
            break;
        }
        if count > 65536 || bytes.last() != Some(&b'\n') {
            return Err("invalid or oversized IPC frame".into());
        }
        let response = match serde_json::from_slice::<Request>(&bytes) {
            Ok(r) => store.handle(r),
            Err(_) => Response {
                id: "".into(),
                result: None,
                error: Some(ApiError {
                    code: "invalidRequest".into(),
                    message: "请求格式无效".into(),
                }),
            },
        };
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}

#[cfg(windows)]
fn resident_client(directory: PathBuf, stop: bool) -> Result<(), String> {
    use cerulean_core::ipc::{Client, UserOperation};
    let operation = if stop {
        UserOperation::Shutdown {
            id: "stop".into(),
            protocol_version: cerulean_domain::PROTOCOL_VERSION,
        }
    } else {
        let mut bytes = Vec::new();
        std::io::stdin()
            .lock()
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 65536 {
            return Err("request too large".into());
        }
        UserOperation::Request {
            request: serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        }
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(Box::pin(async {
        let reply = Client::connect(&directory).await?.call(operation).await?;
        println!(
            "{}",
            serde_json::to_string(&reply).map_err(|e| e.to_string())?
        );
        if reply.response.error.is_some() {
            return Err("service rejected request".into());
        }
        Ok(())
    }))
}
