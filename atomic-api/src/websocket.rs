//! WebSocket server implementation for real-time communication with Atomic clients
//!
//! Following AGENTS.md patterns for configuration-driven design and error handling.
//! This provides the WebSocket infrastructure that will be extended by the atomic-workflow crate.

use crate::message::{Message, MessageHandler, MessagePayload, MessageRouter};
use crate::{ApiError, ApiResult};
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message as WsMessage};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// WebSocket connection wrapper following AGENTS.md patterns
#[derive(Debug)]
pub struct WebSocketConnection {
    /// Unique connection identifier
    pub id: Uuid,
    /// Optional user identifier for authentication
    pub user_id: Option<String>,
    /// Optional session identifier for grouping
    pub session_id: Option<Uuid>,
    /// Client address
    pub addr: SocketAddr,
    /// Connection metadata
    pub metadata: HashMap<String, String>,
}

impl WebSocketConnection {
    /// Factory method following AGENTS.md factory patterns
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            id: Uuid::new_v4(),
            user_id: None,
            session_id: None,
            addr,
            metadata: HashMap::new(),
        }
    }

    /// Builder pattern for setting user ID
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Builder pattern for setting session ID
    pub fn with_session_id(mut self, session_id: Uuid) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Add metadata to connection
    pub fn add_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.metadata.insert(key.into(), value.into());
    }
}

/// WebSocket server state following AGENTS.md configuration patterns
#[derive(Debug, Clone)]
pub struct ServerState {
    /// Configuration-driven message router
    pub message_router: Arc<RwLock<MessageRouter>>,
    /// Active connections
    pub connections: Arc<RwLock<HashMap<Uuid, WebSocketConnection>>>,
    /// Server configuration
    pub config: ServerConfig,
}

/// Server configuration following AGENTS.md configuration-driven design
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Maximum number of concurrent connections
    pub max_connections: usize,
    /// Connection timeout in seconds
    pub connection_timeout: u64,
    /// Enable connection logging
    pub enable_logging: bool,
    /// Custom configuration values
    pub custom: HashMap<String, String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_connections: 1000,
            connection_timeout: 300, // 5 minutes
            enable_logging: true,
            custom: HashMap::new(),
        }
    }
}

