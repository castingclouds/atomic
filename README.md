# Atomic

A mathematically sound distributed version control system that revolutionizes software development through four breakthrough innovations: hybrid patch/snapshot architecture, cryptographic AI attestation, unified node-based DAG, and tag-based dependency consolidation.

## 🚀 Revolutionary Architecture: Four Core Innovations

### 1. Hybrid Patch/Snapshot Model with Tag Consolidation
**The Problem**: Traditional VCS systems force a choice between patches (semantic precision, exponential complexity) or snapshots (scalable but lossy).

**Atomic's Innovation**: A hybrid model that provides **patch-level semantic precision within development cycles** and **snapshot-like scalability across cycles** through mathematical tag consolidation.

```
Timeline Visualization:

Patch Development Phase:
├─ Change A [root]                    (1 dependency: root)
├─ Change B [A]                       (1 dependency: A)
├─ Change C [A,B]                     (2 dependencies)
├─ Change D [A,B,C]                   (3 dependencies)
└─ Change E [A,B,C,D]                 (4 dependencies: 10 total)

🏷️ TAG v1.0 = Mathematical Snapshot[A,B,C,D,E]

Next Development Phase:
├─ Change F [TAG v1.0] ✨              (1 dependency: equivalent to 5!)
├─ Change G [F]                       (1 dependency)
├─ Change H [F,G]                     (2 dependencies)
└─ Change I [F,G,H]                   (3 dependencies: 7 total vs 21!)
```

**Dependency Reduction Mathematics**:
- **Before Tags**: With 100 changes = 5,050 total dependencies (O(n²))
- **With Tags**: With 100 changes + consolidation = ~200 dependencies (O(n))
- **Reduction**: 96% reduction in dependency complexity

**Result**: Semantic precision where it matters + infinite scalability + O(1) dependency access.

### 2. AI Attestation with Merkle Cryptographic Signatures
**The Problem**: AI contributions lack cryptographic verification and attribution integrity across distributed operations.

**Atomic's Innovation**: **Merkle-tree-based cryptographic attestation** where AI contributions are mathematically verifiable and tamper-evident.

```rust
// Cryptographically signed AI attestation
struct AIAttestation {
    change_hash: Hash,           // Merkle hash of the change
    ai_provider: String,         // "openai", "anthropic", etc.
    model: String,              // "gpt-4", "claude-3", etc.
    attestation_hash: Hash,     // Cryptographic proof
    signature: Signature,       // Merkle signature chain
    confidence: f64,            // Algorithmic confidence
    timestamp: u64,             // Cryptographic timestamp
}

// Verification
fn verify_ai_attestation(attestation: &AIAttestation) -> bool {
    merkle_verify_chain(&attestation.signature, &attestation.change_hash)
}
```

**Features**:
- ✅ **Cryptographic Integrity**: Merkle signatures prevent tampering
- ✅ **Distributed Verification**: Anyone can verify AI attestations
- ✅ **Attribution Preservation**: Atomic doesn't use merges, conflicts, and rebase operations (Patches)
- ✅ **Audit Compliance**: Mathematically provable AI contribution history

### 3. Node-Based DAG with Mathematical Correctness
**The Problem**: Traditional VCS treats different entities (commits, tags, branches) as separate systems, causing complexity and inconsistency.

**Atomic's Innovation**: **Unified node-based dependency DAG** where both changes and tags are first-class nodes with mathematical guarantees.

```rust
// Unified node system
pub enum NodeType {
    Change = 0,  // Semantic patch with hunks
    Tag = 1,     // Consolidating snapshot
}

pub struct Node {
    pub hash: Hash,              // Universal cryptographic ID
    pub node_type: NodeType,     // Type-safe operations
    pub state: Merkle,          // Mathematical state proof
}

// Mathematical properties guaranteed:
// - Commutativity: A + B = B + A
// - Associativity: (A + B) + C = A + (B + C)
// - Consistency: Same change produces same result
// - Completeness: All dependencies are resolvable
```

**Operations**:
```bash
# All operations work on any node type
atomic apply <change-hash>     # Apply change node
atomic apply <tag-hash>        # Apply tag node (consolidated changes)
atomic dependencies <any-hash> # Works for changes OR tags
atomic log <any-hash>          # Unified history view
```

**Mathematical Guarantees**:
- 🧮 **Commutative Operations**: Changes can be applied in any order
- 🧮 **Associative Grouping**: Dependency grouping doesn't affect results
- 🧮 **Consistency Proofs**: Same logical change works across all compatible states
- 🧮 **Conflict-Free Resolution**: Automatic semantic conflict resolution

