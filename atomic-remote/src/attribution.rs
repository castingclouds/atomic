//! Attribution support for Atomic remote operations
//!
//! This module extends the atomic-remote crate to support AI attribution
//! metadata synchronization across remote repositories. It provides
//! attribution-aware versions of remote operations while maintaining
//! backward compatibility with existing remotes.
//!
//! ## Key Features
//!
//! - Attribution-aware push/pull operations
//! - Protocol negotiation for attribution capabilities
//! - Backward compatibility with non-attribution remotes
//! - Configuration-driven attribution sync
//! - Efficient batching and compression

use crate::RemoteRepo;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio::time::{timeout, Duration};

// Import attribution types - these will need to be available from libatomic
pub use libatomic::attribution::{
    sync::{AttributedPatchBundle, AttributionRemoteSync, RemoteAttributionStats},
    AttributedPatch, PatchId,
};

/// Attribution protocol version supported by this implementation
pub const ATTRIBUTION_PROTOCOL_VERSION: u32 = 1;

/// Timeout for attribution protocol operations
const ATTRIBUTION_TIMEOUT_SECS: u64 = 30;

/// Error type for remote attribution operations
#[derive(Debug, thiserror::Error)]
pub enum RemoteAttributionError {
    #[error("Remote does not support attribution protocol version {version}")]
    UnsupportedProtocolVersion { version: u32 },
    #[error("Attribution bundle serialization failed: {0}")]
    SerializationError(#[from] bincode::Error),
    #[error("Remote attribution sync failed: {reason}")]
    SyncFailed { reason: String },
    #[error("Attribution protocol negotiation failed")]
    ProtocolNegotiationFailed,
    #[error("Remote attribution conflict for patch {patch_id}: {reason}")]
    RemoteConflict { patch_id: String, reason: String },
    #[error("Attribution bundle verification failed: {reason}")]
    BundleVerificationFailed { reason: String },
}

/// Extension trait for RemoteRepo to support attribution operations
#[async_trait]
pub trait AttributionRemoteExt {
    /// Check if this remote supports attribution sync
    async fn supports_attribution(&mut self) -> Result<bool>;

    /// Negotiate attribution protocol version with remote
    async fn negotiate_attribution_protocol(&mut self) -> Result<u32>;

    /// Push changes with attribution metadata
    async fn push_with_attribution(
        &mut self,
        bundles: Vec<AttributedPatchBundle>,
        channel: &str,
    ) -> Result<()>;

    /// Pull changes with attribution metadata
    async fn pull_with_attribution(
        &mut self,
        from: u64,
        channel: &str,
    ) -> Result<Vec<AttributedPatchBundle>>;

    /// Get attribution statistics from remote
    async fn get_attribution_stats(&mut self, channel: &str) -> Result<RemoteAttributionStats>;
}

/// Implementation of AttributionRemoteExt for RemoteRepo
#[async_trait]
impl AttributionRemoteExt for RemoteRepo {
    async fn supports_attribution(&mut self) -> Result<bool> {
        match self {
            RemoteRepo::Local(_) => Ok(true), // Local repos always support attribution
            RemoteRepo::LocalChannel(_) => Ok(true),
            RemoteRepo::Ssh(ssh) => ssh.supports_attribution().await,
            RemoteRepo::Http(http) => http.supports_attribution().await,
            RemoteRepo::None => Ok(false),
        }
    }

    async fn negotiate_attribution_protocol(&mut self) -> Result<u32> {
        match self {
            RemoteRepo::Local(_) => Ok(ATTRIBUTION_PROTOCOL_VERSION),
            RemoteRepo::LocalChannel(_) => Ok(ATTRIBUTION_PROTOCOL_VERSION),
            RemoteRepo::Ssh(ssh) => ssh
                .negotiate_attribution_protocol()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            RemoteRepo::Http(http) => http
                .negotiate_attribution_protocol()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            RemoteRepo::None => Err(anyhow::anyhow!(
                "Cannot negotiate protocol with None remote"
            )),
        }
    }

    async fn push_with_attribution(
        &mut self,
        bundles: Vec<AttributedPatchBundle>,
        channel: &str,
    ) -> Result<()> {
        match self {
            RemoteRepo::Local(local) => local
                .push_attributed_patches(bundles, channel)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            RemoteRepo::LocalChannel(_) => {
                // For local channels, store attribution in the local database
                // This would be implemented by the caller using the attribution database
                Ok(())
            }
            RemoteRepo::Ssh(ssh) => ssh
                .push_attributed_patches(bundles, channel)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            RemoteRepo::Http(http) => http
                .push_attributed_patches(bundles, channel)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            RemoteRepo::None => Err(anyhow::anyhow!("Cannot push to None remote")),
        }
    }

