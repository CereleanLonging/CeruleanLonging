"""One-time source migration; never run against already migrated sources."""
from pathlib import Path
root=Path(__file__).resolve().parents[1]
def edit(name, f):
    p=root/'crates/core/src'/name
    s=p.read_text(encoding='utf-8-sig'); n=f(s)
    assert n!=s,name
    p.write_text(n,encoding='utf-8')
def once(s,a,b):
    assert s.count(a)==1,a
    return s.replace(a,b)
# Explicit project-wide guards retain their meaning after activity keys change.
import re
for p in (root/'crates/core/src').rglob('*.rs'):
    if p.name=='codex_shared_host.rs': continue
    s=p.read_text(encoding='utf-8-sig')
    n=re.sub(r'(self|host)\.codex_active\.contains_key\(([^\n]+?)\)',r'\1.has_codex_activity(\2)',s)
    if n!=s:p.write_text(n,encoding='utf-8')
def asynchronous(s):
    s=s.replace('workflow_coordinator::{Scope, WorkflowCoordinator}', 'workflow_coordinator::WorkflowClient')
    s=once(s,'        store: Result<Box<DurableWorkflow>, String>,','        interrupted: bool,\n        cleaned: bool,')
    s=once(s,'    store: DurableWorkflow,','    mut store: WorkflowClient,')
    a=s.index('    let path = root.join("workflows").join(&job.project_id);',s.index('pub(crate) fn launch('))
    b=s.index('    let root = root.to_path_buf();',a)
    s=s[:a]+s[b:]
    s=once(s,'        .spawn(move || {','        .spawn(move || {\n            let mut cleaned = true;')
    s=once(s,'                        preparing,','                        preparing,\n                        &mut cleaned,')
    s=once(s,'            let restored = coordinator.finish(interrupted);\n','')
    s=once(s,'                store: restored.map(Box::new),','                interrupted,\n                cleaned: cleaned && !panicked,')
    s=once(s,'    preparing: Arc<AtomicBool>,','    preparing: Arc<AtomicBool>,\n    cleaned: &mut bool,')
    s=once(s,'    let mut client = launch(', '    let mut client = launch(')
    a=s.index('    let outcome = async {',s.index('async fn run('))
    s=s[:a]+'    *cleaned = false;\n'+s[a:]
    s=once(s,'    let cleanup = client.close().await;','    let cleanup = client.close().await;\n    *cleaned = cleanup.is_ok();')
    s=s.replace('scheduler::{CommandContext, DurableWorkflow}', 'scheduler::CommandContext')
    return s
