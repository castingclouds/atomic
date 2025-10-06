use std::path::PathBuf;

use anyhow::{bail, Context};
use atomic_repository::Repository;
use clap::{Parser, ValueHint};
use libatomic::attribution::SerializedAttribution;
use libatomic::change::ChangeHeader;
use libatomic::changestore::ChangeStore;
use libatomic::pristine::{ChannelTxnT, TxnT};
use libatomic::{Base32, TxnTExt};
use log::debug;

use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;

/// Shows AI attribution statistics and information for changes
///
/// # Examples
///
/// Show attribution for the current channel:
///   atomic attribution
///
/// Show detailed statistics with provider breakdown:
///   atomic attribution --stats --providers
///
/// Show attribution for a specific change:
///   atomic attribution --hash ABC123...
///
/// Filter to show only changes from OpenAI with high confidence:
///   atomic attribution --filter-provider openai --min-confidence 0.8
///
/// Output as JSON:
///   atomic attribution --output-format json
#[derive(Parser, Debug)]
pub struct Attribution {
    /// Set the repository where this command should run. Defaults to the first ancestor of the current directory that contains a `.atomic` directory.
    #[clap(long = "repository", value_hint = ValueHint::DirPath)]
    repo_path: Option<PathBuf>,
    /// Show attribution for this channel instead of the current channel
    #[clap(long = "channel")]
    channel: Option<String>,
    /// Show detailed statistics
    #[clap(long = "stats")]
    stats: bool,
    /// Show AI provider breakdown
    #[clap(long = "providers")]
    providers: bool,
    /// Show suggestion type breakdown
    #[clap(long = "suggestion-types")]
    suggestion_types: bool,
    /// Output format (json, plaintext)
    #[clap(long = "output-format", value_enum, default_value = "plaintext")]
    output_format: OutputFormat,
    /// Show only changes from this AI provider
    #[clap(long = "filter-provider")]
    filter_provider: Option<String>,
    /// Show only changes with confidence above this threshold (0.0-1.0)
    #[clap(long = "min-confidence")]
    min_confidence: Option<f64>,
    /// Number of most recent changes to analyze (default: all)
    #[clap(long = "limit")]
    limit: Option<usize>,
    /// Show attribution for this specific change hash (takes precedence over channel analysis)
    #[clap(long = "hash", value_name = "HASH")]
    hash: Option<String>,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum OutputFormat {
    Plaintext,
    Json,
}

#[derive(Serialize)]
struct AttributionReport {
    total_changes: usize,
    ai_assisted_changes: usize,
    human_changes: usize,
    ai_percentage: f64,
    average_ai_confidence: f64,
    provider_breakdown: HashMap<String, ProviderStats>,
    suggestion_type_breakdown: HashMap<String, usize>,
    confidence_distribution: ConfidenceDistribution,
    recent_ai_changes: Vec<ChangeAttribution>,
}

#[derive(Serialize)]
struct ProviderStats {
    count: usize,
    percentage: f64,
    average_confidence: f64,
    models: HashMap<String, usize>,
}

#[derive(Serialize)]
struct ConfidenceDistribution {
    low: usize,    // 0.0 - 0.33
    medium: usize, // 0.34 - 0.66
    high: usize,   // 0.67 - 1.0
}

#[derive(Serialize, Clone)]
struct ChangeAttribution {
    hash: String,
    message: String,
    author: String,
    timestamp: String,
    ai_assisted: bool,
    ai_provider: Option<String>,
    ai_model: Option<String>,
    suggestion_type: Option<String>,
    confidence: Option<f64>,
}

impl Attribution {
    /// Main entry point for attribution analysis
    pub fn run(self) -> Result<(), anyhow::Error> {
        let repo_path = self.repo_path.clone();
        let repo = Repository::find_root(repo_path)
            .with_context(|| "Failed to find atomic repository root")?;
        let txn = repo
            .pristine
            .txn_begin()
            .with_context(|| "Failed to begin transaction")?;

        // Handle specific hash query - following factory pattern for query handling
        if let Some(ref hash_str) = self.hash {
            return self
                .run_single_hash(&repo, &txn, hash_str)
                .with_context(|| format!("Failed to analyze attribution for hash {}", hash_str));
        }

        let channel_name = if let Some(ref c) = self.channel {
            c
        } else {
            txn.current_channel().unwrap_or(libatomic::DEFAULT_CHANNEL)
        };

        let channel = if let Some(channel) = txn.load_channel(&channel_name)? {
            channel
        } else {
            bail!("Channel {:?} not found", channel_name)
        };

        println!("Analyzing attribution for channel '{}'...\n", channel_name);

        // Collect attribution data from changes - using configuration-driven approach
        let mut report = self
            .analyze_attribution(&repo, &txn, &*channel.read())
            .with_context(|| {
                format!("Failed to analyze attribution for channel {}", channel_name)
            })?;

        // Apply filters following the configuration pattern
        if let Some(ref provider) = self.filter_provider {
            report
                .recent_ai_changes
                .retain(|c| c.ai_provider.as_ref().map_or(false, |p| p == provider));
        }

        if let Some(min_confidence) = self.min_confidence {
            if min_confidence < 0.0 || min_confidence > 1.0 {
                bail!(
                    "Confidence threshold must be between 0.0 and 1.0, got: {}",
                    min_confidence
                );
            }
            report
                .recent_ai_changes
                .retain(|c| c.confidence.map_or(false, |conf| conf >= min_confidence));
        }

        // Output results using factory pattern for formatters
        match self.output_format {
            OutputFormat::Json => {
                serde_json::to_writer_pretty(&mut std::io::stdout(), &report)
                    .with_context(|| "Failed to serialize report to JSON")?;
                println!(); // Add trailing newline for JSON output
            }
            OutputFormat::Plaintext => {
                self.print_plaintext_report(&report)
                    .with_context(|| "Failed to print plaintext report")?;
            }
        }

        Ok(())
    }

