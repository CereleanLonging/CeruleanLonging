//! Local-user product commands and cached projections of original confirmations.
use super::*;
use cerulean_domain::historical_money::{HistoricalAccounting, HistoricalAccountingState as State};
use std::borrow::Cow;

pub(super) struct AccountingRecord<'a> {
    pub review: Cow<'a, crate::historical_confirmation::HistoricalReview>,
    pub event: Cow<'a, str>,
    pub state: State,
}
impl AccountingRecord<'_> {
    pub(super) fn view(&self) -> Result<HistoricalAccounting, String> {
        Ok(HistoricalAccounting {
            review: self.review.view()?,
            user_event_digest: self.event.to_string(),
            state: self.state,
        })
    }
}

impl RuntimeHost {
    pub(super) fn historical_currency_request(
        &mut self,
        command: &Command,
    ) -> Option<Result<Snapshot, String>> {
        if !command.is_historical() {
            return None;
        }
        Some((|| {
            let result = self.historical_result(command)?;
            let mut snapshot = self.snapshot();
            snapshot.historical_sources = result.historical_sources;
            snapshot.historical_accounting = result.historical_accounting;
            snapshot.historical_charge_review = result.historical_charge_review;
            snapshot.historical_confirmation = result.historical_confirmation;
            Ok(snapshot)
        })())
    }

    /// Narrow authenticated-user operation. Does not construct a full Snapshot
    /// or drive a scheduler tick. The same original confirmation path is used.
    pub(crate) fn historical_result(
        &mut self,
        command: &Command,
    ) -> Result<cerulean_domain::historical_money::HistoricalResult, String> {
        use cerulean_domain::historical_money::HistoricalResult;
        require(command.is_historical(), "历史接口不接受其他工作台操作")?;
        let mut result = HistoricalResult::default();
        let project = match command {
            Command::ReadHistoricalSources { offset, revision } => {
                result.historical_sources =
                    Some(self.read_historical_sources(*offset, revision.as_deref())?);
                None
            }
            Command::ReadHistoricalAccounting { offset, revision } => {
                result.historical_accounting =
                    Some(self.read_historical_accounting(*offset, revision.as_deref())?);
                None
            }
            Command::ReadHistoricalConfirmation {
                project_id,
                scope,
                review_digest,
            } => {
                result.historical_confirmation =
                    Some(self.read_historical_confirmation(project_id, scope, review_digest)?);
                Some(project_id)
            }
            Command::ReviewHistoricalCharge { input } => {
                result.historical_charge_review = Some(
                    self.review_historical_charge(
                        &input.project_id,
                        &input.scope,
                        input.amount_usd_nanos,
                        &input.billing_reference,
                        &input.explanation,
                    )?
                    .view()?,
                );
                Some(&input.project_id)
            }
            Command::ConfirmHistoricalCharge {
                input,
                review_digest,
            } => {
                sha256(review_digest)?;
                let key = input.scope.key()?;
                let saved = self
                    .projects
                    .get(&input.project_id)
                    .ok_or("historical project missing")?
                    .history()
                    .iter()
                    .filter_map(|e| e.body.context.historical_charge.as_ref())
                    .find(|r| r.source.scope.key().as_ref() == Ok(&key))
                    .cloned();
                let review = if let Some(saved) = saved {
                    require(
                        saved.input() == *input,
                        "historical confirmation input changed",
                    )?;
                    saved
                } else {
                    self.review_historical_charge(
                        &input.project_id,
                        &input.scope,
                        input.amount_usd_nanos,
                        &input.billing_reference,
                        &input.explanation,
                    )?
                };
                self.confirm_historical_charge(&review, review_digest)?;

                result.historical_confirmation = Some(self.read_historical_confirmation(
                    &input.project_id,
                    &input.scope,
                    review_digest,
                )?);
                Some(&input.project_id)
            }
            _ => unreachable!("historical commands checked above"),
        };
        if result.historical_sources.is_none() {
            result.historical_sources = self.read_historical_sources(0, None).ok();
        }
        if result.historical_accounting.is_none() {
            result.historical_accounting = self.read_historical_accounting(0, None).ok();
        }
        if let Some(project) = project {
            let history = self.currency_history_gaps();
            result.currency_budgets = self
                .currency_account_views(history.as_deref())
                .into_iter()
                .filter(|a| &a.project_id == project)
                .collect();
        }
        Ok(result)
    }

    /// In-memory projection of records validated on load/confirmation. Never
    /// reopens active API call journals on a UI polling tick.
    pub(super) fn historical_accounting_records(
        &self,
    ) -> Option<BTreeMap<String, AccountingRecord<'_>>> {
        if !self.management.healthy()
            || self.projects.values().any(|p| p.requires_reopen())
            || self.currency_accounts.values().any(|b| !b.healthy())
        {
            return None;
        }
        let mut views = BTreeMap::new();
        for entries in self.currency_workflow_entries().ok()?.into_values() {
            type ReviewPair<'a> = (
                Cow<'a, crate::historical_confirmation::HistoricalReview>,
                Cow<'a, str>,
            );
            let records: Box<dyn Iterator<Item = ReviewPair<'_>> + '_> = match entries {
                Cow::Borrowed(entries) => Box::new(entries.iter().filter_map(|e| {
                    e.body
                        .context
                        .historical_charge
                        .as_ref()
                        .map(|r| (Cow::Borrowed(r), Cow::Borrowed(e.hash.as_str())))
                })),
                Cow::Owned(entries) => Box::new(entries.into_iter().filter_map(|e| {
                    e.body
                        .context
                        .historical_charge
                        .map(|r| (Cow::Owned(r), Cow::Owned(e.hash)))
                })),
            };
            for (review, event) in records {
                let key = review.source.scope.key().ok()?;
                let book = self.currency_accounts.get(&review.project_id)?;
                let state = match book.state().historical_charges.get(&key) {
                    Some(charge) if *charge == review.charge(&event) => State::Accounted,
                    Some(_) => return None,
                    None => State::ConfirmedPending,
                };
                if views
                    .insert(
                        key,
                        AccountingRecord {
                            review,
                            event,
                            state,
                        },
                    )
                    .is_some()
                {
                    return None;
                }
            }
        }
        for book in self.currency_accounts.values() {
            for (key, charge) in &book.state().historical_charges {
                let v = views.get(key)?;
                if v.review.project_id != charge.project_id || v.state != State::Accounted {
                    return None;
                }
            }
        }
        Some(views)
    }
}
