use cerulean_cache::Project;
use std::{path::PathBuf, process::Command};
mod task_job;

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn process_path(path: &std::path::Path) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(plain) = value.strip_prefix(r"\\?\") {
        return PathBuf::from(
            plain
                .strip_prefix("UNC\\")
                .map(|p| format!("\\\\{p}"))
                .unwrap_or_else(|| plain.into()),
        );
    }
    path.to_path_buf()
}

fn run() -> Result<i32, String> {
    let mut args = std::env::args_os().skip(1);
    let action = args.next().ok_or("缺少缓存操作")?;
    let root = PathBuf::from(args.next().ok_or("缺少源码工程目录")?);
    let project = Project::open(&root)?;
    if action == "run" {
        let program = args.next().ok_or("缺少构建或测试程序")?;
        let session = project.begin()?;
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(project.root())
            .env("CERULEAN_CACHE_SESSION", process_path(&session.directory))
            .env(
                "CARGO_TARGET_DIR",
                process_path(&session.directory.join("cargo")),
            )
            .env(
                "CERULEAN_VITE_CACHE_DIR",
                process_path(&session.directory.join("vite")),
            )
            .env("TMP", process_path(&session.directory.join("scratch")))
            .env("TEMP", process_path(&session.directory.join("scratch")));
        std::fs::create_dir_all(session.directory.join("scratch")).map_err(|e| e.to_string())?;
        // Repo task entry points wait for their build/test children before returning.
        let status = task_job::run(&mut command);
        let succeeded = status.as_ref().is_ok_and(|status| status.success());
        let cleanup = project.finish(session, succeeded);
        if let Err(error) = cleanup {
            eprintln!("{error}");
            if succeeded {
                return Ok(1);
            }
        }
        return status.map(|s| s.code().unwrap_or(1));
    }
    if args.next().is_some() {
        return Err("缓存查询不接受额外路径或参数".into());
    }
    let snapshot = match action.to_str() {
        Some("scan") => project.snapshot(true)?,
        Some("status") => project.snapshot(false)?,
        Some("clean") => project.clean()?,
        Some("collect") => project.collect_after_task()?,
        _ => return Err("未知缓存操作".into()),
    };
    println!(
        "{}",
        serde_json::to_string(&snapshot).map_err(|e| e.to_string())?
    );
    Ok(0)
}