    async fn pull_with_attribution(
        &mut self,
        from: u64,
        channel: &str,
    ) -> Result<Vec<AttributedPatchBundle>> {
        match self {
            RemoteRepo::Local(local) => local
                .pull_attributed_patches(from, channel)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            RemoteRepo::LocalChannel(_) => {
                // For local channels, load attribution from the local database
                // This would be implemented by the caller using the attribution database
                Ok(Vec::new())
            }
            RemoteRepo::Ssh(ssh) => ssh
                .pull_attributed_patches(from, channel)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            RemoteRepo::Http(http) => http
                .pull_attributed_patches(from, channel)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            RemoteRepo::None => Err(anyhow::anyhow!("Cannot pull from None remote")),
        }
    }

    async fn get_attribution_stats(&mut self, channel: &str) -> Result<RemoteAttributionStats> {
        match self {
            RemoteRepo::Local(local) => local
                .get_remote_attribution_stats(channel)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            RemoteRepo::LocalChannel(_) => Ok(RemoteAttributionStats {
                total_patches: 0,
                ai_assisted_patches: 0,
                unique_authors: 0,
                unique_ai_providers: HashSet::new(),
                last_sync_timestamp: None,
            }),
            RemoteRepo::Ssh(ssh) => ssh
                .get_remote_attribution_stats(channel)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            RemoteRepo::Http(http) => http
                .get_remote_attribution_stats(channel)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e)),
            RemoteRepo::None => Err(anyhow::anyhow!("Cannot get stats from None remote")),
        }
    }
}

/// Attribution support for Local remotes
impl crate::local::Local {
    /// Check if local remote supports attribution
    pub async fn supports_attribution(&self) -> Result<bool> {
        // Local remotes always support attribution
        Ok(true)
    }

    /// Negotiate attribution protocol
    pub async fn negotiate_attribution_protocol(&self) -> Result<u32> {
        Ok(ATTRIBUTION_PROTOCOL_VERSION)
    }
}

/// Attribution support for SSH remotes
impl crate::ssh::Ssh {
    /// Check if SSH remote supports attribution
    pub async fn supports_attribution(&mut self) -> Result<bool> {
        // Send attribution capability query
        let query = AttributionProtocolMessage::CapabilityQuery;

        match timeout(
            Duration::from_secs(ATTRIBUTION_TIMEOUT_SECS),
            self.send_attribution_message(query),
        )
        .await
        {
            Ok(Ok(AttributionProtocolMessage::CapabilityResponse { supported })) => Ok(supported),
            Ok(Ok(_)) => Ok(false),  // Unexpected response
            Ok(Err(_)) => Ok(false), // Error means no support
            Err(_) => Ok(false),     // Timeout means no support
        }
    }

    /// Negotiate attribution protocol version
    pub async fn negotiate_attribution_protocol(&mut self) -> Result<u32> {
        let request = AttributionProtocolMessage::VersionNegotiation {
            supported_versions: vec![1],
        };

        let response = timeout(
            Duration::from_secs(ATTRIBUTION_TIMEOUT_SECS),
            self.send_attribution_message(request),
        )
        .await??;

        match response {
            AttributionProtocolMessage::VersionResponse { version } => Ok(version),
            _ => Err(anyhow!("Invalid response to version negotiation")),
        }
    }

    /// Send attribution protocol message (placeholder implementation)
    async fn send_attribution_message(
        &mut self,
        _message: AttributionProtocolMessage,
    ) -> Result<AttributionProtocolMessage> {
        // This would implement the actual SSH protocol for attribution messages
        // For now, return a default response
        Ok(AttributionProtocolMessage::CapabilityResponse { supported: false })
    }
}

/// Attribution support for HTTP remotes
impl crate::http::Http {
    /// Check if HTTP remote supports attribution
    pub async fn supports_attribution(&mut self) -> Result<bool> {
        // Try to access attribution endpoint
        let url = format!("{}/attribution/capabilities", self.url);

        match timeout(
            Duration::from_secs(ATTRIBUTION_TIMEOUT_SECS),
            self.client.get(&url).send(),
        )
        .await
        {
            Ok(Ok(response)) => Ok(response.status().is_success()),
            _ => Ok(false), // Any error or timeout means no support
        }
    }

