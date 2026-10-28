//! Only the host coordinator can issue this capability. API worker clients have
//! no method for producing Verify; their original command allowlist is unchanged.
use super::*;

pub(crate) struct VerificationClient {
    lease: WorkflowClient,
    prepared: Option<crate::scheduler::PreparedVerification>,
    basis: crate::verification_detail::Basis,
    publication_uncertain: bool,
}
impl WorkflowCoordinator {
    pub(crate) fn verification_client(
        &self,
        scope: Scope,
        job: VerificationJob,
    ) -> Result<VerificationClient, String> {
        let lease = self.client(scope.clone())?;
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::BeginVerification { scope, job, reply })
            .map_err(|_| "协调器已停止")?;
        let (prepared, basis) = receive(response)??;
        Ok(VerificationClient {
            lease,
            prepared: Some(prepared),
            basis,
            publication_uncertain: false,
        })
    }
}
impl VerificationClient {
    pub(crate) fn uncertain(&self) -> bool {
        self.publication_uncertain || self.lease.failed
    }
    pub(crate) fn snapshot(&self) -> ExecutionSnapshot {
        self.lease.snapshot.clone()
    }
    pub(crate) fn basis(&self) -> crate::verification_detail::Basis {
        self.basis.clone()
    }
    pub(crate) fn run(
        &mut self,
        pins: &[crate::verifier::PinnedCheck],
        cancel: &AtomicBool,
    ) -> Result<Entry, String> {
        require(!self.lease.failed, "独立验收回执丢失，禁止重试")?;
        let prepared = self.prepared.take().ok_or("独立验收已启动，禁止重复运行")?;
        // Execution and its immutable intent/result remain the original verifier.
        let report = prepared.run(pins, cancel)?;
        let (reply, response) = mpsc::channel();
        self.publication_uncertain = true;
        self.lease
            .sender
            .send(Message::FinishVerification {
                scope: self.lease.scope.clone(),
                cursor: self.lease.cursor(),
                prepared: Box::new(prepared),
                report: Box::new(report),
                reply,
            })
            .map_err(|_| "独立验收已执行但协调器不可用，须核对原结果")?;
        let (entry, _) = self.lease.absorb(response)?;
        let entry = entry.ok_or("独立验收缺少原入账确认")?;
        self.publication_uncertain = false;
        Ok(entry)
    }
}
