use async_trait::async_trait;
use blockcell_core::{Error, Result};
use serde_json::{json, Value};

use crate::{Tool, ToolContext, ToolSchema};

/// Tool for managing core evolution workflows.
pub struct EvolutionWorkflowTool;

impl EvolutionWorkflowTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EvolutionWorkflowTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for EvolutionWorkflowTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "evolution_workflow",
            description: "Manage core evolution workflows: list, inspect, cancel, retry, or unblock capability evolution workflows",
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "get", "steps", "cancel", "retry", "unblock"],
                        "description": "Action to perform on evolution workflows"
                    },
                    "workflow_id": {
                        "type": "string",
                        "description": "Workflow ID (required for get, steps, cancel, retry)"
                    },
                    "capability_id": {
                        "type": "string",
                        "description": "Capability ID (required for unblock)"
                    },
                    "status_filter": {
                        "type": "string",
                        "description": "Filter workflows by status (optional, for list action)"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    fn validate(&self, params: &Value) -> Result<()> {
        let action = params
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Validation("Missing 'action' parameter".to_string()))?;

        match action {
            "list" => {}
            "get" | "steps" | "cancel" | "retry" => {
                if params.get("workflow_id").and_then(|v| v.as_str()).is_none() {
                    return Err(Error::Validation(format!(
                        "Missing 'workflow_id' parameter for action '{}'",
                        action
                    )));
                }
            }
            "unblock" => {
                if params
                    .get("capability_id")
                    .and_then(|v| v.as_str())
                    .is_none()
                {
                    return Err(Error::Validation(
                        "Missing 'capability_id' parameter for action 'unblock'".to_string(),
                    ));
                }
            }
            _ => {
                return Err(Error::Validation(format!(
                    "Unknown action '{}'. Valid: list, get, steps, cancel, retry, unblock",
                    action
                )));
            }
        }
        Ok(())
    }

    async fn execute(&self, ctx: ToolContext, params: Value) -> Result<Value> {
        let store = ctx
            .evolution_workflow_store
            .as_ref()
            .ok_or_else(|| Error::Tool("Evolution workflow store not available".to_string()))?;

        let action = params.get("action").and_then(|v| v.as_str()).unwrap_or("");

        match action {
            "list" => {
                let status_filter = params.get("status_filter").and_then(|v| v.as_str());
                store.list_workflows_json(status_filter)
            }
            "get" => {
                let workflow_id = params
                    .get("workflow_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                store.get_workflow_json(workflow_id)
            }
            "steps" => {
                let workflow_id = params
                    .get("workflow_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                store.get_workflow_steps_json(workflow_id)
            }
            "cancel" => {
                let workflow_id = params
                    .get("workflow_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                store.cancel_workflow(workflow_id)
            }
            "retry" => {
                let workflow_id = params
                    .get("workflow_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                store.retry_workflow(workflow_id)
            }
            "unblock" => {
                let capability_id = params
                    .get("capability_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                store.unblock_capability(capability_id)
            }
            _ => Err(Error::Tool(format!("Unknown action '{}'", action))),
        }
    }
}
