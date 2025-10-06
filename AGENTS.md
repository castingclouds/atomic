# AGENTS.md - Atomic Development Best Practices & Architecture Guide

## Project Overview

Atomic is a mathematically sound distributed version control system built in Rust, emphasizing performance, correctness, and maintainability through sophisticated architectural patterns and best practices.

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Configuration-Driven Design](#configuration-driven-design)
3. [Factory Pattern Implementation](#factory-pattern-implementation)
4. [Singleton Pattern Usage](#singleton-pattern-usage)
5. [DRY Principles with Macros](#dry-principles-with-macros)
6. [Error Handling Strategy](#error-handling-strategy)
7. [Code Organization Patterns](#code-organization-patterns)
8. [Development Guidelines](#development-guidelines)
9. [Testing Strategy](#testing-strategy)
10. [Performance Considerations](#performance-considerations)
11. [Environment Variable Detection Patterns](#environment-variable-detection-patterns)
12. [CLI Integration Patterns](#cli-integration-patterns)
13. [HTTP API Protocol Alignment](#http-api-protocol-alignment)
13. [HTTP API Protocol Alignment](#http-api-protocol-alignment)

## Architecture Overview

### Modular Crate Structure

The project follows a clean separation of concerns through focused crates:

```
atomic/                     # CLI application & command handlers
├── atomic-macros/          # Procedural macros for code generation
├── atomic-config/          # Configuration management system
├── atomic-identity/        # User identity & credential management
├── atomic-interaction/     # User interface & interaction patterns
├── atomic-remote/          # Remote repository operations
├── atomic-repository/      # Repository management
├── libatomic/             # Core VCS engine & algorithms
└── contrib/               # Additional resources
```

**Key Architectural Principles:**
- **Single Responsibility**: Each crate has one clear purpose
- **Dependency Inversion**: Core library (`libatomic`) has minimal dependencies
- **Interface Segregation**: Small, focused trait interfaces
- **Composition over Inheritance**: Struct composition with traits

### Database-Centric Architecture

Uses Sanakirja as the storage backend with macro-generated database operations:

```rust
// Example of database table generation
#[table("channel_changes")]
pub struct ChannelChanges {
    channel: ChannelRef,
    change: ChangeId,
}
```

## Configuration-Driven Design

### Hierarchical Configuration System

Atomic implements a sophisticated configuration hierarchy following the principle of **configuration over code**:

```rust
// Global configuration precedence:
// 1. Environment variables
// 2. Local repository config (.atomic/config.toml)
// 3. User-specific config (~/.config/atomic/config.toml)
// 4. System defaults

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Global {
    pub author: Author,
    pub unrecord_changes: Option<usize>,
    pub colors: Option<Choice>,
    pub pager: Option<Choice>,
    pub template: Option<Templates>,
    pub ignore_kinds: Option<HashMap<String, Vec<String>>>,
}
```

### Configuration Best Practices

1. **Serde Integration**: All configuration structs use serde for serialization
2. **Optional Fields**: Use `Option<T>` for optional configuration values
3. **Default Implementations**: Provide sensible defaults via `Default` trait
4. **Validation**: Validate configuration during deserialization
5. **Backward Compatibility**: Use serde aliases for renamed fields

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Author {
    #[serde(alias = "name", default, skip_serializing_if = "String::is_empty")]
    pub username: String,
    #[serde(alias = "full_name", default, skip_serializing_if = "String::is_empty")]
    pub display_name: String,
}
```

### Remote Configuration Factory

The remote configuration system demonstrates the factory pattern:

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RemoteConfig {
    Ssh { name: String, ssh: String },
    Http { 
        name: String, 
        http: String,
        #[serde(default)]
        headers: HashMap<String, RemoteHttpHeader>,
    },
}

impl RemoteConfig {
    pub fn name(&self) -> &str {
        match self {
            RemoteConfig::Ssh { name, .. } => name,
            RemoteConfig::Http { name, .. } => name,
        }
    }
}
```

## Factory Pattern Implementation

### Identity Factory Pattern

The identity system uses a factory-like pattern for creating complete user identities:

```rust
impl Complete {
    /// Factory method for creating new identities
    pub fn new(
        name: String,
        config: Config,
        public_key: PublicKey,
        credentials: Option<Credentials>,
    ) -> Self {
        if name.is_empty() {
            panic!("Identity name cannot be empty!");
        }
        
        Self {
            name,
            config,
            public_key,
            credentials,
            last_modified: chrono::offset::Utc::now(),
        }
    }

    /// Factory method for default identity
    pub fn default() -> Result<Self, anyhow::Error> {
        let config_path = config::global_config_dir().unwrap().join("config.toml");
        let author: Author = if config_path.exists() {
            // Load from configuration
            let mut config_file = fs::File::open(&config_path)?;
            let mut config_text = String::new();
            config_file.read_to_string(&mut config_text)?;
            let global_config: config::Global = toml::from_str(&config_text)?;
            global_config.author
        } else {
            Author::default()
        };

        let secret_key = SKey::generate(None);
        let public_key = secret_key.public_key();

        Ok(Self::new(
            String::from("default"),
            Config::from(author),
            public_key,
            Some(Credentials::from(secret_key.save(None))),
        ))
    }
}
```

### Best Practices for Factory Patterns

1. **Validation in Constructors**: Always validate inputs in factory methods
2. **Multiple Constructors**: Provide specialized factory methods for common use cases
3. **Result Types**: Use `Result<T, E>` for fallible construction
4. **Builder Pattern**: For complex objects, consider implementing the builder pattern
5. **Default Implementations**: Provide sensible defaults via factory methods

## Singleton Pattern Usage

### Lazy Static Singletons

Atomic uses `lazy_static!` for compile-time singletons, particularly for:

1. **Regex Compilation**: Pre-compiled regular expressions
2. **Global State**: Shared application state
3. **Configuration Themes**: UI themes and styling

```rust
lazy_static! {
    static ref STATE: Regex = Regex::new(r#"state\s+(\S+)(\s+([0-9]+)?)\s+"#).unwrap();
    static ref ID: Regex = Regex::new(r#"id\s+(\S+)\s+"#).unwrap();
    static ref CHANGELIST: Regex = Regex::new(r#"changelist\s+(\S+)\s+([0-9]+)(.*)\s+"#).unwrap();
}

// UI Theme Singleton
lazy_static! {
    static ref THEME: Box<dyn theme::Theme + Send + Sync> = {
        if let Ok((config, _)) = config::Global::load() {
            let color_choice = config.colors.unwrap_or_default();
            match color_choice {
                Choice::Auto | Choice::Always => Box::<theme::ColorfulTheme>::default(),
                Choice::Never => Box::new(theme::SimpleTheme),
            }
        } else {
            Box::<theme::ColorfulTheme>::default()
        }
    };
}

// Progress Bar Manager
lazy_static! {
    static ref MULTI_PROGRESS: MultiProgress = MultiProgress::new();
}
```

### OnceLock for Runtime Singletons

For runtime initialization of singletons, use `OnceLock`:

```rust
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct Credentials {
    secret_key: SecretKey,
    password: OnceLock<String>,  // Runtime-initialized singleton
}

impl Credentials {
    pub fn new(secret_key: SecretKey, password: Option<String>) -> Self {
        Self {
            secret_key,
            password: if let Some(pw) = password {
                OnceLock::from(pw)
            } else {
                OnceLock::new()
            },
        }
    }
}
```

### Singleton Best Practices

1. **Prefer `OnceLock`** over `lazy_static!` for runtime initialization
2. **Thread Safety**: Ensure singletons are `Send + Sync` when needed
3. **Initialization Cost**: Use singletons for expensive-to-compute values
4. **Testing**: Provide reset mechanisms for testing when necessary
5. **Documentation**: Document singleton lifetime and initialization

## DRY Principles with Macros

### Procedural Macros for Database Operations

The `atomic-macros` crate implements extensive procedural macros to eliminate boilerplate:

```rust
// Table definition macro generates CRUD operations
#[table("changes")]
pub struct Changes {
    id: ChangeId,
    hash: Hash,
}

// Generates:
// - get_changes()
// - put_changes()
// - del_changes()
// - cursor_changes()
// - iter_changes()
```

### Cursor Generation Macros

Database cursor operations are generated via macros:

```rust
// Generates cursor implementations
cursor!(
    /// Cursor for iterating over changes
    ChangesCursor,
    changes_table,
    ChangeId,
    Hash
);

// Generates iterator implementations
iter!(
    /// Iterator for changes
    ChangesIter,
    ChangesCursor,
    ChangeId,
    Hash
);
```

### Error Handling Macros

Consistent error handling through derive macros:

```rust
#[derive(Debug, Error)]
pub enum AtomicError {
    #[error("Repository not found: {}", path)]
    RepositoryNotFound { path: String },
    
    #[error("Channel {} not found", channel)]
    ChannelNotFound { channel: String },
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

### Macro Development Best Practices

1. **Error Reporting**: Provide clear error messages with spans
2. **Documentation**: Generate documentation for macro-generated code
3. **Hygiene**: Use proper hygiene to avoid name collisions
4. **Composability**: Design macros to work together
5. **Testing**: Test macros with various input patterns

```rust
// Example of well-documented macro usage
#[proc_macro_derive(AtomicTable, attributes(table))]
pub fn derive_table(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    // Generate implementation with proper error handling
    match generate_table_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
```

## Error Handling Strategy

### Hierarchical Error Types

Atomic implements a sophisticated error hierarchy using `thiserror`:

```rust
#[derive(Debug, Error, Serialize, Deserialize)]
pub enum RemoteError {
    #[error("Repository not found: {}", url)]
    RepositoryNotFound { url: String },
    
    #[error("Channel {} not found for repository {}", channel, url)]
    ChannelNotFound { channel: String, url: String },
    
    #[error("Ambiguous path: {}", path)]
    AmbiguousPath { path: String },
}

#[derive(Debug, Error)]
pub enum LocalApplyError {
    #[error("Working copy error: {0}")]
    WorkingCopy(#[from] crate::working_copy::WorkingCopyError),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

### Error Conversion Patterns

Automatic error conversion using `From` trait:

```rust
impl From<sanakirja::Error> for PristineError {
    fn from(err: sanakirja::Error) -> Self {
        PristineError::Sanakirja(err)
    }
}

// Usage with ? operator
fn database_operation() -> Result<Data, PristineError> {
    let txn = sanakirja::Env::new(&path, 1 << 30)?; // Auto-converts
    Ok(process_data(txn))
}
```

### Context-Rich Error Handling

Using `anyhow` for context propagation:

```rust
use anyhow::{anyhow, Context};

fn load_config() -> Result<Config, anyhow::Error> {
    let file = File::open(&path)
        .with_context(|| format!("Failed to open config at {}", path.display()))?;
    
    let config: Config = toml::from_reader(file)
        .context("Failed to parse TOML configuration")?;
    
    Ok(config)
}
```

## Code Organization Patterns

### Module Structure

Each crate follows consistent module organization:

```
src/
├── lib.rs              # Public API and re-exports
├── error.rs            # Error types and conversions
├── config.rs           # Configuration structures
├── types/              # Core type definitions
│   ├── mod.rs
│   ├── hash.rs
│   └── change.rs
├── operations/         # Core operations
│   ├── mod.rs
│   ├── apply.rs
│   └── record.rs
└── storage/            # Storage layer
    ├── mod.rs
    └── sanakirja.rs
```

### Trait-Based Design

Extensive use of traits for abstraction:

```rust
pub trait MutTxnT: TxnT {
    type GraphError: 'static;
    
    fn commit(self) -> Result<(), Self::GraphError>;
    fn rollback(self) -> Result<(), Self::GraphError>;
}

pub trait ChannelMutTxnT: ChannelTxnT + MutTxnT {
    fn touch_channel(&mut self, channel: &mut ChannelRef) -> Result<(), Self::GraphError>;
    fn add_file(&mut self, file: String) -> Result<(), Self::GraphError>;
}
```

### Re-export Patterns

Clean public API through strategic re-exports:

```rust
// In lib.rs
pub use crate::apply::{apply_change_arc, ApplyError, LocalApplyError};
pub use crate::pristine::{
    ArcTxn, Base32, ChangeId, ChannelRef, Hash, Inode, Merkle, 
    MutTxnT, TxnT, Vertex,
};
pub use crate::record::Builder as RecordBuilder;
```

## Development Guidelines

### Code Quality Standards

1. **Formatting**: Use `rustfmt` with project-specific configuration
2. **Linting**: Enable comprehensive clippy lints:
   ```rust
   #![deny(clippy::all)]
   #![warn(clippy::pedantic)]
   #![warn(clippy::nursery)]
   #![warn(clippy::cargo)]
   ```

3. **Documentation**: Comprehensive doc comments with examples:
   ```rust
   /// Creates a new change record.
   ///
   /// # Arguments
   /// * `files` - Files to include in the change
   /// * `message` - Commit message
   ///
   /// # Examples
   /// ```
   /// let change = create_change(vec!["file.rs"], "Add feature")?;
   /// ```
   ///
   /// # Errors
   /// Returns `RecordError` if files cannot be accessed.
   pub fn create_change(files: Vec<&str>, message: &str) -> Result<Change, RecordError> {
       // Implementation
   }
   ```

### Dependency Management

1. **Feature Flags**: Use cargo features for optional functionality:
   ```toml
   [features]
   default = ["ondisk-repos", "text-changes"]
   ondisk-repos = ["mmap", "zstd", "ignore"]
   git-import = ["git2"]
   ```

2. **Version Consistency**: Maintain consistent dependency versions across workspace
3. **Minimal Dependencies**: Only include necessary dependencies

### Git Workflow

1. **Conventional Commits**: Use conventional commit format
2. **Branch Naming**: Use descriptive branch names (`feature/`, `fix/`, `refactor/`)
3. **Small Commits**: Keep commits focused and atomic
4. **Testing**: Ensure all tests pass before committing

## Testing Strategy

### Unit Testing

Comprehensive unit tests with quickcheck property testing:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck_macros::quickcheck;

    #[quickcheck]
    fn hash_roundtrip(data: Vec<u8>) -> bool {
        let hash = Hash::from_bytes(&data);
        hash.to_bytes() == data
    }

    #[test]
    fn test_config_loading() {
        let config = Config::load().unwrap();
        assert!(config.author.username.len() > 0);
    }
}
```

### Integration Testing

Full integration tests in `tests/` directories:

```rust
// tests/integration_test.rs
#[test]
fn test_full_workflow() {
    let repo = test_repo().unwrap();
    
    // Create a change
    let change = record_change(&repo, "test.txt", "Hello").unwrap();
    
    // Apply the change
    apply_change(&repo, &change).unwrap();
    
    // Verify the result
    assert_eq!(read_file(&repo, "test.txt").unwrap(), "Hello");
}
```

### Property-Based Testing

Use QuickCheck for property-based testing:

```rust
#[quickcheck]
fn merge_commutative(changes1: Vec<Change>, changes2: Vec<Change>) -> bool {
    let result1 = merge_changes(changes1.clone(), changes2.clone());
    let result2 = merge_changes(changes2, changes1);
    result1 == result2
}
```

## Performance Considerations

### Memory Management

1. **Arc for Shared Data**: Use `Arc<T>` for shared immutable data
2. **Cow for Clone-on-Write**: Use `Cow<T>` for potentially borrowed data
3. **Pool Resources**: Pool expensive resources like database connections

### Database Optimization

1. **Batch Operations**: Group database operations for efficiency
2. **Transaction Management**: Use appropriate transaction scope
3. **Index Strategy**: Ensure proper indexing for query performance

```rust
// Batch operations example
pub fn batch_apply_changes(
    txn: &mut impl MutTxnT,
    changes: &[Change],
) -> Result<(), ApplyError> {
    // Begin transaction
    let mut batch_txn = txn.begin_batch()?;
    
    for change in changes {
        apply_single_change(&mut batch_txn, change)?;
    }
    
    // Commit all changes at once
    batch_txn.commit()
}
```

### Async Considerations

For I/O operations, use async patterns:

```rust
#[async_trait]
pub trait RemoteRepoT: Send + Sync + 'static {
    async fn download_changelist(
        &mut self,
        from: u64,
    ) -> Result<Option<(u64, Vec<DownloadChange>)>, anyhow::Error>;
    
    async fn upload_changes(
        &mut self,
        changes: Vec<Change>,
    ) -> Result<(), anyhow::Error>;
}
```

## Environment Variable Detection Patterns

### Factory-Based Environment Detection

Atomic implements environment variable detection using the factory pattern with caching for performance:

```rust
/// Factory for creating attribution contexts from environment variables
pub struct AttributionDetector {
    /// Configuration for AI attribution
    config: AttributionConfig,
    /// Cached environment variables
    env_cache: HashMap<String, String>,
}

impl AttributionDetector {
    /// Factory method with environment variable caching
    pub fn new(config: AttributionConfig) -> Self {
        let env_cache = Self::cache_environment_variables();
        Self { config, env_cache }
    }

    /// Cache relevant environment variables for performance
    fn cache_environment_variables() -> HashMap<String, String> {
        let mut cache = HashMap::new();
        
        let env_vars = [
            "ATOMIC_AI_ENABLED",
            "ATOMIC_AI_PROVIDER", 
            "ATOMIC_AI_MODEL",
            // ... other variables
        ];

        for var in &env_vars {
            if let Ok(value) = env::var(var) {
                cache.insert(var.to_string(), value);
            }
        }
        cache
    }
}
```

### Environment Variable Naming Convention

Use consistent prefixing and descriptive naming:

```rust
pub mod env_vars {
    /// Enable AI attribution tracking
    pub const ATOMIC_AI_ENABLED: &str = "ATOMIC_AI_ENABLED";
    /// AI provider name
    pub const ATOMIC_AI_PROVIDER: &str = "ATOMIC_AI_PROVIDER";
    /// AI model name
    pub const ATOMIC_AI_MODEL: &str = "ATOMIC_AI_MODEL";
    /// AI confidence score
    pub const ATOMIC_AI_CONFIDENCE: &str = "ATOMIC_AI_CONFIDENCE";
}
```

### Best Practices for Environment Detection

1. **Prefix Consistency**: Use consistent prefixes (e.g., `ATOMIC_AI_`)
2. **Performance Caching**: Cache environment variables at initialization
3. **Graceful Fallbacks**: Provide sensible defaults when variables are missing
4. **Type Safety**: Parse and validate environment variable values
5. **Documentation**: Document all environment variables with examples

## CLI Integration Patterns

### Value Enum Pattern for CLI Flags

Use `clap`'s `ValueEnum` for type-safe CLI flag parsing:

```rust
#[derive(Debug, Clone, ValueEnum)]
pub enum AISuggestionType {
    /// AI generated the entire patch
    Complete,
    /// AI suggested, human modified
    Partial,
    /// Human started, AI completed
    Collaborative,
}

impl From<AISuggestionType> for SuggestionType {
    fn from(cli_type: AISuggestionType) -> Self {
        match cli_type {
            AISuggestionType::Complete => SuggestionType::Complete,
            AISuggestionType::Partial => SuggestionType::Partial,
            AISuggestionType::Collaborative => SuggestionType::Collaborative,
        }
    }
}
```

### Configuration-Driven CLI Design

Integrate CLI flags with the configuration system:

```rust
#[derive(Parser, Debug)]
pub struct Record {
    /// Mark this change as AI-assisted
    #[clap(long = "ai-assisted")]
    pub ai_assisted: bool,
    /// Specify the AI provider
    #[clap(long = "ai-provider")]
    pub ai_provider: Option<String>,
    /// Specify the AI model
    #[clap(long = "ai-model")]
    pub ai_model: Option<String>,
}
```

### Environment Variable Injection Pattern

Set environment variables from CLI flags for consistent detection:

```rust
// Setup environment variables from CLI flags if provided
if self.ai_assisted || self.ai_provider.is_some() {
    if self.ai_assisted {
        std::env::set_var("ATOMIC_AI_ENABLED", "true");
    }
    if let Some(ref provider) = self.ai_provider {
        std::env::set_var("ATOMIC_AI_PROVIDER", provider);
    }
}
```

### Parameter Threading Pattern

Thread configuration through function calls without breaking existing APIs:

```rust
fn record<T, C>(
    mut self,
    txn: ArcTxn<T>,
    channel: ChannelRef<T>,
    working_copy: &FileSystem,
    changes: &C,
    repo_path: CanonicalPathBuf,
    header: ChangeHeader,
    extra_deps: &[Hash],
    repo_config: &Config,  // <- New parameter threaded through
) -> Result<...> {
    // Use repo_config for attribution detection
}
```

### CLI Integration Best Practices

1. **Non-Breaking Changes**: Add new CLI flags without breaking existing functionality
2. **Value Enums**: Use `ValueEnum` for type-safe flag parsing
3. **Environment Bridge**: Set environment variables from CLI flags for unified detection
4. **Configuration Integration**: Connect CLI flags to configuration system
5. **Parameter Threading**: Thread configuration through existing APIs cleanly
6. **Validation**: Validate CLI flag combinations and values
7. **Help Documentation**: Provide clear help text for all new flags

---

## HTTP API Protocol Alignment

### Golden Rule: Follow the SSH Protocol

**The HTTP API MUST behave exactly like the SSH protocol** - it's just a different transport mechanism. The underlying logic for handling changes, tags, and repository operations must be identical.

When implementing any HTTP API feature, **first check how the SSH protocol (`atomic/src/commands/protocol.rs`) handles it**, then replicate that exact behavior.

### The Protocol Handler Pattern

All remote protocols (SSH, HTTP, Local) use the same server-side protocol handler found in `atomic/src/commands/protocol.rs`. This handler defines the canonical behavior for:

- Uploading changes (`apply` command)
- Uploading tags (`tagup` command)
- Downloading changes (`change` command)
- Downloading tags (`tag` command)
- Getting changelists (`changelist` command)
- Querying state (`state` command)
- Creating archives (`archive` command)

```rust
// Example: SSH protocol handler for tag upload (protocol.rs)
if let Some(cap) = TAGUP.captures(&buf) {
    // 1. Parse state from command
    if let Some(state) = Merkle::from_base32(cap[1].as_bytes()) {
        let channel = load_channel(&*txn.read(), &cap[2])?;
        
        // 2. Read SHORT tag data from client
        let size: usize = cap[3].parse().unwrap();
        let mut buf = vec![0; size];
        s.read_exact(&mut buf)?;
        let header = libatomic::tag::read_short(std::io::Cursor::new(&buf[..]), &m)?;
        
        // 3. Server REGENERATES full tag file from its own channel state
        let mut w = std::fs::File::create(&temp_path)?;
        libatomic::tag::from_channel(&*txn.read(), &cap[2], &header, &mut w)?;
        
        // 4. Save and update database
        std::fs::rename(&temp_path, &tag_path)?;
        txn.write().put_tags(&mut channel.write().tags, last_t.into(), &m)?;
    }
}
```

### Key Protocol Patterns

#### 1. Tag Upload (tagup)

**Pattern**: Client sends minimal data, server is authoritative

```rust
// HTTP API MUST follow this pattern (atomic-api/src/server.rs):
if let Some(tagup_hash) = params.get("tagup") {
    let state = Merkle::from_base32(tagup_hash.as_bytes())?;
    
    // 1. Parse SHORT tag header from client
    let header = libatomic::tag::read_short(std::io::Cursor::new(&body[..]), &state)?;
    
    // 2. REGENERATE full tag file from server's channel state
    let mut w = std::fs::File::create(&temp_path)?;
    libatomic::tag::from_channel(&txn, channel_name, &header, &mut w)?;
    std::fs::rename(&temp_path, &tag_path)?;
    
    // 3. Update database
    txn.put_tags(&mut channel.write().tags, last_t.into(), &state)?;
}
```

**Why**: Server generates authoritative tag files from its own database, eliminating corruption and version mismatches.

#### 2. Tag Download (tag)

**Pattern**: Send only the header (short version)

```rust
// HTTP API MUST send short version (atomic-api/src/server.rs):
if let Some(tag_hash) = params.get("tag") {
    let state = Merkle::from_base32(tag_hash.as_bytes())?;
    let mut tag = libatomic::tag::OpenTagFile::open(&tag_path, &state)?;
    
    // Send SHORT version, not full file
    let mut buf = Vec::new();
    tag.short(&mut buf)?;
    
    // Protocol format: <8 bytes length><short data>
    response.write_u64::<BigEndian>(buf.len() as u64)?;
    response.write_all(&buf)?;
}
```

**Why**: Reduces bandwidth, matches protocol, client doesn't need full channel state.

#### 3. Change Upload (apply)

**Pattern**: Write file, apply to channel, output to working copy

```rust
// HTTP API MUST follow this pattern (atomic-api/src/server.rs):
if let Some(apply_hash) = params.get("apply") {
    // 1. Write change file
    std::fs::write(&change_path, &body)?;
    
    // 2. Apply to channel
    let txn = repository.pristine.arc_txn_begin()?;
    let mut channel = txn.write().open_or_create_channel(channel_name)?;
    txn.write().apply_change_rec(&repository.changes, &mut channel.write(), &hash)?;
    
    // 3. Output to working copy (SSH protocol does this!)
    libatomic::output::output_repository_no_pending(
        &repository.working_copy,
        &repository.changes,
        &txn,
        &channel,
        "",
        true,
        None,
        num_workers,
        0,
    )?;
    
    // 4. Commit
    txn.commit()?;
}
```

**Why**: Server working copy reflects pushed changes, matching SSH behavior.

#### 4. Changelist Format

**Pattern**: Use trailing dot for tagged entries

```rust
// HTTP API MUST use this exact format (atomic-api/src/server.rs):
for (n, hash, merkle) in txn.log(&*channel.read(), from)? {
    let is_tagged = txn.is_tagged(txn.tags(&*channel.read()), n)?;
    
    if is_tagged {
        writeln!(response, "{}.{}.{}.", n, hash.to_base32(), merkle.to_base32())?;
    } else {
        writeln!(response, "{}.{}.{}", n, hash.to_base32(), merkle.to_base32())?;
    }
}
```

**Why**: Clients recognize tagged changes by trailing dot, consistent across protocols.

### HTTP API Architecture

The HTTP API in `atomic-api/src/server.rs` should be a **thin transport wrapper** that:

1. Validates tenant/portfolio/project paths
2. Opens the repository
3. Delegates to the **same logic** as SSH protocol
4. Returns results in HTTP format

```rust
// Good: Reuses existing protocol logic
async fn post_atomic_protocol(params: Query<Params>, body: Bytes) -> Result<Response> {
    let repository = Repository::find_root(repo_path)?;
    
    // Use EXACT same logic as protocol.rs
    if let Some(apply_hash) = params.get("apply") {
        // ... same apply logic as SSH protocol
    } else if let Some(tagup_hash) = params.get("tagup") {
        // ... same tagup logic as SSH protocol
    }
}

// Bad: Reimplementing protocol logic differently
async fn custom_apply_handler() {
    // DON'T create new apply logic that differs from protocol.rs
}
```

### Common Anti-Patterns to Avoid

❌ **Don't**: Send full tag files when downloading
```rust
// WRONG - wastes bandwidth, violates protocol
let tag_data = std::fs::read(&tag_path)?;
response.write_all(&tag_data)?;
```

✅ **Do**: Send short version
```rust
// CORRECT - matches SSH protocol
let mut tag = OpenTagFile::open(&tag_path, &state)?;
tag.short(&mut response)?;
```

---

❌ **Don't**: Write client tag data directly
```rust
// WRONG - client data may be corrupted/incorrect
std::fs::write(&tag_path, &body)?;
```

✅ **Do**: Regenerate from server state
```rust
// CORRECT - server is authoritative
let header = read_short(Cursor::new(&body[..]), &state)?;
let mut w = File::create(&temp_path)?;
from_channel(&txn, channel_name, &header, &mut w)?;
std::fs::rename(&temp_path, &tag_path)?;
```

---

❌ **Don't**: Skip working copy output
```rust
// WRONG - server files don't reflect pushed changes
txn.apply_change_rec(&changes, &mut channel, &hash)?;
txn.commit()?;
```

✅ **Do**: Output to working copy
```rust
// CORRECT - matches SSH protocol behavior
txn.apply_change_rec(&changes, &mut channel, &hash)?;
output_repository_no_pending(&working_copy, &changes, &txn, &channel, ...)?;
txn.commit()?;
```

### Testing Protocol Alignment

When testing HTTP API endpoints, verify they match SSH protocol behavior:

```rust
#[test]
fn test_tagup_matches_ssh_protocol() {
    // 1. Create tag via SSH protocol
    run_ssh_protocol("tagup STATE channel 100\n<short_data>");
    
    // 2. Create tag via HTTP API
    http_post("?tagup=STATE&to_channel=channel", short_data);
    
    // 3. Verify both produce IDENTICAL results
    let ssh_tag = read_tag_file(&ssh_repo);
    let http_tag = read_tag_file(&http_repo);
    assert_eq!(ssh_tag, http_tag);
}
```

### Benefits of Protocol Alignment

1. **Consistency**: All remotes behave identically
2. **Correctness**: Reuses tested protocol logic
3. **Maintainability**: Single source of truth
4. **Simplicity**: Less code to maintain
5. **Reliability**: Fewer edge cases and bugs

### Reference Implementation

See `atomic/docs/HTTP-API-PROTOCOL-COMPARISON.md` for detailed comparison of SSH vs HTTP implementations.

---

## Summary

This document outlines the comprehensive best practices and architectural patterns used in the Atomic VCS project. The emphasis on configuration-driven design, systematic use of factory and singleton patterns, DRY principles through sophisticated macro systems, and robust error handling creates a maintainable, performant, and extensible codebase.

Key takeaways for contributors:

1. **Configuration First**: Always prefer configuration over hardcoded behavior
2. **Type Safety**: Leverage Rust's type system for correctness
3. **Macro-Driven DRY**: Use macros to eliminate repetitive code
4. **Error Transparency**: Provide clear, actionable error messages
5. **Modular Design**: Keep crates focused and dependencies minimal
6. **Testing**: Comprehensive testing including property-based testing
7. **Performance**: Consider memory and I/O efficiency in design decisions
8. **Environment Detection**: Use factory pattern with caching for environment variable detection
9. **CLI Integration**: Extend CLI interfaces without breaking existing functionality
10. **Parameter Threading**: Thread configuration through APIs cleanly and non-invasively
11. **HTTP API Protocol Alignment**: Always check `protocol.rs` first, then replicate that exact behavior in the HTTP API

Following these patterns ensures consistency with the existing codebase and maintains the high quality standards established in this project.