use super::*;
use crate::{
    currency_approval::{ApprovalAnchor, Catalogue, PlanningApproval},
    currency_budget::{BudgetLedger, PolicyAuthorization},
};

impl RuntimeHost {
    /// Validated history from the one current owner; never reopens its journal.
    pub(super) fn currency_workflow_entries(
        &self,
    ) -> Result<BTreeMap<String, std::borrow::Cow<'_, [crate::scheduler::Entry]>>, String> {
        let mut entries = BTreeMap::new();
        for (project, store) in &self.projects {
            require(
                !store.requires_reopen(),
                "currency workflow requires reopen",
            )?;
            entries.insert(project.clone(), std::borrow::Cow::Borrowed(store.history()));
        }
        #[cfg(windows)]
        for (project, writer) in &self.api_writers {
            let snapshot = writer.owner.snapshot()?;
            let task = snapshot
                .state
                .tasks
                .keys()
                .next()
                .ok_or("currency coordinator has no approved task")?;
            let reader = self.api_read(project, task, "currency-read")?;
            require(
                entries
                    .insert(
                        project.clone(),
                        std::borrow::Cow::Owned(reader.history().to_vec()),
                    )
                    .is_none(),
                "currency workflow has two owners",
            )?;
        }
        Ok(entries)
    }
    pub(super) fn currency_catalogues(&self) -> Result<BTreeMap<String, Catalogue>, String> {
        let setups = crate::project_setup::history(&self.root)?;
        self.currency_workflow_entries()?
            .into_iter()
            .map(|(project, entries)| {
                (if let Some(store) = self.projects.get(&project) {
                    Catalogue::read(store, &setups)
                } else {
                    Catalogue::read_history(&project, &entries, &setups)
                })
                .map(|c| (project, c))
            })
            .collect()
    }
    fn currency_current_approval(&self, project: &str) -> Option<Approval> {
        if let Some(store) = self.projects.get(project) {
            return (!store.requires_reopen())
                .then(|| store.approval().cloned())
                .flatten();
        }
        #[cfg(windows)]
        if let Some(writer) = self.api_writers.get(project) {
            return writer.owner.snapshot().ok()?.approval;
        }
        None
    }
    pub(super) fn management_fee_ack(
        &self,
        operation: &str,
    ) -> Result<Option<crate::management_execution::ScopedFeeAck>, String> {
        if !self.management.is_scoped(operation) {
            return Ok(None);
        }
        let project = &self.management.intent(operation)?.project_id;
        self.currency_accounts
            .get(project)
            .ok_or("original scoped account missing")?
            .scoped_fee_ack(&self.management, operation)
            .map(Some)
    }
    pub(super) fn currency_account_views(
        &self,
        history: Option<&[cerulean_domain::currency_budget::CurrencyHistoryGap]>,
    ) -> Vec<cerulean_domain::currency_budget::ProjectCurrencyBudget> {
        use crate::currency_budget::Outcome;
        let accounted = self.historical_accounting_records();
        self.currency_accounts
            .iter()
            .map(|(project, book)| {
                let state = book.state();
                let current_scope = if let Some(planning) = &book.policy().planning {
                    self.project_setups
                        .get(project)
                        .is_some_and(|s| s.digest == planning.setup_digest)
                } else {
                    self.currency_current_approval(project)
                        .is_some_and(|a| a.revision_digest == book.policy().revision_digest)
                };
                let approval_linked = state
                    .approval_links
                    .contains_key(&state.current_policy_digest)
                    && current_scope;
                cerulean_domain::currency_budget::ProjectCurrencyBudget {
                    project_id: project.clone(),
                    planning_setup_digest: book
                        .policy()
                        .planning
                        .as_ref()
                        .map(|p| p.setup_digest.clone()),
                    revision_digest: book.policy().revision_digest.clone(),
                    ledger_head: book.head().into(),
                    limit_usd_nanos: book.policy().limit_usd_nanos,
                    charged_usd_nanos: state.charged_usd_nanos,
                    reserved_usd_nanos: state.reserved_usd_nanos,
                    available_usd_nanos: if book.healthy()
                        && approval_linked
                        && history.is_some_and(|gaps| {
                            accounted.as_ref().is_some_and(|records| {
                                !gaps.iter().any(|g| {
                                    g.project_id.as_ref().is_none_or(|p| p == project)
                                        && !historical_paging::gap_accounted(g, records)
                                })
                            })
                        }) {
                        state.available_usd_nanos(book.policy()).ok()
                    } else {
                        None
                    },
                    approval_linked,
                    healthy: book.healthy(),
                    blocked: state.blocked,
                    unknown_requests: state
                        .bookings
                        .values()
                        .filter(|b| {
                            b.fee_resolution.is_none()
                                && matches!(b.outcome, Outcome::Reserved | Outcome::Unknown { .. })
                        })
                        .count() as u32,
                    unpriced_requests: state
                        .bookings
                        .values()
                        .filter(|b| {
                            b.fee_resolution.is_none()
                                && matches!(b.outcome, Outcome::Unpriced { .. })
                        })
                        .count() as u32,
                    api_counts: (!state.api_counts.is_empty()).then(|| {
                        use crate::currency_budget::ApiCountOutcome;
                        let mut summary = cerulean_domain::currency_budget::ApiCountSummary {
                            pending: 0,
                            counted: 0,
                            unknown: 0,
                            counted_without_generation: 0,
                            reserved_usd_nanos: 0,
                        };
                        for (id, booking) in &state.api_counts {
                            // These original count fees remain reserved even after generation
                            // settles. This view never adds them to the account a second time.
                            if booking.fee_resolution.is_none() {
                                summary.reserved_usd_nanos += booking.attempt.maximum_usd_nanos;
                            }
                            match booking.outcome {
                                ApiCountOutcome::Pending => summary.pending += 1,
                                ApiCountOutcome::Unknown { .. } => summary.unknown += 1,
                                ApiCountOutcome::Counted { .. } => {
                                    summary.counted += 1;
                                    if !state.bookings.contains_key(id) {
                                        summary.counted_without_generation += 1;
                                    }
                                }
                            }
                        }
                        summary
                    }),
                    management_counts: (!state.management_counts.is_empty()).then(|| {
                        use crate::currency_budget::CountOutcome;
                        let mut summary =
                            cerulean_domain::currency_budget::ManagementCountSummary {
                                pending: 0,
                                counted: 0,
                                unknown: 0,
                                awaiting_generation: 0,
                                reserved_usd_nanos: 0,
                                needs_review: self.management_count_errors.contains_key(project),
                            };
                        for (id, booking) in &state.management_counts {
                            if booking.fee_resolution.is_none() {
                                summary.reserved_usd_nanos += booking.attempt.maximum_usd_nanos;
                            }
                            match booking.outcome {
                                CountOutcome::Pending => summary.pending += 1,
                                CountOutcome::Unknown { .. } => {
                                    summary.unknown += 1;
                                    summary.needs_review |= booking.retirement.is_none();
                                }
                                CountOutcome::Counted { .. }
                                | CountOutcome::ScopedCounted { .. } => {
                                    summary.counted += 1;
                                    if booking.retirement.is_none()
                                        && (!state.bookings.contains_key(id)
                                            || self.management.intent(id).is_err())
                                    {
                                        summary.awaiting_generation += 1;
                                        summary.needs_review |= booking.retirement.is_none();
                                    }
                                }
                            }
                        }
                        summary
                    }),
                }
            })
            .collect()
    }
    pub(super) fn load_currency_accounts(&mut self) -> Result<(), String> {
        let folder = self.root.join("currency-budgets");
        if folder.try_exists().map_err(|e| e.to_string())? {
            crate::currency_guard::require_protected(&self.root, true)?;
            ordinary(&folder, true)?;
            for entry in fs::read_dir(&folder).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let project = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "invalid currency account directory")?;
                identifier(&project)?;
                require(
                    self.projects.contains_key(&project),
                    "currency account belongs to an unknown project",
                )?;
                let book = BudgetLedger::open_existing(&entry.path(), &project)?;
                self.currency_accounts.insert(project, book);
            }
        }
        for project in self.projects.keys().cloned().collect::<Vec<_>>() {
            self.reconcile_currency_account_inner(&project, false)?;
        }
        // This loader runs only at Host startup, with no count workers. Never
        // apply interruption recovery during ordinary live reconciliation.
        for book in self.currency_accounts.values_mut() {
            book.recover_management_counts()?;
            book.recover_api_counts()?;
        }
        for project in self.currency_accounts.keys().cloned().collect::<Vec<_>>() {
            self.refresh_management_count_attention(&project);
        }
        Ok(())
    }
    pub(super) fn reconcile_currency_account(&mut self, project: &str) -> Result<(), String> {
        self.reconcile_currency_account_inner(project, true)
    }
    fn reconcile_currency_account_inner(
        &mut self,
        project: &str,
        observe: bool,
    ) -> Result<(), String> {
        let historical = self.historical_confirmations(observe)?;
        let count_resolutions = self.count_resolution_confirmations()?;
        let api_count_resolutions = self.api_count_resolution_confirmations()?;
        let api_generation_resolutions = self.api_generation_resolution_confirmations()?;
        let history = self
            .currency_catalogues()?
            .remove(project)
            .ok_or("currency project missing")?;
        // While the original coordinator owns live API calls, their journals
        // remain with those workers. The Host's money channel validates and
        // settles them. Whole-inventory recovery remains mandatory when idle.
        let api = if let Some(store) = self.projects.get(project) {
            if self.currency_accounts.contains_key(project) || !history.approvals.is_empty() {
                Some(crate::api_execution::CallInventory::read_with_account(
                    store,
                    self.currency_accounts.get(project),
                )?)
            } else {
                None
            }
        } else {
            #[cfg(windows)]
            require(
                self.api_writers.contains_key(project),
                "original currency workflow owner missing",
            )?;
            #[cfg(not(windows))]
            return Err("original currency workflow owner missing".into());
            None
        };
        if let Some(api) = &api {
            api.check(self.currency_accounts.get(project))?;
        }
        if let Some(book) = self.currency_accounts.get_mut(project) {
            for id in self.management.currency_requests(project) {
                if let Some(saved) = self.management.scoped_application_ack(id) {
                    require(
                        saved == &book.scoped_fee_ack_readback(&self.management, id)?,
                        "scoped application ACK differs from original durable settlement",
                    )?;
                }
            }
            history.reconcile(book)?;
            for resolution in api_generation_resolutions
                .values()
                .filter(|s| s.review.input.project_id == project)
            {
                book.record_api_generation_resolution(resolution.clone())?;
            }
            for resolution in api_count_resolutions
                .values()
                .filter(|s| s.review.input.project_id == project)
            {
                book.record_api_count_resolution(resolution.clone())?;
            }
            for resolution in count_resolutions
                .values()
                .filter(|s| s.review.input.project_id == project)
            {
                book.record_management_count_resolution(resolution.clone())?;
            }
            // Exact original user event is already durable. Preserve this charge
            // before later observations advance the account head during recovery.
            if observe {
                for charge in historical.values().filter(|c| c.project_id == project) {
                    book.record_historical_charge(charge.clone())?;
                }
            }
            for id in self.management.currency_requests(project) {
                require(book.state().bookings.get(id).is_some_and(|b|b.is_management()),
                    "original monetary management request has no reservation; preserve the account and request")?;
            }
            let managed: Vec<_> = book
                .state()
                .bookings
                .iter()
                .filter(|(_, b)| b.is_management())
                .map(|(id, _)| id.clone())
                .collect();
            book.observe_original_calls(
                &self.management,
                &managed,
                api.iter().flat_map(|calls| calls.monetary_journals()),
                api.iter().flat_map(|calls| calls.missing_preparations()),
                observe,
            )?;
            Ok(())
        } else {
            require(
                self.management.currency_requests(project).is_empty(),
                "original monetary management requests have no account; do not recreate",
            )?;
            require(history.approvals.is_empty(), "approved currency account is missing; preserve data, do not create an empty replacement")
        }
    }
    pub(super) fn prepare_currency_approval(
        &mut self,
        project: &str,
    ) -> Result<Option<ApprovalAnchor>, String> {
        self.reconcile_currency_account(project)?;
        let store = &self.projects[project];
        let revision = store.revision().ok_or("currency plan missing")?;
        let Some(settings) = &revision.policy.currency_budget else {
            require(
                !self.currency_accounts.contains_key(project),
                "项目已建立累计金额账本，不能移除预算后重新执行",
            )?;
            return Ok(None);
        };
        crate::currency_guard::require_protected(&self.root, true)?;
        let policy = settings.bind(project, &revision.approval_digest()?)?;
        self.prepare_currency_policy(project, policy).map(Some)
    }
    pub(super) fn prepare_currency_policy(
        &mut self,
        project: &str,
        policy: cerulean_domain::currency_budget::BudgetPolicy,
    ) -> Result<ApprovalAnchor, String> {
        crate::currency_guard::require_protected(&self.root, true)?;
        crate::currency_scoped_guard::require_protected(
            &self.root,
            policy.scoped_quotes.is_some(),
        )?;
        if !self.currency_accounts.contains_key(project) {
            let folder = self.root.join("currency-budgets");
            if !folder.try_exists().map_err(|e| e.to_string())? {
                fs::create_dir(&folder).map_err(|e| e.to_string())?;
            }
            ordinary(&folder, true)?;
            let path = folder.join(project);
            // An unregistered existing directory is evidence, never permission to adopt/reset it.
            require(
                !path.try_exists().map_err(|e| e.to_string())?,
                "currency account appeared outside the current host; reopen before approval",
            )?;
            let book = BudgetLedger::open(&path, &policy)?;
            self.currency_accounts.insert(project.into(), book);
        }
        let book = &self.currency_accounts[project];
        require(book.healthy(), "currency account requires reopen")?;
        let anchor = ApprovalAnchor {
            schema: 1,
            policy_digest: digest(&policy)?,
            previous_policy_digest: digest(book.policy())?,
            previous_ledger_head: book.head().into(),
        };
        if book.policy() != &policy {
            book.check_authorization(PolicyAuthorization {
                previous_policy_digest: anchor.previous_policy_digest.clone(),
                previous_ledger_head: anchor.previous_ledger_head.clone(),
                policy,
                approval_evidence_sha256: "0".repeat(64),
            })?;
        }
        Ok(anchor)
    }
    pub(super) fn authorize_planning_budget(
        &mut self,
        project: &str,
        wanted: &str,
    ) -> Result<(), String> {
        self.planning_idle(project)?;
        let saved = self
            .project_setups
            .get(project)
            .ok_or("请先保存项目配置")?
            .clone();
        require(saved.digest == wanted, "规划配置已变化，请重新审阅金额授权")?;
        let settings = saved
            .setup
            .policy
            .currency_budget
            .as_ref()
            .ok_or("项目未配置金额预算")?;
        let policy = settings.bind_planning(project, &saved.digest, &saved.setup.chief_model)?;
        self.reconcile_currency_account(project)?;
        if let Some(book) = self.currency_accounts.get(project) {
            if book.policy() == &policy
                && book.state().approval_links.contains_key(&digest(&policy)?)
            {
                return Ok(());
            }
        }
        let anchor = self.prepare_currency_policy(project, policy.clone())?;
        let store = self
            .projects
            .get_mut(project)
            .ok_or("project not registered")?;
        store.command(
            &format!("user-planning-budget-{}", store.history().len() + 1),
            "user",
            WorkflowCommand::AuthorizePlanningBudget {
                setup_digest: saved.digest,
            },
            CommandContext {
                approval_source: Some("authenticated local-user planning-budget approval".into()),
                planning_budget: Some(PlanningApproval { policy, anchor }),
                ..Default::default()
            },
            false,
        )?;
        self.reconcile_currency_account(project)
    }
}
