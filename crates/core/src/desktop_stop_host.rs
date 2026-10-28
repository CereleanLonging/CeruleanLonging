//! The user thread persists a request; the sole workflow writer performs stops.
use super::*;
use cancellation::{StopObservation, StopProof, Transport};
use execution::TaskProof;

pub(super) struct BridgeTransport<'a> {
    pub root: &'a Path,
    #[cfg(test)]
    pub fixture: Option<Fixture>,
}
impl BridgeTransport<'_> {
    fn call(&self, request: &ReadRequest) -> Result<ReadReply, String> {
        #[cfg(test)]
        if let Some(f) = self.fixture {
            return f(request);
        }
        observe(self.root, request)
    }
}
impl Transport for BridgeTransport<'_> {
    fn prepare(&mut self, r: &TaskReview) -> Result<StopObservation, String> {
        let value = self
            .call(&ReadRequest::TaskStopPrepare {
                version: 1,
                review: r.clone(),
            })?
            .stop_observation
            .ok_or("停止观察缺失")?;
        serde_json::from_value(value).map_err(|_| "停止观察格式无效".into())
    }
    fn stop(&mut self, r: &TaskReview, o: &StopObservation) -> Result<StopProof, String> {
        let value = self
            .call(&ReadRequest::TaskStop {
                version: 1,
                review: r.clone(),
                observation: serde_json::to_value(o).map_err(|_| "停止请求编码失败")?,
            })?
            .stop_proof
            .ok_or("取消证明缺失")?;
        serde_json::from_value(value).map_err(|_| "取消证明格式无效".into())
    }
    fn read(&mut self, r: &TaskReview, o: &StopObservation) -> Result<StopProof, String> {
        let value = self
            .call(&ReadRequest::TaskStopRead {
                version: 1,
                review: r.clone(),
                observation: serde_json::to_value(o).map_err(|_| "停止回读编码失败")?,
            })?
            .stop_proof
            .ok_or("取消回读证明缺失")?;
        serde_json::from_value(value).map_err(|_| "取消回读证明格式无效".into())
    }
    fn completed(&mut self, r: &TaskReview) -> Result<TaskProof, String> {
        self.call(&ReadRequest::TaskRead {
            version: 1,
            review: r.clone(),
        })?
        .task_proof
        .ok_or("原任务完成结果尚未核实".into())
    }
}
impl RuntimeHost {
    pub(in crate::host) fn cancel_desktop_task(
        &mut self,
        project: &str,
        attempt: &str,
        expected: &str,
        reason: &str,
    ) -> Result<(), String> {
        sha256(expected)?;
        cancellation::validate_reason(reason)?;
        require(self.desktop_probe.is_none(), "请等待当前桌面观察结束")?;
        let review = if let Some(active) = &self.desktop_active {
            require(
                active.status.review.project_id == project
                    && active.status.review.attempt.id == attempt
                    && !active.fault
                    && !active.status.stopping
                    && active.pending_stop.is_none()
                    && active.status.cancellation.is_none(),
                "桌面停止目标不符或停止已请求",
            )?;
            active.status.review.clone()
        } else {
            self.require_available(project)?;
            let store = self.projects.get(project).ok_or("原项目不可用")?;
            let (review, _) = execution::inspect(&self.root, store, project, attempt)?;
            cancellation::review::require_unrestricted(&self.root, store, &review)?;
            require(
                store
                    .state()
                    .tasks
                    .get(&review.attempt.task_id)
                    .is_some_and(|t| {
                        t.attempt.as_deref() == Some(attempt)
                            && matches!(
                                t.state,
                                TaskState::Unknown | TaskState::Running | TaskState::Dispatching
                            )
                    }),
                "原桌面任务当前不可停止",
            )?;
            require(
                cancellation::inspect(&self.root, store, &review)?.is_none(),
                "停止已请求；仅可回读原结果",
            )?;
            review
        };
        require(
            review.id == expected && review.bridge_sha256 == fingerprint(),
            "停止审阅或桥接版本已变化",
        )?;
        let runs: Vec<_> = self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.project_id == project && !r.state.terminal())
            .map(|r| r.id)
            .collect();
        for run in runs {
            self.pause_project_run(&run)?;
        }
        let permit = cancellation::request(&self.root, &review, reason)?;
        self.desktop_access.task_review = None;
        if let Some(active) = &mut self.desktop_active {
            active.pending_stop = Some(permit);
            active.status.stopping = true;
            // Only the owned observer subprocess is interrupted. The external
            // app is touched later by the native, guarded, single Invoke.
            active.cancel.store(true, Ordering::Release);
            Ok(())
        } else {
            self.launch_desktop_operation(review, Operation::Stop(Some(permit)))
        }
    }
}
