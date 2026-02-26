use anyhow::Result;
use serde_json::Value;
use artificer_shared::tools::get_tool_schema;
use super::schema::task::is_task_tool;

/// Validate a tool call before execution.
///
/// Checks that the tool exists in the registry and all required
/// parameters are present in `args`. Task tools bypass schema
/// validation — they are always considered valid here.
pub fn validate_tool_call(tool_name: &str, args: &Value) -> Result<()> {
    // Task tools are handled internally — always valid at this stage
    if is_task_tool(tool_name) {
        return Ok(());
    }

    let schema = get_tool_schema(tool_name)
        .map_err(|_| anyhow::anyhow!("Unknown tool: '{}'", tool_name))?;

    for param in &schema.parameters {
        if param.required && args.get(param.name).is_none() {
            return Err(anyhow::anyhow!(
                "Tool '{}' missing required parameter '{}'",
                tool_name,
                param.name
            ));
        }
    }

    Ok(())
}
