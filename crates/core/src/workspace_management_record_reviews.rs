//! Global reviewed management fragments. Original optional project attribution is
//! derived by the existing receipt verifier, never by a selected UI project.
use super::*;
use cerulean_domain::ManagementRecordReview;
use sha2::Digest;

fn result(
    query: &WorkspaceQuery,
    revision: String,
    entries: Vec<ManagementRecordReview>,
) -> Result<WorkspaceResult, String> {
    let result = match query {
        WorkspaceQuery::ManagementRecordReviews {
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
            WorkspaceResult::ManagementRecordReviews(WorkspaceManagementRecordReviewPage {
                revision,
                offset: *offset,
                total: total as u32,
                next_offset,
                entries,
            })
        }
        WorkspaceQuery::ManagementRecordReview {
            relative_path,
            revision: expected,
        } => {
            sha256(expected)?;
            require(expected == &revision, "原管理复核记录已变化，请刷新首页")?;
            let review = entries
                .into_iter()
                .find(|r| r.relative_path == *relative_path)
                .ok_or("原管理复核记录不存在")?;
            WorkspaceResult::ManagementRecordReview(Box::new(
                WorkspaceManagementRecordReviewDetail { revision, review },
            ))
        }
        _ => return Err("管理复核记录查询类型不符".into()),
    };
    crate::workspace_read::bounded(result)
}

