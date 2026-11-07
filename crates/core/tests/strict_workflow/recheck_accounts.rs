//! Explicit native integration fixture. Only the opt-in account suite creates
//! ordinary Windows identities, using the production provisioning script.
use super::*;
use std::os::windows::process::CommandExt;

pub(super) struct FixtureRoot {
    directory: tempfile::TempDir,
    isolated: bool,
}

impl FixtureRoot {
    pub(super) fn new(isolated: bool) -> Self {
        let directory = if isolated {
            assert_eq!(
                std::env::var("CERULEAN_NATIVE_RECHECK_ACCOUNTS").as_deref(),
                Ok("1"),
                "Use the explicit administrator account-integration suite"
            );
            assert!(
                std::env::var_os("CERULEAN_CACHE_SESSION").is_some(),
                "Managed test session required"
            );
            let parent = std::env::var_os("CERULEAN_NATIVE_RECHECK_ROOT")
                .expect("Owned native account evidence directory required");
            tempfile::tempdir_in(parent).unwrap()
        } else {
            tempfile::tempdir().unwrap()
        };
        Self {
            directory,
            isolated,
        }
    }

    pub(super) fn path(&self) -> &Path {
        self.directory.path()
    }
}

fn powershell(script: &Path, arguments: &[&std::ffi::OsStr]) -> std::process::Output {
    std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .args(arguments)
        .creation_flags(0x08000000)
        .output()
        .unwrap()
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        if self.isolated && self.path().join("native-account-plan.json").exists() {
            let cleanup = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/strict_workflow/recheck-account-cleanup.ps1");
            let plan = self.path().join("native-account-plan.json");
            let result = powershell(&cleanup, &[plan.as_os_str()]);
            if !result.status.success() {
                eprintln!(
                    "Native account cleanup requires reconciliation: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
                // Preserve the root and account receipts if cleanup failed.
                let replacement = tempfile::tempdir().unwrap();
                let original = std::mem::replace(&mut self.directory, replacement);
                eprintln!("Retained native fixture {}", original.keep().display());
                assert!(std::thread::panicking(), "Native account cleanup failed");
            }
        }
    }
}

pub(crate) fn prepare(root: &Path, plan: &mut PlanRevision) {
    let binding = WorkerDirectory::for_task(&plan.execution[0]).unwrap();
    let workspace = std::path::PathBuf::from(binding.root(&plan.project.root).unwrap());
    assert!(workspace.starts_with(fs::canonicalize(root).unwrap()));
    fs::create_dir_all(&workspace).unwrap();
    for name in ["result.txt", "validator-mode"] {
        fs::rename(root.join("project").join(name), workspace.join(name)).unwrap();
    }
    plan.execution[0].worker_directory = Some(binding.clone());
    let data = root.join("data");
    let chief = root.join("chief");
    fs::create_dir(&chief).unwrap();
    let username = format!(
        "CLW{}",
        &digest(&(
            data.to_str().unwrap(),
            plan.project.id.as_str(),
            binding.identity.as_str()
        ))
        .unwrap()[..16]
    );
    let record = serde_json::json!({
        "schemaVersion": 1,
        "ownerSid": cerulean_core::ipc::current_sid().unwrap(),
        "projectId": plan.project.id,
        "dataRoot": data,
        "sharedRoot": plan.project.root,
        "chiefRoot": chief,
        "slots": [{"identity": binding.identity, "workspace": workspace, "username": username}]
    });
    let file = root.join("native-account-plan.json");
    fs::write(&file, serde_json::to_vec(&record).unwrap()).unwrap();
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/provision-worker-accounts.ps1");
    let output = powershell(
        &script,
        &[std::ffi::OsStr::new("-PlanPath"), file.as_os_str()],
    );
    assert!(
        output.status.success(),
        "Native provisioning failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(file.with_extension("result.json")).unwrap()).unwrap();
    assert_eq!(receipt["success"], true, "{receipt}");
}