### 4. Workflow State Management as Change Record Metadata
**The Problem**: Traditional VCS separates workflow management from version control, requiring external systems that lose context and create synchronization issues.

**Atomic's Innovation**: **Workflow state stored directly in change record metadata** with type-safe Rust-based workflow definitions and real-time state management.

**Type-Safe Workflow DSL**:
```rust
// Compile-time verified workflow definitions
simple_workflow! {
    name: "EnterpriseApproval",
    initial_state: Recorded,
    states: {
        Recorded { name: "Locally Recorded" }
        SecurityReview { name: "Security Team Review" }
        CodeReview { name: "Code Review" }
        Approved { name: "Approved for Production" }
        Rejected { name: "Changes Required" }
    },
    transitions: {
        Recorded -> SecurityReview {
            needs_role: "developer",
            trigger: "submit_for_review",
        }
        SecurityReview -> CodeReview {
            needs_role: "security_reviewer",
            trigger: "security_approve",
        }
        CodeReview -> Approved {
            needs_role: "code_reviewer",
            trigger: "approve",
        }
    }
}
```

**Workflow State in Change Records**:
```rust
// Workflow metadata embedded in change header
pub struct ChangeHeader {
    // Standard change metadata
    pub dependencies: Vec<Hash>,
    pub message: String,
    pub timestamp: u64,

    // Workflow state metadata
    pub workflow_state: Option<WorkflowState>,
    pub approval_history: Vec<ApprovalEvent>,
    pub required_approvals: Vec<String>,
}

pub struct WorkflowState {
    pub workflow_name: String,
    pub current_state: String,
    pub state_timestamp: u64,
    pub assigned_reviewers: Vec<String>,
    pub approval_metadata: HashMap<String, String>,
}
```

**Workflow Operations**:
```bash
# Record change with workflow state
atomic record -m "Add authentication" --workflow EnterpriseApproval

# Transition workflow state
atomic workflow approve --change <hash> --reviewer security-team
atomic workflow transition --change <hash> --to CodeReview

# Query workflow status
atomic workflow status --pending --reviewer alice
atomic workflow history --change <hash> --detailed

# Batch workflow operations
atomic workflow approve-batch --reviewer-role security --pattern "security: *"
```

**Benefits**:
- ✅ **Embedded State**: Workflow state travels with changes across distributed systems
- ✅ **Cryptographic Integrity**: Workflow state is part of change hash, tamper-evident
- ✅ **Type Safety**: Compile-time workflow validation prevents invalid state transitions
- ✅ **Audit Trail**: Complete workflow history preserved in change metadata
- ✅ **Distributed**: No external workflow service required, works offline

## 🎯 Business Value Propositions

### AI-Scale Development
- **100+ AI Agents**: Coordinate simultaneously without conflicts
- **Mathematical Correctness**: Guaranteed consistent results across agents
- **Cryptographic Attribution**: Verify AI contributions with mathematical certainty
- **Selective Integration**: Apply only the AI changes you trust

### Enterprise Production Workflows
- **Hotfix Automation**: Apply security fixes to all affected versions automatically
- **Audit Compliance**: Cryptographically verifiable change history
- **Attribution Intelligence**: Real-time AI contribution analytics
- **Mathematical Guarantees**: Provably correct operations

### Developer Experience
- **No Merge Conflicts**: Mathematical conflict resolution
- **Infinite Scalability**: O(n) complexity instead of O(n²)
- **Semantic Operations**: Work with meaning, not just files
- **Universal Operations**: Same commands work for changes and tags

## Project Structure

### Core Components
- **`atomic/`** - Main CLI application with commands for record, apply, push, pull
- **`libatomic/`** - Core VCS engine with mathematical patch operations
- **`atomic-macros/`** - Procedural macros for database operations
- **`atomic-config/`** - Configuration management with hierarchical loading

### Supporting Libraries
- **`atomic-identity/`** - Cryptographic identity and credential management
- **`atomic-interaction/`** - User interface and interaction patterns
- **`atomic-remote/`** - Remote repository operations (SSH, HTTP, Local)
- **`atomic-repository/`** - Repository management and working copy operations

### Development and Build
- **`contrib/`** - Additional resources and example configurations

## Key Features