impl ServerState {
    /// Factory method following AGENTS.md factory patterns
    pub fn new(config: ServerConfig) -> Self {
        Self {
            message_router: Arc::new(RwLock::new(MessageRouter::new())),
            connections: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Register a message handler following AGENTS.md composition patterns
    pub async fn register_handler<H>(&self, handler: H) -> ApiResult<()>
    where
        H: MessageHandler + 'static,
    {
        let mut router = self.message_router.write().await;
        router
            .register_handler(handler)
            .map_err(|e| ApiError::internal(format!("Failed to register message handler: {}", e)))
    }

    /// Get connection count for health monitoring
    pub async fn connection_count(&self) -> usize {
        let connections = self.connections.read().await;
        connections.len()
    }

    /// Add connection to tracking
    pub async fn add_connection(&self, connection: WebSocketConnection) -> Uuid {
        let connection_id = connection.id;
        let mut connections = self.connections.write().await;

        if self.config.enable_logging {
            info!(
                "Adding WebSocket connection: {} from {}",
                connection_id, connection.addr
            );
        }

        connections.insert(connection_id, connection);
        connection_id
    }

    /// Remove connection from tracking
    pub async fn remove_connection(&self, connection_id: Uuid) {
        let mut connections = self.connections.write().await;

        if let Some(connection) = connections.remove(&connection_id) {
            if self.config.enable_logging {
                info!(
                    "Removing WebSocket connection: {} from {}",
                    connection_id, connection.addr
                );
            }
        }
    }
}

/// WebSocket server for handling client connections following AGENTS.md patterns
pub struct WebSocketServer {
    /// Server state
    state: ServerState,
    /// Server bind address
    bind_addr: String,
}

impl WebSocketServer {
    /// Factory method following AGENTS.md factory patterns
    pub fn new(bind_addr: impl Into<String>, config: ServerConfig) -> Self {
        Self {
            state: ServerState::new(config),
            bind_addr: bind_addr.into(),
        }
    }

    /// Get server state for external configuration
    pub fn state(&self) -> &ServerState {
        &self.state
    }

    /// Start the WebSocket server following AGENTS.md async patterns
    pub async fn start(self) -> ApiResult<()> {
        let listener = TcpListener::bind(&self.bind_addr).await.map_err(|e| {
            ApiError::internal(format!(
                "Failed to bind WebSocket server to {}: {}",
                self.bind_addr, e
            ))
        })?;

        info!("WebSocket server listening on {}", self.bind_addr);
        info!("Max connections: {}", self.state.config.max_connections);

        while let Ok((stream, addr)) = listener.accept().await {
            let state = self.state.clone();

            tokio::spawn(async move {
                // Check connection limits
                let current_connections = state.connection_count().await;
                if current_connections >= state.config.max_connections {
                    warn!(
                        "Maximum connections ({}) reached, rejecting connection from {}",
                        state.config.max_connections, addr
                    );
                    return;
                }

                // Handle the connection
                if let Err(e) = handle_connection(stream, addr, state).await {
                    error!("WebSocket connection error from {}: {}", addr, e);
                }
            });
        }

        Ok(())
    }
}

/// Handle individual WebSocket connection following AGENTS.md error handling patterns
async fn handle_connection(stream: TcpStream, addr: SocketAddr, state: ServerState) -> Result<()> {
    debug!("New WebSocket connection from {}", addr);

    // Accept WebSocket connection
    let ws_stream = accept_async(stream).await?;
    info!("WebSocket connection established from {}", addr);

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Create connection tracking
    let connection = WebSocketConnection::new(addr);
    let connection_id = state.add_connection(connection).await;

    // Handle incoming messages
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(WsMessage::Text(text)) => {
                debug!("Received text message from {}: {}", addr, text);

                // Parse message using configuration-driven approach
                match serde_json::from_str::<Message>(&text) {
                    Ok(message) => {
                        // Route message through configured handlers
                        let response = {
                            let mut router = state.message_router.write().await;
                            router.route_message(message).await
                        };

                        match response {
                            Ok(Some(response_msg)) => {
                                let response_text = serde_json::to_string(&response_msg)?;
                                if let Err(e) = ws_sender.send(WsMessage::Text(response_text)).await
                                {
                                    error!("Error sending WebSocket response to {}: {}", addr, e);
                                    break;
                                }
                            }
                            Ok(None) => {
                                debug!("Handler returned no response for message from {}", addr);
                            }
                            Err(e) => {
                                error!("Error handling message from {}: {}", addr, e);

                                // Send error response following AGENTS.md error handling
                                let error_response = Message::new(MessagePayload::Error(
                                    crate::message::ErrorMessage {
                                        error: format!("Server error: {}", e),
                                        code: Some("SERVER_ERROR".to_string()),
                                        details: None,
                                    },
                                ));

                                let error_text = serde_json::to_string(&error_response)?;
                                if let Err(e) = ws_sender.send(WsMessage::Text(error_text)).await {
                                    error!("Error sending error response to {}: {}", addr, e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse message from {}: {}", addr, e);

                        // Send parse error response
                        let error_response =
                            Message::new(MessagePayload::Error(crate::message::ErrorMessage {
                                error: "Invalid message format".to_string(),
                                code: Some("INVALID_MESSAGE".to_string()),
                                details: Some(serde_json::json!({"parse_error": e.to_string()})),
                            }));

                        let error_text = serde_json::to_string(&error_response)?;
                        if let Err(e) = ws_sender.send(WsMessage::Text(error_text)).await {
                            error!("Failed to send parse error response to {}: {}", addr, e);
                        }
                        break;
                    }
                }
            }
            Ok(WsMessage::Close(_)) => {
                debug!("WebSocket connection closed by client: {}", addr);
                break;
            }
            Ok(WsMessage::Ping(data)) => {
                if let Err(e) = ws_sender.send(WsMessage::Pong(data)).await {
                    error!("Failed to send pong to {}: {}", addr, e);
                    break;
                }
            }
            Ok(WsMessage::Pong(_)) => {
                // Acknowledge pong
                debug!("Received pong from {}", addr);
            }
            Ok(WsMessage::Binary(_)) => {
                warn!("Binary messages not supported from {}", addr);
            }
            Ok(WsMessage::Frame(_)) => {
                warn!("Raw frame messages not supported from {}", addr);
            }
            Err(e) => {
                error!("WebSocket error from {}: {}", addr, e);
                break;
            }
        }
    }

    // Clean up connection
    state.remove_connection(connection_id).await;
    debug!("WebSocket connection closed: {}", addr);
    Ok(())
}

/// Default message handler for health checks following AGENTS.md patterns
#[derive(Debug)]
pub struct HealthCheckHandler;

#[async_trait::async_trait]
impl MessageHandler for HealthCheckHandler {
    async fn handle_message(
        &mut self,
        message: Message,
    ) -> crate::message::MessageResult<Option<Message>> {
        match message.payload {
            MessagePayload::HealthCheck => {
                let response = Message::new(MessagePayload::HealthStatus(
                    crate::message::HealthStatusMessage {
                        healthy: true,
                        version: crate::VERSION.to_string(),
                        components: std::collections::HashMap::new(),
                        timestamp: chrono::Utc::now(),
                    },
                ));
                Ok(Some(message.reply(response.payload)))
            }
            _ => Ok(None), // Not handled by this handler
        }
    }

    fn message_types(&self) -> Vec<String> {
        vec!["health_check".to_string()]
    }
}

/// Basic repository status handler example
#[derive(Debug)]
pub struct RepositoryStatusHandler {
    base_path: std::path::PathBuf,
}

impl RepositoryStatusHandler {
    pub fn new(base_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }
}

#[async_trait::async_trait]
impl MessageHandler for RepositoryStatusHandler {
    async fn handle_message(
        &mut self,
        message: Message,
    ) -> crate::message::MessageResult<Option<Message>> {
        match message.payload {
            MessagePayload::RepositoryStatus(ref repo_msg) => {
                // Simple repository status check
                let repo_path = self.base_path.join(&repo_msg.repository);
                let exists = repo_path.exists();

                let mut metadata = std::collections::HashMap::new();
                metadata.insert("exists".to_string(), serde_json::Value::Bool(exists));
                metadata.insert(
                    "path".to_string(),
                    serde_json::Value::String(repo_path.to_string_lossy().to_string()),
                );

                let response = crate::message::RepositoryStatusMessage {
                    repository: repo_msg.repository.clone(),
                    status: if exists {
                        "available".to_string()
                    } else {
                        "not_found".to_string()
                    },
                    metadata,
                };

                Ok(Some(
                    message.reply(MessagePayload::RepositoryStatus(response)),
                ))
            }
            _ => Ok(None),
        }
    }

    fn message_types(&self) -> Vec<String> {
        vec!["repository_status".to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_connection_creation() {
        let addr = "127.0.0.1:8080".parse().unwrap();
        let connection = WebSocketConnection::new(addr)
            .with_user_id("test_user")
            .with_session_id(Uuid::new_v4());

        assert_eq!(connection.addr, addr);
        assert_eq!(connection.user_id, Some("test_user".to_string()));
        assert!(connection.session_id.is_some());
    }

    #[test]
    fn test_server_config_defaults() {
        let config = ServerConfig::default();
        assert_eq!(config.max_connections, 1000);
        assert_eq!(config.connection_timeout, 300);
        assert!(config.enable_logging);
    }

    #[test]
    fn test_server_state_creation() {
        let config = ServerConfig::default();
        let state = ServerState::new(config);

        // Verify initial state
        assert_eq!(state.config.max_connections, 1000);
    }

    #[tokio::test]
    async fn test_health_check_handler() {
        let mut handler = HealthCheckHandler;
        let message = Message::new(MessagePayload::HealthCheck);

        let response = handler.handle_message(message).await.unwrap();
        assert!(response.is_some());

        if let Some(response_msg) = response {
            assert!(matches!(
                response_msg.payload,
                MessagePayload::HealthStatus(_)
            ));
        }
    }

    #[test]
    fn test_handler_message_types() {
        let handler = HealthCheckHandler;
        let types = handler.message_types();
        assert_eq!(types, vec!["health_check"]);
    }
}
