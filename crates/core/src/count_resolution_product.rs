//! Bounded count inventory and explicit original-confirmation lookup.
use super::*;
use cerulean_domain::count_resolution::*;

impl RuntimeHost {
    fn count_records(&self) -> Result<Vec<CountResolutionRecord>, String> {
        self.count_resolution_confirmations()?
            .values()
            .map(|r| {
                let b = &self.currency_accounts[&r.review.input.project_id]
                    .state()
                    .management_counts[&r.review.input.operation_id];
                Ok(CountResolutionRecord {
                    review: r.review.view()?,
                    user_event_digest: r.user_event_digest.clone(),
                    state: if b.resolution(&r.review.input.action).is_some() {
                        CountResolutionState::Recorded
                    } else {
                        CountResolutionState::ConfirmedPending
                    },
                })
            })
            .collect()
    }
    fn read_management_counts(
        &self,
        offset: u32,
        expected: Option<&str>,
    ) -> Result<ManagementCountPage, String> {
        require(
            self.currency_accounts.values().all(|b| b.healthy()),
            "count accounts require reopening",
        )?;
        let records = self.count_records()?;
        let summary = |project: &str, operation: &str, aspect| {
            records
                .iter()
                .find(|r| {
                    r.review.input.project_id == project
                        && r.review.input.operation_id == operation
                        && r.review.input.action.aspect() == aspect
                })
                .map(|r| CountResolutionSummary {
                    review_digest: r.review.review_digest.clone(),
                    user_event_digest: r.user_event_digest.clone(),
                    state: r.state,
                    amount_usd_nanos: match r.review.input.action {
                        CountResolutionAction::SettleFee {
                            amount_usd_nanos, ..
                        } => Some(amount_usd_nanos),
                        _ => None,
                    },
                })
        };
        let entries: Vec<_> = self
            .currency_accounts
            .iter()
            .flat_map(|(project, book)| {
                book.state()
                    .management_counts
                    .iter()
                    .map(|(id, b)| ManagementCountEntry {
                        project_id: project.clone(),
                        operation_id: id.clone(),
                        original_outcome: b.original_outcome().into(),
                        maximum_usd_nanos: b.attempt.maximum_usd_nanos,
                        fee_resolution: summary(project, id, CountResolutionAspect::SettleFee),
                        retirement: summary(project, id, CountResolutionAspect::RetireContinuation),
                    })
            })
            .collect();
        let revision = digest(&("count-inventory-v1", &entries))?;
        require(
            expected.is_none_or(|e| e == revision),
            "计数清单已改变，请重新读取首页",
        )?;
        require(
            offset as usize <= entries.len() && (offset == 0 || expected.is_some()),
            "计数清单分页需要原摘要及有效位置",
        )?;
        let total = u32::try_from(entries.len()).map_err(|_| "count inventory too large")?;
        let page: Vec<_> = entries.into_iter().skip(offset as usize).take(20).collect();
        let next = offset + page.len() as u32;
        Ok(ManagementCountPage {
            revision,
            offset,
            total,
            next_offset: (next < total).then_some(next),
            entries: page,
        })
    }
    fn read_count_confirmation(
        &self,
        project: &str,
        operation: &str,
        aspect: CountResolutionAspect,
        review: &str,
    ) -> Result<CountConfirmationLookup, String> {
        identifier(project)?;
        identifier(operation)?;
        sha256(review)?;
        require(
            self.currency_accounts
                .get(project)
                .is_some_and(|b| b.state().management_counts.contains_key(operation)),
            "original count missing",
        )?;
        let record = self.count_records()?.into_iter().find(|r| {
            r.review.input.project_id == project
                && r.review.input.operation_id == operation
                && r.review.input.action.aspect() == aspect
        });
        require(
            record
                .as_ref()
                .is_none_or(|r| r.review.review_digest == review),
            "原计数已有其他确认，请刷新清单核对",
        )?;
        Ok(CountConfirmationLookup {
            project_id: project.into(),
            operation_id: operation.into(),
            aspect,
            review_digest: review.into(),
            record,
        })
    }
    pub(crate) fn count_resolution_result(
        &mut self,
        command: &Command,
    ) -> Result<CountResolutionResult, String> {
        require(command.is_count_resolution(), "计数核对接口不接受其他操作")?;
        if command.is_api_generation_resolution() {
            return self.api_generation_resolution_result(command);
        }
        if command.is_api_count_resolution() {
            return self.api_count_resolution_result(command);
        }
        let mut result = CountResolutionResult::default();
        match command {
            Command::ReadManagementCounts { offset, revision } => {
                result.counts = Some(self.read_management_counts(*offset, revision.as_deref())?)
            }
            Command::ReviewManagementCountResolution { input } => {
                result.review = Some(self.review_management_count_resolution(input)?.view()?)
            }
            Command::ConfirmManagementCountResolution {
                input,
                review_digest,
            } => {
                input.validate()?;
                sha256(review_digest)?;
                let saved = self
                    .projects
                    .get(&input.project_id)
                    .ok_or("count project missing")?
                    .history()
                    .iter()
                    .filter_map(|e| e.body.context.count_resolution.as_ref())
                    .find(|r| {
                        r.input.operation_id == input.operation_id
                            && r.input.action.aspect() == input.action.aspect()
                    })
                    .cloned();
                let review = match saved {
                    Some(r) => {
                        require(r.input == *input, "count confirmation input changed")?;
                        r
                    }
                    None => self.review_management_count_resolution(input)?,
                };
                self.confirm_management_count_resolution(&review, review_digest)?;
                result.confirmation = Some(self.read_count_confirmation(
                    &input.project_id,
                    &input.operation_id,
                    input.action.aspect(),
                    review_digest,
                )?);
            }
            Command::ReadManagementCountConfirmation {
                project_id,
                operation_id,
                aspect,
                review_digest,
            } => {
                result.confirmation = Some(self.read_count_confirmation(
                    project_id,
                    operation_id,
                    *aspect,
                    review_digest,
                )?)
            }
            _ => unreachable!(),
        }
        // Keep narrow replies bounded; no Snapshot, scheduler tick or automatic send.
        Ok(result)
    }
}
