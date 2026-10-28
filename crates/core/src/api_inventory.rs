//! Read every original API round under its original workflow authority.
//! Holds existing call locks through validation/observation; never creates or sends.
use super::*;
use crate::currency_budget::BudgetLedger;
use cerulean_domain::currency_budget::BudgetPolicy;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

struct OriginalCall {
    journal: ApiCallJournal,
    policy: Option<BudgetPolicy>,
}
struct OriginalRun {
    identity: ApiRunIdentity,
    policy: Option<BudgetPolicy>,
    assignment: String,
    dispatch: Option<String>,
    completion: Option<String>,
}
pub(crate) struct CallInventory {
    calls: BTreeMap<String, OriginalCall>,
    runs: BTreeMap<String, OriginalRun>,
    missing_preparations: BTreeSet<String>,
}
impl CallInventory {
    /// Replay a locked, existing journal without enabling writes or clearing its
    /// read-only/poisoned guard. The normal runtime `read` guard is unchanged.
    pub(crate) fn inspect_existing<T>(
        root: &Path,
        inspect: impl FnOnce(&DurableWorkflow, &Self) -> Result<T, String>,
    ) -> Result<T, String> {
        DurableWorkflow::inspect_existing(root, |store| {
            let entries = fs::read_dir(root)
                .map_err(|e| e.to_string())?
                .take(10001)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            require(
                entries.len() <= 10000,
                "workflow evidence inventory too large",
            )?;
            let mut directories = BTreeSet::new();
            for entry in entries {
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "invalid workflow evidence name")?;
                if name.starts_with("api-attempt-") {
                    ordinary(&entry.path(), true)?;
                    directories.insert(entry.path());
                }
            }
            let inventory = Self::read_inventory(store, None, directories, None)?;
            inventory.check(None)?;
            inspect(store, &inventory)
        })
    }
    pub(crate) fn read(store: &DurableWorkflow) -> Result<Self, String> {
        Self::read_with_account(store, None)
    }
    pub(crate) fn read_with_account(
        store: &DurableWorkflow,
        book: Option<&BudgetLedger>,
    ) -> Result<Self, String> {
        require(
            !store.requires_reopen(),
            "original workflow requires reopen",
        )?;
        Self::read_inventory(
            store,
            book,
            store.api_attempt_directories()?.into_iter().collect(),
            None,
        )
    }
    /// A closed old attempt can be rechecked while unrelated calls are live.
    /// Return only its historical source: a partial inventory must never be
    /// used for account recovery or treated as proof of the whole project.
    pub(crate) fn historical_source_from_active(
        store: &dyn WorkflowAccess,
        attempt: &str,
    ) -> Result<crate::currency_budget::HistoricalSource, String> {
        let task = store
            .history()
            .iter()
            .find_map(|e| match &e.body.event.command {
                WorkflowCommand::Assign {
                    task, attempt: id, ..
                } if id == attempt => Some(task),
                _ => None,
            })
            .ok_or("historical source original assignment missing")?;
        require(
            store
                .history()
                .iter()
                .any(|e| closes_api_scope(&e.body.event.command, task, attempt, true)),
            "original API scope is still active or requires explicit closure",
        )?;
        let path = store.api_attempt_path(attempt)?;
        let mut directories = BTreeSet::new();
        if path.try_exists().map_err(|e| e.to_string())? {
            directories.insert(path);
        }
        let inventory = Self::read_inventory(store, None, directories, Some(attempt))?;
        inventory.historical_source_from(store, attempt)
    }
    fn read_inventory(
        store: &dyn WorkflowAccess,
        book: Option<&BudgetLedger>,
        mut directories: BTreeSet<PathBuf>,
        selected: Option<&str>,
    ) -> Result<Self, String> {
        let mut result = Self {
            calls: BTreeMap::new(),
            runs: BTreeMap::new(),
            missing_preparations: BTreeSet::new(),
        };
        let user = role(store, Role::User)?;
        let policy_version = store
            .configuration()
            .execution_scope
            .as_ref()
            .ok_or("API inventory needs original workflow scope")?
            .policy_version;
        for (index, entry) in store.history().iter().enumerate() {
            let WorkflowCommand::Assign { task, attempt, .. } = &entry.body.event.command else {
                continue;
            };
            if selected.is_some_and(|id| id != attempt) {
                continue;
            }
            let Some(ExecutionEvidence::Attempt { attempt: bound }) = &entry.body.context.execution
            else {
                return Err("API inventory assignment evidence missing".into());
            };
            if !cerulean_domain::api_worker::is_api_adapter(&bound.binding.adapter_id) {
                continue;
            }
            let original = crate::workflow_prefix::prefix(
                store,
                u32::try_from(index + 1).map_err(|_| "API history too large")?,
            )?;
            let plan = original
                .revision
                .as_ref()
                .ok_or("API original plan missing")?;
            original
                .approval
                .as_ref()
                .ok_or("API original approval missing")?
                .validate(plan, &user, policy_version)?;
            require(
                original.state.approved && bound.id == *attempt && bound.task_id == *task,
                "API inventory assignment differs from original approved scope",
            )?;
            let root = store.api_attempt_path(attempt)?;
            let dispatch = store.history()[index + 1..].iter().find(|e| matches!(&e.body.event.command,
                WorkflowCommand::DispatchIntent { task: t, attempt: a } if t == task && a == attempt)).map(|e| e.hash.clone());
            let dispatched = dispatch.is_some();
            if !directories.remove(&root) {
                require(
                    !dispatched,
                    "original dispatched API run is missing; do not recreate",
                )?;
                continue;
            }
            let run: ApiRunIdentity = load(&root, "identity.json")?;
            run.validate_plan(plan)?;
            require(
                run.task_id == *task
                    && run.attempt_id == *attempt
                    && run.binding == bound.binding
                    && run.fence == bound.fence
                    && run.revision_digest == bound.revision_digest,
                "API original run differs from original assignment",
            )?;
            let policy = plan
                .policy
                .currency_budget
                .as_ref()
                .map(|p| p.bind(&plan.project.id, &run.revision_digest))
                .transpose()?;
            let rounds = root.join("rounds");
            ordinary(&rounds, true)?;
            let mut entries = fs::read_dir(&rounds)
                .map_err(|e| e.to_string())?
                .take(130)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            require(
                entries.len() <= run.worker.max_rounds as usize,
                "API original round count exceeds approved limit",
            )?;
            require(
                dispatched || entries.is_empty(),
                "API calls have no original dispatch intent",
            )?;
            entries.sort_by_key(|e| e.file_name());
            let mut seals = Vec::new();
            for (i, entry) in entries.iter().enumerate() {
                let n = i as u32 + 1;
                require(
                    entry.file_name() == format!("{n:03}").as_str(),
                    "API original round missing or unexpected",
                )?;
                let path = entry.path();
                ordinary(&path, true)?;
                let journal = match fs::symlink_metadata(path.join("00.json")) {
                    Ok(_) => ApiCallJournal::read_existing(&path)?,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        let request = round_id(&run.attempt_id, n)?;
                        let prep = book
                            .ok_or("original API intent missing")?
                            .checked_api_preparation(&request)?;
                        require(
                            prep.attempt.run == run && i + 1 == entries.len(),
                            "API preparation gap differs from original last round",
                        )?;
                        if fs::read_dir(&path)
                            .map_err(|e| e.to_string())?
                            .next()
                            .transpose()
                            .map_err(|e| e.to_string())?
                            .is_none()
                        {
                            result.missing_preparations.insert(request);
                            continue;
                        }
                        ApiCallJournal::read_prepared_existing(&path, &prep.attempt.call)?
                    }
                    Err(e) => return Err(e.to_string()),
                };
                check_round_scope(&run, journal.identity(), n)?;
                if i + 1 < entries.len() {
                    journal.require_fee_ack()?;
                    require(
                        matches!(&journal.state().outcome, Some(CallOutcome::Reply { observation }) if !observation.tool_calls.is_empty()),
                        "API later round follows an incomplete, failed or final original call",
                    )?;
                }
                seals.push((journal.identity().clone(), journal.head().to_owned()));
                let request = journal.identity().request_id.clone();
                require(
                    result
                        .calls
                        .insert(
                            request,
                            OriginalCall {
                                journal,
                                policy: policy.clone(),
                            },
                        )
                        .is_none(),
                    "API original request identity repeated",
                )?;
            }
            if let Some(book) = book {
                for (request, booking) in &book.state().bookings {
                    let Some(prep) = booking.scoped_api_preparation.as_deref() else {
                        continue;
                    };
                    if prep.attempt.call.attempt_id != run.attempt_id
                        || result.calls.contains_key(request)
                    {
                        continue;
                    }
                    book.checked_api_preparation(request)?;
                    require(
                        prep.attempt.run == run
                            && (result.missing_preparations.contains(request)
                                || prep.attempt.call.round == entries.len() as u32 + 1),
                        "API preparation has a missing predecessor or changed run",
                    )?;
                    result.missing_preparations.insert(request.clone());
                }
            }
            // A completion is an additional original witness, not the inventory source.
            let mut completion_digest = None;
            match fs::symlink_metadata(root.join("completion.json")) {
                Ok(_) => {
                    let completion: Completion = load(&root, "completion.json")?;
                    require(
                        completion.proof.identity == run
                            && completion.receipt.evidence_sha256 == digest(&completion.proof)?
                            && completion.proof.rounds.len() == seals.len()
                            && completion.proof.rounds.len() == entries.len()
                            && !seals.is_empty()
                            && completion
                                .proof
                                .rounds
                                .iter()
                                .zip(&seals)
                                .all(|(a, (id, head))| &a.identity == id && &a.head == head),
                        "API inventory differs from original completion seal",
                    )?;
                    for (call, _) in &seals {
                        let journal = &result.calls[&call.request_id].journal;
                        journal.require_fee_ack()?;
                        require(
                            matches!(journal.state().outcome, Some(CallOutcome::Reply { .. })),
                            "API completion lacks an original reply",
                        )?;
                    }
                    require(
                        !result.missing_preparations.iter().any(|id| {
                            book.is_some_and(|b| {
                                b.state().bookings[id]
                                    .api_identity
                                    .as_ref()
                                    .is_some_and(|call| call.attempt_id == run.attempt_id)
                            })
                        }),
                        "API completion skips an original preparation",
                    )?;
                    completion_digest = Some(digest(&completion)?);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    require(
                        !root
                            .join("completion.json.pending")
                            .try_exists()
                            .map_err(|e| e.to_string())?,
                        "API completion publication unresolved before billing",
                    )?;
                }
                Err(e) => return Err(e.to_string()),
            }
            require(
                result
                    .runs
                    .insert(
                        attempt.clone(),
                        OriginalRun {
                            identity: run,
                            policy,
                            assignment: entry.hash.clone(),
                            dispatch,
                            completion: completion_digest,
                        },
                    )
                    .is_none(),
                "original API attempt repeated",
            )?;
        }
        require(
            directories.is_empty(),
            "orphan API run has no original assignment; preserve evidence",
        )?;
        Ok(result)
    }
    pub(crate) fn check(&self, book: Option<&BudgetLedger>) -> Result<(), String> {
        for (id, call) in &self.calls {
            require(
                call.journal.state().scoped_request.is_none()
                    || call.policy.as_ref().is_some_and(|p| p.schema == 2),
                "scoped API call has no original scoped approval",
            )?;
            if let Some(policy) = &call.policy {
                let book =
                    book.ok_or("original monetary API call has no account; do not recreate")?;
                book.check_api(&call.journal)?;
                let booking = &book.state().bookings[id];
                require(
                    book.state().policies.get(&booking.policy_digest) == Some(policy),
                    "API reservation policy differs from original approved run",
                )?;
            }
        }
        if let Some(book) = book {
            // Counts precede a call directory. They are still bound to the
            // original approved assignment/dispatch, never orphan liabilities.
            for count in book.state().api_counts.values() {
                let attempt = &count.attempt;
                let run = self
                    .runs
                    .get(&attempt.call.attempt_id)
                    .ok_or("API count has no original run; preserve liability")?;
                let policy = run
                    .policy
                    .as_ref()
                    .ok_or("API count has no original monetary approval")?;
                attempt.validate(policy)?;
                require(
                    run.identity == attempt.run
                        && run.dispatch.as_deref() == Some(attempt.dispatch_head.as_str())
                        && book.state().policies.get(&attempt.policy_digest) == Some(policy),
                    "API count differs from original workflow authority",
                )?;
                for n in 1..attempt.call.round {
                    let previous = self
                        .calls
                        .get(&round_id(&attempt.call.attempt_id, n)?)
                        .ok_or("API count is missing an original preceding round")?;
                    previous.journal.require_fee_ack()?;
                    require(
                        matches!(&previous.journal.state().outcome,Some(CallOutcome::Reply{observation}) if !observation.tool_calls.is_empty()),
                        "API count follows an unresolved or final round",
                    )?;
                }
            }
            for (id, booking) in &book.state().bookings {
                if booking.api_identity.is_some() {
                    if self.missing_preparations.contains(id) {
                        let prep = book.checked_api_preparation(id)?;
                        require(
                            self.runs
                                .get(&prep.attempt.call.attempt_id)
                                .is_some_and(|run| run.completion.is_none()),
                            "API missing preparation cannot have a completion",
                        )?;
                        continue;
                    }
                    let call = self
                        .calls
                        .get(id)
                        .ok_or("API reservation has no original call; preserve full reservation")?;
                    require(
                        call.policy.is_some(),
                        "API reservation has no original monetary approval",
                    )?;
                    book.check_api(&call.journal)?;
                }
            }
        }
        Ok(())
    }
    /// Read-only observations after the full original assignment/round inventory
    /// has been checked. The inventory keeps every existing call lock held.
    pub(crate) fn journals(&self) -> impl Iterator<Item = &ApiCallJournal> {
        self.calls.values().map(|c| &c.journal)
    }
    pub(crate) fn monetary_journals(&self) -> impl Iterator<Item = &ApiCallJournal> {
        self.calls
            .values()
            .filter(|c| c.policy.is_some())
            .map(|c| &c.journal)
    }
    pub(crate) fn missing_preparations(&self) -> impl Iterator<Item = &str> {
        self.missing_preparations.iter().map(String::as_str)
    }
    /// User retirement is a local disposition, not a bill or send permission.
    /// Bind the review to original call evidence and the SAME project account,
    /// including unresolved fees. This read never settles or releases money.
    pub(crate) fn retirement_evidence(
        &self,
        book: Option<&BudgetLedger>,
        attempt: &str,
    ) -> Result<String, String> {
        self.check(book)?;
        let run = self.runs.get(attempt).ok_or("original API run missing")?;
        require(
            run.dispatch.is_some() && run.completion.is_none(),
            "API retirement requires dispatched, unsealed original run",
        )?;
        if let Some(policy) = &run.policy {
            let book = book.ok_or("original monetary API run has no account")?;
            require(
                book.state().policies.get(&digest(policy)?) == Some(policy),
                "API retirement original monetary approval missing",
            )?;
        }
        if let Some(book) = book {
            require(
                book.healthy(),
                "original API monetary ledger requires reopen",
            )?;
        }
        let calls = self
            .calls
            .values()
            .filter(|c| c.journal.identity().attempt_id == attempt)
            .map(|c| (c.journal.identity(), c.journal.head()))
            .collect::<Vec<_>>();
        digest(&serde_json::json!({
            "purpose":"api-retirement-original-evidence-v1",
            "identity":run.identity,"policy":run.policy,
            "assignment":run.assignment,"dispatch":run.dispatch,
            "calls":calls,"currencyHead":book.map(BudgetLedger::head),
            "missingPreparations":self.missing_preparations,
        }))
    }
    pub(crate) fn historical_source(
        &self,
        store: &DurableWorkflow,
        attempt: &str,
    ) -> Result<crate::currency_budget::HistoricalSource, String> {
        require(
            !store.requires_reopen(),
            "original workflow requires reopen",
        )?;
        self.historical_source_from(store, attempt)
    }
    fn historical_source_from(
        &self,
        store: &dyn WorkflowAccess,
        attempt: &str,
    ) -> Result<crate::currency_budget::HistoricalSource, String> {
        use crate::currency_budget::{HistoricalScope, HistoricalSource};
        let run = self.runs.get(attempt).ok_or("original API run missing")?;
        require(
            run.policy.is_none(),
            "monetary API rounds must use their original reservations",
        )?;
        let dispatch = run
            .dispatch
            .as_ref()
            .ok_or("API historical scope was never dispatched")?;
        let assignment_index = store
            .history()
            .iter()
            .position(|e| e.hash == run.assignment)
            .ok_or("historical source original assignment missing from this workflow")?;
        let dispatch_index = store
            .history()
            .iter()
            .position(|e| &e.hash == dispatch)
            .ok_or("historical source original dispatch missing from this workflow")?;
        require(
            dispatch_index > assignment_index,
            "historical source dispatch must follow original assignment",
        )?;
        let close = store.history()[dispatch_index + 1..]
            .iter()
            .find(|e| {
                closes_api_scope(
                    &e.body.event.command,
                    &run.identity.task_id,
                    attempt,
                    run.completion.is_some(),
                )
            })
            .ok_or("original API scope is still active or requires explicit closure")?;
        let scope = HistoricalScope::Api {
            project_id: run.identity.project_id.clone(),
            attempt_id: attempt.into(),
        };
        let mut rounds = Vec::new();
        let mut uncertain = 0u32;
        for call in self
            .calls
            .values()
            .filter(|c| c.journal.identity().attempt_id == attempt)
        {
            let id = call.journal.identity();
            if !matches!(&call.journal.state().outcome, Some(CallOutcome::Reply { observation })
                if observation.usage.billable_usage(id.fingerprint.target.provider()).is_ok())
            {
                uncertain += 1;
            }
            rounds.push((id, call.journal.head()));
        }
        // A closed scope without recorded calls is still uncertain, not a free
        // execution inferred from a valid but possibly incomplete prefix.
        if rounds.is_empty() {
            uncertain = 1;
        }
        let mut references = vec![
            format!("workflow:{}:{}", run.identity.project_id, run.assignment),
            format!("workflow:{}:{dispatch}", run.identity.project_id),
            format!("workflow:{}:{}", run.identity.project_id, close.hash),
        ];
        references.extend(rounds.iter().map(|(id, head)| {
            format!("api:{}:{attempt}:round:{}:{head}", id.project_id, id.round)
        }));
        Ok(HistoricalSource {
            digest: digest(&(
                1,
                &scope,
                &run.identity,
                &run.assignment,
                dispatch,
                &close.hash,
                &run.completion,
                &rounds,
                uncertain,
            ))?,
            scope,
            references,
            uncertain_outcomes: uncertain,
        })
    }
}

