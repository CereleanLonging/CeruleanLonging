//! One local disposition of the frozen 279 trial. No model dispatch or approval.
#[cfg(windows)]
fn main() -> Result<(), String> {
    use cerulean_core::ipc::{Client, UserOperation};
    use cerulean_domain::{Command, Request, PROTOCOL_VERSION};
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: retire_validation_279 <original-data-root> <review-digest>".into());
    }
    let root = std::fs::canonicalize(&args[0]).map_err(|e| e.to_string())?;
    let expected = std::fs::canonicalize(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../reports/a6api-live-279/live/data"),
    )
    .map_err(|e| e.to_string())?;
    if root != expected {
        return Err("Only original isolated 279 data may be retired".into());
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async {
        let command=Command::RetireSubmittedPlan {project_id:"relay279".into(),evidence_digest:args[1].clone(),reason:"End authorized relay279 trial after unusable chief Accept response; preserve independently verified sum.mjs and original billable evidence; no retry and no acceptance claim".into()};
        let reply=Client::connect(&root).await?.call(UserOperation::Command {request:Request {id:"retire-relay279-original".into(),protocol_version:PROTOCOL_VERSION,command}}).await?;
        if let Some(error)=reply.response.error {return Err(format!("{}: {}",error.code,error.message));}
        println!("{}",serde_json::to_string(&reply).map_err(|e|e.to_string())?);
        Ok(())
    })
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows local IPC required");
    std::process::exit(1);
}