    fn run_single_hash<T: TxnT + ChannelTxnT + TxnTExt>(
        &self,
        repo: &Repository,
        txn: &T,
        hash_str: &str,
    ) -> Result<(), anyhow::Error> {
        // Parse the hash - support both full hashes and prefixes, following error handling strategy
        let hash = if let Some(h) = libatomic::Hash::from_base32(hash_str.as_bytes()) {
            h
        } else {
            match txn.hash_from_prefix(hash_str) {
                Ok((hash, _)) => hash,
                Err(e) => {
                    return Err(anyhow::Error::from(e)).with_context(|| {
                        format!("Change hash '{}' not found or ambiguous", hash_str)
                    });
                }
            }
        };

        println!("Analyzing attribution for change {}...\n", hash.to_base32());

        // Get attribution for the specific change - with proper error context
        let change_attribution = self.get_change_attribution(repo, &hash).with_context(|| {
            format!("Failed to load attribution for change {}", hash.to_base32())
        })?;

        // Output results based on format - using factory pattern for output handling
        match self.output_format {
            OutputFormat::Json => {
                serde_json::to_writer_pretty(&mut std::io::stdout(), &change_attribution)
                    .with_context(|| "Failed to serialize change attribution to JSON")?;
                println!(); // Add trailing newline for JSON output
            }
            OutputFormat::Plaintext => {
                self.print_single_change_attribution(&change_attribution)
                    .with_context(|| "Failed to print change attribution")?;
            }
        }

        Ok(())
    }

    fn analyze_attribution<T: TxnT + ChannelTxnT + TxnTExt>(
        &self,
        repo: &Repository,
        txn: &T,
        channel: &T::Channel,
    ) -> Result<AttributionReport, anyhow::Error> {
        // Initialize analysis state following configuration-driven design
        let mut analysis_state = AttributionAnalysisState::new();

        // Get configured limits with proper defaults
        let limit = self.limit.unwrap_or(usize::MAX);
        let mut count = 0;

        // Iterate through changes in reverse chronological order with proper error handling
        for log_entry in txn
            .log(&channel, 0)
            .with_context(|| "Failed to read channel log")?
        {
            let (_, (hash, _)) = log_entry.with_context(|| "Failed to read log entry")?;

            if count >= limit {
                break;
            }
            count += 1;

            let hash: libatomic::Hash = (*hash).into();
            let change_attribution =
                self.get_change_attribution(repo, &hash).with_context(|| {
                    format!("Failed to get attribution for change {}", hash.to_base32())
                })?;

            // Update analysis state using the builder pattern
            analysis_state.process_change(&change_attribution)?;
        }

        // Build final report using factory pattern
        analysis_state.build_report()
    }