edit('async_codex.rs',asynchronous)
def host(s):
    s=once(s,'mod omp_shared_host;','mod omp_shared_host;\n#[cfg(windows)]\n#[path = "codex_shared_host.rs"]\nmod codex_shared_host;')
    s=once(s,'codex_active: BTreeMap<String, crate::async_codex::ActiveCodex>','codex_active: BTreeMap<(String, String), crate::async_codex::ActiveCodex>')
    s=once(s,'        workflows\n            .extend(self.codex_active.values().map(|a| a.snapshot.clone()));','') if '        workflows\n            .extend(self.codex_active.values().map(|a| a.snapshot.clone()));' in s else s
    s=re.sub(r'        workflows\.extend\(self\.codex_active.values\(\).map\(\|a\| a.snapshot.clone\(\)\)\);\n','',s)
    s=s.replace('&& !self.has_codex_activity(project)\n                &&', '&& (!self.has_codex_activity(project) || allow_api)\n                &&',1)
    a=s.index('    fn start_codex_inner(');b=s.index('    fn reconcile_codex(',a)
    block=s[a:b].replace('self.require_available(project)?','self.require_available_inner(project, true)?')
    block=once(block,'        let store = self.projects.get(project).ok_or("project not registered")?;', '''        let view = self.codex_read(project, task, attempt)?;
        let store = &*view;''')
    block=once(block,'        let now = chrono::Utc::now().to_rfc3339();','''        let scoped = management_operation.is_some()
            && api_assignment_scope::isolated(store.revision().ok_or("Codex方案缺失")?, task)?;
        if self.api_writers.contains_key(project) { require(scoped, "Codex并行启动需要批准的独立目录与原管理决定")?; }
        let anchor = if scoped {
            self.validate_api_assignment_scope(management_operation.as_deref().unwrap(), store, task,
                store.history().len() as u32, true)?.sequence
        } else { store.history().len() as u32 };
        let now = chrono::Utc::now().to_rfc3339();''')
    block=block.replace('        let store = self.projects.get(project).ok_or("project unavailable")?;\n','')
    block=once(block,'        self.startups.begin(crate::startup_journal::StartupIntent {','        let intent = crate::startup_journal::StartupIntent {')
    block=once(block,'            schema_version: 1,','            schema_version: if scoped { 2 } else { 1 },')
    block=once(block,'''            workflow_sequence: u32::try_from(store.history().len())
                .map_err(|_| "workflow history limit")?,
            workflow_hash: store.history().last().map(|e| e.hash.clone()),''','''            workflow_sequence: anchor,
            workflow_hash: store.history().get(anchor.checked_sub(1).ok_or("Codex批准锚点缺失")? as usize).map(|e|e.hash.clone()),''')
    block=once(block,'        })?;\n        self.launch_codex_job(job, false)','        };\n        drop(view);\n        self.startups.begin(intent)?;\n        self.launch_codex_job(job, false)')
    s=s[:a]+block+s[b:]
    a=s.index('    fn launch_codex_job(');b=s.index('    #[cfg(windows)]\n    fn poll_codex_workers',a)
    s=s[:a]+'''    fn launch_codex_job(&mut self, job: CodexJob, recovery: bool) -> Result<(), String> {
        let project = job.project_id.clone();
        let key = (project.clone(), job.attempt_id.clone());
        crate::async_codex::save(&self.root, &job)?;
        self.codex_jobs.insert(key.clone(), job.clone());
        let mut new_config = None;
        let launched = (|| {
            if !self.api_writers.contains_key(&project) {
                let store = self.projects.remove(&project).ok_or("Codex项目不可用")?;
                new_config = Some(store.configuration().clone());
                let writer = crate::async_api::ApiWorkflow::start(store, self.root.join("workflows").join(&project))?;
                self.api_writers.insert(project.clone(), writer);
            }
            let client = self.api_writers[&project].owner.codex_client(crate::workflow_coordinator::Scope {
                project:project.clone(),task:job.task_id.clone(),attempt:job.attempt_id.clone(),revision:job.revision_digest.clone(),
            })?;
            crate::async_codex::launch(&self.root,client,job.clone(),recovery,
                #[cfg(test)] self.codex_prepare_gate.take())
        })();
        match launched {
            Ok(active) => { self.codex_active.insert(key,active); Ok(()) }
            Err(error) => {
                if let Some(config)=new_config {
                    let store=match self.api_writers.remove(&project) {
                        Some(mut writer)=>writer.owner.finish(false),
                        None=>DurableWorkflow::open(&self.root.join("workflows").join(&project),config),
                    }?;
                    self.projects.insert(project,store);
                }
                let mut failed=job;
                failed.state=CodexJobState::Failed; failed.message=Some(error.clone());
                self.codex_jobs.insert(key,failed.clone());
                let _=crate::async_codex::save(&self.root,&failed);
                Err(error)
            }
        }
    }
''' + s[b:]
    a=s.index('    fn poll_codex_workers(');b=s.index('    pub fn poll_verifications(',a)
    block=s[a:b]
    block=block.replace('for (id, active) in &mut self.codex_active {','for ((project, attempt), active) in &mut self.codex_active {')
    block=block.replace('assignments.push((id.clone(), update));','assignments.push((project.clone(), attempt.clone(), update));')
    block=block.replace('(id.clone(), active.job.attempt_id.clone())','(project.clone(), active.job.attempt_id.clone())')
    block=once(block,'                        active.snapshot = *snapshot;','''                        if let Some(writer) = self.api_writers.get(project) {
                            if let Ok(authoritative)=writer.owner.snapshot() { active.snapshot=authoritative; }
                        } else if snapshot.sequence >= active.snapshot.sequence { active.snapshot=*snapshot; }''')
    block=once(block,'                    Ok(Update::Done { store, outcome }) => {\n                        ready.push((id.clone(), store, outcome));','                    Ok(Update::Done { interrupted, cleaned, outcome }) => {\n                        ready.push((project.clone(), attempt.clone(), interrupted, cleaned, outcome));')
    a1=block.index('                        ready.push((\n',block.index('Err(std::sync::mpsc::TryRecvError::Disconnected)'))
    b1=block.index('                        break;',a1)
    block=block[:a1]+'''                        active.job.state=CodexJobState::Unknown;
                        active.job.message=Some("Codex后台通道中断，需要重开核查".into());
                        self.codex_jobs.insert((project.clone(),attempt.clone()),active.job.clone());
''' + block[b1:]
    block=block.replace('for (project, update) in assignments {','for (project, attempt, update) in assignments {').replace('self.handle_advice_update(&project, update)','self.handle_advice_update(&project, &attempt, update)')
    a1=block.index('        for (id, store, outcome) in ready {')
    block=block[:a1]+'''        for (project, attempt, interrupted, cleaned, outcome) in ready {
            let key=(project.clone(),attempt.clone());
            let mut active=self.codex_active.remove(&key).expect("active Codex worker");
            crate::worker_observation::persist(&self.root,&mut active);
            let mut job=active.job.clone();
            job.updated_at=chrono::Utc::now().to_rfc3339();
            let stopping=matches!(job.state,CodexJobState::CancelRequested|CodexJobState::Cancelling);
            if !cleaned {
                job.state=CodexJobState::Unknown;
                job.message=Some("Codex进程退出尚未确认，保留原活动与账本所有者".into());
                if let Some(writer)=self.api_writers.get_mut(&project) { writer.interrupted=true; }
                active.disconnected=true; active.job=job.clone(); self.codex_active.insert(key.clone(),active);
            } else if let Err(error)=self.finish_codex_owner(&project,&mut job,interrupted,outcome) {
                job.state=CodexJobState::Unknown; job.message=Some(error.clone());
                self.api_fault=Some(error); active.disconnected=true; active.job=job.clone();
                self.codex_active.insert(key.clone(),active);
            } else if job.state==CodexJobState::Cancelled {
                job.message=Some("原会话已确认本次执行停止；已有修改保留，后续调度仍暂停".into());
            } else if stopping && job.state==CodexJobState::Submitted {
                job.message=Some("任务已先完成，结果已保留并等待独立验收；后续调度仍暂停".into());
            }
            if let Err(error)=crate::async_codex::save(&self.root,&job) {
                self.api_fault=Some(error); job.state=CodexJobState::Unknown;
                job.message=Some("执行状态保存失败，请重开核心对账".into());
            }
            self.codex_jobs.insert(key,job);
        }
    }
'''
    return s[:a]+block+s[b:]
