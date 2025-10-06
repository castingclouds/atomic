//! Atomic API Server Binary
//!
//! Standalone binary for running the Atomic VCS API server.
//! Designed to serve a single repository behind a Fastify reverse proxy.

use atomic_api::{
    ApiServer, HealthCheckHandler, RepositoryStatusHandler, ServerConfig, WebSocketServer,
};
use std::env;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging with DEBUG level by default
    // Override with RUST_LOG environment variable: RUST_LOG=info cargo run --bin atomic-api
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "debug");
    }
    tracing_subscriber::fmt::init();

    // Get base mount path from command line arguments
    let base_mount_path = env::args()
        .nth(1)
        .ok_or("Usage: atomic-api <base-mount-path>")?;

    // Get bind addresses from environment or use defaults
    let rest_bind_addr =
        env::var("ATOMIC_API_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let ws_bind_addr = env::var("ATOMIC_WS_BIND").unwrap_or_else(|_| "127.0.0.1:8081".to_string());

    println!("Starting Atomic API server with WebSocket support...");
    println!("Base mount path: {}", base_mount_path);
    println!("REST API bind address: {}", rest_bind_addr);
    println!("WebSocket bind address: {}", ws_bind_addr);
    println!("REST API routes:");
    println!("  /health");
    println!("  /tenant/<tenant_id>/portfolio/<portfolio_id>/project/<project_id>/changes");
    println!(
        "  /tenant/<tenant_id>/portfolio/<portfolio_id>/project/<project_id>/changes/<change_id>"
    );
    println!("WebSocket endpoints:");
    println!("  ws://{}/", ws_bind_addr);

    // Create REST API server
    let api_server = ApiServer::new(&base_mount_path).await?;

    // Create WebSocket server with configuration following AGENTS.md patterns
    let ws_config = ServerConfig::default();
    let ws_server = WebSocketServer::new(&ws_bind_addr, ws_config);

    // Register default message handlers following AGENTS.md configuration-driven design
    let health_handler = HealthCheckHandler;
    ws_server.state().register_handler(health_handler).await?;

    let repo_handler = RepositoryStatusHandler::new(&base_mount_path);
    ws_server.state().register_handler(repo_handler).await?;

    // Start both servers concurrently
    let api_server_task = {
        let bind_addr = rest_bind_addr.clone();
        tokio::spawn(async move {
            if let Err(e) = api_server.serve(&bind_addr).await {
                eprintln!("REST API server error: {}", e);
            }
        })
    };

    let ws_server_task = tokio::spawn(async move {
        if let Err(e) = ws_server.start().await {
            eprintln!("WebSocket server error: {}", e);
        }
    });

    println!("✅ Both REST API and WebSocket servers started");
    println!("REST API: http://{}", rest_bind_addr);
    println!("WebSocket: ws://{}", ws_bind_addr);

    // Wait for either server to complete (or fail)
    tokio::select! {
        result = api_server_task => {
            if let Err(e) = result {
                eprintln!("REST API server task failed: {}", e);
            }
        }
        result = ws_server_task => {
            if let Err(e) = result {
                eprintln!("WebSocket server task failed: {}", e);
            }
        }
    }

    Ok(())
}
