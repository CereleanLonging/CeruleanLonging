//! Independent bounded readback of original management decisions, never a Snapshot.
use super::*;
use cerulean_domain::execution::{identifier, require, sha256};
#[path = "workspace_management_basis.rs"]
mod basis;
pub(crate) use basis::{original_basis, OriginalBasis};

impl RuntimeHost {
    pub(super) fn workspace_management_decisions(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        let project = match query {
            WorkspaceQuery::ManagementDecisions { project_id, .. }
            | WorkspaceQuery::ManagementDecision { project_id, .. } => project_id,
            _ => return Err("管理记录查询类型不符".into()),
        };
        identifier(project)?;
        require(
            !self.base.poisoned
                && self.management.healthy()
                && self.advice_closures.healthy()
                && self.startups.healthy()
                && self.omp_startups.healthy(),
            "原管理证据写入尚未核对，请保留记录并重开",
        )?;
        #[cfg(windows)]
        require(self.api_fault.is_none(), "原项目协调器需要重开核对")?;
        let basis = {
            #[cfg(windows)]
            {
                if let Some(owner) = self.api_writers.get(project) {
                    require(!owner.interrupted, "原项目协调器已中断，请重开核对")?;
                    owner.owner.workspace_management_basis(project)?
                } else {
                    require(
                        !self
                            .active
                            .values()
                            .any(|a| a.snapshot.project.id == *project)
                            && !self
                                .desktop_active
                                .iter()
                                .any(|a| a.snapshot.project.id == *project)
                            && !self
                                .codex_active
                                .values()
                                .any(|a| a.job.project_id == *project),
                        "原项目由非共享活动写者持有，管理记录暂不可用",
                    )?;
                    original_basis(
                        self.projects.get(project).ok_or("原项目账本不可用")?,
                        project,
                    )?
                }
            }
            #[cfg(not(windows))]
            {
                original_basis(
                    self.projects.get(project).ok_or("原项目账本不可用")?,
                    project,
                )?
            }
        };
        let mut statuses = self
            .management_decisions()
            .into_iter()
            .filter(|s| s.project_id == *project)
            .collect::<Vec<_>>();
        // Retain the original validated timestamp/operation ordering.
        statuses.sort_by_cached_key(|s| {
            let intent = self
                .management
                .intent(&s.operation_id)
                .expect("known management request");
            (
                chrono::DateTime::parse_from_rfc3339(&intent.created_at).expect("validated time"),
                s.operation_id.clone(),
            )
        });
        statuses.reverse();
        let mut records = Vec::with_capacity(statuses.len());
        let mut heads = Vec::with_capacity(statuses.len());
        for status in statuses {
            let operation = status.operation_id.clone();
            let intent = self.management.intent(&operation)?;
            require(intent.project_id == *project, "管理记录原项目身份不符")?;
            heads.push((
                operation.clone(),
                self.management.evidence_digest(&operation)?.to_owned(),
            ));
            let record = self.management_detail(&operation)?;
            require(
                record.project_id == *project && record.operation_id == operation,
                "管理记录原身份不符",
            )?;
            records.push(record);
        }
        let revision = crate::workspace_history::version(&(
            "workspace-management-decisions-v1",
            &basis.version,
            self.management.workspace_basis(project)?,
            &heads,
            &records,
        ))?;
        let result = match query {
            WorkspaceQuery::ManagementDecisions {
                offset,
                revision: expected,
                ..
            } => {
                let entries = records.iter().map(|r| {
                    let message = r.message.as_ref().map(|m| {
                        let mut chars = m.chars();
                        let short: String = chars.by_ref().take(159).collect();
                        if chars.next().is_some() {
                            format!("{short}…")
                        } else {
                            short
                        }
                    });
                    project_setup::ManagementSummary {
                        operation_id: r.operation_id.clone(),
                        project_id: r.project_id.clone(),
                        purpose: r.purpose.clone(),
                        state: r.state.clone(),
                        message,
                        evidence_digest: r.evidence_digest.clone(),
                        can_discard: r.can_discard,
                        can_reply: r.can_reply,
                        has_wait: r.wait_reason.is_some(),
                        has_reply: r.user_reply.is_some(),
                        has_discard: r.discard_reason.is_some(),
                    }
                });
                let (entries, next_offset) = crate::workspace_read::page(
                    entries,
                    records.len(),
                    *offset,
                    expected.as_deref(),
                    &revision,
                )?;
                WorkspaceResult::ManagementDecisions(WorkspaceManagementDecisionPage {
                    project_id: project.clone(),
                    revision,
                    offset: *offset,
                    next_offset,
                    total: u32::try_from(records.len()).map_err(|_| "管理记录数量超出范围")?,
                    entries,
                })
            }
            WorkspaceQuery::ManagementDecision {
                operation_id,
                revision: expected,
                ..
            } => {
                identifier(operation_id)?;
                sha256(expected)?;
                require(expected == &revision, "原管理记录已变化，请刷新首页")?;
                let record = records
                    .into_iter()
                    .find(|r| r.operation_id == *operation_id)
                    .ok_or("原管理记录不存在，未创建替代记录")?;
                WorkspaceResult::ManagementDecision(Box::new(WorkspaceManagementDecisionDetail {
                    project_id: project.clone(),
                    revision,
                    record,
                }))
            }
            _ => return Err("管理记录查询类型不符".into()),
        };
        crate::workspace_read::bounded(result)
    }
}