edit('host.rs',host)
# Shared owner must remain while any native worker owns a capability.
for name in ['omp_shared_host.rs','api_host.rs','shared_verification_host.rs']:
    edit(name,lambda s: s.replace('|| self.has_omp_activity(project)', '|| self.has_omp_activity(project)\n            || self.has_codex_activity(project)').replace('|| self.has_omp_activity(&project)', '|| self.has_omp_activity(&project)\n                || self.has_codex_activity(&project)'))
edit('api_assignment_scope.rs',lambda s:once(s,'plan.task(task)?.adapter_id == "omp"', 'matches!(plan.task(task)?.adapter_id.as_str(), "omp" | "codex")'))
edit('omp_shared_host.rs',lambda s:once(s,'        if adapter == "omp" {','''        if adapter == "codex" {
            let record=self.startups.get(project,attempt).ok_or("Codex启动证据缺失")?;
            if record.intent.schema_version==2 {
                return Ok(Some((record.intent.workflow_sequence,record.intent.workflow_hash.clone().ok_or("Codex历史锚点缺失")?)));
            }
        } else if adapter == "omp" {'''))
edit('startup_journal.rs',lambda s:once(s,'                    && self.protocol == OMP_PROTOCOL','                    && matches!(self.protocol.as_str(), PROTOCOL | OMP_PROTOCOL)'))
def bridge(s):
    a=s.index('        let revision_digest = self');b=s.index('        self.start_codex_inner(',a)
    s=s[:a]+'''        let revision_digest = self.codex_read(&project,&task,attempt)?.revision().ok_or("approved revision missing")?.approval_digest()?;
''' + s[b:]
    s=once(s,'                    .get(project)\n                    .ok_or("managed Codex worker missing")?', '''                    .values().find(|a| a.job.project_id==project && a.job.management_operation.as_deref()==Some(operation))
                    .ok_or("managed Codex worker missing")?''')
    s=once(s,'"codex" => self.codex_active.get_mut(project).unwrap().snapshot = snapshot,','"codex" => self.codex_active.get_mut(&(project.into(), scope.attempt_id.clone())).unwrap().snapshot = snapshot,')
    return s
edit('codex_management_bridge.rs',bridge)
def startup(s):
    s=once(s,'intent.schema_version == 1 && intent.protocol == "codex-durable-before-send-v1"','matches!(intent.schema_version, 1 | 2) && intent.protocol == "codex-durable-before-send-v1"')
    s=once(s,'                "启动意图与实际分配绑定不符")?;','''                "启动意图与实际分配绑定不符")?;
            if intent.schema_version==2 {
                self.validate_api_assignment_scope(intent.management_operation.as_deref().ok_or("Codex管理归属缺失")?,store,&intent.task_id,
                    entry.body.event.sequence.checked_sub(1).ok_or("Codex分配序号无效")?,false)?;
            }''')
    s=once(s,'            let management = self.management.intent(operation)?;','''            let management = self.management.intent(operation)?;
            if intent.schema_version==2 {
                let original=self.validate_api_assignment_scope(operation,store,&intent.task_id,intent.workflow_sequence,false)?;
                require(original.sequence==intent.workflow_sequence,"Codex启动与原管理锚点不符")?;
            }''')
    s=once(s,'                .get(project)\n                .and_then(|active| active.snapshot.revision.as_ref())','                .get(&(project.into(),attempt.into()))\n                .and_then(|active| active.snapshot.revision.as_ref())')
    return s
edit('startup_recovery.rs',startup)
