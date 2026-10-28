//! Global, unassigned API publication reviews from verified durable receipts.
//! Query versions are not approval digests; no Snapshot or selected project is used.
use super::*;
use serde::Serialize;
use sha2::Digest;

fn streamed(value: &impl Serialize) -> Result<String, String> {
    let mut writer = QueryHash(sha2::Sha256::new());
    serde_json::to_writer(&mut writer, value).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", writer.0.finalize()))
}

pub(crate) fn original_basis(
    store: &crate::scheduler::DurableWorkflow,
    project: &str,
) -> Result<String, String> {
    identifier(project)?;
    require(
        !store.requires_reopen(),
        "原项目写入尚未核对，请保留记录并重开",
    )?;
    require(
        store
            .configuration()
            .execution_scope
            .as_ref()
            .is_some_and(|scope| scope.project.id == project),
        "原项目账本身份不符",
    )?;
    streamed(&(
        "workspace-api-publication-reviews-owner-v1",
        project,
        store.configuration(),
        store.state(),
        store.revision(),
        store.approval(),
        store.history(),
    ))
}

fn result(
    query: &WorkspaceQuery,
    revision: String,
    entries: Vec<ApiPublicationReview>,
) -> Result<WorkspaceResult, String> {
    let result = match query {
        WorkspaceQuery::ApiPublicationReviews {
            offset,
            revision: expected,
        } => {
            let total = entries.len();
            let (entries, next_offset) = crate::workspace_read::page(
                entries,
                total,
                *offset,
                expected.as_deref(),
                &revision,
            )?;
            WorkspaceResult::ApiPublicationReviews(WorkspaceApiPublicationReviewPage {
                revision,
                offset: *offset,
                total: total as u32,
                next_offset,
                entries,
            })
        }
        WorkspaceQuery::ApiPublicationReview {
            relative_path,
            revision: expected,
        } => {
            sha256(expected)?;
            require(expected == &revision, "原API发布复核已变化，请刷新首页")?;
            let review = entries
                .into_iter()
                .find(|entry| entry.relative_path == *relative_path)
                .ok_or("原API发布复核不存在，未创建替代记录")?;
            WorkspaceResult::ApiPublicationReview(Box::new(WorkspaceApiPublicationReviewDetail {
                revision,
                review,
            }))
        }
        _ => return Err("API发布复核查询类型不符".into()),
    };
    crate::workspace_read::bounded(result)
}

