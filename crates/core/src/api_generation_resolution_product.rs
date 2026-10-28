//! API generation views use the existing authenticated count-result transport.
use super::*;
use cerulean_domain::{
    api_generation_resolution::*,
    count_resolution::{CountResolutionResult, CountResolutionState, CountResolutionSummary},
};

fn original_details(b: &crate::currency_budget::Booking) -> (Option<String>, Option<String>) {
    use crate::currency_budget::Outcome;
    match &b.outcome {
        Outcome::Unpriced { settlement, reason } => {
            (Some(settlement.response_id.clone()), Some(reason.clone()))
        }
        Outcome::Settled { settlement, .. } => (Some(settlement.response_id.clone()), None),
        _ => (None, None),
    }
}

fn bounded(result: CountResolutionResult) -> Result<CountResolutionResult, String> {
    require(
        serde_json::to_vec(&result)
            .map_err(|e| e.to_string())?
            .len()
            <= 64 * 1024,
        "API生成核对回复超过64KiB，未发送新确认",
    )?;
    Ok(result)
}
impl RuntimeHost {
    fn api_generation_review_view(
        &self,
        r: &crate::api_generation_resolution::ApiGenerationResolutionReview,
    ) -> Result<ApiGenerationReview, String> {
        let count = self
            .currency_accounts
            .get(&r.input.project_id)
            .and_then(|b| b.state().bookings.get(&r.input.request_id))
            .ok_or("original API generation missing")?;
        require(
            count.generation_source_digest()? == r.source_digest,
            "API generation source changed",
        )?;
        Ok(ApiGenerationReview {
            input: r.input.clone(),
            task_id: count
                .api_identity
                .as_ref()
                .ok_or("original API identity missing")?
                .task_id
                .clone(),
            attempt_id: count
                .api_identity
                .as_ref()
                .ok_or("original API identity missing")?
                .attempt_id
                .clone(),
            round: count
                .api_identity
                .as_ref()
                .ok_or("original API identity missing")?
                .round,
            source_digest: r.source_digest.clone(),
            count_digest: r.count_digest.clone(),
            policy_digest: r.policy_digest.clone(),
            previous_ledger_head: r.previous_ledger_head.clone(),
            review_digest: r.digest()?,
            maximum_usd_nanos: r.maximum_usd_nanos,
            original_outcome: r.original_outcome.clone(),
            response_id: original_details(count).0,
            pricing_issue: original_details(count).1,
        })
    }
    fn read_api_generation_confirmation(
        &self,
        project: &str,
        request: &str,
        review: &str,
    ) -> Result<ApiGenerationLookup, String> {
        identifier(project)?;
        identifier(request)?;
        sha256(review)?;
        let count = self
            .currency_accounts
            .get(project)
            .and_then(|b| b.state().bookings.get(request))
            .ok_or("original API generation missing")?;
        let confirmed = self.api_generation_resolution_confirmations()?;
        let record = confirmed
            .get(&digest(&(project, request))?)
            .map(|r| {
                require(
                    r.review.digest()? == review,
                    "原API生成已有其他确认，请刷新清单核对",
                )?;
                Ok::<_, String>(ApiGenerationRecord {
                    review: self.api_generation_review_view(&r.review)?,
                    user_event_digest: r.user_event_digest.clone(),
                    state: if count.fee_resolution.is_some() {
                        CountResolutionState::Recorded
                    } else {
                        CountResolutionState::ConfirmedPending
                    },
                })
            })
            .transpose()?;
        Ok(ApiGenerationLookup {
            project_id: project.into(),
            request_id: request.into(),
            review_digest: review.into(),
            record,
        })
    }
    fn read_api_generations(
        &self,
        offset: u32,
        expected: Option<&str>,
    ) -> Result<ApiGenerationPage, String> {
        require(
            self.currency_accounts.values().all(|b| b.healthy()),
            "count accounts require reopen",
        )?;
        let confirmed = self.api_generation_resolution_confirmations()?;
        let mut entries = Vec::new();
        for (project, book) in &self.currency_accounts {
            for (request, count) in &book.state().bookings {
                if count.scoped_api_preparation.is_none() {
                    continue;
                }
                book.checked_api_preparation(request)?;
                let fee_resolution = confirmed
                    .get(&digest(&(project, request))?)
                    .map(|r| {
                        Ok::<_, String>(CountResolutionSummary {
                            review_digest: r.review.digest()?,
                            user_event_digest: r.user_event_digest.clone(),
                            state: if count.fee_resolution.is_some() {
                                CountResolutionState::Recorded
                            } else {
                                CountResolutionState::ConfirmedPending
                            },
                            amount_usd_nanos: Some(r.review.input.amount_usd_nanos),
                        })
                    })
                    .transpose()?;
                entries.push(ApiGenerationEntry {
                    project_id: project.clone(),
                    request_id: request.clone(),
                    task_id: count
                        .api_identity
                        .as_ref()
                        .ok_or("original API identity missing")?
                        .task_id
                        .clone(),
                    attempt_id: count
                        .api_identity
                        .as_ref()
                        .ok_or("original API identity missing")?
                        .attempt_id
                        .clone(),
                    round: count
                        .api_identity
                        .as_ref()
                        .ok_or("original API identity missing")?
                        .round,
                    original_outcome: count.generation_outcome().into(),
                    response_id: original_details(count).0,
                    pricing_issue: original_details(count).1,
                    maximum_usd_nanos: count.reserved_usd_nanos,
                    fee_resolution,
                });
            }
        }
        page_entries(entries, offset, expected)
    }
    pub(super) fn api_generation_resolution_result(
        &mut self,
        command: &Command,
    ) -> Result<CountResolutionResult, String> {
        require(
            command.is_api_generation_resolution(),
            "API生成核对接口不接受其他操作",
        )?;
        let mut result = CountResolutionResult::default();
        match command {
            Command::ReadApiGenerations { offset, revision } => {
                result.api_generations =
                    Some(self.read_api_generations(*offset, revision.as_deref())?)
            }
            Command::ReviewApiGenerationResolution { input } => {
                result.api_generation_review =
                    Some(self.api_generation_review_view(
                        &self.review_api_generation_resolution(input)?,
                    )?)
            }
            Command::ReadApiGenerationConfirmation {
                project_id,
                request_id,
                review_digest,
            } => {
                result.api_generation_confirmation = Some(self.read_api_generation_confirmation(
                    project_id,
                    request_id,
                    review_digest,
                )?)
            }
            Command::ConfirmApiGenerationResolution {
                input,
                review_digest,
            } => {
                input.validate()?;
                sha256(review_digest)?;
                let confirmed = self.api_generation_resolution_confirmations()?;
                let review = match confirmed.get(&digest(&(&input.project_id, &input.request_id))?)
                {
                    Some(saved) => {
                        require(
                            saved.review.input == *input,
                            "API generation confirmation input changed",
                        )?;
                        saved.review.clone()
                    }
                    None => self.review_api_generation_resolution(input)?,
                };
                // Bound the confirmation response BEFORE writing its user event.
                bounded(CountResolutionResult {
                    api_generation_confirmation: Some(ApiGenerationLookup {
                        project_id: input.project_id.clone(),
                        request_id: input.request_id.clone(),
                        review_digest: review_digest.clone(),
                        record: Some(ApiGenerationRecord {
                            review: self.api_generation_review_view(&review)?,
                            user_event_digest: "0".repeat(64),
                            state: CountResolutionState::ConfirmedPending,
                        }),
                    }),
                    ..Default::default()
                })?;
                self.confirm_api_generation_resolution(&review, review_digest)?;
                result.api_generation_confirmation = Some(self.read_api_generation_confirmation(
                    &input.project_id,
                    &input.request_id,
                    review_digest,
                )?);
            }
            _ => unreachable!(),
        }
        bounded(result)
    }
}