impl RuntimeHost {
    pub(super) fn workspace_management_gateway_evidence(
        &self,
        project_id: &str,
        operation_id: &str,
        revision: &str,
    ) -> Result<WorkspaceResult, String> {
        // Keep one authority path: the existing full original-detail query
        // owns all identity, writer, health and exact-revision checks.
        let query = WorkspaceQuery::ManagementDecision {
            project_id: project_id.into(),
            operation_id: operation_id.into(),
            revision: revision.into(),
        };
        let WorkspaceResult::ManagementDecision(original) =
            self.workspace_management_decisions(&query)?
        else {
            return Err("原管理详情查询类型不符".into());
        };
        let observation = self
            .management
            .gateway_evidence(operation_id)?
            .map(|review| gateway_projection(review, operation_id))
            .transpose()?;
        crate::workspace_read::bounded(WorkspaceResult::ManagementGatewayEvidence(Box::new(
            WorkspaceManagementGatewayEvidenceDetail {
                project_id: original.project_id,
                operation_id: original.record.operation_id,
                revision: original.revision,
                evidence_digest: original.record.evidence_digest,
                observation,
            },
        )))
    }
}

fn gateway_projection(
    review: crate::gateway_response_evidence::GatewayReview,
    operation_id: &str,
) -> Result<WorkspaceGatewayEvidence, String> {
    use cerulean_adapters::api_generation::gateway::{GatewayRequestId, GatewayRequestIdRejection};

    require(
        review.provider == "a6api" && review.local_id == operation_id,
        "网关留证原管理身份不符",
    )?;
    let status = match review.status.as_str() {
        "linked" => WorkspaceGatewayEvidenceStatus::Linked,
        "headerOnly" => WorkspaceGatewayEvidenceStatus::HeaderOnly,
        "missing" => WorkspaceGatewayEvidenceStatus::Missing,
        "noHeadersObserved" => WorkspaceGatewayEvidenceStatus::NoHeadersObserved,
        "unavailable" => WorkspaceGatewayEvidenceStatus::Unavailable,
        _ => return Err("网关留证观察状态不符".into()),
    };
    require(
        review.headers.is_some()
            == matches!(
                status,
                WorkspaceGatewayEvidenceStatus::Linked | WorkspaceGatewayEvidenceStatus::HeaderOnly
            ),
        "网关留证响应头状态不符",
    )?;
    let accepted_id = review.headers.as_ref().and_then(|headers| {
        if let GatewayRequestId::Accepted { value } = &headers.request_id {
            Some(value.as_str())
        } else {
            None
        }
    });
    require(
        review.gateway_request_id.as_deref() == accepted_id,
        "网关留证请求身份不符",
    )?;
    let headers = review
        .headers
        .map(|headers| {
            headers.validate()?;
            let request_id = match headers.request_id {
                GatewayRequestId::Missing {} => WorkspaceGatewayRequestId::Missing {},
                GatewayRequestId::Accepted { value } => {
                    WorkspaceGatewayRequestId::Accepted { value }
                }
                GatewayRequestId::Rejected { reason } => WorkspaceGatewayRequestId::Rejected {
                    reason: match reason {
                        GatewayRequestIdRejection::InvalidLength => {
                            WorkspaceGatewayRequestIdRejection::InvalidLength
                        }
                        GatewayRequestIdRejection::InvalidCharacters => {
                            WorkspaceGatewayRequestIdRejection::InvalidCharacters
                        }
                        GatewayRequestIdRejection::RepeatedHeader => {
                            WorkspaceGatewayRequestIdRejection::RepeatedHeader
                        }
                        GatewayRequestIdRejection::CredentialReflection => {
                            WorkspaceGatewayRequestIdRejection::CredentialReflection
                        }
                    },
                },
            };
            Ok::<_, String>(WorkspaceGatewayHeaders {
                status: headers.status,
                request_id,
            })
        })
        .transpose()?;
    Ok(WorkspaceGatewayEvidence {
        provider: WorkspaceGatewayProvider::A6api,
        local_id: review.local_id,
        body_response_id: review.body_response_id,
        status,
        headers,
    })
}
