"""One-time mechanical migration; original public dispatcher wrappers stay intact."""
from pathlib import Path
root = Path(__file__).resolve().parents[1]
def edit(name, transform):
    p = root / name
    old = p.read_text(encoding='utf-8-sig')
    new = transform(old)
    assert new != old, name
    p.write_text(new, encoding='utf-8')
def once(s, a, b):
    assert s.count(a) == 1, a
    return s.replace(a, b)
edit('crates/core/src/lib.rs', lambda s: once(s, 'pub mod codex_execution;', 'pub mod codex_execution;\nmod codex_workflow_access;'))
def coordinator(s):
    s = once(s, 'mod omp;', 'mod omp;\n#[path = "coordinated_codex.rs"]\nmod codex;')
    s = once(s, '    Omp,\n}', '    Omp,\n    Codex,\n}')
    start = s.index('    ApplyOmp {')
    end = s.index('    Accept {', start)
    s = s[:start] + s[start:end].replace('ApplyOmp', 'ApplyCodex').replace('operation: Option<String>', 'operation: String') + '''    CodexCancellationAnchors {
        scope: Scope,
        reply: mpsc::Sender<Result<StartedCancellationAnchors, String>>,
    },
''' + s[start:]
    start = s.index('                    Message::ApplyOmp {')
    end = s.index('                    Message::Accept {', start)
    block = s[start:end].replace('ApplyOmp', 'ApplyCodex').replace('omp::validate', 'codex::validate').replace('omp::command(&scope,&store,&command,&context)', 'codex::command(&scope,&store,&operation,&command,&context)').replace('OMP', 'Codex')
    block = block.replace('                            let operation=operation.unwrap_or_else(||format!("omp-event-{}",store.history().len()+1));\n', '')
    s = s[:start] + '''                    Message::CodexCancellationAnchors {scope,reply} => {
                        let result=(|| {
                            require(!fault && !store.requires_reopen(),"协调器账本需要重开核对")?;
                            codex::validate(&scope,&store)?;
                            store.started_cancellation_anchors(&scope.task,&scope.attempt)
                        })();
                        let _=reply.send(result);
                    }
''' + block + s[start:]
    s = once(s, '        let message = if self.kind == ClientKind::Omp {', '''        let message = if self.kind == ClientKind::Codex {
            Message::ApplyCodex {
                scope: self.scope.clone(), cursor: self.cursor(),
                operation: operation.ok_or("Codex命令必须保留原操作ID")?,
                principal: principal.into(), require_new, command,
                context: Box::new(context), reply,
            }
        } else if self.kind == ClientKind::Omp {''')
    return s
edit('crates/core/src/workflow_coordinator.rs', coordinator)
def execution(s):
    s = once(s, '    artifacts::ArtifactObserver,', '    artifacts::ArtifactObserver,\n    codex_workflow_access::CodexWorkflowAccess,')
    s = once(s, '// Static dispatch permits', '''/// Opaque access to a scoped native worker. The resident host retains the journal.
pub struct WorkerWorkflow<'a> {
    pub(crate) store: &'a mut dyn CodexWorkflowAccess,
}

// Static dispatch permits''')
    at = s.index('    async fn read(\n')
    s = s[:at] + '''    async fn wait_scoped(
        &mut self,
        _store: &mut WorkerWorkflow<'_>,
        thread: &str,
        turn: &str,
        timeout: Duration,
        cancel: &mut tokio::sync::oneshot::Receiver<String>,
    ) -> Result<WaitOutcome, String> {
        self.wait_or_cancel(thread, turn, timeout, cancel).await
    }
''' + s[at:]
    # All private helpers and the generic engine share evidence validation.
    start = s.index('fn role(')
    prefix, body = s[:start], s[start:]
    body = body.replace('&mut DurableWorkflow', '&mut (impl CodexWorkflowAccess + ?Sized)').replace('&DurableWorkflow', '&(impl CodexWorkflowAccess + ?Sized)')
    # Keep public APIs and their callback type source compatible.
    a = body.index('pub async fn dispatch<C:')
    b = body.index('async fn dispatch_inner<C:', a)
    body = body[:a] + body[a:b].replace('&mut (impl CodexWorkflowAccess + ?Sized)', '&mut DurableWorkflow').replace('&(impl CodexWorkflowAccess + ?Sized)', '&DurableWorkflow') + body[b:]
    body = once(body, 'async fn dispatch_inner<C: WorkerChannel>(\n    store: &mut (impl CodexWorkflowAccess + ?Sized),', 'pub(crate) async fn dispatch_inner<C: WorkerChannel, S: CodexWorkflowAccess>(\n    store: &mut S,')
    body = once(body, '    mut progress: impl FnMut(&(impl CodexWorkflowAccess + ?Sized)),', '    mut progress: impl FnMut(&S),')
    a = body.index('    let sends = store')
    b = body.index('    let prompt = approved_prompt', a)
    body = body[:a] + '    crate::codex_workflow_access::require_budget(store)?;\n' + body[b:]
    body = once(body, '''            channel
                .wait_supervised(store, &binding.thread_id, &turn, limit, cancel)
                .await?''', '''            if let Some(direct) = store.direct() {
                channel.wait_supervised(direct, &binding.thread_id, &turn, limit, cancel).await?
            } else {
                channel.wait_scoped(&mut WorkerWorkflow { store }, &binding.thread_id, &turn, limit, cancel).await?
            }''')
    body = once(body, '''pub async fn reconcile<C: WorkerChannel>(
    store: &mut (impl CodexWorkflowAccess + ?Sized),''', '''pub async fn reconcile<C: WorkerChannel>(
    store: &mut DurableWorkflow,
    channel: &mut C,
    task: &str,
    id: &str,
) -> Result<TurnEvidence, String> {
    reconcile_scoped(store, channel, task, id).await
}
pub(crate) async fn reconcile_scoped<C: WorkerChannel>(
    store: &mut (impl CodexWorkflowAccess + ?Sized),''')
    return prefix + body
