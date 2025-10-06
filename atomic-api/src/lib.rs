//! # Atomic API - REST API Server for Atomic VCS Repository Operations
//!
//! This crate provides a focused REST API server that exposes Atomic VCS operations
//! for a single repository. It's designed to be used behind a Fastify reverse proxy
//! for multi-tenant SaaS deployments.
//!
//! ## Architecture
//!
//! Following AGENTS.md principles, this crate provides:
//! - **Direct Rust Integration**: No FFI/DLL overhead, pure Rust crate usage
//! - **Single Responsibility**: Focus only on Atomic VCS API operations
//! - **Minimal Dependencies**: Only essential dependencies for API functionality
//! - **Error Handling Strategy**: Comprehensive error types with context
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use atomic_api::ApiServer;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let base_mount_path = std::env::args().nth(1)
//!         .expect("Usage: atomic-api <base-mount-path>");
//!
//!     let server = ApiServer::new(base_mount_path).await?;
//!     server.serve("127.0.0.1:8080").await?;
//!     Ok(())
//! }
//! ```

#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]

// Re-exports following AGENTS.md patterns for clean public API
pub use crate::error::{ApiError, ApiResult};
pub use crate::message::{Message, MessageHandler, MessagePayload, MessageRouter};
pub use crate::server::ApiServer;
pub use crate::websocket::{
    HealthCheckHandler, RepositoryStatusHandler, ServerConfig, ServerState, WebSocketServer,
};

// Core modules following AGENTS.md code organization patterns
pub mod error;
pub mod message;
pub mod server;
pub mod websocket;

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_info() {
        assert!(!VERSION.is_empty());
    }
}
