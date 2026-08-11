use crate::PhxCommandV1;
use async_channel::Sender;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const AGENT_CONTROL_SCHEMA_V1: &str = "phoenix.agent-control/v1";
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub const MAX_INLINE_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentControlRequestV1 {
    pub schema: String,
    pub command_id: String,
    pub command: String,
}

impl AgentControlRequestV1 {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            schema: AGENT_CONTROL_SCHEMA_V1.to_string(),
            command_id: uuid::Uuid::new_v4().to_string(),
            command: command.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentControlStatusV1 {
    Ok,
    Conflict,
    Denied,
    Busy,
    Error,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AgentControlResponseV1 {
    pub schema: String,
    pub command_id: String,
    pub canonical_command: String,
    pub status: AgentControlStatusV1,
    pub sequence: u64,
    pub kernel_revision: u64,
    pub workspace_revision: u64,
    pub document_revision: Option<u64>,
    pub replayed: bool,
    pub payload: Value,
    pub error: Option<String>,
}

impl AgentControlResponseV1 {
    pub fn error(request: &AgentControlRequestV1, error: impl Into<String>) -> Self {
        Self {
            schema: AGENT_CONTROL_SCHEMA_V1.to_string(),
            command_id: request.command_id.clone(),
            canonical_command: request.command.trim().to_string(),
            status: AgentControlStatusV1::Error,
            sequence: 0,
            kernel_revision: 0,
            workspace_revision: 0,
            document_revision: None,
            replayed: false,
            payload: Value::Null,
            error: Some(error.into()),
        }
    }
}

pub struct HostRequest {
    pub request: AgentControlRequestV1,
    pub command: PhxCommandV1,
    pub reply: Sender<AgentControlResponseV1>,
}
