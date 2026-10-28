//! Bind a single project account to actual authenticated workflow approvals.
//! Recovery only completes the original journal link; it grants no new approval.
use crate::{
    currency_budget::{ApprovalLink, BudgetLedger, PolicyAuthorization},
    scheduler::DurableWorkflow,
};
use cerulean_domain::{
    currency_budget::BudgetPolicy,
    execution::{digest, require, sha256, ExecutionEvidence, PlanRevision},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalAnchor {
    pub schema: u32,
    pub policy_digest: String,
    pub previous_policy_digest: String,
    pub previous_ledger_head: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanningApproval {
    pub policy: BudgetPolicy,
    pub anchor: ApprovalAnchor,
}
impl PlanningApproval {
    pub(crate) fn validate(&self, project: &str, setup_digest: &str) -> Result<(), String> {
        self.policy.validate()?;
        sha256(&self.anchor.previous_policy_digest)?;
        sha256(&self.anchor.previous_ledger_head)?;
        require(
            self.policy.project_id == project
                && self
                    .policy
                    .planning
                    .as_ref()
                    .is_some_and(|s| s.setup_digest == setup_digest)
                && self.anchor.schema == 1
                && self.anchor.policy_digest == digest(&self.policy)?,
            "planning permission differs from its reviewed configuration or account",
        )
    }
}
pub(crate) fn validate_anchor(
    revision: &PlanRevision,
    anchor: Option<&ApprovalAnchor>,
) -> Result<(), String> {
    match (&revision.policy.currency_budget, anchor) {
        (None, None) => Ok(()),
        (Some(settings), Some(anchor)) => {
            sha256(&anchor.previous_policy_digest)?;
            sha256(&anchor.previous_ledger_head)?;
            require(
                anchor.schema == 1
                    && anchor.policy_digest
                        == digest(
                            &settings.bind(&revision.project.id, &revision.approval_digest()?)?,
                        )?,
                "currency approval anchor differs from reviewed plan",
            )
        }
        _ => Err("currency approval requires its original account anchor".into()),
    }
}
pub(crate) struct Approved {
    policy: BudgetPolicy,
    anchor: ApprovalAnchor,
    source: String,
}
pub(crate) struct Catalogue {
    pub(crate) api_generation_resolutions: Vec<(
        crate::api_generation_resolution::ApiGenerationResolutionReview,
        String,
    )>,
    pub(crate) api_count_resolutions: Vec<(
        crate::api_count_resolution::ApiCountResolutionReview,
        String,
    )>,
    policies: BTreeMap<String, BudgetPolicy>,
    pub(crate) approvals: Vec<Approved>,
    pub(crate) count_resolutions: Vec<(crate::count_resolution::CountResolutionReview, String)>,
    pub(crate) historical: Vec<(crate::historical_confirmation::HistoricalReview, String)>,
}
impl Catalogue {
    pub(crate) fn read(
        store: &DurableWorkflow,
        setups: &[cerulean_domain::project_setup::SavedProjectSetup],
    ) -> Result<Self, String> {
        require(
            !store.requires_reopen(),
            "workflow must be reopened before currency approval",
        )?;
        let project = &store
            .configuration()
            .execution_scope
            .as_ref()
            .ok_or("planning budget needs scoped workflow")?
            .project
            .id;
        Self::read_history(project, store.history(), setups)
    }
    /// Only the Host passes authenticated idle/coordinator history here.
    pub(crate) fn read_history(
        project: &str,
        entries: &[crate::scheduler::Entry],
        setups: &[cerulean_domain::project_setup::SavedProjectSetup],
    ) -> Result<Self, String> {
        let mut result = Self {
            api_count_resolutions: vec![],
            api_generation_resolutions: vec![],
            policies: BTreeMap::new(),
            approvals: vec![],
            historical: vec![],
            count_resolutions: vec![],
        };
        let mut current = None;
        for saved in setups.iter().filter(|s| s.setup.project_id == project) {
            if let Some(settings) = &saved.setup.policy.currency_budget {
                let policy =
                    settings.bind_planning(project, &saved.digest, &saved.setup.chief_model)?;
                result.policies.insert(digest(&policy)?, policy);
            }
        }
        for entry in entries {
            if let Some(review) = &entry.body.context.api_generation_resolution {
                let cerulean_domain::workflow::WorkflowCommand::ConfirmApiGenerationResolution {
                    review_digest,
                } = &entry.body.event.command
                else {
                    return Err("API generation review has no original user command".into());
                };
                review.validate_for(project, review_digest)?;
                require(
                    entry.body.context.approval_source.as_deref()
                        == Some(crate::api_generation_resolution::APPROVAL_SOURCE)
                        && result
                            .approvals
                            .last()
                            .is_some_and(|a| a.anchor.policy_digest == review.policy_digest),
                    "API generation confirmation has no original current user-approved policy",
                )?;
                require(
                    !result
                        .api_generation_resolutions
                        .iter()
                        .any(|(r, _)| r.input.request_id == review.input.request_id),
                    "duplicate API generation confirmation",
                )?;
                result
                    .api_generation_resolutions
                    .push((review.clone(), entry.hash.clone()));
            }
            if let Some(review) = &entry.body.context.api_count_resolution {
                let cerulean_domain::workflow::WorkflowCommand::ConfirmApiCountResolution {
                    review_digest,
                } = &entry.body.event.command
                else {
                    return Err("API count review has no original user command".into());
                };
                review.validate_for(project, review_digest)?;
                require(
                    entry.body.context.approval_source.as_deref()
                        == Some(crate::api_count_resolution::APPROVAL_SOURCE)
                        && result
                            .approvals
                            .last()
                            .is_some_and(|a| a.anchor.policy_digest == review.policy_digest),
                    "API count confirmation has no original current user-approved policy",
                )?;
                require(
                    !result
                        .api_count_resolutions
                        .iter()
                        .any(|(r, _)| r.input.request_id == review.input.request_id),
                    "duplicate API count confirmation",
                )?;
                result
                    .api_count_resolutions
                    .push((review.clone(), entry.hash.clone()));
            }
            if let Some(review) = &entry.body.context.count_resolution {
                let cerulean_domain::workflow::WorkflowCommand::ConfirmManagementCountResolution {
                    review_digest,
                } = &entry.body.event.command
                else {
                    return Err("count review has no original user command".into());
                };
                review.validate(project, review_digest)?;
                require(
                    entry.body.context.approval_source.as_deref()
                        == Some(crate::count_resolution::APPROVAL_SOURCE)
                        && result
                            .approvals
                            .last()
                            .is_some_and(|a| a.anchor.policy_digest == review.policy_digest),
                    "count resolution has no original current user-approved policy",
                )?;
                require(
                    !result
                        .count_resolutions
                        .iter()
                        .any(|(r, _)| r.key().ok() == review.key().ok()),
                    "duplicate count resolution user event",
                )?;
                result
                    .count_resolutions
                    .push((review.clone(), entry.hash.clone()));
                continue;
            }
            if let Some(review) = &entry.body.context.historical_charge {
                let cerulean_domain::workflow::WorkflowCommand::ConfirmHistoricalCharge {
                    review_digest,
                } = &entry.body.event.command
                else {
                    return Err("historical review has no original user command".into());
                };
                review.validate(project, review_digest)?;
                require(
                    entry.body.context.approval_source.as_deref()
                        == Some(crate::historical_confirmation::APPROVAL_SOURCE)
                        && result
                            .approvals
                            .last()
                            .is_some_and(|a| a.anchor.policy_digest == review.policy_digest),
                    "historical confirmation has no original current user-approved policy",
                )?;
                require(
                    !result
                        .historical
                        .iter()
                        .any(|(r, _)| r.source.scope.key().ok() == review.source.scope.key().ok()),
                    "duplicate original historical confirmation",
                )?;
                result.historical.push((review.clone(), entry.hash.clone()));
                continue;
            }
            if let Some(planning) = &entry.body.context.planning_budget {
                let cerulean_domain::workflow::WorkflowCommand::AuthorizePlanningBudget {
                    setup_digest,
                } = &entry.body.event.command
                else {
                    return Err("planning permission has no matching user command".into());
                };
                planning.validate(project, setup_digest)?;
                require(
                    entry.body.context.approval_source.as_deref()
                        == Some("authenticated local-user planning-budget approval")
                        && result.policies.get(&planning.anchor.policy_digest)
                            == Some(&planning.policy),
                    "planning permission has no exact original saved configuration",
                )?;
                require(
                    !result
                        .approvals
                        .iter()
                        .any(|a| a.anchor.policy_digest == planning.anchor.policy_digest),
                    "duplicate planning permission",
                )?;
                result.approvals.push(Approved {
                    policy: planning.policy.clone(),
                    anchor: planning.anchor.clone(),
                    source: entry.hash.clone(),
                });
                continue;
            }
            match &entry.body.context.execution {
                Some(ExecutionEvidence::Revision { revision }) => {
                    current = Some(revision);
                    if let Some(settings) = &revision.policy.currency_budget {
                        let policy =
                            settings.bind(&revision.project.id, &revision.approval_digest()?)?;
                        result.policies.insert(digest(&policy)?, policy);
                    }
                }
                Some(ExecutionEvidence::Approval { .. }) => {
                    let revision = current.ok_or("currency approval has no original proposal")?;
                    validate_anchor(revision, entry.body.context.currency_approval.as_ref())?;
                    if let Some(anchor) = &entry.body.context.currency_approval {
                        require(
                            entry.body.context.approval_source.as_deref()
                                == Some("authenticated local-user service request"),
                            "currency approval source is not the authenticated user",
                        )?;
                        require(
                            !result
                                .approvals
                                .iter()
                                .any(|a| a.anchor.policy_digest == anchor.policy_digest),
                            "duplicate currency approval revision",
                        )?;
                        result.approvals.push(Approved {
                            policy: result
                                .policies
                                .get(&anchor.policy_digest)
                                .ok_or("approved currency policy missing")?
                                .clone(),
                            anchor: anchor.clone(),
                            source: entry.hash.clone(),
                        });
                    } else {
                        require(
                            result.approvals.is_empty(),
                            "an approved currency project cannot remove its cumulative budget",
                        )?;
                    }
                }
                _ => {}
            }
        }
        Ok(result)
    }
    pub(crate) fn reconcile(&self, book: &mut BudgetLedger) -> Result<(), String> {
        require(book.healthy(), "currency account requires reopen")?;
        for count in book.state().management_counts.values() {
            for saved in [count.fee_resolution.as_deref(), count.retirement.as_deref()]
                .into_iter()
                .flatten()
            {
                require(
                    self.count_resolutions
                        .iter()
                        .any(|(r, h)| r.resolution(h) == *saved),
                    "count resolution has no original user event",
                )?;
            }
        }
        for count in book.state().bookings.values() {
            if let Some(saved) = &count.fee_resolution {
                require(
                    self.api_generation_resolutions
                        .iter()
                        .any(|(r, h)| r.resolution(h) == **saved),
                    "API generation fee has no original user event",
                )?;
            }
        }
        for count in book.state().api_counts.values() {
            if let Some(saved) = &count.fee_resolution {
                require(
                    self.api_count_resolutions
                        .iter()
                        .any(|(r, h)| r.resolution(h) == **saved),
                    "API count fee has no original user event",
                )?;
            }
        }
        for (key, charge) in &book.state().historical_charges {
            require(self.historical.iter().any(|(r,h)| r.source.scope.key().as_ref() == Ok(key) && r.charge(h) == *charge),
                "historical charges require original user-event reconciliation; preserve the account")?;
        }
        for (hash, policy) in &book.state().policies {
            require(
                self.policies.get(hash) == Some(policy),
                "currency account policy has no original workflow proposal",
            )?;
        }
        for (hash, link) in &book.state().approval_links {
            let approved = self
                .approvals
                .iter()
                .find(|a| &a.anchor.policy_digest == hash)
                .ok_or("currency authorization has no original user approval")?;
            require(
                link == &approved.link(),
                "currency authorization differs from original user approval",
            )?;
        }
        for booking in book.state().bookings.values() {
            require(
                book.state()
                    .approval_links
                    .contains_key(&booking.policy_digest),
                "currency spending has no linked user approval",
            )?;
        }
        let missing: Vec<_> = self
            .approvals
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                !book
                    .state()
                    .approval_links
                    .contains_key(&a.anchor.policy_digest)
            })
            .collect();
        require(
            missing.len() <= 1
                && missing
                    .first()
                    .is_none_or(|(i, _)| *i + 1 == self.approvals.len()),
            "currency authorization history is incomplete; preserve original account",
        )?;
        if let Some((_, approved)) = missing.first() {
            require(
                book.head() == approved.anchor.previous_ledger_head
                    && digest(book.policy())? == approved.anchor.previous_policy_digest,
                "currency account changed since original approval; no automatic reconciliation",
            )?;
            if book.policy() == &approved.policy {
                book.bind_initial_approval(approved.link())?;
            } else {
                book.authorize(PolicyAuthorization {
                    previous_policy_digest: approved.anchor.previous_policy_digest.clone(),
                    previous_ledger_head: approved.anchor.previous_ledger_head.clone(),
                    policy: approved.policy.clone(),
                    approval_evidence_sha256: approved.source.clone(),
                })?;
            }
        }
        if let Some(last) = self.approvals.last() {
            require(
                book.policy() == &last.policy,
                "currency account does not match latest approved policy",
            )?;
        } else {
            require(
                book.state().policies.len() == 1
                    && book.state().bookings.is_empty()
                    && book.state().approval_links.is_empty(),
                "unapproved currency account is not a pristine preparation",
            )?;
        }
        Ok(())
    }
}
impl Approved {
    fn link(&self) -> ApprovalLink {
        ApprovalLink {
            previous_policy_digest: self.anchor.previous_policy_digest.clone(),
            previous_ledger_head: self.anchor.previous_ledger_head.clone(),
            approval_evidence_sha256: self.source.clone(),
        }
    }
}
