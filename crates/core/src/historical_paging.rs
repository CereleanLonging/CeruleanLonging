//! Bounded transport projections; original journals and global checks are kept.
use super::historical_product::AccountingRecord;
use super::*;
use cerulean_domain::{currency_budget::CurrencyHistoryGap, historical_money::*};

const PAGE_SIZE: usize = 20;
const PAGE_BYTES: usize = 64 * 1024;
type Records<'a> = BTreeMap<String, AccountingRecord<'a>>;

pub(super) fn gap_accounted(gap: &CurrencyHistoryGap, records: &Records<'_>) -> bool {
    let scope = if gap.reason
        == cerulean_domain::currency_budget::CurrencyHistoryReason::ApiWithoutBudget
    {
        let Some(project_id) = &gap.project_id else {
            return false;
        };
        HistoricalScope::Api {
            project_id: project_id.clone(),
            attempt_id: gap.source_id.clone(),
        }
    } else {
        HistoricalScope::Management {
            operation_id: gap.source_id.clone(),
            original_project_id: gap.project_id.clone(),
        }
    };
    scope
        .key()
        .ok()
        .and_then(|key| records.get(&key))
        .is_some_and(|r| {
            r.state == HistoricalAccountingState::Accounted
                && gap_matches(&r.review.source.scope, gap)
        })
}

fn accounting_revision(records: &Records<'_>) -> Result<String, String> {
    // The original event hash covers the full original review, including long
    // references. The state additionally binds publication of its amount.
    digest(&(
        "historical-accounting-page-v1",
        records
            .iter()
            .map(|(key, r)| (key, r.event.as_ref(), r.state))
            .collect::<Vec<_>>(),
    ))
}
fn page_start(
    total: usize,
    offset: u32,
    expected: Option<&str>,
    revision: &str,
) -> Result<usize, String> {
    require(
        offset == 0 || expected.is_some(),
        "历史续页需要原版本摘要，请刷新首页",
    )?;
    if let Some(expected) = expected {
        sha256(expected)?;
        require(expected == revision, "历史记录已变化，请刷新首页后继续翻页")?;
    }
    let offset = offset as usize;
    require(offset <= total, "历史页偏移超出范围")?;
    Ok(offset)
}
fn push_bounded<T: serde::Serialize>(
    items: &mut Vec<T>,
    bytes: &mut usize,
    item: T,
) -> Result<bool, String> {
    let size = serde_json::to_vec(&item).map_err(|e| e.to_string())?.len() + 1;
    require(
        size <= PAGE_BYTES,
        "单条历史列表摘要超出大小限制，请保留原记录",
    )?;
    if *bytes + size > PAGE_BYTES {
        require(
            !items.is_empty(),
            "单条历史摘要无法装入有界页，请保留原记录",
        )?;
        return Ok(false);
    }
    *bytes += size;
    items.push(item);
    Ok(true)
}
fn accounting_page(
    records: &Records<'_>,
    offset: u32,
    expected: Option<&str>,
) -> Result<HistoricalAccountingPage, String> {
    let revision = accounting_revision(records)?;
    let start = page_start(records.len(), offset, expected, &revision)?;
    let mut entries = Vec::new();
    let mut bytes = 1024; // Reserved for the fixed page metadata and JSON framing.
    for r in records.values().skip(start).take(PAGE_SIZE) {
        let summary = HistoricalAccountingSummary {
            project_id: r.review.project_id.clone(),
            scope: r.review.source.scope.clone(),
            amount_usd_nanos: r.review.amount_usd_nanos,
            review_digest: r.review.digest()?,
            user_event_digest: r.event.to_string(),
            source_digest: r.review.source.digest.clone(),
            uncertain_outcomes: r.review.source.uncertain_outcomes,
            state: r.state,
        };
        if !push_bounded(&mut entries, &mut bytes, summary)? {
            break;
        }
    }
    let end = start + entries.len();
    Ok(HistoricalAccountingPage {
        revision,
        offset,
        total: records.len() as u32,
        next_offset: (end < records.len()).then_some(end as u32),
        entries,
    })
}
fn source_page(
    gaps: &[CurrencyHistoryGap],
    records: Option<&Records<'_>>,
    offset: u32,
    expected: Option<&str>,
) -> Result<HistoricalSourcePage, String> {
    let accounted_revision = records.map(accounting_revision).transpose()?;
    let revision = digest(&("historical-source-page-v1", gaps, accounted_revision))?;
    let start = page_start(gaps.len(), offset, expected, &revision)?;
    let accounted = |g: &CurrencyHistoryGap| records.map(|rs| gap_accounted(g, rs));
    let pending_total =
        records.map(|_| gaps.iter().filter(|g| accounted(g) != Some(true)).count() as u32);
    let mut entries = Vec::new();
    let mut bytes = 1024;
    for gap in gaps.iter().skip(start).take(PAGE_SIZE) {
        if !push_bounded(
            &mut entries,
            &mut bytes,
            HistoricalSourceEntry {
                gap: gap.clone(),
                accounted: accounted(gap),
            },
        )? {
            break;
        }
    }
    let end = start + entries.len();
    Ok(HistoricalSourcePage {
        revision,
        offset,
        total: gaps.len() as u32,
        pending_total,
        next_offset: (end < gaps.len()).then_some(end as u32),
        entries,
    })
}