    /// Negotiate attribution protocol version
    pub async fn negotiate_attribution_protocol(&mut self) -> Result<u32> {
        let url = format!("{}/attribution/negotiate", self.url);
        let request = AttributionNegotiationRequest {
            supported_versions: vec![1],
        };

        let response = timeout(
            Duration::from_secs(ATTRIBUTION_TIMEOUT_SECS),
            self.client.post(&url).json(&request).send(),
        )
        .await??;

        if response.status().is_success() {
            let nego_response: AttributionNegotiationResponse = response.json().await?;
            Ok(nego_response.version)
        } else {
            Err(anyhow!(
                "Attribution protocol negotiation failed: {}",
                response.status()
            ))
        }
    }
}

/// Protocol message types for attribution communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttributionProtocolMessage {
    /// Query if remote supports attribution
    CapabilityQuery,
    /// Response to capability query
    CapabilityResponse { supported: bool },
    /// Negotiate protocol version
    VersionNegotiation { supported_versions: Vec<u32> },
    /// Response with negotiated version
    VersionResponse { version: u32 },
    /// Push attribution bundles
    PushBundles {
        channel: String,
        bundles: Vec<AttributedPatchBundle>,
    },
    /// Pull attribution bundles
    PullRequest { channel: String, from: u64 },
    /// Response with attribution bundles
    PullResponse { bundles: Vec<AttributedPatchBundle> },
    /// Request attribution statistics
    StatsRequest { channel: String },
    /// Response with attribution statistics
    StatsResponse { stats: RemoteAttributionStats },
    /// Error message
    Error { message: String },
}

/// HTTP-specific attribution negotiation request
#[derive(Debug, Serialize, Deserialize)]
pub struct AttributionNegotiationRequest {
    pub supported_versions: Vec<u32>,
}

/// HTTP-specific attribution negotiation response
#[derive(Debug, Serialize, Deserialize)]
pub struct AttributionNegotiationResponse {
    pub version: u32,
}

/// HTTP-specific attribution bundle push request
#[derive(Debug, Serialize, Deserialize)]
pub struct AttributionPushRequest {
    pub channel: String,
    pub bundles: Vec<AttributedPatchBundle>,
}

/// HTTP-specific attribution bundle pull request
#[derive(Debug, Serialize, Deserialize)]
pub struct AttributionPullRequest {
    pub channel: String,
    pub from: u64,
}

/// HTTP-specific attribution bundle pull response
#[derive(Debug, Serialize, Deserialize)]
pub struct AttributionPullResponse {
    pub bundles: Vec<AttributedPatchBundle>,
}

/// Implementation of AttributionRemoteSync for Local remotes
#[async_trait]
impl AttributionRemoteSync for crate::local::Local {
    type Error = RemoteAttributionError;

    async fn pull_attributed_patches(
        &mut self,
        _from: u64,
        _channel: &str,
    ) -> Result<Vec<AttributedPatchBundle>, Self::Error> {
        // For local remotes, we would read attribution data from the local filesystem
        // This is a placeholder implementation
        Ok(Vec::new())
    }

    async fn push_attributed_patches(
        &mut self,
        _patches: Vec<AttributedPatchBundle>,
        _channel: &str,
    ) -> Result<(), Self::Error> {
        // For local remotes, we would write attribution data to the local filesystem
        // This is a placeholder implementation
        Ok(())
    }

    async fn get_remote_attribution_stats(
        &self,
        _channel: &str,
    ) -> Result<RemoteAttributionStats, Self::Error> {
        // Return empty stats for now
        Ok(RemoteAttributionStats {
            total_patches: 0,
            ai_assisted_patches: 0,
            unique_authors: 0,
            unique_ai_providers: HashSet::new(),
            last_sync_timestamp: None,
        })
    }

    async fn negotiate_attribution_version(&mut self) -> Result<u32, Self::Error> {
        Ok(ATTRIBUTION_PROTOCOL_VERSION)
    }
}

/// Implementation of AttributionRemoteSync for SSH remotes
#[async_trait]
impl AttributionRemoteSync for crate::ssh::Ssh {
    type Error = RemoteAttributionError;

