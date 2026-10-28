//! Durable workflow proof shared by idle and coordinated management readers.
use crate::scheduler::DurableWorkflow;
use sha2::{Digest, Sha256};
use std::io::Write;

pub(crate) struct OriginalBasis {
    pub(crate) version: String,
}

struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn original_basis(
    store: &DurableWorkflow,
    project_id: &str,
) -> Result<OriginalBasis, String> {
    cerulean_domain::execution::identifier(project_id)?;
    if store.requires_reopen() {
        return Err("原项目写入尚未核对，请保留记录并重开".into());
    }
    if !store
        .configuration()
        .execution_scope
        .as_ref()
        .is_some_and(|scope| scope.project.id == project_id)
    {
        return Err("原项目账本身份与指挥请求项目不符".into());
    }
    let mut hash = HashWriter(Sha256::new());
    serde_json::to_writer(
        &mut hash,
        &(
            "workspace-management-original-basis-v1",
            project_id,
            store.configuration(),
            store.state(),
            store.revision(),
            store.approval(),
            store.history(),
        ),
    )
    .map_err(|error| error.to_string())?;
    Ok(OriginalBasis {
        version: format!("{:x}", hash.0.finalize()),
    })
}