edit('crates/core/src/codex_execution.rs', execution)
def supervised(s):
    s = once(s, '    codex_execution::WorkerChannel,', '    codex_execution::{WorkerChannel, WorkerWorkflow},\n    codex_workflow_access::CodexWorkflowAccess,')
    s = once(s, '        store: &mut DurableWorkflow,', '        store: &mut (impl CodexWorkflowAccess + ?Sized),') if s.count('        store: &mut DurableWorkflow,') == 1 else s.replace('        store: &mut DurableWorkflow,', '        store: &mut (impl CodexWorkflowAccess + ?Sized),', 1)
    a = s.index('    async fn wait_supervised(')
    b = s.index('    async fn interrupt(', a)
    block = s[a:b]
    generic = block.replace('async fn wait_supervised(', 'async fn wait_with_advice(').replace('store: &mut DurableWorkflow', 'store: &mut (impl CodexWorkflowAccess + ?Sized)')
    s = s[:a] + '''    async fn wait_supervised(&mut self, store: &mut DurableWorkflow, t: &str, u: &str,
        d: Duration, cancel: &mut tokio::sync::oneshot::Receiver<String>) -> Result<WaitOutcome, String> {
        self.wait_with_advice(store, t, u, d, cancel).await
    }
    async fn wait_scoped(&mut self, store: &mut WorkerWorkflow<'_>, t: &str, u: &str,
        d: Duration, cancel: &mut tokio::sync::oneshot::Receiver<String>) -> Result<WaitOutcome, String> {
        self.wait_with_advice(store.store, t, u, d, cancel).await
    }
''' + s[b:]
    return once(s, "impl SupervisedWorker<'_> {", "impl SupervisedWorker<'_> {\n" + generic)
edit('crates/core/src/supervised_worker.rs', supervised)
def asynchronous(s):
    s = once(s, '    host::ordinary,', '    host::ordinary,\n    codex_workflow_access::CodexWorkflowAccess,\n    workflow_access::WorkflowAccess,\n    workflow_coordinator::{Scope, WorkflowCoordinator},')
    s = s.replace('store: &DurableWorkflow', 'store: &(impl WorkflowAccess + ?Sized)')
    s = s.replace('store: &mut DurableWorkflow', 'store: &mut impl CodexWorkflowAccess')
    a = s.index('    let count = store', s.index('pub(crate) fn preflight('))
    b = s.index('    Ok(worker)', a)
    s = s[:a] + '    crate::codex_workflow_access::require_budget(store)?;\n' + s[b:]
    s = once(s, '    mut store: DurableWorkflow,', '    store: DurableWorkflow,')
    s = once(s, '    let config = store.configuration().clone();\n', '')
    s = once(s, '    let path = root.join("workflows").join(&job.project_id);', '''    let path = root.join("workflows").join(&job.project_id);
    let mut coordinator = WorkflowCoordinator::start(store, path)?;
    let mut store = coordinator.codex_client(Scope {
        project: job.project_id.clone(), task: job.task_id.clone(),
        attempt: job.attempt_id.clone(), revision: job.revision_digest.clone(),
    })?;''')
    a = s.index('            let restored = if store.requires_reopen() || panicked {')
    b = s.index('            let _ = sender.send(Update::Done', a)
    s = s[:a] + '''            // The worker drops its capability before the sole owner is returned.
            drop(store);
            let restored = coordinator.finish(panicked);
''' + s[b:]
    s = s.replace('store.prepare_task_workspace_cancellable(', 'store.prepare_codex_workspace(')
    s = s.replace('crate::codex_execution::reconcile(', 'crate::codex_execution::reconcile_scoped(')
    s = s.replace('crate::codex_execution::dispatch_controlled(', 'crate::codex_execution::dispatch_inner(')
    s = once(s, '                cancellation,\n                |store|', '                Some(cancellation),\n                |store|')
    return s
edit('crates/core/src/async_codex.rs', asynchronous)
