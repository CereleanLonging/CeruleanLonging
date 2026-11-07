//! Executes only the immutable test recipe approved with the task. The host
//! owns the surrounding Windows Job, restricted account and sealed snapshot.
use cerulean_domain::objective_checker;
#[cfg(windows)]
use cerulean_domain::{
    execution::require,
    objective_checker::{BoundScript, Language},
};
#[cfg(windows)]
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
#[cfg(windows)]
use std::{
    fs::OpenOptions,
    process::{Command, Stdio},
};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--build-info"] {
        println!(
            "{}",
            serde_json::json!({"product":"CeruleanLonging Independent Checker","version":env!("CARGO_PKG_VERSION"),"protocol":objective_checker::PROTOCOL})
        );
        return Ok(());
    }
    if args == ["--stdio-once"] {
        let mut bytes = vec![];
        std::io::stdin()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let result = objective_checker::handle(&bytes)?;
        return std::io::stdout()
            .write_all(&result)
            .map_err(|e| e.to_string());
    }
    #[cfg(windows)]
    let recipe = BoundScript::parse(&args)?;
    // The core must assign us to a kill-on-close Job before our first instruction.
    // Inherited Job membership also covers every descendant of the test runtime.
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::{
            JobObjects::IsProcessInJob, Threading::GetCurrentProcess,
        };
        let mut member = 0;
        require(
            unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut member) } != 0
                && member != 0,
            "独立检查必须由工作台受管进程组启动",
        )?;
    }
    #[cfg(not(windows))]
    return Err("独立检查执行当前仅支持 Windows 受管进程组".into());
    #[cfg(windows)]
    execute(&recipe)
}
#[cfg(windows)]
fn execute(recipe: &BoundScript) -> Result<(), String> {
    use std::os::windows::{fs::OpenOptionsExt, process::CommandExt};
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&recipe.runtime.path)
        .map_err(|_| "已批准测试运行时不可读")?;
    require(
        file.metadata().map_err(|e| e.to_string())?.len() <= 256 * 1024 * 1024,
        "测试运行时体积超限",
    )?;
    let mut hash = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    require(
        format!("{:x}", hash.finalize()) == recipe.runtime.sha256,
        "测试运行时摘要与批准方案不符",
    )?;
    let mut command = Command::new(&recipe.runtime.path);
    command.env_clear().creation_flags(0x08000000);
    for name in ["SystemRoot", "WINDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    match recipe.test.language {
        Language::Python => {
            command.args(["-I", "-B", "-c", &recipe.test.source]);
        }
        Language::JavaScript => {
            // The host rejects reparse points when sealing the source tree.
            // Avoid Node realpath walking private ancestors of that sealed tree.
            command.args([
                "--preserve-symlinks",
                "--input-type=module",
                "--eval",
                &recipe.test.source,
            ]);
        }
    }
    // Stream directly into the host's bounded pipes: the host enforces both the
    // approved deadline and output limit and kills the complete owned Job.
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| format!("客观测试无法启动：{e}"))?;
    require(status.success(), "客观测试进程返回失败")
}