    fn get_change_attribution(
        &self,
        repo: &Repository,
        hash: &libatomic::Hash,
    ) -> Result<ChangeAttribution, anyhow::Error> {
        let change = repo
            .changes
            .get_change(hash)
            .with_context(|| format!("Failed to load change {}", hash.to_base32()))?;
        let header = repo
            .changes
            .get_header(&(*hash).into())
            .with_context(|| format!("Failed to load change header {}", hash.to_base32()))?;

        // Extract author information using factory pattern
        let author = AuthorExtractor::extract_author(&header);

        // Try to load attribution from metadata using configuration-driven approach
        debug!(
            "Change metadata length: {}, first 50 bytes: {:?}",
            change.hashed.metadata.len(),
            &change.hashed.metadata[..change.hashed.metadata.len().min(50)]
        );

        if !change.hashed.metadata.is_empty() {
            match bincode::deserialize::<SerializedAttribution>(&change.hashed.metadata) {
                Ok(attribution_data) => {
                    debug!(
                        "Successfully deserialized attribution: ai_assisted={}, provider={:?}",
                        attribution_data.ai_assisted,
                        attribution_data.ai_metadata.as_ref().map(|m| &m.provider)
                    );
                    return Ok(ChangeAttributionBuilder::new()
                        .with_hash(hash.to_base32())
                        .with_message(header.message.clone())
                        .with_author(author)
                        .with_timestamp(header.timestamp.to_rfc2822())
                        .with_serialized_attribution(&attribution_data)
                        .build());
                }
                Err(e) => {
                    debug!("Failed to deserialize attribution metadata: {}", e);
                }
            }
        }

        // Auto-detect AI assistance using pattern matching
        let ai_detection_result = AIDetector::analyze_change_content(
            &header.message,
            header.description.as_deref().unwrap_or(""),
        );

        Ok(ChangeAttributionBuilder::new()
            .with_hash(hash.to_base32())
            .with_message(header.message.clone())
            .with_author(author)
            .with_timestamp(header.timestamp.to_rfc2822())
            .with_ai_detection(ai_detection_result)
            .build())
    }

    fn print_plaintext_report(&self, report: &AttributionReport) -> Result<(), std::io::Error> {
        let mut stdout = std::io::stdout();

        // Summary statistics
        writeln!(stdout, "=== Attribution Summary ===")?;
        writeln!(stdout, "Total changes analyzed: {}", report.total_changes)?;
        writeln!(
            stdout,
            "AI-assisted changes: {} ({:.1}%)",
            report.ai_assisted_changes, report.ai_percentage
        )?;
        writeln!(stdout, "Human-authored changes: {}", report.human_changes)?;
        if report.ai_assisted_changes > 0 {
            writeln!(
                stdout,
                "Average AI confidence: {:.1}%",
                report.average_ai_confidence * 100.0
            )?;
        }
        writeln!(stdout)?;

        // Provider breakdown
        if self.providers && !report.provider_breakdown.is_empty() {
            writeln!(stdout, "=== AI Provider Breakdown ===")?;
            for (provider, stats) in &report.provider_breakdown {
                writeln!(stdout, "{}:", provider)?;
                writeln!(
                    stdout,
                    "  Changes: {} ({:.1}%)",
                    stats.count, stats.percentage
                )?;
                writeln!(
                    stdout,
                    "  Avg Confidence: {:.1}%",
                    stats.average_confidence * 100.0
                )?;
                if !stats.models.is_empty() {
                    writeln!(stdout, "  Models:")?;
                    for (model, count) in &stats.models {
                        writeln!(stdout, "    {}: {}", model, count)?;
                    }
                }
            }
            writeln!(stdout)?;
        }

        // Suggestion type breakdown
        if self.suggestion_types && !report.suggestion_type_breakdown.is_empty() {
            writeln!(stdout, "=== Suggestion Type Breakdown ===")?;
            for (suggestion_type, count) in &report.suggestion_type_breakdown {
                writeln!(stdout, "{}: {}", suggestion_type, count)?;
            }
            writeln!(stdout)?;
        }

        // Confidence distribution
        if report.ai_assisted_changes > 0 && self.stats {
            writeln!(stdout, "=== Confidence Distribution ===")?;
            writeln!(
                stdout,
                "Low (0-33%): {}",
                report.confidence_distribution.low
            )?;
            writeln!(
                stdout,
                "Medium (34-66%): {}",
                report.confidence_distribution.medium
            )?;
            writeln!(
                stdout,
                "High (67-100%): {}",
                report.confidence_distribution.high
            )?;
            writeln!(stdout)?;
        }

        // Recent AI changes
        if !report.recent_ai_changes.is_empty() && self.stats {
            writeln!(stdout, "=== Recent Changes ===")?;
            for change in report.recent_ai_changes.iter().take(10) {
                writeln!(stdout, "Change: {}", change.hash)?;
                writeln!(stdout, "  Message: {}", change.message)?;
                writeln!(stdout, "  Author: {}", change.author)?;
                writeln!(stdout, "  Date: {}", change.timestamp)?;
                if change.ai_assisted {
                    writeln!(stdout, "  AI-Assisted: Yes")?;
                    if let Some(ref provider) = change.ai_provider {
                        writeln!(stdout, "    Provider: {}", provider)?;
                    }
                    if let Some(ref model) = change.ai_model {
                        writeln!(stdout, "    Model: {}", model)?;
                    }
                    if let Some(ref suggestion_type) = change.suggestion_type {
                        writeln!(stdout, "    Type: {}", suggestion_type)?;
                    }
                    if let Some(confidence) = change.confidence {
                        writeln!(stdout, "    Confidence: {:.1}%", confidence * 100.0)?;
                    }
                } else {
                    writeln!(stdout, "  AI-Assisted: No")?;
                }
                writeln!(stdout)?;
            }
        }

        Ok(())
    }

