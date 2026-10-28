//! Explicit local baseline CLI. No implicit execution, retries or key arguments.
#[cfg(windows)]
#[tokio::main]
async fn main() {
    use cerulean_core::baseline_runner::{self, RunApproval, RunSpec};
    use cerulean_domain::execution::{require, sha256};
    use sha2::{Digest, Sha256};
    use std::{io::Read, path::Path, sync::atomic::AtomicBool};
    let args: Vec<_> = std::env::args().skip(1).collect();
    fn selected<T: serde::de::DeserializeOwned>(path: &str, expected: &str) -> Result<T, String> {
        sha256(expected)?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        require(
            bytes.len() <= 1024 * 1024 && format!("{:x}", Sha256::digest(&bytes)) == expected,
            "selected file changed or exceeds limit",
        )?;
        serde_json::from_slice(&bytes).map_err(|_| "invalid selected JSON".into())
    }
    let result:Result<serde_json::Value,String>=async {
        match args.first().map(String::as_str) {
            Some("compose") if args.len()==3 => {
                let request=selected::<cerulean_core::baseline_materials::ComposeRequest>(&args[1],&args[2])?;
                let (spec,proof)=cerulean_core::baseline_materials::compose(&request)?;
                Ok(serde_json::json!({"spec":spec,"materialProof":proof,"authorized":false,"dispatched":false}))
            },
            Some("prepare") if args.len()==4 => {
                let spec=selected::<RunSpec>(&args[2],&args[3])?;
                require(spec.schema==2,"new CLI preparations require bound schema-2 materials")?;
                baseline_runner::prepare(Path::new(&args[1]),spec)
            },
            Some("inspect") if args.len()==3 => baseline_runner::inspect(Path::new(&args[1]),&args[2]),
            Some("run") if args.len()==6 => {
                let approval=selected::<RunApproval>(&args[3],&args[4])?;
                let (_keep_sender,mut cancel)=tokio::sync::oneshot::channel();
                let end=baseline_runner::run_from_vault(Path::new(&args[1]),&args[2],approval,
                    Path::new(&args[5]),&mut cancel,&AtomicBool::new(false)).await?;
                serde_json::to_value(end).map_err(|e|e.to_string())
            }
            _=>Err("usage: baseline_experiment compose <request.json> <file-sha256> | prepare <new-data-root> <spec.json> <file-sha256> | inspect <data-root> <spec-digest> | run <data-root> <spec-digest> <approval.json> <file-sha256> <credential-root>".into()),
        }
    }.await;
    match result {
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
            if args.first().is_some_and(|s| s == "run") && value["accepted"] != true {
                std::process::exit(2);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows baseline execution required");
    std::process::exit(1)
}
