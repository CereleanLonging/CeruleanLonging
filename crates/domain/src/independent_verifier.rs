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
    InspectChecks {
        project: String,
        operation: String,
        manifest: crate::execution::ArtifactManifest,
        approved: Vec<crate::execution::AcceptanceCheck>,
        report: crate::execution::VerificationReport,
    },
    ArchiveChecks {
        archive: crate::independent_checks::CheckArchive,
    },
    AttachCheckEvidence {
        material: ReviewMaterial,
        archive: crate::independent_checks::CheckArchive,
    },
    WeightedTally {
        policy: crate::review_weights::WeightPolicy,
        archive: crate::independent_checks::CheckArchive,
        material: ReviewMaterial,
        seats: Vec<ReviewSeat>,
        ballots: Vec<RevealedBallot>,
    },
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
    AuditBallot {
        material: ReviewMaterial,
        seat: ReviewSeat,
        model: String,
        ballot: Option<Ballot>,
        audit: ReviewToolAudit,
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
    ChecksInspected {
        record: crate::independent_checks::CheckRecord,
    },
    CheckArchiveValidated {
        digest: String,
        basis_points: u32,
    },
    Weighted {
        result: crate::review_weights::WeightedResult,
    },
    Prepared {
        material: ReviewMaterial,
    },
    BallotChecked {},
    BallotAudited {
        ballot: Ballot,
    },
    Tallied {
        passed: bool,
        votes: u32,
    },
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
    s.validate_source()?;
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
pub fn summarize_checks(
    material: &ReviewMaterial,
    archive: &crate::independent_checks::CheckArchive,
) -> Result<ReviewCheckEvidence, String> {
    material.validate()?;
    require(
        archive.basis_points()? == 10000,
        "执行端独立检查尚未全部合格",
    )?;
    let total = archive
        .records
        .iter()
        .map(|record| record.checks.len() as u32)
        .sum();
    require(material.objective_checks == total, "检查数量与封存报告不符")?;
    Ok(ReviewCheckEvidence {
        task_deliveries: archive.records.len() as u32,
        checks_total: total,
        checks_passed: total,
        source_files: material
            .fragments
            .iter()
            .map(|fragment| &fragment.file)
            .collect::<BTreeSet<_>>()
            .len() as u32,
        assembled_delivery: archive.assembled.is_some(),
    })
}

pub fn evaluate(operation: &Operation) -> Result<Output, String> {
    match operation {
        Operation::InspectChecks {
            project,
            operation,
            manifest,
            approved,
            report,
        } => Ok(Output::ChecksInspected {
            record: crate::independent_checks::inspect(
                project, operation, manifest, approved, report,
            )?,
        }),
        Operation::ArchiveChecks { archive } => Ok(Output::CheckArchiveValidated {
            digest: digest(archive)?,
            basis_points: archive.basis_points()?,
        }),
        Operation::AttachCheckEvidence { material, archive } => {
            material.validate()?;
            require(
                material.independent_check_evidence.is_none(),
                "材料已有检查摘要，禁止覆盖",
            )?;
            let evidence = summarize_checks(material, archive)?;
            let mut material = material.clone();
            material.independent_check_evidence = Some(evidence);
            material.validate()?;
            Ok(Output::Prepared { material })
        }
        Operation::WeightedTally {
            policy,
            archive,
            material,
            seats,
            ballots,
        } => Ok(Output::Weighted {
            result: crate::review_weights::evaluate(policy, archive, material, seats, ballots)?,
        }),
        Operation::AuditBallot {
            material,
            seat,
            model,
            ballot,
            audit,
        } => {
            material.validate()?;
            validate_seat(seat)?;
            if audit.status == ReviewToolStatus::Clean {
                check_ballot(
                    material,
                    seat,
                    model,
                    ballot.as_ref().ok_or("缺少模型评分")?,
                )?;
            }
            Ok(Output::BallotAudited {
                ballot: audit.effective_ballot(ballot.as_ref(), material)?,
            })
        }
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
                independent_check_evidence: None,
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
                    require(workers.insert(s.identity()?), "执行评审身份重复")?;
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
                if let Some(audit) = &b.tool_audit {
                    require(
                        audit.effective_ballot(b.original_ballot.as_ref(), material)? == b.ballot,
                        "工具核验结果与计票不符",
                    )?;
                    if audit.status == ReviewToolStatus::Clean {
                        check_ballot(material, seat, &b.model, &b.ballot)?;
                    }
                } else {
                    check_ballot(material, seat, &b.model, &b.ballot)?;
                }
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
    fn check_archive() -> crate::independent_checks::CheckArchive {
        use crate::independent_checks::{CheckArchive, CheckFact, CheckRecord};
        CheckArchive {
            schema_version: 1,
            project_id: "private-project".into(),
            revision: "a".repeat(64),
            final_task: "private-task".into(),
            final_source: "b".repeat(64),
            assembled: None,
            records: vec![CheckRecord {
                schema_version: 1,
                project_id: "private-project".into(),
                revision: "a".repeat(64),
                task_id: "private-task".into(),
                attempt_id: "private-attempt".into(),
                operation_id: "private-operation".into(),
                source_digest: "b".repeat(64),
                manifest_digest: "c".repeat(64),
                report_digest: "d".repeat(64),
                observed_at: "2026-11-07T00:00:00Z".into(),
                checks: vec![CheckFact {
                    id: "private-check".into(),
                    command_digest: "e".repeat(64),
                    tool_digest: "f".repeat(64),
                    passed: true,
                    reason: "private-report-content".into(),
                }],
            }],
        }
    }
    #[test]
    fn attached_evidence_is_anonymous_and_preserves_legacy_serialization() {
        let original = material();
        let bytes = serde_json::to_vec(&original).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("independentCheckEvidence"));
        let reread: ReviewMaterial = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&reread).unwrap(), bytes);
        let Output::Prepared { material } = evaluate(&Operation::AttachCheckEvidence {
            material: original,
            archive: check_archive(),
        })
        .unwrap() else {
            panic!("prepared material expected")
        };
        assert_eq!(
            material.independent_check_evidence,
            Some(ReviewCheckEvidence {
                task_deliveries: 1,
                checks_total: 1,
                checks_passed: 1,
                source_files: 1,
                assembled_delivery: false,
            })
        );
        assert!(!serde_json::to_string(&material)
            .unwrap()
            .contains("private-"));
        assert!(evaluate(&Operation::AttachCheckEvidence {
            material,
            archive: check_archive(),
        })
        .is_err());
    }
    #[test]
    fn attached_evidence_rejects_failed_incomplete_and_foreign_archives() {
        for case in 0..4 {
            let mut archive = check_archive();
            match case {
                0 => archive.records[0].checks[0].passed = false,
                1 => archive.records[0].checks.clear(),
                2 => archive.records[0].project_id = "foreign".into(),
                _ => {
                    let mut extra = archive.records[0].checks[0].clone();
                    extra.id = "extra".into();
                    archive.records[0].checks.push(extra);
                }
            }
            assert!(evaluate(&Operation::AttachCheckEvidence {
                material: material(),
                archive,
            })
            .is_err());
        }
    }
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
                max_output_tokens: None,
                id: r.into(),
                role: r.into(),
                target: Some(crate::api_worker::ApiTarget::OpenAi {}),
                transport: None,
                model: r.into(),
                allowed_models: vec![r.into()],
            })
            .collect()
    }
    fn ballots() -> Vec<RevealedBallot> {
        seats()
            .into_iter()
            .map(|s| RevealedBallot {
                tool_audit: None,
                original_ballot: None,
                seat: s.id,
                model: s.model,
                ballot: Ballot {
                    assessments: vec![],
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
    #[test]
    fn tool_calls_and_missing_audit_override_a_pass_vote() {
        let material = material();
        let seat = seats().remove(0);
        let original = Ballot {
            assessments: vec![],
            vote: Vote::Pass,
            citations: vec![material.fragments[0].id.clone()],
            reason: "Model says pass".into(),
        };
        for status in [
            ReviewToolStatus::ToolCallObserved,
            ReviewToolStatus::Unverifiable,
        ] {
            let audit = ReviewToolAudit {
                tools: if status == ReviewToolStatus::ToolCallObserved {
                    vec!["web_search".into()]
                } else {
                    vec![]
                },
                status,
                source: "api-response".into(),
                request: "request-1".into(),
                evidence_digest: "a".repeat(64),
                reason: "Captured scoring trace".into(),
            };
            let Output::BallotAudited { ballot } = evaluate(&Operation::AuditBallot {
                material: material.clone(),
                seat: seat.clone(),
                model: seat.model.clone(),
                ballot: Some(original.clone()),
                audit: audit.clone(),
            })
            .unwrap() else {
                panic!("audit output")
            };
            assert_eq!(ballot.vote, Vote::Fail);
            assert_eq!(original.vote, Vote::Pass);
            let mut votes = ballots();
            votes[0] = RevealedBallot {
                seat: seat.id.clone(),
                model: seat.model.clone(),
                ballot,
                tool_audit: Some(audit),
                original_ballot: Some(original.clone()),
            };
            assert_eq!(
                evaluate(&Operation::Tally {
                    material: material.clone(),
                    seats: seats(),
                    ballots: votes.clone()
                })
                .unwrap(),
                Output::Tallied {
                    passed: false,
                    votes: 3
                }
            );
            votes[0].ballot.vote = Vote::Pass;
            assert!(evaluate(&Operation::Tally {
                material: material.clone(),
                seats: seats(),
                ballots: votes
            })
            .is_err());
        }
    }
}