    fn print_single_change_attribution(
        &self,
        change: &ChangeAttribution,
    ) -> Result<(), std::io::Error> {
        use std::io::Write;
        let mut stdout = std::io::stdout();

        // Use configuration-driven formatting
        writeln!(stdout, "=== Change Attribution ===")?;
        writeln!(stdout, "Hash: {}", change.hash)?;
        writeln!(stdout, "Message: {}", change.message)?;
        writeln!(stdout, "Author: {}", change.author)?;
        writeln!(stdout, "Date: {}", change.timestamp)?;
        writeln!(stdout)?;

        writeln!(stdout, "=== AI Attribution Status ===")?;

        if change.ai_assisted {
            writeln!(stdout, "AI-Assisted: Yes")?;

            // Use factory pattern for detailed information display
            if let Some(ref provider) = change.ai_provider {
                writeln!(stdout, "  Provider: {}", provider)?;
            }
            if let Some(ref model) = change.ai_model {
                writeln!(stdout, "  Model: {}", model)?;
            }
            if let Some(ref suggestion_type) = change.suggestion_type {
                writeln!(stdout, "  Type: {}", suggestion_type)?;
            }
            if let Some(confidence) = change.confidence {
                let confidence_level = match confidence {
                    c if c >= 0.8 => "High",
                    c if c >= 0.6 => "Medium",
                    c if c >= 0.4 => "Low",
                    _ => "Very Low",
                };
                writeln!(
                    stdout,
                    "  Confidence: {:.1}% ({})",
                    confidence * 100.0,
                    confidence_level
                )?;
            }
        } else {
            writeln!(stdout, "AI-Assisted: No")?;
        }

        Ok(())
    }
}

/// Analysis state for attribution processing - follows builder pattern
struct AttributionAnalysisState {
    total_changes: usize,
    ai_assisted_changes: usize,
    human_changes: usize,
    confidence_sum: f64,
    confidence_count: usize,
    provider_stats: HashMap<String, Vec<f64>>,
    model_stats: HashMap<String, HashMap<String, usize>>,
    suggestion_type_breakdown: HashMap<String, usize>,
    low_conf: usize,
    medium_conf: usize,
    high_conf: usize,
    recent_changes: Vec<ChangeAttribution>,
}

impl AttributionAnalysisState {
    /// Factory method for creating new analysis state
    fn new() -> Self {
        Self {
            total_changes: 0,
            ai_assisted_changes: 0,
            human_changes: 0,
            confidence_sum: 0.0,
            confidence_count: 0,
            provider_stats: HashMap::new(),
            model_stats: HashMap::new(),
            suggestion_type_breakdown: HashMap::new(),
            low_conf: 0,
            medium_conf: 0,
            high_conf: 0,
            recent_changes: Vec::new(),
        }
    }

