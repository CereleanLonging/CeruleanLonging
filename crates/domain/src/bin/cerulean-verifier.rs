//! The bound independent verifier has only a one-shot anonymous-pipe protocol.
//! It never opens projects, a credential store, a model endpoint, or a ledger.
use cerulean_domain::independent_verifier::{self, MAX_MESSAGE_BYTES, TOOL_PROTOCOL};
use std::io::{Read, Write};
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["--build-info"] {
        println!(
            "{}",
            serde_json::json!({"product":"CeruleanLonging Independent Verifier","version":env!("CARGO_PKG_VERSION"),"protocol":TOOL_PROTOCOL,"offline":true})
        );
        return Ok(());
    }
    if args != ["--stdio-once"] {
        return Err("此工具由工作台固定调用；只支持 --stdio-once 或 --build-info".into());
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take((MAX_MESSAGE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "独立验收输入读取失败")?;
    let output = independent_verifier::handle(&bytes)?;
    std::io::stdout()
        .lock()
        .write_all(&output)
        .map_err(|_| "独立验收输出失败")?;
    Ok(())
}
