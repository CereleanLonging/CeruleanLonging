//! API count views use the existing authenticated count-result transport.
use super::*;
use cerulean_domain::{
    api_count_resolution::*,
    count_resolution::{CountResolutionResult, CountResolutionState, CountResolutionSummary},
};

fn bounded(result: CountResolutionResult) -> Result<CountResolutionResult, String> {
    require(
        serde_json::to_vec(&result)
            .map_err(|e| e.to_string())?
            .len()
            <= 64 * 1024,
        "API计数核对回复超过64KiB，未发送新确认",
    )?;
    Ok(result)
}
impl RuntimeHost {
    fn api_count_review_view(
        &self,
        r: &crate::api_count_resolution::ApiCountResolutionReview,
    ) -> Result<ApiCountReview, String> {
        let count = self
            .currency_accounts
            .get(&r.input.project_id)
            .and_then(|b| b.state().api_counts.get(&r.input.request_id))
            .ok_or("original API count missing")?;
        require(
            count.source_digest()? == r.source_digest,
            "API count source changed",
        )?;
        Ok(ApiCountReview {
            input: r.input.clone(),
            task_id: count.attempt.call.task_id.clone(),
            attempt_id: count.attempt.call.attempt_id.clone(),
            round: count.attempt.call.round,
            source_digest: r.source_digest.clone(),
            generation_digest: r.generation_digest.clone(),
            policy_digest: r.policy_digest.clone(),
            previous_ledger_head: r.previous_ledger_head.clone(),
            review_digest: r.digest()?,
            maximum_usd_nanos: r.maximum_usd_nanos,
            original_outcome: r.original_outcome.clone(),
        })
    }
    fn read_api_count_confirmation(
        &self,
        project: &str,
        request: &str,
        review: &str,
    ) -> Result<ApiCountLookup, String> {
        identifier(project)?;
        identifier(request)?;
        sha256(review)?;
        let count = self
            .currency_accounts
            .get(project)
            .and_then(|b| b.state().api_counts.get(request))
            .ok_or("original API count missing")?;
        let confirmed = self.api_count_resolution_confirmations()?;
        let record = confirmed
            .get(&digest(&(project, request))?)
            .map(|r| {
                require(
                    r.review.digest()? == review,
                    "原API计数已有其他确认，请刷新清单核对",
                )?;
                Ok::<_, String>(ApiCountRecord {
                    review: self.api_count_review_view(&r.review)?,
                    user_event_digest: r.user_event_digest.clone(),
                    state: if count.fee_resolution.is_some() {
                        CountResolutionState::Recorded
                    } else {
                        CountResolutionState::ConfirmedPending
                    },
                })
            })
            .transpose()?;
        Ok(ApiCountLookup {
            project_id: project.into(),
            request_id: request.into(),
            review_digest: review.into(),
            record,
        })
    }
    fn read_api_counts(&self, offset: u32, expected: Option<&str>) -> Result<ApiCountPage, String> {
        require(
            self.currency_accounts.values().all(|b| b.healthy()),
            "count accounts require reopen",
        )?;
        let confirmed = self.api_count_resolution_confirmations()?;
        let mut entries = Vec::new();
        for (project, book) in &self.currency_accounts {
            for (request, count) in &book.state().api_counts {
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
                entries.push(ApiCountEntry {
                    project_id: project.clone(),
                    request_id: request.clone(),
                    task_id: count.attempt.call.task_id.clone(),
                    attempt_id: count.attempt.call.attempt_id.clone(),
                    round: count.attempt.call.round,
                    original_outcome: count.original_outcome().into(),
                    maximum_usd_nanos: count.attempt.maximum_usd_nanos,
                    fee_resolution,
                });
            }
        }
        page_entries(entries, offset, expected)
    }
    pub(super) fn api_count_resolution_result(
        &mut self,
        command: &Command,
    ) -> Result<CountResolutionResult, String> {
        require(
            command.is_api_count_resolution(),
            "API计数核对接口不接受其他操作",
        )?;
        let mut result = CountResolutionResult::default();
        match command {
            Command::ReadApiCounts { offset, revision } => {
                result.api_counts = Some(self.read_api_counts(*offset, revision.as_deref())?)
            }
            Command::ReviewApiCountResolution { input } => {
                result.api_review =
                    Some(self.api_count_review_view(&self.review_api_count_resolution(input)?)?)
            }
            Command::ReadApiCountConfirmation {
                project_id,
                request_id,
                review_digest,
            } => {
                result.api_confirmation =
                    Some(self.read_api_count_confirmation(project_id, request_id, review_digest)?)
            }
            Command::ConfirmApiCountResolution {
                input,
                review_digest,
            } => {
                input.validate()?;
                sha256(review_digest)?;
                let confirmed = self.api_count_resolution_confirmations()?;
                let review = match confirmed.get(&digest(&(&input.project_id, &input.request_id))?)
                {
                    Some(saved) => {
                        require(
                            saved.review.input == *input,
                            "API count confirmation input changed",
                        )?;
                        saved.review.clone()
                    }
                    None => self.review_api_count_resolution(input)?,
                };
                // Bound the confirmation response BEFORE writing its user event.
                bounded(CountResolutionResult {
                    api_confirmation: Some(ApiCountLookup {
                        project_id: input.project_id.clone(),
                        request_id: input.request_id.clone(),
                        review_digest: review_digest.clone(),
                        record: Some(ApiCountRecord {
                            review: self.api_count_review_view(&review)?,
                            user_event_digest: "0".repeat(64),
                            state: CountResolutionState::ConfirmedPending,
                        }),
                    }),
                    ..Default::default()
                })?;
                self.confirm_api_count_resolution(&review, review_digest)?;
                result.api_confirmation = Some(self.read_api_count_confirmation(
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
    entries: Vec<ApiCountEntry>,
    offset: u32,
    expected: Option<&str>,
) -> Result<ApiCountPage, String> {
    let revision = digest(&("api-count-inventory-v1", &entries))?;
    require(
        expected.is_none_or(|r| r == revision),
        "API计数清单已改变，请重新读取首页",
    )?;
    require(
        offset as usize <= entries.len() && (offset == 0 || expected.is_some()),
        "API计数分页需要原版本及有效位置",
    )?;
    let total = u32::try_from(entries.len()).map_err(|_| "API count inventory too large")?;
    let entries: Vec<_> = entries.into_iter().skip(offset as usize).take(20).collect();
    let next = offset + entries.len() as u32;
    Ok(ApiCountPage {
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
    fn api_count_product_pages_require_original_revision_and_preserve_separate_requests() {
        let entries: Vec<_> = (0..21)
            .map(|i| ApiCountEntry {
                project_id: "p".into(),
                request_id: format!("r{i:03}"),
                task_id: "task".into(),
                attempt_id: format!("attempt-{i}"),
                round: 1,
                original_outcome: "unknown".into(),
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
