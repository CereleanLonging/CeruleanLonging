//! Original per-project recovery evidence, without mutation or a full Snapshot.
use super::*;
use sha2::Digest;

#[derive(serde::Serialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
enum RecoveryRef<'a> {
    ManagementRecord(&'a cerulean_domain::ManagementRecordReview),
    StartupClosure(&'a cerulean_domain::StartupClosureReview),
    StartupReservation(&'a cerulean_domain::StartupReservation),
    OmpStop(&'a cerulean_domain::omp_jobs::OmpStopIssue),
}
impl RecoveryRef<'_> {
    fn key(&self) -> (u8, &str, &str) {
        match self {
            Self::ManagementRecord(r) => (0, &r.relative_path, ""),
            Self::StartupClosure(r) => (1, &r.relative_path, ""),
            Self::StartupReservation(r) => (2, &r.executor, &r.attempt_id),
            Self::OmpStop(r) => (3, &r.pending_path, ""),
        }
    }
    fn matches(&self, kind: WorkspaceRecoveryKind, id: &str) -> bool {
        match (self, kind) {
            (Self::ManagementRecord(r), WorkspaceRecoveryKind::ManagementRecord) => {
                r.relative_path == id
            }
            (Self::StartupClosure(r), WorkspaceRecoveryKind::StartupClosure) => {
                r.relative_path == id
            }
            (Self::StartupReservation(r), WorkspaceRecoveryKind::StartupReservation) => {
                id.strip_prefix(&r.executor)
                    .and_then(|s| s.strip_prefix(':'))
                    == Some(r.attempt_id.as_str())
            }
            (Self::OmpStop(r), WorkspaceRecoveryKind::OmpStop) => r.pending_path == id,
            _ => false,
        }
    }
    fn summary(&self) -> WorkspaceRecoverySummary {
        let (kind, id, project, task, attempt, has_message) = match self {
            Self::ManagementRecord(r) => (
                WorkspaceRecoveryKind::ManagementRecord,
                r.relative_path.clone(),
                r.project_id.as_ref().expect("selected project"),
                None,
                None,
                !r.reason.is_empty(),
            ),
            Self::StartupClosure(r) => (
                WorkspaceRecoveryKind::StartupClosure,
                r.relative_path.clone(),
                &r.project_id,
                None,
                Some(r.attempt_id.clone()),
                !r.reason.is_empty(),
            ),
            Self::StartupReservation(r) => (
                WorkspaceRecoveryKind::StartupReservation,
                format!("{}:{}", r.executor, r.attempt_id),
                &r.project_id,
                None,
                Some(r.attempt_id.clone()),
                !r.reason.is_empty(),
            ),
            Self::OmpStop(r) => (
                WorkspaceRecoveryKind::OmpStop,
                r.pending_path.clone(),
                &r.project_id,
                Some(r.task_id.clone()),
                Some(r.attempt_id.clone()),
                !r.message.is_empty(),
            ),
        };
        WorkspaceRecoverySummary {
            kind,
            id,
            project_id: project.clone(),
            task_id: task,
            attempt_id: attempt,
            has_message,
        }
    }
    fn record(&self) -> WorkspaceRecoveryRecord {
        match self {
            Self::ManagementRecord(r) => WorkspaceRecoveryRecord::ManagementRecord((*r).clone()),
            Self::StartupClosure(r) => WorkspaceRecoveryRecord::StartupClosure((*r).clone()),
            Self::StartupReservation(r) => {
                WorkspaceRecoveryRecord::StartupReservation((*r).clone())
            }
            Self::OmpStop(r) => WorkspaceRecoveryRecord::OmpStop((*r).clone()),
        }
    }
}
impl RuntimeHost {
    pub(super) fn workspace_recovery(
        &self,
        query: &WorkspaceQuery,
        project_version: &str,
    ) -> Result<WorkspaceResult, String> {
        let project_id = match query {
            WorkspaceQuery::Recovery { project_id, .. }
            | WorkspaceQuery::RecoveryDetail { project_id, .. } => project_id,
            _ => return Err("原恢复查询类型不符".into()),
        };
        require(
            !self.base.poisoned
                && self.management.healthy()
                && self.startups.healthy()
                && self.omp_startups.healthy(),
            "原恢复证据写入尚未核对，请保留记录并重开",
        )?;
        #[cfg(windows)]
        require(
            self.api_fault.is_none(),
            "API原写入需要重开核对，恢复记录暂不可用",
        )?;
        let mut records = Vec::new();
        records.extend(
            self.management
                .reviews
                .iter()
                .filter(|r| r.project_id.as_ref() == Some(project_id))
                .map(RecoveryRef::ManagementRecord),
        );
        records.extend(
            self.startups
                .closure_reviews
                .iter()
                .chain(&self.omp_startups.closure_reviews)
                .filter(|r| &r.project_id == project_id)
                .map(RecoveryRef::StartupClosure),
        );
        records.extend(
            self.startups
                .reservations
                .iter()
                .chain(&self.omp_startups.reservations)
                .filter(|r| &r.project_id == project_id)
                .map(RecoveryRef::StartupReservation),
        );
        records.extend(
            self.omp_stop_issues
                .iter()
                .filter(|r| &r.project_id == project_id)
                .map(RecoveryRef::OmpStop),
        );
        // Stable kind/id order; reject ambiguous identity instead of choosing a record.
        records.sort_by(|a, b| a.key().cmp(&b.key()));
        for pair in records.windows(2) {
            require(
                pair[0].key() != pair[1].key(),
                "原恢复记录身份重复，请保留证据核对",
            )?;
        }
        let mut hash = QueryHash(sha2::Sha256::new());
        serde_json::to_writer(
            &mut hash,
            &(
                "workspace-recovery-v1",
                project_id,
                project_version,
                &records,
            ),
        )
        .map_err(|e| e.to_string())?;
        let revision = format!("{:x}", hash.0.finalize());
        let result = match query {
            WorkspaceQuery::Recovery {
                offset,
                revision: expected,
                ..
            } => {
                let (entries, next_offset) = crate::workspace_read::page(
                    records.iter().map(RecoveryRef::summary),
                    records.len(),
                    *offset,
                    expected.as_deref(),
                    &revision,
                )?;
                WorkspaceResult::Recovery(WorkspaceRecoveryPage {
                    project_id: project_id.clone(),
                    revision,
                    offset: *offset,
                    total: records.len() as u32,
                    next_offset,
                    entries,
                })
            }
            WorkspaceQuery::RecoveryDetail {
                kind,
                id,
                revision: expected,
                ..
            } => {
                cerulean_domain::execution::sha256(expected)?;
                require(expected == &revision, "原恢复证据已变化，请刷新恢复首页")?;
                let record = records
                    .iter()
                    .find(|r| r.matches(*kind, id))
                    .ok_or("原恢复记录不存在，未创建替代记录")?;
                WorkspaceResult::RecoveryDetail(Box::new(WorkspaceRecoveryDetail {
                    project_id: project_id.clone(),
                    revision,
                    record: record.record(),
                }))
            }
            _ => return Err("原恢复查询类型不符".into()),
        };
        crate::workspace_read::bounded(result)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    fn query(project: &str, offset: u32, revision: Option<String>) -> WorkspaceQuery {
        WorkspaceQuery::Recovery {
            project_id: project.into(),
            offset,
            revision,
        }
    }
    #[test]
    fn workspace_recovery_pages_original_evidence_scope_revision_and_bounds() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        for n in 0..45 {
            host.startups
                .reservations
                .push(cerulean_domain::StartupReservation {
                    executor: "codex".into(),
                    project_id: "project".into(),
                    attempt_id: format!("attempt-{n:02}"),
                    reason: "original reason".into(),
                    at: "2026-10-14T00:00:00Z".into(),
                });
        }
        host.startups
            .reservations
            .push(cerulean_domain::StartupReservation {
                executor: "codex".into(),
                project_id: "other".into(),
                attempt_id: "other-attempt".into(),
                reason: "not selected".into(),
                at: "2026-10-14T00:00:00Z".into(),
            });
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let WorkspaceResult::Recovery(first) =
            host.workspace_query(&query("project", 0, None)).unwrap()
        else {
            panic!("recovery page")
        };
        assert_eq!(first.total, 45);
        assert_eq!(first.entries.len(), 20);
        assert!(first.entries.iter().all(|s| s.project_id == "project"));
        let mut offset = first.next_offset;
        let mut count = first.entries.len();
        while let Some(next) = offset {
            let WorkspaceResult::Recovery(page) = host
                .workspace_query(&query("project", next, Some(first.revision.clone())))
                .unwrap()
            else {
                panic!("page")
            };
            assert_eq!(page.offset, next);
            count += page.entries.len();
            offset = page.next_offset;
        }
        assert_eq!(count, 45);
        assert!(host.workspace_query(&query("project", 20, None)).is_err());
        assert!(host.workspace_query(&query("missing", 0, None)).is_err());
        let original = &first.entries[0];
        let detail = |id: &str, revision: &str| WorkspaceQuery::RecoveryDetail {
            project_id: "project".into(),
            kind: original.kind,
            id: id.into(),
            revision: revision.into(),
        };
        let WorkspaceResult::RecoveryDetail(found) = host
            .workspace_query(&detail(&original.id, &first.revision))
            .unwrap()
        else {
            panic!("detail")
        };
        let WorkspaceRecoveryRecord::StartupReservation(record) = found.record else {
            panic!("original record")
        };
        assert_eq!(
            serde_json::to_value(record).unwrap(),
            serde_json::to_value(&host.startups.reservations[0]).unwrap()
        );
        assert!(host
            .workspace_query(&detail("missing", &first.revision))
            .is_err());
        assert!(host
            .workspace_query(&detail(&original.id, &"0".repeat(64)))
            .is_err());
        host.startups.reservations[0].reason = "changed original evidence".into();
        assert!(host
            .workspace_query(&detail(&original.id, &first.revision))
            .is_err());
        assert!(host
            .workspace_query(&query("project", 20, Some(first.revision.clone())))
            .is_err());
        host.startups.reservations[0].reason = "x".repeat(crate::workspace_read::RESULT_BYTES);
        let WorkspaceResult::Recovery(refreshed) =
            host.workspace_query(&query("project", 0, None)).unwrap()
        else {
            panic!("bounded summary")
        };
        assert!(host
            .workspace_query(&detail(&original.id, &refreshed.revision))
            .is_err());
        host.base.poisoned = true;
        assert!(host.workspace_query(&query("project", 0, None)).is_err());
        host.base.poisoned = false;
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }
    #[test]
    fn workspace_recovery_keeps_same_path_kinds_and_rejects_duplicates() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        let path = "original/same.pending";
        host.management
            .reviews
            .push(cerulean_domain::ManagementRecordReview {
                relative_path: path.into(),
                operation_id: "operation".into(),
                sequence: 1,
                project_id: Some("project".into()),
                reason: "management original".into(),
                at: "2026-10-14T00:00:00Z".into(),
            });
        host.startups
            .closure_reviews
            .push(cerulean_domain::StartupClosureReview {
                relative_path: path.into(),
                executor: "codex".into(),
                project_id: "project".into(),
                attempt_id: "attempt".into(),
                intent_hash: "a".repeat(64),
                reason: "closure original".into(),
                at: "2026-10-14T00:00:00Z".into(),
            });
        host.omp_stop_issues
            .push(cerulean_domain::omp_jobs::OmpStopIssue {
                project_id: "project".into(),
                task_id: "task".into(),
                attempt_id: "attempt".into(),
                message: "stop original".into(),
                pending_path: path.into(),
                pending_sha256: None,
                pending_bytes: None,
                evidence_digest: None,
                acknowledged: false,
            });
        let WorkspaceResult::Recovery(page) =
            host.workspace_query(&query("project", 0, None)).unwrap()
        else {
            panic!("page")
        };
        assert_eq!(page.total, 3);
        for entry in &page.entries {
            let WorkspaceResult::RecoveryDetail(detail) = host
                .workspace_query(&WorkspaceQuery::RecoveryDetail {
                    project_id: "project".into(),
                    kind: entry.kind,
                    id: entry.id.clone(),
                    revision: page.revision.clone(),
                })
                .unwrap()
            else {
                panic!("detail")
            };
            let expected = match entry.kind {
                WorkspaceRecoveryKind::ManagementRecord => {
                    serde_json::to_value(&host.management.reviews[0]).unwrap()
                }
                WorkspaceRecoveryKind::StartupClosure => {
                    serde_json::to_value(&host.startups.closure_reviews[0]).unwrap()
                }
                WorkspaceRecoveryKind::OmpStop => {
                    serde_json::to_value(&host.omp_stop_issues[0]).unwrap()
                }
                _ => panic!("unexpected kind"),
            };
            assert_eq!(
                serde_json::to_value(detail.record).unwrap()["value"],
                expected
            );
        }
        host.omp_stop_issues.push(host.omp_stop_issues[0].clone());
        assert!(host.workspace_query(&query("project", 0, None)).is_err());
    }
    #[test]
    fn workspace_recovery_rejects_interrupted_project_owner() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        host.startups
            .reservations
            .push(cerulean_domain::StartupReservation {
                executor: "codex".into(),
                project_id: "project".into(),
                attempt_id: "attempt".into(),
                reason: "retained original".into(),
                at: "2026-10-14T00:00:00Z".into(),
            });
        let original =
            serde_json::to_value(host.workspace_query(&query("project", 0, None)).unwrap())
                .unwrap();
        let revision = original["value"]["revision"].as_str().unwrap().to_owned();
        let detail = WorkspaceQuery::RecoveryDetail {
            project_id: "project".into(),
            kind: WorkspaceRecoveryKind::StartupReservation,
            id: "codex:attempt".into(),
            revision,
        };
        let original_detail = serde_json::to_value(host.workspace_query(&detail).unwrap()).unwrap();
        let store = host.projects.remove("project").unwrap();
        let writer =
            crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project"))
                .unwrap();
        host.api_writers.insert("project".into(), writer);
        assert_eq!(
            serde_json::to_value(host.workspace_query(&query("project", 0, None)).unwrap())
                .unwrap(),
            original
        );
        assert_eq!(
            serde_json::to_value(host.workspace_query(&detail).unwrap()).unwrap(),
            original_detail
        );
        host.api_writers.get_mut("project").unwrap().interrupted = true;
        assert!(host
            .workspace_query(&query("project", 0, None))
            .unwrap_err()
            .contains("协调器"));
        let mut writer = host.api_writers.remove("project").unwrap();
        host.projects
            .insert("project".into(), writer.owner.finish(false).unwrap());
        assert_eq!(
            serde_json::to_value(host.workspace_query(&query("project", 0, None)).unwrap())
                .unwrap(),
            original
        );
        assert_eq!(
            serde_json::to_value(host.workspace_query(&detail).unwrap()).unwrap(),
            original_detail
        );
    }
    #[test]
    fn workspace_recovery_json_escaped_summaries_use_server_byte_cursor() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        for n in 0..20 {
            host.management
                .reviews
                .push(cerulean_domain::ManagementRecordReview {
                    relative_path: format!("{n:02}{}", "\"\\\n".repeat(900)),
                    operation_id: format!("operation-{n}"),
                    sequence: 1,
                    project_id: Some("project".into()),
                    reason: "original".into(),
                    at: "2026-10-14T00:00:00Z".into(),
                });
        }
        let WorkspaceResult::Recovery(first) =
            host.workspace_query(&query("project", 0, None)).unwrap()
        else {
            panic!("page")
        };
        let cursor = first.next_offset.unwrap();
        assert!(
            cursor > 0 && cursor < 20,
            "escaped bytes must stop before entry cap"
        );
        assert_eq!(cursor as usize, first.entries.len());
        let WorkspaceResult::Recovery(next) = host
            .workspace_query(&query("project", cursor, Some(first.revision.clone())))
            .unwrap()
        else {
            panic!("next page")
        };
        assert_eq!(next.offset, cursor);
        assert_eq!(
            next.entries[0].id,
            host.management.reviews[cursor as usize].relative_path
        );
        assert!(
            serde_json::to_vec(&WorkspaceResult::Recovery(first))
                .unwrap()
                .len()
                < 64 * 1024
        );
    }
}
