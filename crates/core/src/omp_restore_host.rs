//! Request-local restore review. User command transport supplies identity;
//! client paths and principal strings are never accepted.
use super::*;
#[cfg(windows)]
use crate::omp_file_tools::write_evidence::{self, restore};
#[cfg(windows)]
use cerulean_domain::omp_restore::*;
impl RuntimeHost {
    pub(super) fn omp_restore_request(
        &self,
        command: &Command,
    ) -> Option<Result<Snapshot, String>> {
        #[cfg(windows)]
        {
            self.omp_restore_request_with_readback(command, |host, project, task, attempt, call| {
                host.omp_files_view(project, task, attempt, 0, Some(call))
            })
        }
        #[cfg(not(windows))]
        {
            matches!(
                command,
                Command::ReadOmpFiles { .. } | Command::RestoreOmpFile { .. }
            )
            .then(|| Err("文件还原需要Windows核心".into()))
        }
    }

    #[cfg(windows)]
    fn omp_restore_request_with_readback(
        &self,
        command: &Command,
        readback: impl FnOnce(&Self, &str, &str, &str, &str) -> Result<OmpFilesView, String>,
    ) -> Option<Result<Snapshot, String>> {
        if !matches!(
            command,
            Command::ReadOmpFiles { .. } | Command::RestoreOmpFile { .. }
        ) {
            return None;
        }
        #[cfg(not(windows))]
        {
            Some(Err("文件还原需要Windows核心".into()))
        }
        #[cfg(windows)]
        {
            Some((|| match command {
                Command::ReadOmpFiles {
                    project_id,
                    task_id,
                    attempt_id,
                    offset,
                    call_id,
                } => {
                    let mut snapshot = self.snapshot();
                    snapshot.omp_files = Some(self.omp_files_view(
                        project_id,
                        task_id,
                        attempt_id,
                        *offset,
                        call_id.as_deref(),
                    )?);
                    Ok(snapshot)
                }
                Command::RestoreOmpFile {
                    project_id,
                    task_id,
                    attempt_id,
                    call_id,
                    evidence_digest,
                } => {
                    sha256(evidence_digest)?;
                    let store = self.omp_restore_available(project_id, task_id, attempt_id)?;
                    let root = store.omp_file_service_path(attempt_id)?;
                    #[cfg(test)]
                    eprintln!("omp restore 328 phase=review project={project_id} task={task_id} attempt={attempt_id} call={call_id}");
                    let reviewed = restore::review(store, &root, task_id, attempt_id, call_id)?;
                    require(
                        digest(&reviewed)? == *evidence_digest,
                        "文件或复核证据已变化，请重新读取并确认",
                    )?;
                    #[cfg(test)]
                    eprintln!("omp restore 328 phase=restore project={project_id} task={task_id} attempt={attempt_id} call={call_id}");
                    let receipt = restore::restore_original(
                        store, &root, task_id, attempt_id, "user", &reviewed,
                    )?;
                    #[cfg(test)]
                    eprintln!("omp restore 328 phase=post-restore-readback project={project_id} task={task_id} attempt={attempt_id} call={call_id}");
                    let mut snapshot = self.snapshot();
                    let preview = readback(self, project_id, task_id, attempt_id, call_id);
                    let readback_error = match preview {
                        Ok(view) => {
                            snapshot.omp_files = Some(view);
                            None
                        }
                        Err(error) => Some(error),
                    };
                    snapshot.omp_restore = Some(OmpRestoreOutcome {
                        project_id: project_id.clone(),
                        task_id: task_id.clone(),
                        attempt_id: attempt_id.clone(),
                        call_id: call_id.clone(),
                        review_digest: receipt.review_digest,
                        restored_sha256: receipt.restored_sha256,
                        at: receipt.at,
                        readback_error,
                    });
                    Ok(snapshot)
                }
                _ => unreachable!(),
            })())
        }
    }
    #[cfg(windows)]
    fn omp_restore_available(
        &self,
        project: &str,
        task: &str,
        attempt: &str,
    ) -> Result<&DurableWorkflow, String> {
        for id in [project, task, attempt] {
            identifier(id)?;
        }
        require(
            !self.has_codex_activity(project)
                && !self.active.contains_key(project)
                && !self.has_omp_activity(project)
                && self
                    .desktop_active
                    .as_ref()
                    .is_none_or(|a| a.snapshot.project.id != project),
            "项目仍有执行或验收，请等待结束后复核",
        )?;
        require(
            !self
                .runs
                .all()
                .iter()
                .any(|r| r.project_id == project && r.state == project_setup::RunState::Active),
            "请先暂停项目调度再复核文件",
        )?;
        require(
            !self
                .management
                .statuses()
                .iter()
                .any(|s| s.project_id == project && !s.state.resolved()),
            "项目仍有未决指挥事务",
        )?;
        require(
            !self.jobs.values().any(|j| {
                j.project_id == project
                    && matches!(
                        j.state,
                        VerificationState::Running
                            | VerificationState::Cancelling
                            | VerificationState::Unknown
                    )
            }),
            "项目验收仍未收口",
        )?;
        require(
            !self.codex_jobs.values().any(|j| {
                j.project_id == project
                    && matches!(
                        j.state,
                        CodexJobState::Starting
                            | CodexJobState::Executing
                            | CodexJobState::Reconciling
                            | CodexJobState::Unknown
                    )
            }),
            "项目存在其他未收口执行",
        )?;
        let store = self.projects.get(project).ok_or("项目账本暂不可用")?;
        require(
            !store.requires_reopen() && store.state().revision_requested.is_none(),
            "项目正在修订或需要重开恢复",
        )?;
        require(
            store.state().tasks.iter().all(|(id, t)| {
                id == task
                    || !matches!(
                        t.state,
                        TaskState::Assigned
                            | TaskState::Dispatching
                            | TaskState::Running
                            | TaskState::Cancelling
                            | TaskState::Unknown
                    )
            }),
            "项目有其他活动或待对账任务",
        )?;
        require(
            store.state().tasks.get(task).is_some_and(|t| {
                t.state == TaskState::Unknown && t.attempt.as_deref() == Some(attempt)
            }),
            "仅能复核当前未知尝试",
        )?;
        Ok(store)
    }
    #[cfg(windows)]
    fn omp_files_view(
        &self,
        project: &str,
        task: &str,
        attempt: &str,
        offset: u32,
        call_id: Option<&str>,
    ) -> Result<OmpFilesView, String> {
        let store = self.omp_restore_available(project, task, attempt)?;
        let (_, _, intent, _) = crate::omp_execution::scope(store, task, attempt)?;
        let root = store.omp_file_service_path(attempt)?;
        require(root.is_dir(), "该尝试尚无文件工具证据")?;
        let journal = crate::omp_tool_journal::OmpToolJournal::open(&root.join("events"), &intent)?;
        let ids = journal
            .observation()
            .calls
            .iter()
            .filter(|(_, c)| c.tool_name == "cerulean_write_file")
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        drop(journal);
        require(offset as usize <= ids.len(), "文件记录页已变化，请刷新")?;
        let files = ids
            .iter()
            .skip(offset as usize)
            .take(16)
            .map(|id| match write_evidence::inspect(&root, &intent, id) {
                Ok(v) => OmpFileItem {
                    call_id: id.clone(),
                    path: Some(v.path),
                    state: format!("{:?}", v.state).to_lowercase(),
                    error: None,
                },
                Err(_) => OmpFileItem {
                    call_id: id.clone(),
                    path: None,
                    state: "unavailable".into(),
                    error: Some("写入证据暂不可核查".into()),
                },
            })
            .collect();
        let detail = if let Some(id) = call_id {
            require(
                ids.iter().any(|known| known == id),
                "文件调用不属于本次尝试",
            )?;
            let reviewed = restore::review(store, &root, task, attempt, id)?;
            let (original, current) =
                write_evidence::preview_content(&root, &intent, &reviewed.write)?;
            let repair = root
                .join("repairs")
                .join(format!("{:x}", sha2::Sha256::digest(id.as_bytes())));
            let restore_status = if fs::symlink_metadata(&repair).is_ok() {
                Some(
                    match restore::inspect_restore(store, &root, task, attempt, id) {
                        Ok(v) if v.result_recorded && v.current_matches_original => "restored",
                        Ok(v) if v.result_recorded => "changed-after-restore",
                        Ok(_) => "restore-interrupted",
                        Err(_) => "restore-unavailable",
                    }
                    .into(),
                )
            } else {
                None
            };
            let evidence_digest = if restore_status.is_none() {
                Some(digest(&reviewed)?)
            } else {
                None
            };
            Some(OmpFileReview {
                project_id: project.into(),
                task_id: task.into(),
                attempt_id: attempt.into(),
                call_id: id.into(),
                path: reviewed.write.path.clone(),
                workflow_sequence: store.history().len() as u32,
                state: format!("{:?}", reviewed.write.state).to_lowercase(),
                original_sha256: reviewed.write.before_sha256,
                current_sha256: reviewed.write.current_sha256,
                original,
                current,
                evidence_digest,
                restore_status,
            })
        } else {
            None
        };
        Ok(OmpFilesView {
            project_id: project.into(),
            task_id: task.into(),
            attempt_id: attempt.into(),
            offset,
            total: ids.len() as u32,
            files,
            detail,
        })
    }
}
#[cfg(windows)]
use sha2::Digest;
#[cfg(all(test, windows))]
#[path = "omp_restore_host/tests.rs"]
mod tests;