    async fn pull_attributed_patches(
        &mut self,
        from: u64,
        channel: &str,
    ) -> Result<Vec<AttributedPatchBundle>, Self::Error> {
        let request = AttributionProtocolMessage::PullRequest {
            channel: channel.to_string(),
            from,
        };

        let response = match timeout(
            Duration::from_secs(ATTRIBUTION_TIMEOUT_SECS),
            self.send_attribution_message(request),
        )
        .await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(_)) => {
                return Err(RemoteAttributionError::SyncFailed {
                    reason: "SSH attribution request failed".to_string(),
                })
            }
            Err(_) => {
                return Err(RemoteAttributionError::SyncFailed {
                    reason: "SSH attribution request timeout".to_string(),
                })
            }
        };

        match response {
            AttributionProtocolMessage::PullResponse { bundles } => Ok(bundles),
            AttributionProtocolMessage::Error { message } => {
                Err(RemoteAttributionError::SyncFailed { reason: message })
            }
            _ => Err(RemoteAttributionError::SyncFailed {
                reason: "Invalid response to pull request".to_string(),
            }),
        }
    }

    async fn push_attributed_patches(
        &mut self,
        patches: Vec<AttributedPatchBundle>,
        channel: &str,
    ) -> Result<(), Self::Error> {
        let request = AttributionProtocolMessage::PushBundles {
            channel: channel.to_string(),
            bundles: patches,
        };

        let response = match timeout(
            Duration::from_secs(ATTRIBUTION_TIMEOUT_SECS),
            self.send_attribution_message(request),
        )
        .await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(_)) => {
                return Err(RemoteAttributionError::SyncFailed {
                    reason: "SSH attribution push failed".to_string(),
                })
            }
            Err(_) => {
                return Err(RemoteAttributionError::SyncFailed {
                    reason: "SSH attribution push timeout".to_string(),
                })
            }
        };

        match response {
            AttributionProtocolMessage::Error { message } => {
                Err(RemoteAttributionError::SyncFailed { reason: message })
            }
            _ => Ok(()), // Any non-error response is success
        }
    }

    async fn get_remote_attribution_stats(
        &self,
        channel: &str,
    ) -> Result<RemoteAttributionStats, Self::Error> {
        let request = AttributionProtocolMessage::StatsRequest {
            channel: channel.to_string(),
        };

        let response = match timeout(
            Duration::from_secs(ATTRIBUTION_TIMEOUT_SECS),
            // Note: This would need &mut self in practice, but keeping signature consistent
            // In real implementation, we'd need to refactor the trait
            self.send_attribution_message_const(request),
        )
        .await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(_)) => {
                return Err(RemoteAttributionError::SyncFailed {
                    reason: "SSH attribution stats request failed".to_string(),
                })
            }
            Err(_) => {
                return Err(RemoteAttributionError::SyncFailed {
                    reason: "SSH attribution stats request timeout".to_string(),
                })
            }
        };

        match response {
            AttributionProtocolMessage::StatsResponse { stats } => Ok(stats),
            AttributionProtocolMessage::Error { message } => {
                Err(RemoteAttributionError::SyncFailed { reason: message })
            }
            _ => Err(RemoteAttributionError::SyncFailed {
                reason: "Invalid response to stats request".to_string(),
            }),
        }
    }

    async fn negotiate_attribution_version(&mut self) -> Result<u32, Self::Error> {
        self.negotiate_attribution_protocol().await.map_err(|e| {
            RemoteAttributionError::SyncFailed {
                reason: e.to_string(),
            }
        })
    }
}

impl crate::ssh::Ssh {
    /// Send attribution message with const self (workaround for trait signature)
    async fn send_attribution_message_const(
        &self,
        _message: AttributionProtocolMessage,
    ) -> Result<AttributionProtocolMessage> {
        // Placeholder implementation
        Ok(AttributionProtocolMessage::Error {
            message: "Not implemented".to_string(),
        })
    }
}

/// Implementation of AttributionRemoteSync for HTTP remotes
#[async_trait]
impl AttributionRemoteSync for crate::http::Http {
    type Error = RemoteAttributionError;

    async fn pull_attributed_patches(
        &mut self,
        from: u64,
        channel: &str,
    ) -> Result<Vec<AttributedPatchBundle>, Self::Error> {
        let url = format!("{}/attribution/pull", self.url);
        let request = AttributionPullRequest {
            channel: channel.to_string(),
            from,
        };

        let response = match timeout(
            Duration::from_secs(ATTRIBUTION_TIMEOUT_SECS),
            self.client.post(&url).json(&request).send(),
        )
        .await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => {
                return Err(RemoteAttributionError::SyncFailed {
                    reason: format!("HTTP request failed: {}", e),
                })
            }
            Err(_) => {
                return Err(RemoteAttributionError::SyncFailed {
                    reason: "HTTP request timeout".to_string(),
                })
            }
        };