impl RuntimeHost {
    pub(super) fn workspace_api_publication_reviews(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        require(
            !self.base.poisoned && self.runs.healthy(),
            "原写入尚未核对，请保留记录并重开",
        )?;
        #[cfg(not(windows))]
        {
            let _ = query;
            Err("当前平台不支持原API发布复核读取".into())
        }
        #[cfg(windows)]
        {
            require(self.api_fault.is_none(), "原协调器需要重开核对")?;
            require(
                !self.active.values().any(|active| !active.shared) && self.desktop_active.is_none(),
                "原账本由非共享活动写者持有，API发布复核暂不可用",
            )?;
            // Every live worker must still have its actual shared writer. Cached
            // progress and startup project hints are never publication ownership.
            for project in self
                .active
                .keys()
                .chain(self.api_active.keys().map(|(project, _)| project))
                .chain(self.codex_active.keys().map(|(project, _)| project))
                .chain(self.omp_active.keys().map(|(project, _)| project))
            {
                require(
                    self.api_writers.contains_key(project),
                    "原共享账本所有者缺失",
                )?;
            }
            let mut owners = BTreeMap::new();
            for (project, store) in &self.projects {
                if !self.api_writers.contains_key(project) {
                    owners.insert(project.as_str(), original_basis(store, project)?);
                }
            }
            for (project, writer) in &self.api_writers {
                require(!writer.interrupted, "原共享写者已中断，请重开核对")?;
                owners.insert(
                    project.as_str(),
                    writer
                        .owner
                        .workspace_api_publication_reviews_basis(project)?,
                );
            }
            // Reverify receipts against their exact original pending bytes on
            // every request. In-memory Snapshot collections are not a basis.
            let entries = crate::api_publication::reviews(&self.root)?;
            let revision = streamed(&(
                "workspace-api-publication-reviews-v1",
                &self.base.last_hash,
                &self.base.events,
                owners,
                self.runs.sequence(),
                self.runs.evidence_digest(),
                &entries,
            ))?;
            result(query, revision, entries)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(index: usize) -> ApiPublicationReview {
        ApiPublicationReview {
            relative_path: format!("api-startups/{index:064x}.pending"),
            evidence_digest: "a".repeat(64),
            reason: "retain original evidence".into(),
            at: "2026-10-15T00:00:00Z".into(),
        }
    }
    fn list(offset: u32, revision: Option<String>) -> WorkspaceQuery {
        WorkspaceQuery::ApiPublicationReviews { offset, revision }
    }
    #[test]
    fn workspace_api_publication_reviews_pagination_identity_and_no_project() {
        let revision = "a".repeat(64);
        let entries: Vec<_> = (0..21).map(entry).collect();
        let WorkspaceResult::ApiPublicationReviews(page) =
            result(&list(0, None), revision.clone(), entries.clone()).unwrap()
        else {
            panic!("page")
        };
        assert_eq!(page.entries.len(), 20);
        assert_eq!(page.next_offset, Some(20));
        assert_eq!(page.total, 21);
        let WorkspaceResult::ApiPublicationReviews(last) = result(
            &list(20, Some(revision.clone())),
            revision.clone(),
            entries.clone(),
        )
        .unwrap() else {
            panic!("page")
        };
        assert_eq!(last.entries[0].relative_path, entries[20].relative_path);
        assert_eq!(last.entries.len(), 1);
        assert_eq!(last.next_offset, None);
        let detail = result(
            &WorkspaceQuery::ApiPublicationReview {
                relative_path: entries[0].relative_path.clone(),
                revision: revision.clone(),
            },
            revision,
            entries.clone(),
        )
        .unwrap();
        let json = serde_json::to_value(detail).unwrap();
        assert_eq!(
            json["value"]["review"],
            serde_json::to_value(&entries[0]).unwrap()
        );
        assert!(json["value"].get("projectId").is_none());
        assert!(json["value"]["review"].get("projectId").is_none());
    }
    #[test]
    fn workspace_api_publication_reviews_byte_caps_stale_and_unknown() {
        let revision = "a".repeat(64);
        let mut exact = entry(0);
        let overhead = serde_json::to_vec(&exact).unwrap().len() - exact.reason.len();
        exact.reason = "x".repeat(64 * 1024 - 1024 - overhead - 1);
        let page = result(
            &list(0, None),
            revision.clone(),
            vec![exact.clone(), entry(1)],
        )
        .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
        let WorkspaceResult::ApiPublicationReviews(page) = page else {
            panic!("page")
        };
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.next_offset, Some(1));
        exact.reason.push('x');
        assert!(result(&list(0, None), revision.clone(), vec![exact]).is_err());
        assert!(result(&list(1, None), revision.clone(), vec![entry(0)]).is_err());
        assert!(result(
            &list(0, Some("b".repeat(64))),
            revision.clone(),
            vec![entry(0)]
        )
        .is_err());
        for (path, expected) in [
            (entry(2).relative_path, revision.clone()),
            (entry(0).relative_path, "b".repeat(64)),
        ] {
            assert!(result(
                &WorkspaceQuery::ApiPublicationReview {
                    relative_path: path,
                    revision: expected
                },
                revision.clone(),
                vec![entry(0)]
            )
            .is_err());
        }
        let query = WorkspaceQuery::ApiPublicationReview {
            relative_path: entry(0).relative_path,
            revision: revision.clone(),
        };
        let initial = result(&query, revision.clone(), vec![entry(0)]).unwrap();
        let size = serde_json::to_vec(&initial).unwrap().len();
        let mut exact = entry(0);
        exact
            .reason
            .push_str(&"x".repeat(crate::workspace_read::RESULT_BYTES - size));
        assert_eq!(
            serde_json::to_vec(&result(&query, revision.clone(), vec![exact.clone()]).unwrap())
                .unwrap()
                .len(),
            crate::workspace_read::RESULT_BYTES
        );
        exact.reason.push('x');
        assert!(result(&query, revision, vec![exact]).is_err());
    }
}