    /// Process a single change attribution following configuration-driven design
    fn process_change(
        &mut self,
        change_attribution: &ChangeAttribution,
    ) -> Result<(), anyhow::Error> {
        self.total_changes += 1;

        if change_attribution.ai_assisted {
            self.ai_assisted_changes += 1;

            // Process confidence data with proper validation
            if let Some(confidence) = change_attribution.confidence {
                if confidence < 0.0 || confidence > 1.0 {
                    return Err(anyhow::anyhow!(
                        "Invalid confidence value: {} (must be between 0.0 and 1.0)",
                        confidence
                    ));
                }

                self.confidence_sum += confidence;
                self.confidence_count += 1;

                // Categorize confidence levels following configuration pattern
                if confidence <= 0.33 {
                    self.low_conf += 1;
                } else if confidence <= 0.66 {
                    self.medium_conf += 1;
                } else {
                    self.high_conf += 1;
                }
            }

            // Process provider statistics using factory pattern
            if let Some(ref provider) = change_attribution.ai_provider {
                self.provider_stats
                    .entry(provider.clone())
                    .or_insert_with(Vec::new)
                    .push(change_attribution.confidence.unwrap_or(0.5));

                if let Some(ref model) = change_attribution.ai_model {
                    self.model_stats
                        .entry(provider.clone())
                        .or_insert_with(HashMap::new)
                        .entry(model.clone())
                        .and_modify(|e| *e += 1)
                        .or_insert(1);
                }
            }

            // Track suggestion types with proper handling
            if let Some(ref suggestion_type) = change_attribution.suggestion_type {
                self.suggestion_type_breakdown
                    .entry(suggestion_type.clone())
                    .and_modify(|e| *e += 1)
                    .or_insert(1);
            }
        } else {
            self.human_changes += 1;
        }

        self.recent_changes.push(change_attribution.clone());
        Ok(())
    }

    /// Build final attribution report using factory pattern
    fn build_report(self) -> Result<AttributionReport, anyhow::Error> {
        // Calculate provider breakdown with proper error handling
        let mut provider_breakdown = HashMap::new();
        for (provider, confidences) in self.provider_stats {
            let count = confidences.len();
            if count == 0 {
                continue;
            }

            let avg_confidence = confidences.iter().sum::<f64>() / count as f64;
            let percentage = if self.total_changes > 0 {
                (count as f64 / self.total_changes as f64) * 100.0
            } else {
                0.0
            };

            provider_breakdown.insert(
                provider.clone(),
                ProviderStats {
                    count,
                    percentage,
                    average_confidence: avg_confidence,
                    models: self.model_stats.get(&provider).cloned().unwrap_or_default(),
                },
            );
        }

        // Calculate overall statistics with safety checks
        let ai_percentage = if self.total_changes > 0 {
            (self.ai_assisted_changes as f64 / self.total_changes as f64) * 100.0
        } else {
            0.0
        };

        let average_ai_confidence = if self.confidence_count > 0 {
            self.confidence_sum / self.confidence_count as f64
        } else {
            0.0
        };

        Ok(AttributionReport {
            total_changes: self.total_changes,
            ai_assisted_changes: self.ai_assisted_changes,
            human_changes: self.human_changes,
            ai_percentage,
            average_ai_confidence,
            provider_breakdown,
            suggestion_type_breakdown: self.suggestion_type_breakdown,
            confidence_distribution: ConfidenceDistribution {
                low: self.low_conf,
                medium: self.medium_conf,
                high: self.high_conf,
            },
            recent_ai_changes: self.recent_changes,
        })
    }
}

/// Factory for extracting author information following AGENTS.md patterns
struct AuthorExtractor;

impl AuthorExtractor {
    /// Extract author string from change header using configuration-driven approach
    fn extract_author(header: &ChangeHeader) -> String {
        header
            .authors
            .first()
            .map(|a| {
                if let Some(name) = a.0.get("name") {
                    if let Some(email) = a.0.get("email") {
                        format!("{} <{}>", name, email)
                    } else {
                        name.clone()
                    }
                } else if let Some(key) = a.0.get("key") {
                    format!("Key: {}", &key[..8.min(key.len())]) // Show first 8 chars of key
                } else {
                    "Unknown".to_string()
                }
            })
            .unwrap_or_else(|| "Unknown".to_string())
    }
}

/// AI detection result for pattern-based analysis
#[derive(Debug, Clone)]
struct AIDetectionResult {
    ai_assisted: bool,
    provider: Option<String>,
    confidence: Option<f64>,
}

/// AI content detector using pattern matching following singleton pattern
struct AIDetector;

impl AIDetector {
    /// Analyze change content for AI assistance indicators
    fn analyze_change_content(message: &str, description: &str) -> AIDetectionResult {
        let combined_text = format!("{} {}", message, description).to_lowercase();

        // AI indicators with associated confidence levels - configuration-driven
        let ai_patterns = &[
            ("ai-assisted", 0.9),
            ("ai-generated", 0.9),
            ("copilot", 0.8),
            ("claude", 0.8),
            ("gpt", 0.8),
            ("chatgpt", 0.8),
            ("ai:", 0.7),
            ("assistant:", 0.7),
            ("auto-generated", 0.6),
            ("github copilot", 0.8),
            ("codewhisperer", 0.7),
            ("tabnine", 0.6),
        ];

        // Find the highest confidence match
        let mut best_match = None;
        let mut best_confidence = 0.0;

        for (pattern, confidence) in ai_patterns {
            if combined_text.contains(pattern) {
                if *confidence > best_confidence {
                    best_confidence = *confidence;
                    best_match = Some(*pattern);
                }
            }
        }

        if let Some(matched_pattern) = best_match {
            // Determine provider from pattern following factory pattern
            let provider = Self::determine_provider(matched_pattern);

            AIDetectionResult {
                ai_assisted: true,
                provider,
                confidence: Some(best_confidence),
            }
        } else {
            AIDetectionResult {
                ai_assisted: false,
                provider: None,
                confidence: None,
            }
        }
    }

