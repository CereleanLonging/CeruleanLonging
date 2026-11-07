//! Fixed objective checker and host-observed runtimes; never a model-selected exe.
use cerulean_domain::{
    execution::{require, ValidationFile},
    independent_verifier::{Operation, Output},
    objective_checker::{Language, ID},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
const PIN: Option<&str> = option_env!("CERULEAN_CHECKER_SHA256");
pub fn expected_sha256() -> Option<&'static str> {
    PIN
}
fn fixed_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = exe.parent().ok_or("无法定位独立检查工具")?;
    #[cfg(debug_assertions)]
    let parent = if parent.file_name().is_some_and(|n| n == "deps") {
        parent.parent().ok_or("开发目录缺失")?
    } else {
        parent
    };
    Ok(parent.join("cerulean-checker.exe"))
}
pub fn status() -> crate::independent_verifier::Status {
    let path = fixed_path();
    let result = path
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|p| crate::independent_verifier::pin(p, PIN));
    crate::independent_verifier::Status {
        ready: result.is_ok(),
        version: env!("CARGO_PKG_VERSION").into(),
        path: path.map(|p| p.to_string_lossy().into()).unwrap_or_default(),
        sha256: result.as_ref().ok().map(|(_, h)| h.clone()),
        error: result.err(),
    }
}
pub(crate) fn registration() -> Result<cerulean_domain::execution::RegisteredValidator, String> {
    let path = fixed_path()?;
    let (_guard, _) = crate::independent_verifier::pin(&path, PIN)?;
    Ok(cerulean_domain::execution::RegisteredValidator {
        id: ID.into(),
        name: "工作台独立检查工具 · 固定关联".into(),
        tool: crate::verifier::PinnedCheck::describe(&path, &[], Default::default())?,
        registered_by: "workbench".into(),
        registered_at: "2026-11-05T00:00:00Z".into(),
    })
}
pub(crate) fn inspect(operation: Operation) -> Result<Output, String> {
    require(
        matches!(&operation, Operation::InspectChecks { .. }),
        "独立检查不接受计票操作",
    )?;
    #[cfg(test)]
    {
        cerulean_domain::independent_verifier::evaluate(&operation)
    }
    #[cfg(not(test))]
    {
        crate::independent_verifier::call_bound(
            &fixed_path()?,
            PIN,
            cerulean_domain::objective_checker::PROTOCOL,
            operation,
        )
    }
}
pub(crate) fn runtimes() -> BTreeMap<Language, ValidationFile> {
    let mut found = BTreeMap::new();
    // Only known interpreter names from absolute host installation paths. Never
    // search cwd, a project directory, or a path suggested by a model.
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if !directory.is_absolute() {
            continue;
        }
        for (language, name) in [
            (Language::JavaScript, "node.exe"),
            (Language::Python, "python.exe"),
        ] {
            if found.contains_key(&language) {
                continue;
            }
            let path = directory.join(name);
            if let Ok(tool) = crate::verifier::PinnedCheck::describe(&path, &[], Default::default())
            {
                found.insert(language, tool.executable);
            }
        }
    }
    found
}
pub(crate) fn require_current(
    check: &cerulean_domain::execution::AcceptanceCheck,
) -> Result<(), String> {
    let current = registration()?;
    require(
        check
            .tool
            .as_ref()
            .is_some_and(|t| t.executable == current.tool.executable),
        "请使用当前工作台固定独立检查工具重新规划并批准",
    )?;
    require_approved(check)
}