impl RuntimeHost {
    pub(super) fn workspace_management_record_reviews(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        require(
            !self.base.poisoned && self.management.healthy(),
            "原管理写入尚未核对，请保留证据并重开",
        )?;
        // load verifies receipt hashes, exact retained fragment bytes and the
        // preceding management chain. This collection carries no workflow
        // permission, so no unrelated workflow cache or owner is consulted.
        let entries = crate::management_review::load(&self.root)?;
        let mut hash = QueryHash(sha2::Sha256::new());
        serde_json::to_writer(
            &mut hash,
            &("workspace-management-record-reviews-v1", &entries),
        )
        .map_err(|e| e.to_string())?;
        for entry in &entries {
            let (evidence, reviewed) =
                crate::management_review::inspect(&self.root, &entry.relative_path)?;
            require(reviewed, "原管理复核凭据缺失")?;
            // Include exact fragment/prefix proof, not merely displayed fields.
            serde_json::to_writer(&mut hash, &(&entry.relative_path, evidence))
                .map_err(|e| e.to_string())?;
        }
        result(query, format!("{:x}", hash.0.finalize()), entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn list(offset: u32, revision: Option<String>) -> WorkspaceQuery {
        WorkspaceQuery::ManagementRecordReviews { offset, revision }
    }
    #[test]
    fn workspace_management_record_reviews_durable_pages_reopen_and_tamper() {
        let temp = tempfile::tempdir().unwrap();
        let host = RuntimeHost::open(temp.path()).unwrap();
        for n in 0..23 {
            let path = format!("management-decisions/review-{n:02}.0.pending");
            fs::write(temp.path().join(&path), b"{\"partial\":").unwrap();
            let (digest, _) = crate::management_review::inspect(temp.path(), &path).unwrap();
            crate::management_review::acknowledge(
                temp.path(),
                &path,
                &digest,
                "retain original fragment",
                |_| Ok(()),
            )
            .unwrap();
        }
        assert!(host.management.reviews.is_empty()); // stale cache must not win
        let before = crate::reader_generation::tests::snapshot(temp.path());
        let WorkspaceResult::ManagementRecordReviews(first) =
            host.workspace_query(&list(0, None)).unwrap()
        else {
            panic!("page")
        };
        assert_eq!(
            (first.total, first.entries.len(), first.next_offset),
            (23, 20, Some(20))
        );
        let WorkspaceResult::ManagementRecordReviews(last) = host
            .workspace_query(&list(20, Some(first.revision.clone())))
            .unwrap()
        else {
            panic!("page")
        };
        let all: Vec<_> = first.entries.iter().chain(&last.entries).collect();
        assert_eq!(all.len(), 23);
        assert!(all
            .iter()
            .all(|r| r.project_id.is_none() && r.sequence == 0));
        assert_eq!(
            serde_json::to_value(&all).unwrap(),
            serde_json::to_value(crate::management_review::load(temp.path()).unwrap()).unwrap()
        );
        let q = WorkspaceQuery::ManagementRecordReview {
            relative_path: all[22].relative_path.clone(),
            revision: first.revision.clone(),
        };
        let detail = serde_json::to_value(host.workspace_query(&q).unwrap()).unwrap();
        assert!(detail["value"]["review"]["projectId"].is_null());
        assert!(host.workspace_query(&list(20, None)).is_err());
        assert!(host
            .workspace_query(&list(24, Some(first.revision)))
            .is_err());
        drop(host);
        let mut reopened = RuntimeHost::open(temp.path()).unwrap();
        assert_eq!(
            serde_json::to_value(reopened.workspace_query(&q).unwrap()).unwrap(),
            detail
        );
        assert_eq!(
            crate::reader_generation::tests::snapshot(temp.path()),
            before
        );
        reopened.base.poisoned = true;
        assert!(reopened.workspace_query(&list(0, None)).is_err());
        reopened.base.poisoned = false;
        fs::write(temp.path().join(&all[22].relative_path), b"{\"changed\":").unwrap();
        assert!(reopened.workspace_query(&q).is_err());
        assert!(reopened.workspace_query(&list(0, None)).is_err());
    }
    #[test]
    fn workspace_management_record_reviews_identity_caps_and_unavailable() {
        let revision = "a".repeat(64);
        let review = ManagementRecordReview {
            relative_path: "management-decisions/op.1.pending".into(),
            operation_id: "op".into(),
            sequence: 1,
            project_id: Some("original-project".into()),
            reason: "original reason".into(),
            at: "2026-10-15T00:00:00Z".into(),
        };
        let q = WorkspaceQuery::ManagementRecordReview {
            relative_path: review.relative_path.clone(),
            revision: revision.clone(),
        };
        let v = result(&q, revision.clone(), vec![review.clone()]).unwrap();
        assert_eq!(
            serde_json::to_value(&v).unwrap()["value"]["review"]["projectId"],
            "original-project"
        );
        let size = serde_json::to_vec(&v).unwrap().len();
        let mut limit = review.clone();
        limit
            .reason
            .push_str(&"x".repeat(crate::workspace_read::RESULT_BYTES - size));
        assert_eq!(
            serde_json::to_vec(&result(&q, revision.clone(), vec![limit.clone()]).unwrap())
                .unwrap()
                .len(),
            crate::workspace_read::RESULT_BYTES
        );
        limit.reason.push('x');
        assert!(result(&q, revision.clone(), vec![limit]).is_err());
        let mut oversized = review.clone();
        oversized.reason = "字".repeat(24000);
        assert!(result(&list(0, None), revision.clone(), vec![oversized]).is_err());
        assert!(result(&q, "b".repeat(64), vec![review]).is_err());
        assert!(result(&q, revision.clone(), vec![]).is_err());
        assert!(result(&list(1, Some(revision.clone())), revision, vec![]).is_err());
        let temp = tempfile::tempdir().unwrap();
        let host = RuntimeHost::open(temp.path()).unwrap();
        let WorkspaceResult::ManagementRecordReviews(empty) =
            host.workspace_query(&list(0, None)).unwrap()
        else {
            panic!("page")
        };
        assert_eq!(empty.total, 0);
        drop(host);
        fs::write(temp.path().join("management-decisions/op.0.pending"), b"{").unwrap();
        let service = crate::diagnostic_service::ServiceHost::open(temp.path()).unwrap();
        assert!(service.is_diagnostic());
        assert!(service.workspace_query(&list(0, None)).is_err());
        assert!(service.workspace_query(&q).is_err());
    }
}
