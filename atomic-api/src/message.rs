//! Message types for Atomic API WebSocket communication following AGENTS.md patterns
//!
//! Provides basic WebSocket message infrastructure that can be extended by configuration.
//! Workflow definitions and states will be loaded from configuration, not defined in code.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Result type for message handling operations
pub type MessageResult<T> = Result<T, MessageError>;

/// Base message envelope for all WebSocket communication following AGENTS.md patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique message identifier
    pub id: Uuid,
    /// Message timestamp
    pub timestamp: DateTime<Utc>,
    /// Optional sender identifier
    pub sender: Option<String>,
    /// Optional recipient identifier
    pub recipient: Option<String>,
    /// Optional correlation ID for request/response pairing
    pub correlation_id: Option<Uuid>,
    /// Message payload containing the actual data
    pub payload: MessagePayload,
}

impl Message {
    /// Factory method following AGENTS.md factory patterns
    pub fn new(payload: MessagePayload) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            sender: None,
            recipient: None,
            correlation_id: None,
            payload,
        }
    }

    /// Builder pattern methods following AGENTS.md best practices
    pub fn with_sender(mut self, sender: impl Into<String>) -> Self {
        self.sender = Some(sender.into());
        self
    }

    pub fn with_recipient(mut self, recipient: impl Into<String>) -> Self {
        self.recipient = Some(recipient.into());
        self
    }

    pub fn with_correlation_id(mut self, correlation_id: Uuid) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    /// Create a reply message following AGENTS.md patterns
    pub fn reply(&self, payload: MessagePayload) -> Self {
        Message::new(payload)
            .with_correlation_id(self.id)
            .with_recipient(self.sender.clone().unwrap_or_default())
    }
}

/// Message payload types - configuration-driven, not hardcoded workflows
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum MessagePayload {
    // System Messages
    HealthCheck,
    HealthStatus(HealthStatusMessage),

    // Generic Configuration Messages - workflows loaded from config
    LoadWorkflows(LoadWorkflowsMessage),
    WorkflowsLoaded(WorkflowsLoadedMessage),

    // Generic State Management - states defined in configuration
    StateTransition(StateTransitionMessage),
    StateChanged(StateChangedMessage),

    // Repository Operations
    RepositoryStatus(RepositoryStatusMessage),
    ChangeStatusUpdate(ChangeStatusMessage),

    // Generic Data Messages
    Data(DataMessage),

    // Response Messages
    Success(SuccessMessage),
    Error(ErrorMessage),

    // Connection Management
    Subscribe(SubscribeMessage),
    Unsubscribe(UnsubscribeMessage),

    // Broadcast Messages
    Broadcast(BroadcastMessage),
}

/// Health status message following AGENTS.md patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatusMessage {
    pub healthy: bool,
    pub version: String,
    pub components: HashMap<String, ComponentHealth>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub healthy: bool,
    pub message: Option<String>,
    pub last_check: DateTime<Utc>,
}

/// Load workflows from configuration message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadWorkflowsMessage {
    /// Optional path to workflow configurations
    pub config_path: Option<String>,
    /// Whether to reload existing workflows
    pub reload: bool,
}

/// Workflows loaded response with configuration-driven data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowsLoadedMessage {
    /// Number of workflows loaded from configuration
    pub workflow_count: usize,
    /// Workflow names loaded from config
    pub workflow_names: Vec<String>,
    /// Configuration source
    pub config_source: String,
}

/// Generic state transition message - states defined in configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransitionMessage {
    /// Resource identifier (e.g., change hash)
    pub resource_id: String,
    /// Current state identifier (from configuration)
    pub from_state: String,
    /// Target state identifier (from configuration)
    pub to_state: String,
    /// Action identifier (from configuration)
    pub action: String,
    /// Actor performing the transition
    pub actor: String,
    /// Additional context data
    pub context: HashMap<String, serde_json::Value>,
}

/// State changed notification message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChangedMessage {
    /// Resource identifier
    pub resource_id: String,
    /// Previous state (from configuration)
    pub old_state: String,
    /// New state (from configuration)
    pub new_state: String,
    /// Action that caused the change (from configuration)
    pub action: String,
    /// Actor who performed the change
    pub actor: String,
    /// Change timestamp
    pub timestamp: DateTime<Utc>,
}

/// Repository status message following AGENTS.md patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryStatusMessage {
    /// Repository path or identifier
    pub repository: String,
    /// Current status
    pub status: String,
    /// Additional repository metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Change status update message for repository changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeStatusMessage {
    /// Change identifier (hash)
    pub change_id: String,
    /// Repository identifier
    pub repository: String,
    /// Current status (from configuration)
    pub status: String,
    /// Status metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Generic data message for extensibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMessage {
    /// Data type identifier
    pub data_type: String,
    /// Arbitrary data payload
    pub data: serde_json::Value,
    /// Data metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Success response message following AGENTS.md error handling strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessMessage {
    /// Success message
    pub message: String,
    /// Optional success data
    pub data: Option<serde_json::Value>,
}

/// Error response message following AGENTS.md error handling strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMessage {
    /// Error message
    pub error: String,
    /// Optional error code
    pub code: Option<String>,
    /// Optional error details
    pub details: Option<serde_json::Value>,
}

/// Subscribe to message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeMessage {
    /// Message types to subscribe to
    pub message_types: Vec<String>,
    /// Optional filter criteria
    pub filters: HashMap<String, serde_json::Value>,
}

