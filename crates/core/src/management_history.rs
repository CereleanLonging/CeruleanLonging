//! Request-local, revision-bound pages. Full text is returned only on a detail read.
use super::*;
use cerulean_domain::project_setup::{ManagementCount, ManagementHistory, ManagementSummary};
const PAGE_SIZE: usize = 50;

impl RuntimeHost {
    pub(super) fn management_page(
        &self,
        project: Option<&str>,
        offset: u32,
        expected: Option<&str>,
    ) -> Result<(Vec<ManagementSummary>, ManagementHistory), String> {
        if let Some(project) = project {
            identifier(project)?;
            require(
                self.projects.contains_key(project)
                    || self
                        .management
                        .statuses()
                        .iter()
                        .any(|s| s.project_id == project),
                "项目不存在",
            )?;
        }
        let mut statuses = self.management_decisions();
        let evidence = statuses
            .iter()
            .map(|s| {
                Ok((
                    &s.operation_id,
                    &s.state,
                    self.management.evidence_digest(&s.operation_id)?,
                    self.advice_closures.get(&s.operation_id),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let revision = digest(&evidence)?;
        require(
            offset == 0 || expected.is_some(),
            "续页需要记录版本，请从首页刷新",
        )?;
        if let Some(expected) = expected {
            require(expected == revision, "指挥记录已变化，请刷新首页后继续翻页")?;
        }
        let mut projects: BTreeMap<String, ManagementCount> = BTreeMap::new();
        for status in &statuses {
            let count = projects
                .entry(status.project_id.clone())
                .or_insert_with(|| ManagementCount {
                    project_id: status.project_id.clone(),
                    ..Default::default()
                });
            count.total += 1;
            count.pending += u32::from(!status.state.resolved());
            count.running +=
                u32::from(status.state == crate::management_execution::DecisionState::Running);
            count.waiting += u32::from(self.management.unanswered_wait(&status.operation_id));
        }
        statuses.retain(|s| project.is_none_or(|p| s.project_id == p));
        // Parse validated timestamps instead of relying on lexical timezone ordering.
        statuses.sort_by_cached_key(|s| {
            let intent = self
                .management
                .intent(&s.operation_id)
                .expect("known request");
            (
                chrono::DateTime::parse_from_rfc3339(&intent.created_at).expect("validated time"),
                s.operation_id.clone(),
            )
        });
        statuses.reverse();
        let total = statuses.len() as u32;
        require(offset <= total, "页偏移超出当前记录范围")?;
        let next_offset =
            (offset as usize + PAGE_SIZE < statuses.len()).then_some(offset + PAGE_SIZE as u32);
        let summaries = statuses
            .into_iter()
            .skip(offset as usize)
            .take(PAGE_SIZE)
            .map(|s| {
                let view = self.management_view(s);
                ManagementSummary {
                    operation_id: view.operation_id,
                    project_id: view.project_id,
                    purpose: view.purpose,
                    state: view.state,
                    message: view.message.map(|m| {
                        let mut chars = m.chars();
                        let short: String = chars.by_ref().take(159).collect();
                        if chars.next().is_some() {
                            format!("{short}…")
                        } else {
                            short
                        }
                    }),
                    evidence_digest: view.evidence_digest,
                    can_discard: view.can_discard,
                    can_reply: view.can_reply,
                    has_wait: view.wait_reason.is_some(),
                    has_reply: view.user_reply.is_some(),
                    has_discard: view.discard_reason.is_some(),
                }
            })
            .collect();
        Ok((
            summaries,
            ManagementHistory {
                total,
                offset,
                next_offset,
                project_id: project.map(str::to_owned),
                revision,
                projects: projects.into_values().collect(),
            },
        ))
    }
    pub(super) fn management_read(&self, command: &Command) -> Option<Result<Snapshot, String>> {
        match command {
            Command::ReviewVerificationRecheckDisposition {
                project_id,
                operation_id,
                reason,
            } => Some((|| {
                let review =
                    self.read_recheck_disposition_review(project_id, operation_id, reason)?;
                let mut snapshot = self.snapshot();
                snapshot.verification_recheck_disposition_review = Some(review);
                Ok(snapshot)
            })()),
            Command::ReviewVerificationRecheck { input } => Some((|| {
                let review = self.review_verification_recheck(input)?;
                let mut snapshot = self.snapshot();
                snapshot.verification_recheck_review = Some(review);
                Ok(snapshot)
            })()),
            Command::ReadVerificationRecheck {
                project_id,
                operation_id,
            } => Some((|| {
                let status = self.read_verification_recheck(project_id, operation_id)?;
                let mut snapshot = self.snapshot();
                snapshot.verification_recheck_status = Some(status);
                Ok(snapshot)
            })()),
            Command::ReadWorkerObservation {
                project_id,
                attempt_id,
            } => Some((|| {
                identifier(project_id)?;
                identifier(attempt_id)?;
                #[cfg(windows)]
                {
                    let job = self
                        .codex_jobs
                        .get(&(project_id.clone(), attempt_id.clone()))
                        .ok_or("执行记录不存在")?;
                    let view = if let Some(active) = self
                        .codex_active
                        .get(&(project_id.clone(), attempt_id.clone()))
                        .filter(|a| a.job.attempt_id == *attempt_id && a.observation.is_some())
                    {
                        crate::worker_observation::view(
                            job,
                            active.observation.as_ref().unwrap().snapshot(),
                            active.observation_error.clone(),
                            active
                                .snapshot
                                .revision
                                .as_ref()
                                .ok_or("监督原方案缺失")?
                                .supervision_for(&job.task_id)?
                                .cloned(),
                        )?
                    } else {
                        let view = crate::worker_observation::read(&self.root, job)?;
                        let store = self
                            .projects
                            .get(project_id)
                            .ok_or("项目账本暂由执行器持有，请稍后读取历史观察")?;
                        let policy = crate::host::workspace_observation::original_policy(
                            store,
                            job,
                            &view.trace,
                        )?;
                        require(
                            view.supervision.as_ref() == policy.as_ref(),
                            "观察策略与原方案不符",
                        )?;
                        view
                    };
                    let mut snapshot = self.snapshot();
                    snapshot.worker_observation = Some(view);
                    Ok(snapshot)
                }
                #[cfg(not(windows))]
                {
                    Err("执行观察需要Windows运行时".into())
                }
            })()),
            Command::ReadVerificationDetail {
                project_id,
                operation_id,
            } => Some((|| {
                identifier(project_id)?;
                identifier(operation_id)?;
                let job = self
                    .jobs
                    .get(&(project_id.clone(), operation_id.clone()))
                    .ok_or("验收记录不存在")?;
                let basis = if let Some(store) = self.projects.get(project_id) {
                    crate::verification_detail::basis(store, job)?
                } else {
                    #[cfg(windows)]
                    {
                        if self.api_writers.contains_key(project_id) {
                            let reader =
                                self.api_read(project_id, &job.task_id, &job.attempt_id)?;
                            crate::verification_detail::basis_from_history(reader.history(), job)?
                        } else {
                            let active = self
                                .active
                                .get(project_id)
                                .filter(|a| a.job.operation_id == *operation_id)
                                .ok_or("项目正在执行，请稍后读取历史验收详情")?;
                            active.detail_basis.clone()
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        return Err("项目验收状态不可用".into());
                    }
                };
                let mut detail = crate::verification_detail::read(&self.root, job, &basis)?;
                #[cfg(windows)]
                {
                    detail.can_retry_start = self
                        .verification_start_retry_available(project_id, operation_id)
                        .is_ok();
                }
                if job.state == VerificationState::Unknown {
                    match self.verification_recovery_review(job) {
                        Ok(review) => detail.recovery = Some(review),
                        Err(error) => {
                            detail.recovery_error = Some(error.chars().take(1000).collect())
                        }
                    }
                }
                let mut snapshot = self.snapshot();
                snapshot.verification_detail = Some(detail);
                Ok(snapshot)
            })()),
            Command::ReadCodexStartup {
                project_id,
                attempt_id,
            } => Some((|| {
                let detail = self.startup_detail(project_id, attempt_id)?;
                let mut snapshot = self.snapshot();
                snapshot.codex_startup_detail = Some(detail);
                Ok(snapshot)
            })()),
            Command::ReadManagementPage {
                project_id,
                offset,
                revision,
            } => Some((|| {
                let (decisions, history) =
                    self.management_page(project_id.as_deref(), *offset, revision.as_deref())?;
                let mut snapshot = self.snapshot();
                snapshot.management_decisions = decisions;
                snapshot.management_history = history;
                Ok(snapshot)
            })()),
            Command::ReadManagementDetail { operation_id } => Some((|| {
                identifier(operation_id)?;
                let detail = self.management_detail(operation_id)?;
                let mut snapshot = self.snapshot();
                snapshot.management_detail = Some(detail);
                Ok(snapshot)
            })()),
            _ => None,
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::management_execution::{DecisionIntent, UserResolution};
    use cerulean_adapters::openai::DecisionObservation;
    use cerulean_domain::management::{DecisionEnvelope, ManagementDecision, Purpose};
    fn fixture(root: &Path, count: usize) -> RuntimeHost {
        fs::create_dir(root.join("project")).unwrap();
        fs::create_dir(root.join("project-two")).unwrap();
        let mut host = RuntimeHost::open(&root.join("data")).unwrap();
        for id in ["project", "project-two"] {
            host.register(Project {
                id: id.into(),
                root: root.join(id).to_string_lossy().into(),
                timezone: "Asia/Shanghai".into(),
            })
            .unwrap();
        }
        for n in 0..count {
            let project = if n % 2 == 0 { "project" } else { "project-two" };
            let id = format!("history-{n:04}");
            let context = serde_json::json!({"requirement":"isolated history capacity fixture","workflow":{"project":project}});
            let state_digest = digest(&context).unwrap();
            let created_at = (chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                + chrono::Duration::seconds(n as i64))
            .to_rfc3339();
            host.management
                .begin(DecisionIntent {
                    operation_id: id.clone(),
                    project_id: project.into(),
                    purpose: Purpose::Plan,
                    model: "fixture-history".into(),
                    state_digest: state_digest.clone(),
                    context,
                    created_at,
                })
                .unwrap();
            if n + 1 == count {
                continue;
            }
            host.management
                .finish(
                    &id,
                    Ok(DecisionObservation {
                        response_id: format!("resp_fixture_{n}"),
                        requested_model: "fixture-history".into(),
                        actual_model: "fixture-history".into(),
                        input_tokens: 0,
                        output_tokens: 0,
                        envelope: DecisionEnvelope {
                            state_digest,
                            decision: ManagementDecision::Wait {
                                reason: "\u{1}".repeat(8000),
                            },
                        },
                    }),
                )
                .unwrap();
            let reviewed_hash = host.management.evidence_digest(&id).unwrap().to_owned();
            host.management
                .resolve(
                    &id,
                    &reviewed_hash,
                    UserResolution::Discard {
                        reason: "\u{2}".repeat(8000),
                    },
                )
                .unwrap();
        }
        host
    }
    fn read(host: &mut RuntimeHost, command: Command) -> Result<Snapshot, String> {
        let response = host.handle(Request {
            id: "history-read".into(),
            protocol_version: PROTOCOL_VERSION,
            command,
        });
        response
            .result
            .ok_or_else(|| response.error.unwrap().message)
    }
    #[test]
    fn pages_cover_large_history_with_exact_counts_and_no_full_text_in_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), 122);
        let default = host.snapshot();
        assert_eq!(default.management_decisions.len(), 50);
        assert_eq!(default.management_history.total, 122);
        assert!(default.management_detail.is_none());
        assert!(serde_json::to_vec(&default).unwrap().len() < 256 * 1024);
        let rev = default.management_history.revision.clone();
        let mut ids = std::collections::BTreeSet::new();
        let mut offset = 0;
        loop {
            let page = read(
                &mut host,
                Command::ReadManagementPage {
                    project_id: None,
                    offset,
                    revision: Some(rev.clone()),
                },
            )
            .unwrap();
            for d in page.management_decisions {
                assert!(ids.insert(d.operation_id));
                assert!(d.message.as_ref().is_none_or(|m| m.chars().count() <= 160));
            }
            if let Some(next) = page.management_history.next_offset {
                offset = next;
            } else {
                break;
            }
        }
        assert_eq!(ids.len(), 122);
        let one = read(
            &mut host,
            Command::ReadManagementPage {
                project_id: Some("project".into()),
                offset: 0,
                revision: None,
            },
        )
        .unwrap();
        assert_eq!(one.management_history.total, 61);
        assert!(one
            .management_decisions
            .iter()
            .all(|d| d.project_id == "project"));
        assert_eq!(
            one.management_history
                .projects
                .iter()
                .find(|p| p.project_id == "project-two")
                .unwrap()
                .pending,
            1
        );
        let detail = read(
            &mut host,
            Command::ReadManagementDetail {
                operation_id: "history-0000".into(),
            },
        )
        .unwrap();
        let full = detail.management_detail.unwrap();
        assert_eq!(full.wait_reason.unwrap(), "\u{1}".repeat(8000));
        assert_eq!(full.discard_reason.unwrap(), "\u{2}".repeat(8000));
        assert_eq!(host.management.statuses().len(), 122);
        assert!(host.snapshot().management_detail.is_none());
        assert!(host.projects.values().all(|p| p.history().is_empty()));
    }
    #[test]
    fn stale_pages_rejected_after_resolution_and_queries_never_consume_ready_plans() {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), 4);
        let rev = host.snapshot().management_history.revision;
        assert!(read(
            &mut host,
            Command::ReadManagementPage {
                project_id: None,
                offset: 1,
                revision: None
            }
        )
        .is_err());
        assert!(read(
            &mut host,
            Command::ReadManagementPage {
                project_id: None,
                offset: 99,
                revision: Some(rev.clone())
            }
        )
        .is_err());
        assert!(read(
            &mut host,
            Command::ReadManagementDetail {
                operation_id: "missing".into()
            }
        )
        .is_err());
        let hash = host
            .management
            .evidence_digest("history-0003")
            .unwrap()
            .to_string();
        host.resolve_management(
            "history-0003",
            &hash,
            UserResolution::Discard {
                reason: "explicit user disposition".into(),
            },
        )
        .unwrap();
        assert!(read(
            &mut host,
            Command::ReadManagementPage {
                project_id: None,
                offset: 1,
                revision: Some(rev)
            }
        )
        .unwrap_err()
        .contains("记录已变化"));
        let context = serde_json::json!({"requirement":"read-only fixture"});
        let state = digest(&context).unwrap();
        host.management
            .begin(DecisionIntent {
                operation_id: "ui-plan-read-only".into(),
                project_id: "project".into(),
                purpose: Purpose::Plan,
                model: "fixture".into(),
                state_digest: state.clone(),
                context,
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .unwrap();
        host.management
            .finish(
                "ui-plan-read-only",
                Ok(DecisionObservation {
                    response_id: "resp_read_fixture".into(),
                    requested_model: "fixture".into(),
                    actual_model: "fixture".into(),
                    input_tokens: 0,
                    output_tokens: 0,
                    envelope: DecisionEnvelope {
                        state_digest: state,
                        decision: ManagementDecision::Wait {
                            reason: "do not consume this on read".into(),
                        },
                    },
                }),
            )
            .unwrap();
        read(
            &mut host,
            Command::ReadManagementDetail {
                operation_id: "ui-plan-read-only".into(),
            },
        )
        .unwrap();
        read(
            &mut host,
            Command::ReadManagementPage {
                project_id: None,
                offset: 0,
                revision: None,
            },
        )
        .unwrap();
        assert!(host.management.application("ui-plan-read-only").is_none());
        assert!(!host.planning_errors.contains_key("ui-plan-read-only"));
    }
    #[test]
    #[ignore = "native 1000-request history capacity fixture; zero provider calls"]
    fn seed_native_management_history_fixture() {
        let root = PathBuf::from(std::env::var_os("CERULEAN_HISTORY_NATIVE_DIR").unwrap())
            .canonicalize()
            .unwrap();
        let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../reports")
            .canonicalize()
            .unwrap();
        assert!(root.starts_with(reports) && !root.join("data").exists());
        let host = fixture(&root, 1000);
        assert_eq!(host.snapshot().management_history.total, 1000);
        fs::write(root.join("fixture.json"),serde_json::to_vec_pretty(&serde_json::json!({"total":1000,"projectCounts":{"project":500,"project-two":500},"unknownOperation":"history-0999","oldestOperation":"history-0000","waitCharacter":1,"discardCharacter":2,"textLength":8000,"realModelCalls":0})).unwrap()).unwrap();
    }
}