fn closes_api_scope(command: &WorkflowCommand, task: &str, attempt: &str, completed: bool) -> bool {
    use cerulean_domain::workflow::ReconcileOutcome;
    match command {
        WorkflowCommand::Submit {
            task: t,
            attempt: id,
            ..
        } if id == attempt && t == task => completed,
        WorkflowCommand::RetireUnknownApi {
            task: t,
            attempt: id,
            ..
        } if id == attempt && t == task => true,
        WorkflowCommand::Reconcile {
            task: t,
            attempt: id,
            outcome: ReconcileOutcome::Cancelled,
            ..
        } if id == attempt && t == task => true,
        _ => false,
    }
}

pub(crate) fn check_round_scope(
    run: &ApiRunIdentity,
    call: &CallIdentity,
    round: u32,
) -> Result<(), String> {
    require(
        call.round == round
            && call.project_id == run.project_id
            && call.task_id == run.task_id
            && call.attempt_id == run.attempt_id
            && call.revision_digest == run.revision_digest
            && call.request_id == round_id(&run.attempt_id, round)?
            && call.fingerprint.target == run.worker.target
            && call.fingerprint.requested_model == run.worker.model
            && call.fingerprint.max_output_tokens <= run.worker.max_output_tokens
            && call.fingerprint.timeout_seconds == run.worker.request_timeout_seconds,
        "API round scope differs from original approved run",
    )
}