/// Unsubscribe from message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeMessage {
    /// Message types to unsubscribe from
    pub message_types: Vec<String>,
}

/// Broadcast message to multiple recipients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastMessage {
    /// Target recipients (empty = broadcast to all)
    pub recipients: Vec<String>,
    /// Message to broadcast
    pub message: Box<Message>,
}

/// Message handling errors following AGENTS.md error patterns
#[derive(Debug, thiserror::Error)]
pub enum MessageError {
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Invalid message format: {message}")]
    InvalidFormat { message: String },

    #[error("Handler not found for message type: {message_type}")]
    HandlerNotFound { message_type: String },

    #[error("Configuration error: {message}")]
    Configuration { message: String },
}

/// Trait for handling messages following AGENTS.md trait-based design
#[async_trait::async_trait]
pub trait MessageHandler: Send + Sync + std::fmt::Debug {
    /// Handle a message and optionally return a response
    async fn handle_message(&mut self, message: Message) -> MessageResult<Option<Message>>;

    /// Get the message types this handler can process
    fn message_types(&self) -> Vec<String>;
}

/// Message router for dispatching messages to appropriate handlers
/// Following AGENTS.md composition patterns
pub struct MessageRouter {
    handlers: HashMap<String, Box<dyn MessageHandler>>,
}

impl std::fmt::Debug for MessageRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageRouter")
            .field("handler_count", &self.handlers.len())
            .field("handler_types", &self.handlers.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl MessageRouter {
    /// Factory method following AGENTS.md patterns
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a message handler for specific message types
    pub fn register_handler<H>(&mut self, handler: H) -> MessageResult<()>
    where
        H: MessageHandler + 'static,
    {
        let message_types = handler.message_types();
        let handler = Box::new(handler);

        // For now, each handler handles only its first message type
        // Future improvement: support multiple message types per handler
        if let Some(message_type) = message_types.first() {
            self.handlers.insert(message_type.clone(), handler);
        }

        Ok(())
    }

    /// Route a message to the appropriate handler
    pub async fn route_message(&mut self, message: Message) -> MessageResult<Option<Message>> {
        let message_type = self.get_message_type(&message.payload);

        if let Some(handler) = self.handlers.get_mut(&message_type) {
            handler.handle_message(message).await
        } else {
            Err(MessageError::HandlerNotFound { message_type })
        }
    }

    /// Extract message type from payload following AGENTS.md patterns
    fn get_message_type(&self, payload: &MessagePayload) -> String {
        match payload {
            MessagePayload::HealthCheck => "health_check".to_string(),
            MessagePayload::HealthStatus(_) => "health_status".to_string(),
            MessagePayload::LoadWorkflows(_) => "load_workflows".to_string(),
            MessagePayload::WorkflowsLoaded(_) => "workflows_loaded".to_string(),
            MessagePayload::StateTransition(_) => "state_transition".to_string(),
            MessagePayload::StateChanged(_) => "state_changed".to_string(),
            MessagePayload::RepositoryStatus(_) => "repository_status".to_string(),
            MessagePayload::ChangeStatusUpdate(_) => "change_status_update".to_string(),
            MessagePayload::Data(data) => format!("data_{}", data.data_type),
            MessagePayload::Success(_) => "success".to_string(),
            MessagePayload::Error(_) => "error".to_string(),
            MessagePayload::Subscribe(_) => "subscribe".to_string(),
            MessagePayload::Unsubscribe(_) => "unsubscribe".to_string(),
            MessagePayload::Broadcast(_) => "broadcast".to_string(),
        }
    }
}

impl Default for MessageRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation_follows_agents_md_patterns() {
        let payload = MessagePayload::HealthCheck;
        let message = Message::new(payload.clone())
            .with_sender("client")
            .with_recipient("server");

        assert_eq!(message.sender, Some("client".to_string()));
        assert_eq!(message.recipient, Some("server".to_string()));
        assert!(matches!(message.payload, MessagePayload::HealthCheck));
    }

    #[test]
    fn test_message_reply_correlation() {
        let original = Message::new(MessagePayload::HealthCheck).with_sender("client");

        let reply = original.reply(MessagePayload::Success(SuccessMessage {
            message: "Health check ok".to_string(),
            data: None,
        }));

        assert_eq!(reply.correlation_id, Some(original.id));
        assert_eq!(reply.recipient, Some("client".to_string()));
    }

    #[test]
    fn test_configuration_driven_state_transition() {
        // States and actions come from configuration, not hardcoded
        let transition = StateTransitionMessage {
            resource_id: "change-123".to_string(),
            from_state: "recorded".to_string(), // From config
            to_state: "submitted".to_string(),  // From config
            action: "submit".to_string(),       // From config
            actor: "user@example.com".to_string(),
            context: HashMap::new(),
        };

        let message = Message::new(MessagePayload::StateTransition(transition));
        assert!(matches!(
            message.payload,
            MessagePayload::StateTransition(_)
        ));
    }

    #[test]
    fn test_message_router_creation() {
        let router = MessageRouter::new();
        assert_eq!(router.handlers.len(), 0);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let message = Message::new(MessagePayload::Success(SuccessMessage {
            message: "Test".to_string(),
            data: None,
        }));

        let json = serde_json::to_string(&message).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(message.id, deserialized.id);
        assert!(matches!(deserialized.payload, MessagePayload::Success(_)));
    }
}
