use super::*;
use cerulean_domain::execution::{Project, WorkerDirectory};
use std::{collections::BTreeMap, fs, io::Write, path::PathBuf};

// This diagnostic reuses an explicitly selected disposable identity. It never
// provisions accounts, retries verification, or changes historical evidence.
#[test]
#[ignore = "requires existing disposable account and unique diagnostic name; no UAC or network"]
fn isolated_powershell_directory_diagnostic() {
    let root = PathBuf::from(std::env::var_os("BLIND_REVIEW_HOST_NATIVE_ROOT").unwrap())
        .canonicalize()
        .unwrap();
    assert!(root.starts_with(Path::new(r"D:\test").canonicalize().unwrap()));
    let label = std::env::var("BLIND_REVIEW_DIAGNOSTIC_LABEL").unwrap();
    assert!(
        label.starts_with("cwd-")
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    );
    let data = root.join("data");
    let records = fs::read_dir(data.join("worker-accounts/project"))
        .unwrap()
        .map(|p| p.unwrap().path().join("account.json"))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    let record: crate::windows_worker_account::AccountRecord =
        serde_json::from_slice(&fs::read(&records[0]).unwrap()).unwrap();
    let project = Project {
        id: record.project_id.clone(),
        root: record.shared_root.clone(),
        timezone: "UTC".into(),
    };
    let binding = WorkerDirectory {
        identity: record.identity.clone(),
        relative_path: format!("executors/{}", record.identity),
    };
    let account = crate::windows_worker_account::load_binding(&data, &project, &binding).unwrap();
    let operation = data.join(&label);
    fs::create_dir(&operation).unwrap();
    let work = operation.join("work");
    let temp = operation.join("temp");
    fs::create_dir(&work).unwrap();
    fs::create_dir(&temp).unwrap();
    fs::write(work.join("input.txt"), b"approved input").unwrap();
    let script = root.join(format!("{label}.ps1"));
    let source = r#"param([string]$Expected)
$ErrorActionPreference='Stop'
$r=[ordered]@{native=[Environment]::CurrentDirectory;provider=[string](Get-Location);systemDrive=$env:SystemDrive}
try {$r.nativeRead=[IO.File]::ReadAllText('input.txt')} catch {$r.nativeError=$_.Exception.GetType().FullName}
try {$r.explicitRead=[IO.File]::ReadAllText((Join-Path $Expected 'input.txt'))} catch {$r.explicitError=$_.Exception.GetType().FullName}
try {Set-Location -LiteralPath $Expected; $r.explicitLocation=[string](Get-Location)} catch {$r.locationError=$_.Exception.Message}
[Console]::Write(($r|ConvertTo-Json -Compress))
"#;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&script)
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
    let exe = system.join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let args = vec![
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-File".into(),
        launch_path(&script).unwrap().to_string_lossy().into(),
        "-Expected".into(),
        launch_path(&work).unwrap().to_string_lossy().into(),
    ];
    let mut reports = vec![];
    crate::windows_worker_account::with_check_workspaces(&[&work, &temp], &account, || {
        for (name, cwd, env) in [
            ("private-parent", work.as_path(), BTreeMap::new()),
            ("system-drive", work.as_path(), BTreeMap::from([("SYSTEMDRIVE".into(), system.components().next().unwrap().as_os_str().to_string_lossy().into_owned())])),
            ("worker-directory", Path::new(&account.record.workspace), BTreeMap::new()),
        ] {
            let outcome = run_isolated(&exe, &args, cwd, &temp, Duration::from_secs(15), &AtomicBool::new(false), &env, None, &account)?;
            let report = serde_json::json!({"name":name,"cwd":cwd,"exitCode":outcome.exit_code,"timedOut":outcome.timed_out,"failure":outcome.failure,"stdout":String::from_utf8_lossy(&outcome.stdout),"stderr":String::from_utf8_lossy(&outcome.stderr)});
            fs::OpenOptions::new().create_new(true).write(true).open(operation.join(format!("{name}.json"))).unwrap().write_all(serde_json::to_string_pretty(&report).unwrap().as_bytes()).unwrap();
            reports.push(report);
            if outcome.exit_code != 0 || outcome.timed_out || outcome.failure.is_some() { return Err("diagnostic process failed; stopped".into()); }
        }
        Ok(())
    }).unwrap();
    println!("{}", serde_json::to_string_pretty(&reports).unwrap());
}
