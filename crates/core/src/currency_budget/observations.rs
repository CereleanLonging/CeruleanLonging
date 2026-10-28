//! Preflight all original outcomes against one account before publishing any.
use super::*;
use crate::management_execution::DecisionJournal;

impl BudgetLedger {
    pub(crate) fn observe_original_calls<'a>(
        &mut self,
        management: &DecisionJournal,
        management_ids: &[String],
        api: impl Iterator<Item = &'a ApiCallJournal>,
        api_missing: impl Iterator<Item = &'a str>,
        publish: bool,
    ) -> Result<(), String> {
        // Startup may inspect complete logical pending records, but only the
        // original journal's recovery can publish them before money is written.
        if publish && !management_ids.is_empty() {
            management.require_billing_published()?;
        }
        let mut events = Vec::new();
        for id in management_ids {
            events.extend(self.management_observation(management, id)?);
        }
        for journal in api {
            events.extend(self.api_observation(journal)?);
        }
        for request in api_missing {
            events.extend(self.api_absent_preparation_observation(request)?);
        }
        self.observe_events(events, publish)
    }

    pub(super) fn observe_events(
        &mut self,
        events: Vec<Event>,
        publish: bool,
    ) -> Result<(), String> {
        require(self.healthy(), "currency ledger requires reopen")?;
        require(
            self.count
                .checked_add(events.len())
                .is_some_and(|n| n <= MAX_RECORDS),
            "currency ledger cannot hold the whole observation batch",
        )?;
        let at = chrono::Utc::now().to_rfc3339();
        let mut state = self.state.clone();
        let mut head = self.head.clone();
        for (index, event) in events.iter().enumerate() {
            require(
                matches!(
                    event,
                    Event::Settle { .. } | Event::Unknown { .. } | Event::NotSent { .. }
                ),
                "observation batch cannot change authorization or reserve requests",
            )?;
            state.advance(&self.policy, event, &at, &head)?;
            // Use the exact encoding, time, sequence and chain that publication
            // will use. A late size/state conflict must not write an early fee.
            head = encode_record(
                self.count + index,
                &head,
                &self.policy_digest,
                at.clone(),
                event.clone(),
            )?
            .0;
        }
        if publish {
            // Exclusive &mut self plus the existing writer lock keeps the
            // preflight head stable. I/O failures still retain their durable
            // prefix and poison the owner; this is not a disk transaction.
            for event in events {
                self.publish_at(event, at.clone())?;
            }
        }
        Ok(())
    }
}
