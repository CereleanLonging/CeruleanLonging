from pathlib import Path
root=Path(__file__).resolve().parents[1]/'crates/core/src'
def edit(name,f):
    p=root/name;s=p.read_text(encoding='utf-8-sig');n=f(s);assert n!=s,name;p.write_text(n,encoding='utf-8')
def once(s,a,b):
    assert s.count(a)==1,a
    return s.replace(a,b)
edit('lib.rs',lambda s:once(s,'mod codex_workflow_access;', 'mod codex_workflow_access;\nmod supervision_scope;'))
edit('host.rs',lambda s:once(s,'''        #[cfg(windows)]
        snapshot
            .workflows
            .extend(self.codex_active.values().map(|a| a.snapshot.clone()));
''',''))
edit('codex_management_bridge.rs',lambda s:once(s,'scope.attempt_id.clone()', 'active.attempt_id.clone()'))
def cancellation(s):
    s=s.replace('.get(project)','.get(&(project.into(),attempt.into()))').replace('.get_mut(project)','.get_mut(&(project.into(),attempt.into()))')
    s=s.replace('self.codex_active[project]','self.codex_active[&(project.into(),attempt.into())]')
    s=s.replace('.filter_map(|(project, active)| {','.filter_map(|((project, _), active)| {')
    return s
edit('codex_cancellation.rs',cancellation)
edit('management_history.rs',lambda s:once(s,'.get(project_id)\n                        .filter(|a| a.job.attempt_id', '.get(&(project_id.clone(),attempt_id.clone()))\n                        .filter(|a| a.job.attempt_id'))
def requests(s):
    s=once(s,'bound.version == 1 && intent.purpose', 'matches!(bound.version,1|2) && intent.purpose')
    s=once(s,'.get(&run.project_id)\n            .ok_or("原执行线程已结束")?', '.get(&(run.project_id.clone(),observation.attempt_id.clone()))\n            .ok_or("原执行线程已结束")?')
    s=once(s,'        let state = &active.snapshot;','''        let current=self.api_writers.get(&run.project_id).map(|w|w.owner.snapshot()).transpose()?;
        let state=current.as_ref().unwrap_or(&active.snapshot);''')
    s=once(s,'        let active = &self.codex_active[&run.project_id];','''        let attempts:Vec<_>=self.codex_active.keys().filter(|(p,_)|p==&run.project_id).map(|(_,a)|a.clone()).collect();
        for attempt in attempts { self.poll_supervision_attempt(run,&attempt)?; }
        Ok(())
    }
    fn poll_supervision_attempt(&mut self, run:&ProjectRun, attempt:&str) -> Result<(),String> {
        let active=&self.codex_active[&(run.project_id.clone(),attempt.into())];''')
    s=once(s,'            version: 1,','''            version: if crate::api_assignment_scope::isolated(active.snapshot.revision.as_ref().ok_or("监督方案缺失")?,&active.job.task_id)? {2} else {1},''') if False else s
    # api_assignment_scope is a sibling host module.
    a=s.index('        let bound = Binding {');b=s.index('        context["supervision"]',a)
    block=s[a:b].replace('version: 1,','version: if api_assignment_scope::isolated(active.snapshot.revision.as_ref().ok_or("监督方案缺失")?,&active.job.task_id)? {2} else {1},')
    s=s[:a]+block+s[b:]
    s=once(s,'self.codex_active.get(&intent.project_id)', 'self.codex_active.get(&(intent.project_id.clone(),b.attempt_id.clone()))')
    # Permanent replay validates the saved workflow prefix, including interleaved tasks.
    a=s.index('            let observation: WorkerObservationView =',s.index('pub(super) fn validate_supervision_requests'))
    s=s[:a]+'''            if b.version==2 {
                let original:ExecutionSnapshot=serde_json::from_value(intent.context["workflow"].clone()).map_err(|_|"监督原快照无效")?;
                crate::supervision_scope::validate(store,&original,&b.task_id,&b.attempt_id,&b.thread_id,&b.turn_id,original.sequence)?;
                if let Some(entry)=store.history().iter().find(|e| e.body.operation_id==format!("mgmt-{}",intent.operation_id)) {
                    crate::supervision_scope::validate(store,&original,&b.task_id,&b.attempt_id,&b.thread_id,&b.turn_id,
                        entry.body.event.sequence.checked_sub(1).ok_or("监督事件序号无效")?)?;
                }
            }
''' + s[a:]
    return s