/// A compatibility list is complete or unavailable. Never send a truncated list
/// with a shape that an older client could mistake for complete history.
fn small_complete<T: serde::Serialize + Clone>(items: &[T]) -> Option<Vec<T>> {
    if items.len() > PAGE_SIZE || serde_json::to_vec(items).ok()?.len() > PAGE_BYTES {
        return None;
    }
    Some(items.to_vec())
}
fn legacy_records(records: &Records<'_>) -> Option<Vec<HistoricalAccounting>> {
    if records.len() > PAGE_SIZE {
        return None;
    }
    let mut result = Vec::new();
    let mut bytes = 2;
    for r in records.values() {
        if !push_bounded(&mut result, &mut bytes, r.view().ok()?).ok()? {
            return None;
        }
    }
    Some(result)
}

impl RuntimeHost {
    pub(super) fn project_historical_pages(
        &self,
        snapshot: &mut Snapshot,
        gaps: Option<&[CurrencyHistoryGap]>,
    ) {
        let records = self.historical_accounting_records();
        snapshot.currency_history = gaps.and_then(small_complete);
        snapshot.currency_accounted = records.as_ref().and_then(legacy_records);
        snapshot.historical_sources =
            gaps.and_then(|g| source_page(g, records.as_ref(), 0, None).ok());
        snapshot.historical_accounting = records
            .as_ref()
            .and_then(|r| accounting_page(r, 0, None).ok());
    }
    pub(super) fn read_historical_sources(
        &self,
        offset: u32,
        expected: Option<&str>,
    ) -> Result<HistoricalSourcePage, String> {
        let gaps = self
            .currency_history_gaps()
            .ok_or("原历史清单不可用，请保留记录并重开核对")?;
        source_page(
            &gaps,
            self.historical_accounting_records().as_ref(),
            offset,
            expected,
        )
    }
    pub(super) fn read_historical_accounting(
        &self,
        offset: u32,
        expected: Option<&str>,
    ) -> Result<HistoricalAccountingPage, String> {
        let records = self
            .historical_accounting_records()
            .ok_or("原历史确认与账本不可用，请保留记录并重开核对")?;
        accounting_page(&records, offset, expected)
    }
    pub(super) fn read_historical_confirmation(
        &self,
        project: &str,
        scope: &HistoricalScope,
        expected: &str,
    ) -> Result<HistoricalConfirmationLookup, String> {
        identifier(project)?;
        scope.validate(project)?;
        sha256(expected)?;
        require(self.projects.contains_key(project), "原历史确认项目不存在")?;
        let records = self
            .historical_accounting_records()
            .ok_or("原历史确认与账本不可用，请保留记录并重开核对")?;
        let found = records.get(&scope.key()?);
        let record = if let Some(r) = found {
            require(
                r.review.project_id == project
                    && &r.review.source.scope == scope
                    && r.review.digest()? == expected,
                "原历史确认与查询项目、范围或审阅摘要不一致",
            )?;
            require(
                self.historical_source_inner(scope, false)? == r.review.source,
                "原历史来源已变化，请保留记录并核对",
            )?;
            Some(r.view()?)
        } else {
            None
        };
        Ok(HistoricalConfirmationLookup {
            project_id: project.into(),
            scope: scope.clone(),
            review_digest: expected.into(),
            record,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn currency_historical_paging_source_byte_limit_unknown_status_and_distinct_revision() {
        let gaps = (0..20).map(|i| CurrencyHistoryGap {
            project_id: None, source_id: format!("old-{i}"),
            reason: cerulean_domain::currency_budget::CurrencyHistoryReason::ReviewedManagementRecord,
            reference: "\u{1}".repeat(2048),
        }).collect::<Vec<_>>();
        let first = source_page(&gaps, None, 0, None).unwrap();
        assert!(first.entries.len() < 20 && !first.entries.is_empty());
        assert_eq!(first.pending_total, None);
        assert!(first.entries.iter().all(|e| e.accounted.is_none()));
        let mut page = first.clone();
        let mut seen = std::collections::BTreeSet::new();
        loop {
            assert!(serde_json::to_vec(&page).unwrap().len() <= PAGE_BYTES);
            for e in &page.entries {
                assert!(seen.insert(e.gap.source_id.clone()));
            }
            if let Some(offset) = page.next_offset {
                page = source_page(&gaps, None, offset, Some(&first.revision)).unwrap();
            } else {
                break;
            }
        }
        assert_eq!(seen.len(), 20);
        let empty = Records::new();
        let known = source_page(&gaps, Some(&empty), 0, None).unwrap();
        assert_eq!(known.pending_total, Some(20));
        assert_ne!(first.revision, known.revision);
        assert!(source_page(&gaps, Some(&empty), 0, Some(&first.revision)).is_err());
        assert!(source_page(&gaps, None, 0, Some(&accounting_revision(&empty).unwrap())).is_err());
        let oversized = CurrencyHistoryGap {
            reference: "r".repeat(PAGE_BYTES),
            ..gaps[0].clone()
        };
        assert!(source_page(&[oversized], None, 0, None).is_err()); // never emit a non-advancing cursor
    }
    #[test]
    fn currency_historical_paging_256_long_records_have_complete_bounded_summary_pages() {
        use crate::historical_confirmation::HistoricalReview;
        let originals = (0..256)
            .map(|i| HistoricalReview {
                schema: 1,
                project_id: "project".into(),
                source: HistoricalSource {
                    scope: HistoricalScope::Management {
                        operation_id: format!("old-{i}"),
                        original_project_id: Some("project".into()),
                    },
                    digest: "d".repeat(64),
                    references: vec!["r".repeat(2048); 256],
                    uncertain_outcomes: 1,
                },
                policy_digest: "c".repeat(64),
                previous_ledger_head: "a".repeat(64),
                amount_usd_nanos: 17,
                billing_reference: "b".repeat(8000),
                explanation: "e".repeat(8000),
            })
            .collect::<Vec<_>>();
        for r in &originals {
            r.validate("project", &r.digest().unwrap()).unwrap();
        }
        let events = (0..256).map(|i| format!("{i:064x}")).collect::<Vec<_>>();
        let records = originals
            .iter()
            .zip(&events)
            .map(|(r, e)| {
                (
                    r.source.scope.key().unwrap(),
                    AccountingRecord {
                        review: std::borrow::Cow::Borrowed(r),
                        event: std::borrow::Cow::Borrowed(e),
                        state: HistoricalAccountingState::Accounted,
                    },
                )
            })
            .collect::<Records<'_>>();
        assert!(legacy_records(&records).is_none());
        let mut offset = 0;
        let mut revision = None;
        let mut seen = std::collections::BTreeSet::new();
        let mut pages = 0;
        loop {
            let page = accounting_page(&records, offset, revision.as_deref()).unwrap();
            pages += 1;
            assert_eq!(page.total, 256);
            assert!(page.entries.len() <= 20);
            assert!(serde_json::to_vec(&page).unwrap().len() < PAGE_BYTES);
            for e in &page.entries {
                assert!(seen.insert(e.user_event_digest.clone()));
            }
            if let Some(next) = page.next_offset {
                assert!(next > offset);
                offset = next;
                revision = Some(page.revision);
            } else {
                break;
            }
        }
        assert_eq!((pages, seen.len()), (13, 256));
        let single = records.into_iter().take(1).collect::<Records<'_>>();
        assert!(legacy_records(&single).is_none()); // even one long record uses detail
        assert_eq!(accounting_page(&single, 0, None).unwrap().entries.len(), 1);
    }
}
