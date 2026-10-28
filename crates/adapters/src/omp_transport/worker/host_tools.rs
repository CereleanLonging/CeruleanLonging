//! Typed OMP v18.4.4 host-tool registration. This grants no execution by itself.
use super::*;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}
impl HostToolDefinition {
    pub fn validate(&self) -> Result<(), String> {
        identifier(&self.name)?;
        require(
            self.name.starts_with("cerulean_") && self.name.len() <= 80,
            "OMP host tool must use workbench namespace",
        )?;
        require(
            !self.description.trim().is_empty()
                && self.description.len() <= 4096
                && !self.description.contains('\0'),
            "OMP host tool description invalid",
        )?;
        require(
            self.parameters.is_object()
                && self.parameters["type"] == "object"
                && self.parameters["properties"].is_object()
                && self.parameters["additionalProperties"] == false,
            "OMP host tool schema must be a closed object",
        )?;
        require(
            serde_json::to_vec(&self.parameters)
                .map_err(|_| "OMP host tool schema invalid")?
                .len()
                <= 32768,
            "OMP host tool schema exceeds limit",
        )
    }
}

impl<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin> WorkerTransport<R, W> {
    /// One immutable registration per fresh channel. No prompt is sent; native
    /// tools must be absent. The trusted host still owes approval and handlers.
    pub async fn register_host_tools(
        &mut self,
        identity: &SessionIdentity,
        tools: &[HostToolDefinition],
    ) -> Result<(), String> {
        require(
            !self.consumed && self.rpc.connected && !self.rpc.broken && self.host_tools.is_none(),
            "OMP host tool registration requires a fresh unregistered channel",
        )?;
        require(
            !tools.is_empty() && tools.len() <= 8,
            "OMP host tool count out of bounds",
        )?;
        let mut names = BTreeSet::new();
        for tool in tools {
            tool.validate()?;
            require(
                names.insert(tool.name.clone()),
                "OMP host tool name repeated",
            )?;
        }
        // Validate everything before any new wire operation. A dropped/failed
        // registration cannot be retried on this connection.
        self.rpc.broken = true;
        let before = self
            .rpc
            .response(
                json!({"type":"get_state"}),
                Instant::now() + Duration::from_secs(10),
            )
            .await?;
        require(
            idle_bound(&before, identity)?
                && before["dumpTools"].as_array().is_some_and(Vec::is_empty),
            "OMP host registration requires native tools absent",
        )?;
        let ack = self
            .rpc
            .response(
                json!({"type":"set_host_tools","tools":tools}),
                Instant::now() + Duration::from_secs(10),
            )
            .await?;
        let ack_names = ack["toolNames"]
            .as_array()
            .ok_or("OMP host registration acknowledgement missing")?;
        require(
            ack_names.len() == tools.len()
                && ack_names
                    .iter()
                    .all(|v| v.as_str().is_some_and(|s| names.contains(s)))
                && ack_names
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == names.len(),
            "OMP host registration acknowledgement differs",
        )?;
        let after = self
            .rpc
            .response(
                json!({"type":"get_state"}),
                Instant::now() + Duration::from_secs(10),
            )
            .await?;
        require(
            idle_bound(&after, identity)?,
            "OMP registered session changed or became active",
        )?;
        validate_catalog(&after, tools)?;
        self.host_tools = Some(tools.to_vec());
        self.rpc.broken = false;
        Ok(())
    }
    pub fn registered_host_tools(&self) -> Option<&[HostToolDefinition]> {
        self.host_tools.as_deref()
    }
}

pub(super) fn validate_catalog(state: &Value, tools: &[HostToolDefinition]) -> Result<(), String> {
    let advertised = state["dumpTools"]
        .as_array()
        .ok_or("OMP registered tool directory missing")?;
    require(
        advertised.len() == tools.len(),
        "OMP registered tool count differs",
    )?;
    for tool in tools {
        let observed = advertised
            .iter()
            .filter(|v| v["name"] == tool.name)
            .collect::<Vec<_>>();
        require(
            observed.len() == 1
                && observed[0]["description"] == tool.description
                && observed[0]["parameters"] == tool.parameters,
            "OMP registered tool schema differs from host",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