edit('supervision_requests.rs',requests)
def delivery(s):
    s=once(s,'.get(project)\n            .ok_or("advice worker ended")?', '.get(&(project.into(),b.attempt_id.clone()))\n            .ok_or("advice worker ended")?')
    a=s.index('        let active = &self.codex_active[&run.project_id];');b=s.index('        if let Some(status)',a)
    s=s[:a]+s[b:]
    s=once(s,'            self.advice_authorized(&run.project_id, &status.operation_id)?;','''            let b=self.advice_authorized(&run.project_id, &status.operation_id)?;
            let active=&self.codex_active[&(run.project_id.clone(),b.attempt_id)];''')
    s=once(s,'    fn prepare_worker_advice(\n        &mut self,\n        project: &str,','    fn prepare_worker_advice(\n        &mut self,\n        project: &str,\n        attempt: &str,')
    s=once(s,'.get(project)\n            .is_none_or(|a| a.advice_gate', '.get(&(project.into(),attempt.into()))\n            .is_none_or(|a| a.advice_gate')
    # Prepare must accept only the current native worker's bound advice.
    a=s.index('    fn prepare_worker_advice(');b=s.index('    fn commit_worker_advice(',a)
    block=s[a:b]
    block=block.replace('.is_ok_and(|i| i.context.get("supervision").is_some())', '.is_ok_and(|i| i.context["supervision"]["attemptId"]==attempt)')
    block=once(block,'        self.advice_authorized(project, &operation)?;', '''        let b=self.advice_authorized(project, &operation)?;
        if b.version==2 { self.validate_shared_advice(&operation,&b,&snapshot,None,true)?; }''')
    block=block.replace('digest(&snapshot)? == digest(&self.codex_active[project].snapshot)?', 'b.version==2 || digest(&snapshot)? == digest(&self.codex_active[&(project.into(),attempt.into())].snapshot)?')
    block=block.replace('self.codex_active[project]', 'self.codex_active[&(project.into(),attempt.into())]')
    s=s[:a]+block+s[b:]
    a=s.index('    fn commit_worker_advice(');b1=s.index('    pub(super) fn delivery_for(',a)
    block=s[a:b1]
    block=once(block,'        let active = &self.codex_active[project];','''        let key=(project.into(),b.attempt_id.clone());
        if b.version==2 { self.validate_shared_advice(operation,&b,&snapshot,Some(&entry),false)?; }
        let active = &self.codex_active[&key];''')
    block=block.replace('&& entry.body.event.sequence == active.snapshot.sequence + 1','&& (b.version==2 || entry.body.event.sequence == active.snapshot.sequence + 1)')
    block=block.replace('self.codex_active.get_mut(project)', 'self.codex_active.get_mut(&key)').replace('self.codex_active[project]', 'self.codex_active[&key]')
    s=s[:a]+block+s[b1:]
    s=once(s,'pub(super) fn handle_advice_update(&mut self, project: &str, update: AdviceUpdate)', 'pub(super) fn handle_advice_update(&mut self, project: &str, attempt: &str, update: AdviceUpdate)')
    s=once(s,'self.prepare_worker_advice(project, *snapshot)','self.prepare_worker_advice(project, attempt, *snapshot)')
    s=once(s,'reply.send(self.commit_worker_advice(project, &operation, *entry, *snapshot))','reply.send((|| { require(binding(self.management.intent(&operation)?)?.is_some_and(|b|b.attempt_id==attempt),"纠偏回执尝试不符")?; self.commit_worker_advice(project, &operation, *entry, *snapshot) })())')
    s=once(s,'reply.send(self.finish_worker_advice(project, &operation))','reply.send((|| { require(binding(self.management.intent(&operation)?)?.is_some_and(|b|b.attempt_id==attempt),"纠偏结束尝试不符")?; self.finish_worker_advice(project, &operation) })())')
    # Avoid guessing an advice event sequence from a later active snapshot.
    s=once(s,'        } else {\n            let active = self\n                .codex_active\n                .get(&d.project_id)', '''        } else if self.api_writers.contains_key(&d.project_id) {
            let reader=self.codex_read(&d.project_id,&d.task_id,&d.attempt_id)?;
            require(reader.history().iter().any(valid_entry),"delivery lacks original shared advice event")?;
        } else {
            let active = self
                .codex_active
                .get(&(d.project_id.clone(),d.attempt_id.clone()))''')
    return s
edit('supervision_delivery.rs',delivery)
def runner(s):
    s=s.replace('if let Some(active) = self.codex_active.get(project) {','for active in self.codex_active.values().filter(|a|a.job.project_id==project) {')
    s=s.replace('if let Some(active) = self.codex_active.get(&run.project_id) {','for active in self.codex_active.values().filter(|a|a.job.project_id==run.project_id) {')
    s=once(s,'''            self.poll_supervision_request(run)?;
            return Ok(());''','''            self.poll_supervision_request(run)?;
            if self.management.statuses().iter().any(|s|s.project_id==*project && s.purpose==Purpose::Advise && !s.state.resolved()) { return Ok(()); }''')
    s=once(s,'''        if self.has_codex_activity(project)
            || self.desktop_active''','''        if self.desktop_active''')
    return s
edit('project_runner.rs',runner)
def parallel(s):
    a=s.index('            if self.api_active.values().any');b=s.index('                return Ok(());',a)
    block=s[a:b]
    block=block.replace('            }) {','''            }) || self.codex_active.values().any(|a| a.job.project_id==*project
                && a.job.management_operation.as_deref()==Some(&pending.operation_id)) {''')
    s=s[:a]+block+s[b:]
    s=once(s,'''            } else {
                self.start_managed_api(&pending.operation_id, &attempt)''','''            } else if plan.task(task_id)?.adapter_id=="codex" {
                self.start_managed_codex(&pending.operation_id,&attempt)
            } else {
                self.start_managed_api(&pending.operation_id, &attempt)''')
    return s.replace('独立API或OMP任务','独立API、OMP或Codex任务')
edit('project_runner_parallel.rs',parallel)
for name in ['api_assignment_scope.rs','acceptance_scope.rs']:
    edit(name,lambda s:once(s,'| WorkflowCommand::CancelIntent { .. }','| WorkflowCommand::CancelIntent { .. }\n                        | WorkflowCommand::Advise { .. }'))
def verification(s):
    s=once(s,'''                    .omp_active
                    .contains_key(&(run.project_id.clone(), attempt.into()))''','''                    .omp_active
                    .contains_key(&(run.project_id.clone(), attempt.into()))
                || self.codex_active.contains_key(&(run.project_id.clone(), attempt.into()))''')
    s=once(s,'''                    .contains_key(&(project.into(), attempt.into())),''','''                    .contains_key(&(project.into(), attempt.into()))
                && !self.codex_active.contains_key(&(project.into(),attempt.into())),''')
    return s
edit('shared_verification_host.rs',verification)