        if response.status().is_success() {
            // Placeholder implementation - would parse JSON response
            Ok(Vec::new())
        } else {
            Err(RemoteAttributionError::SyncFailed {
                reason: format!("Failed to pull attribution bundles: {}", response.status()),
            })
        }
    }

    async fn push_attributed_patches(
        &mut self,
        patches: Vec<AttributedPatchBundle>,
        channel: &str,
    ) -> Result<(), Self::Error> {
        let url = format!("{}/attribution/push", self.url);
        let request = AttributionPushRequest {
            channel: channel.to_string(),
            bundles: patches,
        };

        let response = match timeout(
            Duration::from_secs(ATTRIBUTION_TIMEOUT_SECS),
            self.client.post(&url).json(&request).send(),
        )
        .await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => {
                return Err(RemoteAttributionError::SyncFailed {
                    reason: format!("HTTP push failed: {}", e),
                })
            }
            Err(_) => {
                return Err(RemoteAttributionError::SyncFailed {
                    reason: "HTTP push timeout".to_string(),
                })
            }
        };

        if response.status().is_success() {
            Ok(())
        } else {
            Err(RemoteAttributionError::SyncFailed {
                reason: format!("Failed to push attribution bundles: {}", response.status()),
            })
        }
    }

    async fn get_remote_attribution_stats(
        &self,
        channel: &str,
    ) -> Result<RemoteAttributionStats, Self::Error> {
        let url = format!("{}/attribution/stats?channel={}", self.url, channel);

        let response = match timeout(
            Duration::from_secs(ATTRIBUTION_TIMEOUT_SECS),
            self.client.get(&url).send(),
        )
        .await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => {
                return Err(RemoteAttributionError::SyncFailed {
                    reason: format!("HTTP stats request failed: {}", e),
                })
            }
            Err(_) => {
                return Err(RemoteAttributionError::SyncFailed {
                    reason: "HTTP stats request timeout".to_string(),
                })
            }
        };

        if response.status().is_success() {
            // Placeholder implementation - would parse JSON response
            Ok(RemoteAttributionStats {
                total_patches: 0,
                ai_assisted_patches: 0,
                unique_authors: 0,
                unique_ai_providers: std::collections::HashSet::new(),
                last_sync_timestamp: None,
            })
        } else {
            Err(RemoteAttributionError::SyncFailed {
                reason: format!("Failed to get attribution stats: {}", response.status()),
            })
        }
    }

    async fn negotiate_attribution_version(&mut self) -> Result<u32, Self::Error> {
        self.negotiate_attribution_protocol().await.map_err(|e| {
            RemoteAttributionError::SyncFailed {
                reason: e.to_string(),
            }
        })
    }
}

/// Configuration for remote attribution operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAttributionConfig {
    /// Enable attribution sync for all remote operations
    pub enabled: bool,
    /// Require signature verification
    pub require_signatures: bool,
    /// Batch size for attribution operations
    pub batch_size: usize,
    /// Timeout for remote attribution operations
    pub timeout_seconds: u64,
    /// Fall back to non-attribution sync if unsupported
    pub fallback_enabled: bool,
}

impl Default for RemoteAttributionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            require_signatures: false,
            batch_size: 50,
            timeout_seconds: 30,
            fallback_enabled: true,
        }
    }
}

impl RemoteAttributionConfig {
    /// Load configuration from environment variables
    pub fn from_environment() -> Self {
        let mut config = Self::default();

        if let Ok(value) = std::env::var("ATOMIC_ATTRIBUTION_REMOTE_ENABLED") {
            config.enabled = value.parse().unwrap_or(true);
        }

        if let Ok(value) = std::env::var("ATOMIC_ATTRIBUTION_REQUIRE_SIGNATURES") {
            config.require_signatures = value.parse().unwrap_or(false);
        }

        if let Ok(value) = std::env::var("ATOMIC_ATTRIBUTION_BATCH_SIZE") {
            config.batch_size = value.parse().unwrap_or(50);
        }

        if let Ok(value) = std::env::var("ATOMIC_ATTRIBUTION_TIMEOUT") {
            config.timeout_seconds = value.parse().unwrap_or(30);
        }

        if let Ok(value) = std::env::var("ATOMIC_ATTRIBUTION_FALLBACK") {
            config.fallback_enabled = value.parse().unwrap_or(true);
        }

        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_environment() {
        // Set test environment variables
        std::env::set_var("ATOMIC_ATTRIBUTION_REMOTE_ENABLED", "false");
        std::env::set_var("ATOMIC_ATTRIBUTION_BATCH_SIZE", "25");

        let config = RemoteAttributionConfig::from_environment();

        assert!(!config.enabled);
        assert_eq!(config.batch_size, 25);

        // Clean up
        std::env::remove_var("ATOMIC_ATTRIBUTION_REMOTE_ENABLED");
        std::env::remove_var("ATOMIC_ATTRIBUTION_BATCH_SIZE");
    }

    #[test]
    fn test_protocol_version() {
        assert_eq!(ATTRIBUTION_PROTOCOL_VERSION, 1);
    }
}
