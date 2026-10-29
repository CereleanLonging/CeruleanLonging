from pathlib import Path
root=Path(__file__).resolve().parents[1]/'crates/core/src'
def edit(name,f):
    p=root/name;s=p.read_text(encoding='utf-8-sig');n=f(s);assert n!=s,name;p.write_text(n,encoding='utf-8')
def once(s,a,b):
    assert s.count(a)==1,a
    return s.replace(a,b)
for name in ['application_recovery.rs','codex_cancellation.rs','management_application.rs','supervision_requests.rs','unsent_assignment.rs']:
    edit(name,lambda s:once(s,'host.codex_active.insert(\n            "project".into(),','host.codex_active.insert(\n            (job.project_id.clone(),job.attempt_id.clone()),'))
edit('resource_leases.rs',lambda s:once(s,'host.codex_active.insert(\n            "project".into(),','host.codex_active.insert(\n            ("project".into(),attempt.id.clone()),').replace('host.codex_active.remove("project")','host.codex_active.remove(&("project".into(),attempt.id.clone()))'))
edit('startup_recovery.rs',lambda s:once(s,'host.codex_active.insert(\n            "project".into(),','host.codex_active.insert(\n            ("project".into(),"startup-native".into()),').replace('host.codex_active.remove("project")','host.codex_active.remove(&("project".into(),"startup-native".into()))'))
edit('application_recovery.rs',lambda s:once(s,'host.codex_active.remove("project")','host.codex_active.remove(&("project".into(),"prepared-native".into()))'))
edit('unsent_assignment.rs',lambda s:once(s,'host.codex_active.remove("project")','host.codex_active.clear()'))
edit('codex_worktree_tests.rs',lambda s:s.replace('host.codex_active["project"]','host.codex_active[&("project".into(),"attempt".into())]'))
edit('project_dag_tests.rs',lambda s:s.replace('host.codex_active[&job.project_id]','host.codex_active[&(job.project_id.clone(),job.attempt_id.clone())]'))
import re
edit('supervision_requests.rs',lambda s:re.sub(r'(host\.codex_active|host\.codex_active\s*)\.get_mut\("project"\)',r'\1.get_mut(&("project".into(),"advice-attempt".into()))',s).replace('.get_mut("project")\n', '.get_mut(&("project".into(),"advice-attempt".into()))\n').replace('host.codex_active["project"]','host.codex_active[&("project".into(),"advice-attempt".into())]'))
def management(s):
    s=s.replace('host.codex_active["project"]','host.codex_active[&("project".into(),"attempt-1".into())]')
    a=s.index('        let thread = std::thread::spawn(move || {',s.index('    fn simulated_worker('))
    s=s[:a]+'''        let writer=crate::async_api::ApiWorkflow::start(store,host.root.join("workflows/project")).unwrap();
        let mut store=writer.owner.codex_client(crate::workflow_coordinator::Scope {
            project:job.project_id.clone(),task:job.task_id.clone(),attempt:job.attempt_id.clone(),revision:job.revision_digest.clone(),
        }).unwrap();
        host.api_writers.insert("project".into(),writer);
''' + s[a:]
    s=once(s,'            let mut store = store;\n','')
    s=once(s,'''            sender
                .send(Update::Done {
                    store: Ok(Box::new(store)),''','''            use crate::codex_workflow_access::CodexWorkflowAccess;
            let interrupted=store.requires_reopen();
            drop(store);
            sender
                .send(Update::Done {
                    interrupted,
                    cleaned:true,''')
    return s
edit('management_application.rs',management)
edit('startup_journal.rs',lambda s:once(s,'        assert!(invalid.validate().is_err());\n        journal.begin(scoped.clone()).unwrap();','        assert!(invalid.validate().is_ok());\n        assert!(journal.begin(invalid).is_err());\n        journal.begin(scoped.clone()).unwrap();'))
