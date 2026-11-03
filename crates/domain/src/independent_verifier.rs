//! Offline protocol for the workbench-bound verifier process. No paths, keys,
//! network operations, task dispatch, or ledger writes are accepted here.
use crate::{
    blind_review::*,
    execution::{digest, identifier, require},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub mod material;
pub const TOOL_PROTOCOL: &str = "cerulean-independent-verifier-v1";
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Operation {
    Prepare {
        requirement: String,
        criteria: Vec<String>,
        files: Vec<Vec<u8>>,
        redactions: Vec<Redaction>,
        objective_checks: u32,
    },
    CheckBallot {
        material: ReviewMaterial,
        seat: ReviewSeat,
        model: String,
        ballot: Ballot,
    },
    Tally {
        material: ReviewMaterial,
        seats: Vec<ReviewSeat>,
        ballots: Vec<RevealedBallot>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub protocol: String,
    pub operation: Operation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Output {
    Prepared { material: ReviewMaterial },
    BallotChecked {},
    Tallied { passed: bool, votes: u32 },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Response {
    pub protocol: String,
    pub version: String,
    pub request_digest: String,
    pub process_id: u32,
    pub result: Result<Output, String>,
}

fn validate_seat(s: &ReviewSeat) -> Result<(), String> {
    identifier(&s.id)?;
    s.target.url()?;
    require(
        matches!(s.role.as_str(), "chief" | "deputy" | "worker"),
        "独立验收席位角色无效",
    )?;
    require(
        !s.model.is_empty() && s.model.len() <= 200,
        "独立验收模型无效",
    )?;
    require(
        !s.allowed_models.is_empty()
            && s.allowed_models.len() <= 8
            && s.allowed_models
                .iter()
                .all(|m| !m.is_empty() && m.len() <= 200 && !m.chars().any(char::is_control)),
        "独立验收模型规则无效",
    )
}
fn check_ballot(
    material: &ReviewMaterial,
    seat: &ReviewSeat,
    model: &str,
    ballot: &Ballot,
) -> Result<(), String> {
    material.validate()?;
    validate_seat(seat)?;
    require(
        seat.allowed_models.iter().any(|m| m == model),
        "实际评审模型不符合批准规则",
    )?;
    ballot.validate(material)
}
pub fn evaluate(operation: &Operation) -> Result<Output, String> {
    match operation {
        Operation::Prepare {
            requirement,
            criteria,
            files,
            redactions,
            objective_checks,
        } => {
            require(criteria.len() <= 100, "验收标准数量超限")?;
            let anonymous = files
                .iter()
                .cloned()
                .map(|b| (String::new(), b))
                .collect::<Vec<_>>();
            let material = ReviewMaterial {
                protocol: PROTOCOL.into(),
                requirement: material::sanitize(requirement, redactions)?,
                criteria: criteria
                    .iter()
                    .map(|c| material::sanitize(c, redactions))
                    .collect::<Result<_, _>>()?,
                fragments: material::fragments(&anonymous, redactions)?,
                objective_checks: *objective_checks,
            };
            material.validate()?;
            Ok(Output::Prepared { material })
        }
        Operation::CheckBallot {
            material,
            seat,
            model,
            ballot,
        } => {
            check_ballot(material, seat, model, ballot)?;
            Ok(Output::BallotChecked {})
        }
        Operation::Tally {
            material,
            seats,
            ballots,
        } => {
            material.validate()?;
            require(
                (3..=MAX_SEATS).contains(&seats.len()) && ballots.len() == seats.len(),
                "完整票集缺失，不计算或揭晓部分得分",
            )?;
            let mut ids = BTreeSet::new();
            let mut workers = BTreeSet::new();
            for s in seats {
                validate_seat(s)?;
                require(ids.insert(&s.id), "独立验收席位重复")?;
                if s.role == "worker" {
                    require(
                        workers.insert(digest(&(&s.target, &s.model))?),
                        "执行评审身份重复",
                    )?;
                }
            }
            for role in ["chief", "deputy"] {
                require(
                    seats.iter().filter(|s| s.role == role).count() == 1,
                    "主副指挥各需一席",
                )?;
            }
            let mut seen = BTreeSet::new();
            for b in ballots {
                require(seen.insert(&b.seat), "独立验收票据重复")?;
                let seat = seats
                    .iter()
                    .find(|s| s.id == b.seat)
                    .ok_or("票据席位不属于本轮名册")?;
                check_ballot(material, seat, &b.model, &b.ballot)?;
            }
            Ok(Output::Tallied {
                passed: ballots.iter().all(|b| b.ballot.vote == Vote::Pass),
                votes: ballots.len() as u32,
            })
        }
    }
}
pub fn handle(bytes: &[u8]) -> Result<Vec<u8>, String> {
    require(bytes.len() <= MAX_MESSAGE_BYTES, "独立验收请求超限")?;
    let request: Request = serde_json::from_slice(bytes).map_err(|_| "独立验收请求格式无效")?;
    require(request.protocol == TOOL_PROTOCOL, "独立验收协议不匹配")?;
    let response = Response {
        protocol: TOOL_PROTOCOL.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        request_digest: digest(&request)?,
        process_id: std::process::id(),
        result: evaluate(&request.operation),
    };
    let output = serde_json::to_vec(&response).map_err(|_| "独立验收响应编码失败")?;
    require(output.len() <= MAX_MESSAGE_BYTES, "独立验收响应超限")?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn material() -> ReviewMaterial {
        let Output::Prepared { material } = evaluate(&Operation::Prepare {
            requirement: "Build a CSV tool".into(),
            criteria: vec!["All approved tests pass".into()],
            files: vec![b"export const total = 1;".to_vec()],
            redactions: vec![],
            objective_checks: 1,
        })
        .unwrap() else {
            panic!()
        };
        material
    }
    fn seats() -> Vec<ReviewSeat> {
        ["chief", "deputy", "worker"]
            .into_iter()
            .map(|r| ReviewSeat {
                id: r.into(),
                role: r.into(),
                target: crate::api_worker::ApiTarget::OpenAi {},
                model: r.into(),
                allowed_models: vec![r.into()],
            })
            .collect()
    }
    fn ballots() -> Vec<RevealedBallot> {
        seats()
            .into_iter()
            .map(|s| RevealedBallot {
                seat: s.id,
                model: s.model,
                ballot: Ballot {
                    vote: Vote::Pass,
                    citations: vec!["fragment-0001".into()],
                    reason: "Matches the approved criterion".into(),
                },
            })
            .collect()
    }
    #[test]
    fn preparation_redacts_and_fragments_without_paths() {
        let src = "const sample = 'secret-value';\n".repeat(100);
        let Output::Prepared { material } = evaluate(&Operation::Prepare {
            requirement: "Verify secret-value".into(),
            criteria: vec!["secret-value is replaced".into()],
            files: vec![src.into_bytes()],
            redactions: vec![Redaction {
                literal: "secret-value".into(),
                replacement: "REDACTED".into(),
            }],
            objective_checks: 1,
        })
        .unwrap() else {
            panic!()
        };
        assert!(material.fragments.len() > 1);
        assert!(material
            .fragments
            .iter()
            .all(|f| f.content.len() <= FRAGMENT_BYTES && !f.content.contains("secret-value")));
        assert!(!material.requirement.contains("secret-value"));
    }
    #[test]
    fn preparation_rejects_secrets_binary_and_missing_objective_checks() {
        for (files, checks) in [
            (vec![b"sk-secret".to_vec()], 1),
            (vec![vec![255]], 1),
            (vec![b"valid".to_vec()], 0),
        ] {
            assert!(evaluate(&Operation::Prepare {
                requirement: "a requirement".into(),
                criteria: vec!["a check".into()],
                files,
                redactions: vec![],
                objective_checks: checks
            })
            .is_err());
        }
    }
    #[test]
    fn tally_requires_complete_distinct_roster_and_unanimity() {
        let mut bs = ballots();
        let op = |bs: Vec<RevealedBallot>| Operation::Tally {
            material: material(),
            seats: seats(),
            ballots: bs,
        };
        assert_eq!(
            evaluate(&op(bs.clone())).unwrap(),
            Output::Tallied {
                passed: true,
                votes: 3
            }
        );
        bs[2].ballot.vote = Vote::Fail;
        assert_eq!(
            evaluate(&op(bs.clone())).unwrap(),
            Output::Tallied {
                passed: false,
                votes: 3
            }
        );
        bs.pop();
        assert!(evaluate(&op(bs)).is_err());
        let mut bs = ballots();
        bs[2] = bs[1].clone();
        assert!(evaluate(&op(bs)).is_err());
    }
    #[test]
    fn ballot_rejects_wrong_model_and_foreign_citations() {
        let op = |model: String, ballot: Ballot| Operation::CheckBallot {
            material: material(),
            seat: seats()[0].clone(),
            model,
            ballot,
        };
        let mut b = ballots()[0].ballot.clone();
        assert!(evaluate(&op("other".into(), b.clone())).is_err());
        b.citations = vec!["foreign".into()];
        assert!(evaluate(&op("chief".into(), b)).is_err());
    }
    #[test]
    fn wire_binds_exact_request_and_rejects_paths_and_oversize() {
        let request = Request {
            protocol: TOOL_PROTOCOL.into(),
            operation: Operation::Tally {
                material: material(),
                seats: seats(),
                ballots: ballots(),
            },
        };
        let raw = serde_json::to_vec(&request).unwrap();
        let reply: Response = serde_json::from_slice(&handle(&raw).unwrap()).unwrap();
        assert_eq!(reply.request_digest, digest(&request).unwrap());
        assert_eq!(reply.version, env!("CARGO_PKG_VERSION"));
        let mut json = serde_json::to_value(request).unwrap();
        json["path"] = "C:\\secret".into();
        assert!(handle(&serde_json::to_vec(&json).unwrap()).is_err());
        assert!(handle(&vec![b' '; MAX_MESSAGE_BYTES + 1]).is_err());
    }
}
