//! API ownership uses the same protected file operations and original write
//! evidence, with an API scope in the tool ledger; never an invented OMP scope.
use super::*;
use crate::api_execution::ApiRunIdentity;
use cerulean_adapters::api_generation::{ApiTool, ToolCall, ToolResult};
pub(crate) struct ApiFileTools {
    root: Arc<Root>,
    journal: OmpToolJournal<ApiRunIdentity>,
    identity: ApiRunIdentity,
    errors: crate::api_tool_checkpoint::ErrorTexts,
}
impl ApiFileTools {
    pub fn create(
        plan: &PlanRevision,
        approval: &Approval,
        user: &str,
        identity: &ApiRunIdentity,
        path: &Path,
    ) -> Result<Self, String> {
        approval.validate(plan, user, plan.policy.version)?;
        identity.validate_plan(plan)?;
        let mut file_plan = plan.clone();
        file_plan.policy.tools = identity.worker.file_tools.clone();
        let root = file_root(
            &file_plan,
            identity
                .worker
                .write_scope
                .as_ref()
                .map(|_| {
                    plan.task(&identity.task_id)
                        .map(|t| t.artifact_paths.as_slice())
                })
                .transpose()?,
            &identity.binding.workspace,
            path,
            digest(identity)?,
        )?;
        let journal = OmpToolJournal::create(&path.join("events"), identity)?;
        Ok(Self {
            root,
            journal,
            identity: identity.clone(),
            errors: Default::default(),
        })
    }
    pub fn definitions(&self) -> Vec<ApiTool> {
        file_definitions(&self.root)
            .into_iter()
            .map(|t| ApiTool {
                name: t.name,
                description: t.description,
                parameters: t.parameters,
            })
            .collect()
    }
    pub fn head(&self) -> &str {
        self.journal.head()
    }
    pub fn unresolved(&self) -> usize {
        self.journal.observation().unresolved()
    }
    pub(crate) fn checkpoint(&self) -> Result<crate::api_tool_checkpoint::ObservedTools, String> {
        Ok(crate::api_tool_checkpoint::ObservedTools {
            checkpoint: self.journal.checkpoint()?,
            errors: self.errors.clone(),
        })
    }
    pub fn execute(&mut self, ordinal: u32, call: &ToolCall) -> Result<ToolResult, String> {
        require(
            self.definitions().iter().any(|t| t.name == call.name),
            "API file tool not approved",
        )?;
        let id = format!("api-tool-{ordinal}");
        let event = HostEvent::Dispatch {
            id: id.clone(),
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            arguments_sha256: digest(&call.arguments)?,
        };
        self.journal.record(&self.identity, &event)?;
        let result = match call.name.as_str() {
            "cerulean_read_file" if self.root.read => read_file(
                &self.root,
                serde_json::from_value(call.arguments.clone())
                    .map_err(|_| "Invalid read arguments"),
            ),
            "cerulean_write_file" if self.root.write => write_file(
                &self.root,
                &id,
                &digest(&event)?,
                serde_json::from_value(call.arguments.clone())
                    .map_err(|_| "Invalid write arguments"),
            ),
            _ => Err(FileError::Rejected("File permission denied".into())),
        };
        let output = match result {
            Ok(text) => ToolResult {
                call_id: call.id.clone(),
                text,
                is_error: false,
            },
            Err(FileError::Rejected(text)) => ToolResult {
                call_id: call.id.clone(),
                text,
                is_error: true,
            },
            Err(FileError::Unknown) => {
                return Err("API file outcome unknown; retain original evidence".into())
            }
        };
        self.journal.record(
            &self.identity,
            &HostEvent::Result {
                id,
                result_sha256: hash(output.text.as_bytes()),
                is_error: output.is_error,
            },
        )?;
        // Keep only bounded failure text already hashed by the original journal.
        // Successful file contents and write payloads never enter this projection.
        if output.is_error && output.text.len() <= crate::api_tool_checkpoint::MAX_ERROR_BYTES {
            self.errors.insert(call.id.clone(), output.text.clone());
        }
        Ok(output)
    }
}