/// Existing submissions retain the exact checker and runtime authorized in
/// their durable plan. PinnedCheck locks and hashes those bytes before launch.
/// New proposals and changed checks still bind the currently packaged tool.
pub(crate) fn require_approved(
    check: &cerulean_domain::execution::AcceptanceCheck,
) -> Result<(), String> {
    let tool = check.tool.as_ref().ok_or("独立检查批准绑定缺失")?;
    tool.validate()?;
    require(
        Path::new(&tool.executable.path)
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("cerulean-checker.exe")),
        "批准程序并非工作台独立检查工具",
    )?;
    let recipe = cerulean_domain::objective_checker::BoundScript::parse(&check.args)?;
    require(
        check
            .tool
            .as_ref()
            .is_some_and(|t| t.inputs == [recipe.runtime.clone()] && t.environment.is_empty())
            && check.expected_assertions == recipe.test.assertions
            && check.timeout_seconds == recipe.timeout_seconds,
        "独立检查批准绑定不完整",
    )?;
    require(
        Path::new(&check.program) == Path::new(&tool.executable.path),
        "独立检查程序路径不符",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cerulean_domain::{execution::*, objective_checker::*};
    use std::{sync::atomic::AtomicBool, time::Duration};
    fn test_check(source: &str) -> AcceptanceCheck {
        let runtime = runtimes()
            .remove(&Language::JavaScript)
            .expect("Install Node for explicit native checker tests");
        let registration = registration().expect("Build checker beside core tests first");
        bind(
            &AcceptanceCheck {
                id: "behavior".into(),
                name: None,
                program: registration.tool.executable.path.clone(),
                args: vec![],
                timeout_seconds: 8,
                expected_assertions: vec![],
                tool: Some(registration.tool),
            },
            &ScriptTest {
                language: Language::JavaScript,
                source: source.into(),
                assertions: vec!["sum".into()],
            },
            &runtime,
        )
        .unwrap()
    }
    fn run(check: &AcceptanceCheck, cwd: &Path, seconds: u64) -> crate::verifier::process::Outcome {
        let temp = tempfile::tempdir().unwrap();
        crate::verifier::process::run(
            Path::new(&check.program),
            &check.args,
            cwd,
            temp.path(),
            Duration::from_secs(seconds),
            &AtomicBool::new(false),
            &Default::default(),
        )
        .unwrap()
    }
    #[test]
    #[ignore = "Build the real checker first; executes synthetic code in an owned Windows Job, no accounts/providers"]
    fn objective_checker_native_behavior_failure_and_missing_assertions() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join("sum.mjs"),
            "export const sum=(a,b)=>a+b;",
        )
        .unwrap();
        let check=test_check("import {sum} from './sum.mjs'; console.log(JSON.stringify({schemaVersion:1,assertions:[{id:'sum',passed:sum(2,3)===5 && sum(-1,1)===0,detail:'positive and negative additions'}]}));");
        require_current(&check).unwrap();
        let outcome = run(&check, project.path(), 8);
        assert_eq!(
            outcome.exit_code,
            0,
            "{}",
            String::from_utf8_lossy(&outcome.stderr)
        );
        require_assertions(
            &check,
            &parse_assertions(&check, &outcome.stdout).unwrap(),
            true,
        )
        .unwrap();
        std::fs::write(
            project.path().join("sum.mjs"),
            "export const sum=(a,b)=>a-b;",
        )
        .unwrap();
        let failed = run(&check, project.path(), 8);
        assert!(require_assertions(
            &check,
            &parse_assertions(&check, &failed.stdout).unwrap(),
            true
        )
        .is_err());
        for source in ["console.log('ok')", "console.log(JSON.stringify({schemaVersion:1,assertions:[]}))", "console.log(JSON.stringify({schemaVersion:1,assertions:[{id:'sum',passed:true,detail:'one'},{id:'sum',passed:true,detail:'two'}]}))"] {
            let missing=test_check(source);
            let result=run(&missing,project.path(),8);
            assert!(parse_assertions(&missing,&result.stdout).is_err());
        }
    }
    #[test]
    #[ignore = "Real checker plus owned descendant cleanup under Windows"]
    fn objective_checker_native_tamper_timeout_overflow_and_child_cleanup() {
        let project = tempfile::tempdir().unwrap();
        let mut check = test_check("console.log('should not run')");
        let mut recipe = BoundScript::parse(&check.args).unwrap();
        recipe.runtime.sha256 = "0".repeat(64);
        check.args = recipe.arguments().unwrap();
        assert!(require_current(&check).is_err());
        let changed = run(&check, project.path(), 8);
        assert_ne!(changed.exit_code, 0);
        assert!(changed.stdout.is_empty());
        let timeout=test_check("import {spawn} from 'node:child_process'; const child=spawn(process.execPath,['--eval','setInterval(()=>{},1000)'],{stdio:'ignore'}); console.log(child.pid); setInterval(()=>{},1000);");
        let timed = run(&timeout, project.path(), 2);
        assert!(timed.timed_out);
        let pid = String::from_utf8(timed.stdout)
            .unwrap()
            .trim()
            .parse::<u32>()
            .expect("child was spawned");
        use windows_sys::Win32::System::Threading::*;
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if !process.is_null() {
            let mut code = 0;
            unsafe {
                GetExitCodeProcess(process, &mut code);
                windows_sys::Win32::Foundation::CloseHandle(process);
            };
            assert_ne!(code, 259, "descendant escaped owned Job");
        }
        let overflow = test_check("for(;;)console.log('x'.repeat(65536));");
        let result = run(&overflow, project.path(), 8);
        assert!(result.failure.is_some());
        assert!(result.stdout.len() <= crate::verifier::process::OUTPUT_LIMIT);
    }
    #[test]
    #[ignore = "Real distinct checker and verifier protocols"]
    fn objective_checker_native_rejects_blind_protocol() {
        let path = fixed_path().unwrap();
        let bad=serde_json::to_vec(&serde_json::json!({"protocol":cerulean_domain::independent_verifier::TOOL_PROTOCOL,"operation":{"kind":"prepare","requirement":"r","criteria":[],"files":[],"redactions":[],"objectiveChecks":0}})).unwrap();
        let result =
            crate::verifier::process::run_bound_input(&path, bad, path.parent().unwrap()).unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
    }
}