- **Mathematical Soundness**: Based on theory of asynchronous work with formal guarantees
- **Cryptographic Security**: Merkle trees for integrity, signatures for attestation
- **AI Attestations**: First-class AI contribution tracking with verification
- **Distributed**: Fully distributed with no central authority required
- **Performance**: Sanakirja database backend optimized for VCS operations
- **Node Unification**: Changes and tags as unified graph nodes
- **Tag Consolidation**: O(n²) to O(n) dependency reduction

## Getting Started

### Installation

#### From Source
```bash
git clone https://github.com/castingclouds/atomic.git
cd atomic
cargo build --release --bin atomic --bin atomic-api
sudo cp target/release/atomic /usr/local/bin/
sudo cp target/release/atomic-api /usr/local/bin/
```

#### Package Managers (Coming Soon!)
```bash
# macOS
brew install atomic-vcs

# Ubuntu/Debian
curl -fsSL https://atomic-vcs.com/install.sh | sudo bash

# Arch Linux
yay -S atomic-vcs
```

### Basic Commands

#### Create a Repository
```bash
atomic init myproject
cd myproject
```

#### Track Files and Record Changes
```bash
# Add files to tracking
atomic add README.md src/main.rs

# Record a change (creates a patch)
atomic record -m "Initial implementation"

# Record with AI attestation
atomic record --ai-assisted --ai-provider openai -m "AI-generated documentation"
```

#### Create Consolidating Tags
```bash
# Create a consolidating tag (mathematical snapshot)
atomic tag create v1.0 -m "First release"

# Continue development with minimal dependencies
atomic record -m "Post-release feature"  # Depends only on TAG v1.0
```

#### View History and Attribution
```bash
# View change history
atomic log

# View AI attribution analysis
atomic attribution --stats --providers

# Verify cryptographic signatures
atomic verify --ai-attestations
```

#### Collaborate with Remotes
```bash
# Add remote repository
atomic remote add origin ssh://lee@beatomic.dev/user/portfolio/project/code

# Push changes and tags
atomic push origin

# Pull changes from remote
atomic pull origin
```

### Working with Channels
Channels provide filtered views of the DAG:

```bash
# Create feature channel
atomic fork main feature-auth

# Work on feature
atomic record -m "Add authentication" --channel feature-auth

# Merge back to main
atomic apply --channel main <auth-changes>
```

### Mathematical Operations
```bash
# Apply changes in any order (commutativity)
atomic apply <change-1> <change-2>  # Same result as:
atomic apply <change-2> <change-1>

# Group dependencies (associativity)
atomic apply --group <tag-1> <changes>  # Same result as:
atomic apply <tag-1> && atomic apply <changes>

# Verify mathematical properties
atomic verify --mathematical-properties
```

## Advanced Features

### Cryptographic AI Attestation
```bash
# Configure AI attestation
export ATOMIC_AI_ENABLED=true
export ATOMIC_AI_PROVIDER=anthropic
export ATOMIC_AI_MODEL=claude-4.5

# All changes now include cryptographic attestation
atomic record -m "AI-assisted refactoring"

# Verify attestations
atomic verify --ai-attestations --provider anthropic
atomic attribution --confidence-analysis --cryptographic-proofs
```

### Production Hotfix Workflows
```bash
# Apply hotfix to historical tag and propagate
atomic hotfix --target-tag v1.0 --security-fix CVE-2024-1234
# Creates: v1.0.1, v1.1.1, v1.2.1, current-dev-patched

# Selective hotfix application
atomic hotfix --target-tag v2.0 --changes <change-hash> --propagate-to v2.1,v2.2
```

### Node-Based Operations
```bash
# Query any node type
atomic log                   # Works for changes OR tags
atomic change <hash>         # Shows dependencies for any node
atomic log --from <hash>     # History from any node

# Apply any node type
atomic apply <change-hash>   # Apply semantic patch
atomic apply <tag-hash>      # Apply consolidated snapshot
```

## Server Setup and Remote Connections

### Setting Up a Server

#### SSH Server Setup
```bash
# Install atomic on server
curl -fsSL https://atomic-vcs.com/install.sh | sudo bash

# Create bare repository
sudo atomic init --bare /var/lib/atomic/myrepo.atomic
sudo chown -R git:git /var/lib/atomic

# Configure SSH access
echo 'command="atomic serve /var/lib/atomic" ssh-rsa AAAAB...' >> ~/.ssh/authorized_keys
```

