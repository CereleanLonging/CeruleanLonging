//! Fixed sibling executable, hash-pinned by the reviewed desktop build. The
//! worker and provider protocols cannot select a program or call this service.
use cerulean_domain::{
    blind_review::*,
    execution::{digest, require},
    independent_verifier::*,
};
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

const PIN: Option<&str> = option_env!("CERULEAN_VERIFIER_SHA256");
pub fn expected_sha256() -> Option<&'static str> {
    PIN
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub ready: bool,
    pub version: String,
    pub path: String,
    pub sha256: Option<String>,
    pub error: Option<String>,
}
fn fixed_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|_| "无法定位工作台程序")?;
    let parent = exe.parent().ok_or("工作台程序目录缺失")?;
    // Unit-test binaries live in deps; production only accepts the sibling.
    #[cfg(debug_assertions)]
    let parent = if parent.file_name().is_some_and(|n| n == "deps") {
        parent.parent().ok_or("开发目录缺失")?
    } else {
        parent
    };
    Ok(parent.join("cerulean-verifier.exe"))
}
fn pin(path: &Path, expected: Option<&str>) -> Result<(File, String), String> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .map_err(|_| "独立验收工具缺失或被占用，请使用完整工作台程序目录")?;
    let tool = crate::verifier::PinnedCheck::describe(path, &[], Default::default())?;
    if let Some(expected) = expected {
        require(
            tool.executable.sha256 == expected,
            "独立验收工具与工作台构建摘要不符；未启动",
        )?;
    } else {
        require(
            cfg!(debug_assertions),
            "发行构建未绑定独立验收工具；请使用正式封包入口",
        )?;
    }
    Ok((file, tool.executable.sha256))
}
pub fn status() -> Status {
    let path = fixed_path();
    let result = path
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|p| pin(p, PIN));
    Status {
        ready: result.is_ok(),
        version: env!("CARGO_PKG_VERSION").into(),
        path: path
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        sha256: result.as_ref().ok().map(|(_, h)| h.clone()),
        error: result.err(),
    }
}
fn call_native(
    path: &Path,
    expected: Option<&str>,
    operation: Operation,
) -> Result<Output, String> {
    let (_guard, _sha) = pin(path, expected)?;
    let request = Request {
        protocol: TOOL_PROTOCOL.into(),
        operation,
    };
    let key = digest(&request)?;
    let bytes = serde_json::to_vec(&request).map_err(|_| "独立验收请求编码失败")?;
    require(bytes.len() <= MAX_MESSAGE_BYTES, "独立验收请求超限")?;
    let outcome = crate::verifier::process::run_bound_input(
        path,
        bytes,
        path.parent().ok_or("验收工具目录缺失")?,
    )?;
    require(
        !outcome.timed_out
            && outcome.failure.is_none()
            && outcome.exit_code == 0
            && outcome.stderr.is_empty(),
        "独立验收进程未正常完成；没有回退到模型自评",
    )?;
    require(
        outcome.stdout.len() <= MAX_MESSAGE_BYTES,
        "独立验收响应超限",
    )?;
    let response: Response =
        serde_json::from_slice(&outcome.stdout).map_err(|_| "独立验收响应格式无效")?;
    require(
        response.protocol == TOOL_PROTOCOL
            && response.version == env!("CARGO_PKG_VERSION")
            && response.request_digest == key
            && response.process_id != std::process::id()
            && response.process_id > 0,
        "独立验收响应与本次工作台请求不符",
    )?;
    response.result
}
fn call(operation: Operation) -> Result<Output, String> {
    #[cfg(test)]
    {
        evaluate(&operation)
    }
    #[cfg(not(test))]
    {
        call_native(&fixed_path()?, PIN, operation)
    }
}
pub(crate) fn prepare(
    requirement: &str,
    criteria: &[String],
    files: &[(String, Vec<u8>)],
    redactions: &[Redaction],
    checks: u32,
) -> Result<ReviewMaterial, String> {
    match call(Operation::Prepare {
        requirement: requirement.into(),
        criteria: criteria.to_vec(),
        files: files.iter().map(|(_, b)| b.clone()).collect(),
        redactions: redactions.to_vec(),
        objective_checks: checks,
    })? {
        Output::Prepared { material } => {
            material.validate()?;
            Ok(material)
        }
        _ => Err("独立验收材料响应类型不符".into()),
    }
}
pub(crate) fn check_ballot(
    material: &ReviewMaterial,
    seat: &ReviewSeat,
    model: &str,
    ballot: &Ballot,
) -> Result<(), String> {
    match call(Operation::CheckBallot {
        material: material.clone(),
        seat: seat.clone(),
        model: model.into(),
        ballot: ballot.clone(),
    })? {
        Output::BallotChecked {} => Ok(()),
        _ => Err("独立验收票据响应类型不符".into()),
    }
}
pub(crate) fn tally(
    material: &ReviewMaterial,
    seats: &[ReviewSeat],
    ballots: &[RevealedBallot],
) -> Result<bool, String> {
    match call(Operation::Tally {
        material: material.clone(),
        seats: seats.to_vec(),
        ballots: ballots.to_vec(),
    })? {
        Output::Tallied { passed, votes } => {
            require(votes as usize == seats.len(), "独立验收票数与名册不符")?;
            Ok(passed)
        }
        _ => Err("独立验收计票响应类型不符".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn operation() -> Operation {
        Operation::Prepare {
            requirement: "Verify the approved CSV project".into(),
            criteria: vec!["Matches the objective checks".into()],
            files: vec![b"export const total = 1;".to_vec()],
            redactions: vec![],
            objective_checks: 1,
        }
    }
    #[test]
    #[ignore = "Build cerulean-verifier first; launches the real bound child process"]
    fn independent_verifier_native_pipe_matches_pure_result() {
        let path = fixed_path().unwrap();
        let (_, hash) = pin(&path, None).unwrap();
        assert_eq!(
            call_native(&path, Some(&hash), operation()).unwrap(),
            evaluate(&operation()).unwrap()
        );
        let Output::Prepared { material } = call_native(&path, Some(&hash), operation()).unwrap()
        else {
            panic!()
        };
        let seats = ["chief", "deputy", "worker"]
            .into_iter()
            .map(|role| ReviewSeat {
                id: role.into(),
                role: role.into(),
                target: cerulean_domain::api_worker::ApiTarget::OpenAi {},
                model: role.into(),
                allowed_models: vec![role.into()],
            })
            .collect::<Vec<_>>();
        let mut ballots = Vec::new();
        for seat in &seats {
            let ballot = Ballot {
                vote: Vote::Pass,
                citations: vec!["fragment-0001".into()],
                reason: "Matches the approved source and criterion".into(),
            };
            assert_eq!(
                call_native(
                    &path,
                    Some(&hash),
                    Operation::CheckBallot {
                        material: material.clone(),
                        seat: seat.clone(),
                        model: seat.model.clone(),
                        ballot: ballot.clone()
                    }
                )
                .unwrap(),
                Output::BallotChecked {}
            );
            ballots.push(RevealedBallot {
                seat: seat.id.clone(),
                model: seat.model.clone(),
                ballot,
            });
        }
        let tally = |ballots| Operation::Tally {
            material: material.clone(),
            seats: seats.clone(),
            ballots,
        };
        assert_eq!(
            call_native(&path, Some(&hash), tally(ballots.clone())).unwrap(),
            Output::Tallied {
                passed: true,
                votes: 3
            }
        );
        ballots[2].ballot.vote = Vote::Fail;
        assert_eq!(
            call_native(&path, Some(&hash), tally(ballots.clone())).unwrap(),
            Output::Tallied {
                passed: false,
                votes: 3
            }
        );
        ballots.pop();
        assert!(call_native(&path, Some(&hash), tally(ballots))
            .unwrap_err()
            .contains("完整票集缺失"));
    }
    #[test]
    #[ignore = "Build cerulean-verifier first; checks native binding failures"]
    fn independent_verifier_native_rejects_missing_or_changed_binary() {
        let path = fixed_path().unwrap();
        assert!(call_native(&path, Some(&"0".repeat(64)), operation())
            .unwrap_err()
            .contains("摘要不符"));
        assert!(call_native(
            &path.with_file_name("absent-verifier.exe"),
            None,
            operation()
        )
        .is_err());
    }
    #[test]
    #[ignore = "Build cerulean-verifier first; real anonymous-pipe request above pipe capacity"]
    fn independent_verifier_native_large_input_and_rejection() {
        let path = fixed_path().unwrap();
        let mut op = operation();
        if let Operation::Prepare { files, .. } = &mut op {
            files[0] = b"const value=1;\n".repeat(5000);
        }
        assert_eq!(
            call_native(&path, None, op.clone()).unwrap(),
            evaluate(&op).unwrap()
        );
        if let Operation::Prepare { files, .. } = &mut op {
            files[0] = b"sk-secret".to_vec();
        }
        assert!(call_native(&path, None, op)
            .unwrap_err()
            .contains("疑似秘密"));
    }
}