    /// Determine AI provider from matched pattern using factory pattern
    fn determine_provider(pattern: &str) -> Option<String> {
        match pattern {
            "copilot" | "github copilot" => Some("github".to_string()),
            "claude" => Some("anthropic".to_string()),
            "gpt" | "chatgpt" => Some("openai".to_string()),
            "codewhisperer" => Some("amazon".to_string()),
            "tabnine" => Some("tabnine".to_string()),
            _ => Some("auto-detected".to_string()),
        }
    }
}

/// Builder for constructing ChangeAttribution following builder pattern
struct ChangeAttributionBuilder {
    hash: Option<String>,
    message: Option<String>,
    author: Option<String>,
    timestamp: Option<String>,
    ai_assisted: bool,
    ai_provider: Option<String>,
    ai_model: Option<String>,
    suggestion_type: Option<String>,
    confidence: Option<f64>,
}

impl ChangeAttributionBuilder {
    /// Create new builder using factory pattern
    fn new() -> Self {
        Self {
            hash: None,
            message: None,
            author: None,
            timestamp: None,
            ai_assisted: false,
            ai_provider: None,
            ai_model: None,
            suggestion_type: None,
            confidence: None,
        }
    }

    /// Builder methods for configuration-driven construction
    fn with_hash(mut self, hash: String) -> Self {
        self.hash = Some(hash);
        self
    }

    fn with_message(mut self, message: String) -> Self {
        self.message = Some(message);
        self
    }

    fn with_author(mut self, author: String) -> Self {
        self.author = Some(author);
        self
    }

    fn with_timestamp(mut self, timestamp: String) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    fn with_serialized_attribution(mut self, attribution: &SerializedAttribution) -> Self {
        self.ai_assisted = attribution.ai_assisted;
        self.confidence = attribution.confidence;

        if let Some(ref metadata) = attribution.ai_metadata {
            self.ai_provider = Some(metadata.provider.clone());
            self.ai_model = Some(metadata.model.clone());
            self.suggestion_type = Some(format!("{:?}", metadata.suggestion_type));
        }

        self
    }

    fn with_ai_detection(mut self, detection: AIDetectionResult) -> Self {
        self.ai_assisted = detection.ai_assisted;
        self.ai_provider = detection.provider;
        self.confidence = detection.confidence;

        if detection.ai_assisted {
            self.suggestion_type = Some("Complete".to_string());
        }

        self
    }

    /// Build final ChangeAttribution with validation
    fn build(self) -> ChangeAttribution {
        ChangeAttribution {
            hash: self.hash.unwrap_or_else(|| "unknown".to_string()),
            message: self.message.unwrap_or_else(|| "No message".to_string()),
            author: self.author.unwrap_or_else(|| "Unknown".to_string()),
            timestamp: self.timestamp.unwrap_or_else(|| "Unknown".to_string()),
            ai_assisted: self.ai_assisted,
            ai_provider: self.ai_provider,
            ai_model: self.ai_model,
            suggestion_type: self.suggestion_type,
            confidence: self.confidence,
        }
    }
}
