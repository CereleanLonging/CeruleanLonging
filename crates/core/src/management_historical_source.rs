//! Request-local batch readback of complete original closed management scopes.
use super::*;
use crate::currency_budget::{HistoricalScope, HistoricalSource};
use std::collections::BTreeSet;

type Records = BTreeMap<String, (String, usize)>;

impl DecisionJournal {
    pub(crate) fn historical_source(
        &self,
        id: &str,
        published: bool,
    ) -> Result<HistoricalSource, String> {
        self.historical_sources(&[id], published)?
            .remove(id)
            .ok_or("historical management source missing".into())
    }
    /// Two fresh whole-journal validations per batch, not per original charge.
    /// Nothing is cached across requests or a write/publication boundary.
    pub(crate) fn historical_sources(
        &self,
        ids: &[&str],
        published: bool,
    ) -> Result<BTreeMap<String, HistoricalSource>, String> {
        self.historical_sources_inner(ids, published, || {})
    }
    fn historical_sources_inner(
        &self,
        ids: &[&str],
        published: bool,
        after_read: impl FnOnce(),
    ) -> Result<BTreeMap<String, HistoricalSource>, String> {
        require(
            ids.len() <= MAX_OPERATIONS,
            "historical management batch too large",
        )?;
        for id in ids {
            identifier(id)?;
        }
        let ids: BTreeSet<_> = ids.iter().copied().collect();
        if ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        require(self.healthy(), "management history requires reopen")?;
        if published {
            self.require_billing_published()?;
        }
        let data = self.root.parent().ok_or("management data root missing")?;
        let fresh = Self::read_existing(data)?;
        if published {
            fresh.require_billing_published()?;
        }
        for id in &ids {
            self.same_historical_head(&fresh, id)?;
        }
        let records = fresh.historical_records(&ids)?;
        after_read();
        // Revalidate all retained fragments and chains, including unrelated
        // scopes, then reread selected raw bytes to catch formatting-only changes.
        let after = Self::read_existing(data)?;
        if published {
            after.require_billing_published()?;
        }
        let after_records = after.historical_records(&ids)?;
        require(
            records == after_records,
            "management source bytes changed while reading",
        )?;
        let gaps: BTreeSet<_> = fresh
            .currency_history_gaps()
            .into_iter()
            .map(|g| g.source_id)
            .collect();
        let mut result = BTreeMap::new();
        for id in ids {
            fresh.same_historical_head(&after, id)?;
            require(
                gaps.contains(id),
                "management scope has no unaccounted historical source",
            )?;
            let reviews: Vec<_> = fresh
                .reviews
                .iter()
                .filter(|r| r.operation_id == id)
                .collect();
            let (project, uncertain) = if let Some(op) = fresh.operations.get(id) {
                require(
                    op.applied || op.resolution.is_some(),
                    "close the original management decision before historical accounting",
                )?;
                (
                    Some(op.intent.project_id.clone()),
                    u32::from(if op.deputy_request.is_some() {
                        op.deputy_response
                            .as_ref()
                            .and_then(|r| r.billing())
                            .is_none()
                    } else {
                        op.transport.is_none() || op.observation.is_none()
                    }),
                )
            } else {
                require(
                    reviews.iter().any(|r| r.sequence == 0),
                    "original management intent or reviewed fragment missing",
                )?;
                (None, 1)
            };
            let scope = HistoricalScope::Management {
                operation_id: id.into(),
                original_project_id: project,
            };
            let records = &records[id];
            result.insert(
                id.into(),
                HistoricalSource {
                    digest: digest(&(1, &scope, records, &reviews, uncertain))?,
                    scope,
                    references: records.keys().cloned().collect(),
                    uncertain_outcomes: uncertain,
                },
            );
        }
        Ok(result)
    }
    fn same_historical_head(&self, other: &Self, id: &str) -> Result<(), String> {
        require(
            self.operations.get(id).map(|o| &o.chain_hash)
                == other.operations.get(id).map(|o| &o.chain_hash),
            "management source changed since host read; reopen before review",
        )?;
        let old: Vec<_> = self
            .reviews
            .iter()
            .filter(|r| r.operation_id == id)
            .collect();
        let new: Vec<_> = other
            .reviews
            .iter()
            .filter(|r| r.operation_id == id)
            .collect();
        require(
            digest(&old)? == digest(&new)?,
            "management source review changed; reopen",
        )
    }
    fn historical_records(
        &self,
        ids: &BTreeSet<&str>,
    ) -> Result<BTreeMap<String, Records>, String> {
        let mut groups: BTreeMap<String, (Records, usize)> = ids
            .iter()
            .map(|id| ((*id).into(), (BTreeMap::new(), 0)))
            .collect();
        let pending: BTreeMap<_, _> = self
            .pending_publications
            .iter()
            .map(|p| (&p.pending, p))
            .collect();
        let entries = fs::read_dir(&self.root)
            .map_err(|e| e.to_string())?
            .take(MAX_OPERATIONS * 10 + 1)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        require(
            entries.len() <= MAX_OPERATIONS * 10,
            "management journal too large",
        )?;
        for item in entries {
            let name = item
                .file_name()
                .into_string()
                .map_err(|_| "invalid management source name")?;
            let Some((id, _)) = name.split_once('.') else {
                continue;
            };
            let Some((records, total)) = groups.get_mut(id) else {
                continue;
            };
            require(
                records.len() < 256,
                "historical management source has too many records",
            )?;
            let path = item.path();
            ordinary(&path, false)?;
            let mut bytes = Vec::new();
            fs::File::open(&path)
                .map_err(|e| e.to_string())?
                .take((MAX_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            #[cfg(test)]
            read_metrics::raw(bytes.len());
            require(
                bytes.len() <= MAX_BYTES,
                "historical management source record too large",
            )?;
            *total = total
                .checked_add(bytes.len())
                .ok_or("historical source size overflow")?;
            require(
                *total <= 16 * 1024 * 1024,
                "historical management source exceeds review size",
            )?;
            let canonical = if let Some(p) = pending.get(&path) {
                require(
                    p.bytes == bytes,
                    "pending management source changed during read",
                )?;
                p.target.file_name().unwrap().to_string_lossy().into_owned()
            } else {
                name
            };
            let value = (format!("{:x}", sha2::Sha256::digest(&bytes)), bytes.len());
            if let Some(old) =
                records.insert(format!("management-decisions/{canonical}"), value.clone())
            {
                require(old == value, "management source publication aliases differ")?;
            }
        }
        groups
            .into_iter()
            .map(|(id, (records, _))| {
                require(
                    !records.is_empty(),
                    "original management source records missing",
                )?;
                Ok((id, records))
            })
            .collect()
    }
}
#[cfg(test)]
#[path = "management_historical_batch_tests.rs"]
mod tests;
