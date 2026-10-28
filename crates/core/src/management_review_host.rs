use super::*;
impl RuntimeHost {
    pub(super) fn validate_management_reviews(&self) -> Result<(), String> {
        for review in &self.management.reviews {
            if let Some(project) = &review.project_id {
                require(
                    self.projects.contains_key(project),
                    "管理复核前置链引用的项目不存在",
                )?;
            }
            let operation = &review.operation_id;
            let workflow_operation = format!("mgmt-{operation}");
            if review.sequence <= 2 {
                for store in self.projects.values() {
                    require(
                        !store
                            .history()
                            .iter()
                            .any(|e| e.body.operation_id == workflow_operation),
                        "半写管理复核与已提交工作流冲突",
                    )?;
                }
                for startup in self
                    .startups
                    .all()
                    .into_iter()
                    .chain(self.omp_startups.all())
                {
                    if startup.intent.management_operation.as_ref() != Some(operation) {
                        continue;
                    }
                    require(review.sequence == 2, "半写请求或响应仍关联执行启动")?;
                    let i = &startup.intent;
                    let store = self
                        .projects
                        .get(&i.project_id)
                        .ok_or("管理复核关联项目缺失")?;
                    require(!store.history().iter().any(|e|e.body.event.command.attempt()==Some(i.attempt_id.as_str()) || matches!(&e.body.context.execution,Some(ExecutionEvidence::Attempt{attempt}) if attempt.id==i.attempt_id)),"半写管理准备仍有关联尝试事件")?;
                    for path in [
                        store.omp_attempt_path(&i.attempt_id)?,
                        store.omp_file_service_path(&i.attempt_id)?,
                    ] {
                        require(
                            matches!(fs::symlink_metadata(path),Err(e) if e.kind()==std::io::ErrorKind::NotFound),
                            "半写管理准备仍有关联执行或工具记录",
                        )?;
                    }
                    require(
                        !self
                            .jobs
                            .values()
                            .any(|j| j.project_id == i.project_id && j.attempt_id == i.attempt_id),
                        "半写管理准备仍有关联验收任务",
                    )?;
                }
            }
            // An uncommitted user disposition cannot suppress an assignment
            // or change the prefix later by reconstructing a missing receipt.
            if review.sequence == 4
                && !self
                    .management
                    .application(operation)
                    .is_some_and(|(_, applied, _)| applied)
            {
                for store in self.projects.values() {
                    require(
                        !store
                            .history()
                            .iter()
                            .any(|e| e.body.operation_id == workflow_operation),
                        "半写处置与未确认的已提交应用冲突",
                    )?;
                }
            }
        }
        Ok(())
    }
}