#### HTTP Server Setup
```bash
# Run atomic API server
atomic-api serve --port 8080 --repos-dir /var/lib/atomic

# With SSL and authentication
atomic-api serve \
  --port 443 \
  --ssl-cert /etc/ssl/certs/atomic.crt \
  --ssl-key /etc/ssl/private/atomic.key \
  --auth-method jwt \
  --repos-dir /var/lib/atomic
```

### Connecting to Remote Repositories

#### Configure Remote Repositories
```bash
# SSH remote
atomic remote add origin ssh://git@example.com/path/to/repo.atomic

# HTTP remote
atomic remote add origin https://atomic.example.com/api/repo

# HTTP with authentication
atomic remote add origin https://atomic.example.com/api/repo
atomic config set remote.origin.headers.Authorization "Bearer YOUR_TOKEN"
```

#### Working with Remotes
```bash
# Push changes and tags
atomic push origin

# Push specific changes
atomic push origin <change-hash>

# Push consolidating tags
atomic push origin --tags --consolidating

# Pull latest changes
atomic pull origin

# Pull specific tags
atomic pull origin --tag v1.0

# Clone repository
atomic clone ssh://git@example.com/repo.atomic myrepo
```

### Authentication
Configure authentication for HTTP remotes:

```toml
# .atomic/config.toml
[remote.origin]
http = "https://atomic.example.com/api/repo"
headers.Authorization = "Bearer YOUR_JWT_TOKEN"
headers.X-API-Key = "your-api-key"
```

### Security Best Practices
- Use SSH keys or JWT tokens for authentication
- Enable SSL/TLS for HTTP remotes
- Regularly rotate API keys and tokens
- Verify cryptographic signatures on AI attestations
- Use consolidating tags to create verifiable checkpoints

## Storage Architecture

### Database Structure
Atomic uses Sanakirja, a transactional key-value store optimized for VCS operations:

- **ACID Properties**: Full transaction support with rollback capability
- **Copy-on-Write**: Efficient storage with automatic deduplication
- **Concurrent Access**: Multiple readers with exclusive writer
- **Cryptographic Hashing**: Merkle trees for integrity verification

### Key Database Components
- **Changes**: Semantic patches with dependencies and hunks
- **Tags**: Consolidating snapshots with Merkle state proofs
- **Nodes**: Unified graph with change and tag nodes
- **Dependencies**: Mathematical dependency DAG
- **AI Attestations**: Cryptographic proofs of AI contributions
- **Attribution**: Detailed AI contribution analytics

### Performance Characteristics
- **O(1) Tag Lookups**: Consolidating tags provide constant-time access
- **O(n) Dependency Resolution**: Linear complexity instead of exponential
- **Parallel Operations**: Concurrent read access with lock-free data structures
- **Efficient Storage**: Content-addressed storage with automatic compression

## Contributing

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details on:

- Code style and formatting requirements
- Test coverage expectations
- Pull request process
- Mathematical correctness verification
- Cryptographic security review process

## License

Licensed under the GNU General Public License v2.0. See [LICENSE](LICENSE) for details.

## The Future of Version Control: Mathematical Guarantees + AI Intelligence

Atomic represents the next evolution of version control systems - one that provides **mathematical guarantees** for correctness while enabling **AI-scale development** with **cryptographic attestation**.

Traditional systems force developers to choose between semantic precision or scalability. Atomic's hybrid patch/snapshot model, unified node architecture, and tag consolidation eliminate this trade-off forever.

**Key Differentiators**:
- 🧮 **Mathematical Soundness**: Formally proven correctness properties
- 🔐 **Cryptographic Security**: Merkle trees and signature verification
- 🤖 **AI-Native Design**: First-class AI attestation and coordination
- 📈 **Infinite Scalability**: O(n) complexity with tag consolidation
- 🎯 **Enterprise Ready**: Audit compliance and production workflows

### Ready for Production

Atomic is ready for enterprise adoption with:
- ✅ **Protocol Stability**: HTTP API aligned with SSH protocol
- ✅ **Mathematical Verification**: Formally verified correctness properties
- ✅ **Cryptographic Security**: Tamper-evident AI attestations
- ✅ **Performance Optimization**: Tag consolidation for O(n) complexity
- ✅ **Enterprise Features**: Audit trails, role-based access, compliance

## Acknowledgements

Built on the mathematical foundations of patch theory and distributed systems research. Special thanks to the Pijul project for pioneering patch-based version control and the research community advancing the theory of asynchronous work.

Atomic extends these foundations with cryptographic attestation, AI-native design, and enterprise workflow capabilities while maintaining mathematical correctness guarantees.

---

**Experience the future of version control. Try Atomic today.**