fn page_entries(
    entries: Vec<ApiGenerationEntry>,
    offset: u32,
    expected: Option<&str>,
) -> Result<ApiGenerationPage, String> {
    let revision = digest(&("api-generation-inventory-v1", &entries))?;
    require(
        expected.is_none_or(|r| r == revision),
        "API生成清单已改变，请重新读取首页",
    )?;
    require(
        offset as usize <= entries.len() && (offset == 0 || expected.is_some()),
        "API生成分页需要原版本及有效位置",
    )?;
    let total = u32::try_from(entries.len()).map_err(|_| "API generation inventory too large")?;
    let entries: Vec<_> = entries.into_iter().skip(offset as usize).take(20).collect();
    let next = offset + entries.len() as u32;
    Ok(ApiGenerationPage {
        revision,
        offset,
        total,
        next_offset: (next < total).then_some(next),
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn api_generation_product_pages_require_original_revision_and_preserve_separate_requests() {
        let entries: Vec<_> = (0..21)
            .map(|i| ApiGenerationEntry {
                project_id: "p".into(),
                request_id: format!("r{i:03}"),
                task_id: "task".into(),
                attempt_id: format!("attempt-{i}"),
                round: 1,
                original_outcome: "unknown".into(),
                response_id: None,
                pricing_issue: None,
                maximum_usd_nanos: 17,
                fee_resolution: None,
            })
            .collect();
        let first = page_entries(entries.clone(), 0, None).unwrap();
        assert_eq!(first.entries.len(), 20);
        assert_eq!(first.next_offset, Some(20));
        assert!(page_entries(entries.clone(), 20, None).is_err());
        let next = page_entries(entries.clone(), 20, Some(&first.revision)).unwrap();
        assert_eq!(next.entries[0].request_id, "r020");
        assert_eq!(next.next_offset, None);
        let mut changed = entries.clone();
        changed[0].original_outcome = "counted".into();
        assert!(page_entries(changed, 20, Some(&first.revision)).is_err());
        assert!(page_entries(entries, 22, Some(&first.revision)).is_err());
    }
}
