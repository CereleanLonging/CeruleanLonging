//! Versioned evidence scoring. These values are not calibrated probabilities or
//! coverage estimates. Model criterion claims remain explicitly model claims.
use crate::{
    blind_review::*,
    execution::{digest, require},
    independent_checks::CheckArchive,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CriterionAssessment {
    pub criterion: u32, // 1-based index in the approved material
    pub aligned: bool,
    pub citations: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WeightPolicy {
    pub version: u32,
    pub instruction_share: u32,
    pub check_share: u32,
    pub minimum_seat: u32,
    pub minimum_overall: u32,
    pub calibration: String,
}
impl Default for WeightPolicy {
    fn default() -> Self {
        Self {
            version: 1,
            instruction_share: 5000,
            check_share: 5000,
            minimum_seat: 8000,
            minimum_overall: 9000,
            calibration: "unknown".into(),
        }
    }
}
impl WeightPolicy {
    pub fn validate(&self) -> Result<(), String> {
        require(self == &Self::default(), "加权验收判据未批准或版本不支持")
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeatWeight {
    pub seat: String,
    pub instruction_basis_points: u32,
    pub check_basis_points: u32,
    pub total_basis_points: u32,
    pub missing_criteria: Vec<u32>,
    pub unaligned_criteria: Vec<u32>,
    pub veto: Option<String>,
    pub passed: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WeightedResult {
    pub policy: WeightPolicy,
    pub archive_digest: String,
    pub seats: Vec<SeatWeight>,
    pub overall_basis_points: u32,
    pub passed: bool,
}
pub fn evaluate(
    policy: &WeightPolicy,
    archive: &CheckArchive,
    material: &ReviewMaterial,
    seats: &[ReviewSeat],
    ballots: &[RevealedBallot],
) -> Result<WeightedResult, String> {
    policy.validate()?;
    // Reuse exact roster, model, original/effective-vote and audit validation.
    let crate::independent_verifier::Output::Tallied {
        passed: unanimous, ..
    } = crate::independent_verifier::evaluate(&crate::independent_verifier::Operation::Tally {
        material: material.clone(),
        seats: seats.to_vec(),
        ballots: ballots.to_vec(),
    })?
    else {
        return Err("基本计票响应无效".into());
    };
    let checks = archive.basis_points()?;
    let mut scores = vec![];
    for seat in seats {
        let vote = ballots
            .iter()
            .find(|v| v.seat == seat.id)
            .ok_or("席位票据缺失")?;
        let veto = match vote.tool_audit.as_ref().map(|a| &a.status) {
            Some(ReviewToolStatus::Clean) => None,
            Some(ReviewToolStatus::ToolCallObserved) => Some("评分中发现工具调用或请求".into()),
            _ => Some("评分过程核验依据不足".into()),
        };
        let mut missing = vec![];
        let mut unaligned = vec![];
        let mut aligned = 0;
        for criterion in 1..=material.criteria.len() as u32 {
            match vote
                .ballot
                .assessments
                .iter()
                .find(|a| a.criterion == criterion)
            {
                Some(a) if a.aligned && !a.citations.is_empty() => aligned += 1,
                Some(_) => unaligned.push(criterion),
                None => missing.push(criterion),
            }
        }
        let instruction = aligned * 10000 / material.criteria.len() as u32;
        let total = if veto.is_some() {
            0
        } else {
            (instruction * policy.instruction_share + checks * policy.check_share) / 10000
        };
        scores.push(SeatWeight {
            seat: seat.id.clone(),
            instruction_basis_points: instruction,
            check_basis_points: checks,
            total_basis_points: total,
            missing_criteria: missing,
            unaligned_criteria: unaligned,
            passed: veto.is_none()
                && vote.ballot.vote == Vote::Pass
                && total >= policy.minimum_seat,
            veto,
        });
    }
    let overall = scores.iter().map(|s| s.total_basis_points).sum::<u32>() / seats.len() as u32;
    Ok(WeightedResult {
        policy: policy.clone(),
        archive_digest: digest(archive)?,
        passed: unanimous && scores.iter().all(|s| s.passed) && overall >= policy.minimum_overall,
        overall_basis_points: overall,
        seats: scores,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api_worker::ApiTarget,
        independent_checks::{CheckFact, CheckRecord},
    };
    fn fixture() -> (
        CheckArchive,
        ReviewMaterial,
        Vec<ReviewSeat>,
        Vec<RevealedBallot>,
    ) {
        let record = CheckRecord {
            schema_version: 1,
            project_id: "project".into(),
            revision: "a".repeat(64),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            operation_id: "operation".into(),
            source_digest: "b".repeat(64),
            manifest_digest: "c".repeat(64),
            report_digest: "d".repeat(64),
            observed_at: "2026-11-05T01:00:00Z".into(),
            checks: vec![CheckFact {
                id: "check".into(),
                command_digest: "e".repeat(64),
                tool_digest: "f".repeat(64),
                passed: true,
                reason: "verified".into(),
            }],
        };
        let archive = CheckArchive {
            assembled: None,
            schema_version: 1,
            project_id: record.project_id.clone(),
            revision: record.revision.clone(),
            final_task: record.task_id.clone(),
            final_source: record.source_digest.clone(),
            records: vec![record],
        };
        let material = ReviewMaterial {
            independent_check_evidence: None,
            protocol: PROTOCOL.into(),
            requirement: "Approved synthetic task".into(),
            criteria: (1..=10).map(|i| format!("requirement {i}")).collect(),
            fragments: vec![Fragment {
                id: "f1".into(),
                file: "file1".into(),
                part: 1,
                content: "const result=1;".into(),
            }],
            objective_checks: 1,
        };
        let seats: Vec<_> = ["chief", "deputy", "worker"]
            .iter()
            .map(|role| ReviewSeat {
                reasoning_effort: None,
                max_output_tokens: None,
                id: (*role).into(),
                role: (*role).into(),
                target: Some(ApiTarget::OpenAi {}),
                transport: None,
                model: "model".into(),
                allowed_models: vec!["model".into()],
            })
            .collect();
        let ballots = seats
            .iter()
            .map(|s| {
                let ballot = Ballot {
                    vote: Vote::Pass,
                    reason: "reviewed approved criteria".into(),
                    citations: vec!["f1".into()],
                    assessments: (1..=10)
                        .map(|criterion| CriterionAssessment {
                            criterion,
                            aligned: true,
                            citations: vec!["f1".into()],
                        })
                        .collect(),
                };
                RevealedBallot {
                    seat: s.id.clone(),
                    model: "model".into(),
                    original_ballot: Some(ballot.clone()),
                    ballot,
                    tool_audit: Some(ReviewToolAudit {
                        status: ReviewToolStatus::Clean,
                        source: "api-response".into(),
                        request: "request".into(),
                        evidence_digest: "a".repeat(64),
                        tools: vec![],
                        reason: "complete synthetic trace".into(),
                    }),
                }
            })
            .collect();
        (archive, material, seats, ballots)
    }
    fn omit(vote: &mut RevealedBallot, count: usize) {
        vote.ballot.assessments.truncate(count);
        vote.original_ballot = Some(vote.ballot.clone());
    }
    #[test]
    fn weights_require_both_thresholds_without_removing_failed_seats() {
        let (archive, material, seats, mut ballots) = fixture();
        let score = |b: &[RevealedBallot]| {
            evaluate(&WeightPolicy::default(), &archive, &material, &seats, b).unwrap()
        };
        assert!(score(&ballots).passed);
        omit(&mut ballots[0], 6);
        assert_eq!(score(&ballots).seats[0].total_basis_points, 8000);
        assert!(score(&ballots).passed); // 80,100,100 => 93.33
        omit(&mut ballots[1], 6);
        assert!(!score(&ballots).passed); // 80,80,100 => 86.66
        assert!(score(&ballots).seats.iter().all(|s| s.passed));
        omit(&mut ballots[0], 5);
        assert!(!score(&ballots).seats[0].passed);
        assert_eq!(score(&ballots).seats.len(), 3);
    }
    #[test]
    fn missing_evidence_tool_veto_and_forged_reports_never_gain_weight() {
        let (mut archive, material, seats, mut ballots) = fixture();
        omit(&mut ballots[0], 0);
        assert_eq!(
            evaluate(
                &WeightPolicy::default(),
                &archive,
                &material,
                &seats,
                &ballots
            )
            .unwrap()
            .seats[0]
                .total_basis_points,
            5000
        );
        ballots[1].tool_audit.as_mut().unwrap().status = ReviewToolStatus::Unverifiable;
        let audit = ballots[1].tool_audit.as_ref().unwrap().clone();
        ballots[1].ballot = audit
            .effective_ballot(ballots[1].original_ballot.as_ref(), &material)
            .unwrap();
        let scores = evaluate(
            &WeightPolicy::default(),
            &archive,
            &material,
            &seats,
            &ballots,
        )
        .unwrap();
        assert_eq!(scores.seats[1].total_basis_points, 0);
        assert!(!scores.passed);
        archive.records[0].revision = "f".repeat(64);
        assert!(evaluate(
            &WeightPolicy::default(),
            &archive,
            &material,
            &seats,
            &ballots
        )
        .is_err());
    }
    #[test]
    fn duplicate_or_foreign_criterion_claims_are_rejected() {
        let (_, material, _, mut ballots) = fixture();
        let duplicate = ballots[0].ballot.assessments[0].clone();
        ballots[0].ballot.assessments.push(duplicate);
        assert!(ballots[0].ballot.validate(&material).is_err());
        ballots[0].ballot.assessments.pop();
        ballots[0].ballot.assessments[0].citations = vec!["foreign".into()];
        assert!(ballots[0].ballot.validate(&material).is_err());
    }
}
