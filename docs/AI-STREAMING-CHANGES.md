# AI Streaming Changes: Advanced AI Attestation Patterns

## Overview

This document describes an innovative approach to AI attestation in Atomic that goes beyond traditional record-based workflows. Instead of having AI make changes to files and then recording them later, we can have AI agents generate changes directly in Atomic's native format and stream them for immediate application with rich metadata.

## The Traditional vs Streaming Approach

### Traditional AI Workflow
```
AI modifies files → Human runs `atomic record --ai-assisted` → Basic AI flags added
```

**Limitations:**
- Coarse-grained attribution (entire changeset marked as AI-assisted)
- Limited metadata (just basic flags)
- No real-time tracking of AI decision-making
- Difficult to separate AI vs human contributions within a single recording session

### AI Streaming Change Workflow
```
AI generates atomic changes → Stream via TOML/Rust API → Apply with rich metadata → Continuous audit trail
```

**Advantages:**
- Granular attestation for each micro-change
- Rich metadata including confidence, reasoning, prompt context
- Real-time application and tracking
- Perfect separation of AI vs human contributions
- Advanced querying and review capabilities

## Implementation Methods

### Method 1: TOML Streaming

AI agents can generate changes directly in Atomic's text format with comprehensive metadata:

```toml
# AI-Generated Change Example
message = "Add comprehensive error handling to user input validation"
description = "Added try-catch blocks and logging based on static analysis of error paths"

[author]
name = "Claude-3.5-Sonnet"
email = "ai-assistant@anthropic.com"

# Rich AI attestation metadata
[metadata.ai_attestation]
provider = "anthropic"
model = "claude-3.5-sonnet-20241022"
confidence = 0.95
suggestion_type = "complete"
prompt_id = "prompt_abc123"
interaction_id = "session_def456"
reasoning = "Static analysis revealed 3 uncaught error paths that could cause panics"
human_modified = false
review_status = "pending"
prompt_hash = "SHA256:a1b2c3d4..."
context_tokens = 2048
generated_tokens = 156

# Optional: Multi-agent coordination
agent_role = "code_generator" 
depends_on_agents = []
collaboration_id = "collab_789"

# Optional: Prompt engineering context
[metadata.ai_attestation.prompt_context]
system_prompt_hash = "SHA256:sys_prompt_hash..."
user_prompt = "Add error handling to this function"
context_files = ["src/validation.rs", "tests/validation_tests.rs"]
previous_interaction_count = 3
conversation_turn = 5

[[changes]]
[changes.NewVertex]
up_context = ["fn validate_user_input(input: &str) -> Result<i32, ValidationError> {"]
down_context = ["    if input.is_empty() {"]
start = 67
end = 67
contents = '''    match input.parse::<i32>() {
        Ok(val) => {
            if val > 0 {
                Ok(val)
            } else {
                log::warn!("Non-positive input received: {}", input);
                Err(ValidationError::InvalidRange)
            }
        }
        Err(parse_err) => {
            log::error!("Failed to parse input '{}': {}", input, parse_err);
            Err(ValidationError::ParseError(parse_err))
        }
    }'''

[changes.NewVertex.inode]
pos = 123

# Additional change for imports
[[changes]]
[changes.NewVertex]
up_context = ["use std::fmt;"]
down_context = [""]
start = 0
end = 0
contents = "use log::{warn, error};"

[changes.NewVertex.inode]
pos = 456
```

**Application:**
```bash
# AI agent pipes change to atomic
ai-agent generate-change --function validate_user_input | atomic apply

# Or from file
atomic apply ai-generated-change.toml

# Or via HTTP API
curl -X POST http://localhost:8080/api/apply \
  -H "Content-Type: application/toml" \
  --data-binary @ai-change.toml
```

### Method 2: Rust API Integration (Zed Example)

Perfect for editor integration where AI assistance happens in real-time:

```rust
use libatomic::change::*;
use libatomic::attribution::*;
use libatomic::pristine::*;

/// AI suggestion with rich context
#[derive(Debug)]
pub struct AISuggestion {
    pub description: String,
    pub confidence: f64,
    pub reasoning: String,
    pub code_changes: Vec<CodeChange>,
    pub session_id: String,
    pub prompt_hash: String,
    pub model_info: ModelInfo,
    pub human_review_required: bool,
}

#[derive(Debug)]
pub struct CodeChange {
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub new_content: String,
    pub change_type: ChangeType, // Insert, Delete, Replace
}

#[derive(Debug)]
pub enum ChangeType {
    Insert,
    Delete, 
    Replace,
}

#[derive(Debug)]
pub struct ModelInfo {
    pub provider: String,
    pub model: String,
    pub version: String,
    pub context_window: usize,
    pub temperature: f32,
}

/// Zed AI Assistant Integration
pub struct ZedAtomicIntegration {
    repo: Repository,
    txn: ArcTxn<MutTxn>,
    channel: ChannelRef<MutTxn>,
}

impl ZedAtomicIntegration {
    /// Apply an AI suggestion immediately with full attestation
    pub async fn apply_ai_suggestion(
        &mut self, 
        suggestion: AISuggestion
    ) -> Result<Hash, Error> {
        let mut change = LocalChange::new();
        
        // Set change metadata
        change.hashed.header.message = suggestion.description.clone();
        change.hashed.header.description = Some(suggestion.reasoning.clone());
        change.hashed.header.authors = vec![Author {
            name: format!("{}-{}", suggestion.model_info.provider, suggestion.model_info.model),
            email: format!("ai@{}.com", suggestion.model_info.provider),
        }];
        
        // Create comprehensive AI attribution
        let attribution = SerializedAttribution {
            ai_assisted: true,
            provider: Some(suggestion.model_info.provider.clone()),
            model: Some(suggestion.model_info.model.clone()),
            confidence: Some(suggestion.confidence),
            suggestion_type: SuggestionType::Complete,
            interaction_id: Some(suggestion.session_id.clone()),
            prompt_hash: Some(suggestion.prompt_hash.clone()),
            reasoning: Some(suggestion.reasoning.clone()),
            human_review_status: if suggestion.human_review_required {
                ReviewStatus::Pending
            } else {
                ReviewStatus::AutoApproved
            },
            attribution_version: 1,
            // Extended metadata
            ai_metadata: Some(AIMetadata {
                provider: suggestion.model_info.provider,
                model: suggestion.model_info.model,
                version: Some(suggestion.model_info.version),
                context_window: Some(suggestion.model_info.context_window),
                temperature: Some(suggestion.model_info.temperature),
                generated_at: chrono::Utc::now(),
            }),
            author: Some(AuthorInfo {
                id: AuthorId::new(0), // AI author ID
                name: "AI Assistant".to_string(),
                email: "ai@zed.dev".to_string(),
                is_ai: true,
            }),
        };
        
        // Serialize attribution metadata
        change.hashed.metadata = bincode::serialize(&attribution)?;
        
        // Convert code changes to atomic hunks
        change.hashed.changes = self.convert_to_atomic_hunks(&suggestion.code_changes)?;
        change.contents = self.extract_content_bytes(&suggestion.code_changes);
        
        // Save and apply the change
        let hash = self.repo.changes.save_change(&mut change, |change, hash| {
            // Optional: Sign with AI attestation key
            change.unhashed = Some(serde_json::json!({
                "ai_attestation": {
                    "applied_by": "zed-ai-assistant",
                    "applied_at": chrono::Utc::now(),
                    "auto_applied": !suggestion.human_review_required
                }
            }));
            Ok::<_, anyhow::Error>(())
        })?;
        
        // Apply to current channel
        let inode_updates = self.calculate_inode_updates(&suggestion.code_changes)?;
        self.txn.write().apply_local_change(
            &mut self.channel, 
            &change, 
            &hash, 
            &inode_updates
        )?;
        
        // Notify user
        self.notify_change_applied(&hash, &suggestion).await?;
        
        Ok(hash)
    }
    
    /// Stream multiple AI suggestions with batching
    pub async fn stream_ai_suggestions(
        &mut self,
        suggestions: impl Stream<Item = AISuggestion>
    ) -> Result<Vec<Hash>, Error> {
        let mut applied_hashes = Vec::new();
        
        // Apply high-confidence suggestions immediately
        // Queue low-confidence ones for review
        suggestions.for_each(|suggestion| async {
            if suggestion.confidence > 0.9 && !suggestion.human_review_required {
                match self.apply_ai_suggestion(suggestion).await {
                    Ok(hash) => {
                        applied_hashes.push(hash);
                        println!("✓ Auto-applied AI suggestion: {}", hash.to_base32());
                    }
                    Err(e) => {
                        eprintln!("✗ Failed to apply suggestion: {}", e);
                    }
                }
            } else {
                self.queue_for_human_review(suggestion).await;
                println!("📋 Queued low-confidence suggestion for review");
            }
        }).await;
        
        Ok(applied_hashes)
    }
    
    /// Convert code changes to atomic format
    fn convert_to_atomic_hunks(
        &self, 
        code_changes: &[CodeChange]
    ) -> Result<Vec<Hunk<Option<Hash>, Local>>, Error> {
        // Implementation would convert CodeChange to Atomic's hunk format
        // This involves analyzing file structure and generating appropriate
        // NewVertex and EdgeMap operations
        todo!("Convert code changes to atomic hunks")
    }
    
    /// Extract content bytes for change
    fn extract_content_bytes(&self, code_changes: &[CodeChange]) -> Vec<u8> {
        code_changes.iter()
            .map(|change| change.new_content.as_bytes())
            .flatten()
            .cloned()
            .collect()
    }
    
    /// Calculate inode updates for file system sync
    fn calculate_inode_updates(
        &self, 
        code_changes: &[CodeChange]
    ) -> Result<HashMap<usize, InodeUpdate>, Error> {
        // Track which inodes are affected by the changes
        todo!("Calculate inode updates")
    }
    
    /// Queue suggestion for human review
    async fn queue_for_human_review(&self, suggestion: AISuggestion) {
        // Add to review queue - could be in-editor UI, separate tool, etc.
        todo!("Queue for human review")
    }
    
    /// Notify user of applied change
    async fn notify_change_applied(
        &self, 
        hash: &Hash, 
        suggestion: &AISuggestion
    ) -> Result<(), Error> {
        println!("🤖 Applied AI change: {} (confidence: {:.1}%)", 
                hash.to_base32(), 
                suggestion.confidence * 100.0);
        Ok(())
    }
}

/// Example usage in Zed
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut zed_atomic = ZedAtomicIntegration::new("path/to/repo")?;
    
    // Single suggestion
    let suggestion = AISuggestion {
        description: "Add null check to prevent panic".to_string(),
        confidence: 0.96,
        reasoning: "Static analysis found potential null pointer access".to_string(),
        code_changes: vec![/* ... */],
        session_id: "session_123".to_string(),
        prompt_hash: "SHA256:prompt_hash".to_string(),
        model_info: ModelInfo {
            provider: "anthropic".to_string(),
            model: "claude-3.5-sonnet".to_string(),
            version: "20241022".to_string(),
            context_window: 200000,
            temperature: 0.1,
        },
        human_review_required: false,
    };
    
    let hash = zed_atomic.apply_ai_suggestion(suggestion).await?;
    println!("Applied change: {}", hash.to_base32());
    
    Ok(())
}
```

## Advanced Workflows

### Multi-Agent Coordination

```toml
# Agent 1: Code generation
message = "Generate initial error handling structure"
[metadata.ai_attestation]
provider = "anthropic" 
model = "claude-3.5-sonnet"
agent_role = "code_generator"
collaboration_id = "multi_agent_session_456"
depends_on = []

# Agent 2: Code review and improvement
message = "Enhance error handling with better logging"
[metadata.ai_attestation]
provider = "openai"
model = "gpt-4"
agent_role = "code_reviewer"
collaboration_id = "multi_agent_session_456"
depends_on = ["ABC123DEF456"]  # Hash of Agent 1's change
review_verdict = "approved_with_enhancements"
improvements = ["Added structured logging", "Improved error messages"]
```

### Confidence-Based Auto-Application

```rust
// Only auto-apply high-confidence changes
match suggestion.confidence {
    conf if conf > 0.95 => {
        apply_ai_suggestion(suggestion).await?;
        println!("✓ High confidence - auto-applied");
    }
    conf if conf > 0.80 => {
        queue_for_quick_review(suggestion);
        println!("📋 Medium confidence - queued for quick review");
    }
    _ => {
        queue_for_thorough_review(suggestion);
        println!("🔍 Low confidence - queued for thorough review");
    }
}
```

### Prompt Engineering Integration

```toml
[metadata.ai_attestation.prompt_engineering]
system_prompt_hash = "SHA256:abc123..."
user_prompt = "Add comprehensive error handling with logging"
context_files = ["src/lib.rs", "tests/integration.rs", "docs/error_handling.md"]
previous_interaction_count = 3
conversation_turn = 5
context_similarity_score = 0.87
retrieval_augmented = true
knowledge_cutoff = "2024-04-01"
```

## Querying and Analysis

Once you have AI streaming changes, you get powerful querying capabilities:

```bash
# Show all AI changes
atomic log --ai-only

# Show changes by confidence level  
atomic log --ai-confidence-range 0.8..1.0

# Show changes from specific model
atomic log --ai-provider anthropic --ai-model claude-3.5-sonnet

# Show changes pending review
atomic log --review-status pending

# Get detailed attribution for specific change
atomic attribution --hash ABC123DEF --detailed --json

# Show collaboration between agents
atomic log --collaboration-id multi_agent_session_456

# Query by reasoning patterns
atomic log --ai-reasoning-contains "static analysis"

# Show human modifications of AI suggestions
atomic log --human-modified true
```

## Review and Approval Workflows

```bash
# Review pending AI changes
atomic review list --status pending

# Approve a change
atomic review approve --hash ABC123 --comment "Logic looks correct"

# Reject a change  
atomic review reject --hash DEF456 --reason "Edge case not handled"

# Bulk approve high-confidence changes
atomic review bulk-approve --confidence-min 0.95 --max-count 10

# Show review statistics
atomic review stats --timerange 7d
```

## Integration Examples

### GitHub Copilot + Atomic
```rust
// Intercept Copilot suggestions and convert to Atomic changes
pub fn copilot_to_atomic(copilot_suggestion: CopilotSuggestion) -> AISuggestion {
    AISuggestion {
        confidence: copilot_suggestion.confidence,
        model_info: ModelInfo {
            provider: "github".to_string(),
            model: "copilot".to_string(),
            // ...
        },
        // ... convert copilot format to atomic format
    }
}
```

### ChatGPT API Integration
```python
# Python script to convert ChatGPT responses to Atomic changes
import openai
import toml
import subprocess

def generate_atomic_change(prompt, file_content):
    response = openai.ChatCompletion.create(
        model="gpt-4",
        messages=[
            {"role": "system", "content": "You are a code assistant. Generate changes in Atomic TOML format."},
            {"role": "user", "content": f"Modify this code: {file_content}\nRequest: {prompt}"}
        ]
    )
    
    # Parse response and convert to TOML
    change_toml = convert_to_atomic_toml(response)
    
    # Apply via atomic
    result = subprocess.run(
        ["atomic", "apply"], 
        input=change_toml.encode(), 
        capture_output=True
    )
    
    return result.stdout.decode()
```

### MCP Tool Integration (Model Context Protocol)

The Model Context Protocol (MCP) enables seamless integration between AI models and tools like Atomic. Here's how to implement Atomic as an MCP tool for systems like Claude or Codex:

```typescript
// MCP Tool Definition for Atomic Streaming Changes
export interface AtomicMCPTool {
  name: "atomic_streaming_change";
  description: "Apply AI-generated changes directly to Atomic repository with full attestation";
  inputSchema: {
    type: "object";
    properties: {
      message: {
        type: "string";
        description: "Human-readable description of the change";
      };
      reasoning: {
        type: "string"; 
        description: "AI's reasoning for making this change";
      };
      confidence: {
        type: "number";
        minimum: 0;
        maximum: 1;
        description: "AI's confidence level in this change (0.0 to 1.0)";
      };
      changes: {
        type: "array";
        items: {
          type: "object";
          properties: {
            file_path: { type: "string" };
            line_start: { type: "number" };
            line_end: { type: "number" };
            new_content: { type: "string" };
            change_type: { 
              type: "string";
              enum: ["insert", "delete", "replace"];
            };
          };
        };
      };
      auto_apply: {
        type: "boolean";
        description: "Whether to apply immediately or queue for review";
      };
      session_context: {
        type: "object";
        properties: {
          conversation_id: { type: "string" };
          turn_number: { type: "number" };
          user_prompt: { type: "string" };
          context_files: {
            type: "array";
            items: { type: "string" };
          };
        };
      };
    };
    required: ["message", "reasoning", "confidence", "changes"];
  };
}

// MCP Tool Implementation
export class AtomicMCPHandler {
  constructor(private atomicRepo: string) {}

  async handleToolCall(input: any): Promise<MCPResult> {
    try {
      // Generate TOML change from MCP input
      const atomicChange = this.generateAtomicTOML(input);
      
      // Apply change through Atomic
      const result = await this.applyChange(atomicChange, input.auto_apply);
      
      return {
        success: true,
        result: {
          hash: result.hash,
          applied: result.applied,
          message: `Successfully ${result.applied ? 'applied' : 'queued'} change: ${input.message}`,
          confidence: input.confidence,
          review_required: !result.applied
        }
      };
    } catch (error) {
      return {
        success: false,
        error: `Failed to apply atomic change: ${error.message}`
      };
    }
  }

  private generateAtomicTOML(input: any): string {
    const timestamp = new Date().toISOString();
    
    const toml = `
# AI-Generated Change via MCP
message = "${input.message}"
description = "${input.reasoning}"

[author]
name = "MCP-AI-Assistant"
email = "mcp@ai-assistant.dev"

[metadata.ai_attestation]
provider = "mcp"
model = "unknown"  # MCP doesn't always provide model info
confidence = ${input.confidence}
suggestion_type = "complete"
interaction_id = "${input.session_context?.conversation_id || 'mcp_session'}"
reasoning = "${input.reasoning}"
human_modified = false
review_status = "${input.auto_apply && input.confidence > 0.9 ? 'auto_approved' : 'pending'}"
applied_via = "mcp_tool"
applied_at = "${timestamp}"

# MCP-specific metadata
[metadata.ai_attestation.mcp_context]
conversation_id = "${input.session_context?.conversation_id || ''}"
turn_number = ${input.session_context?.turn_number || 0}
user_prompt = "${input.session_context?.user_prompt || ''}"
context_files = ${JSON.stringify(input.session_context?.context_files || [])}
tool_version = "1.0.0"

${this.generateChangeBlocks(input.changes)}
`;
    return toml;
  }

  private generateChangeBlocks(changes: any[]): string {
    return changes.map((change, index) => `
[[changes]]
[changes.NewVertex]
up_context = []  # Would be populated from actual file analysis
down_context = []
start = ${change.line_start}
end = ${change.line_end}
contents = """${change.new_content}"""

[changes.NewVertex.inode]
pos = ${index + 1000}  # Generate appropriate inode positions
`).join('\n');
  }

  private async applyChange(tomlContent: string, autoApply: boolean): Promise<{hash: string, applied: boolean}> {
    const tempFile = `/tmp/atomic_change_${Date.now()}.toml`;
    
    // Write TOML to temp file
    await fs.writeFile(tempFile, tomlContent);
    
    try {
      // Apply through Atomic CLI
      const command = autoApply ? 
        `cd ${this.atomicRepo} && atomic apply ${tempFile}` :
        `cd ${this.atomicRepo} && atomic apply --queue-for-review ${tempFile}`;
      
      const result = await exec(command);
      const hash = this.extractHashFromOutput(result.stdout);
      
      return {
        hash,
        applied: autoApply
      };
    } finally {
      // Clean up temp file
      await fs.unlink(tempFile).catch(() => {});
    }
  }

  private extractHashFromOutput(output: string): string {
    const hashMatch = output.match(/Hash: ([A-Z0-9]+)/);
    return hashMatch ? hashMatch[1] : 'unknown';
  }
}

// Usage in Claude Desktop or similar MCP client
export const atomicMCPTool: MCPTool = {
  name: "atomic_streaming_change",
  handler: new AtomicMCPHandler(process.env.ATOMIC_REPO_PATH || "."),
  schema: /* AtomicMCPTool schema */
};
```

### Agent Tool Implementation for Codex/Claude

For direct integration with AI coding assistants:

```python
# Agent Tool for Claude/Codex Integration
import asyncio
import subprocess
import tempfile
import json
from typing import Dict, List, Any, Optional
from dataclasses import dataclass
from datetime import datetime

@dataclass
class CodeChange:
    file_path: str
    line_start: int
    line_end: int
    new_content: str
    change_type: str  # insert, delete, replace

@dataclass  
class AtomicChangeRequest:
    message: str
    reasoning: str
    confidence: float
    changes: List[CodeChange]
    session_id: str
    model_info: Dict[str, Any]
    auto_apply: bool = False

class AtomicAgentTool:
    """AI Agent Tool for streaming changes to Atomic with full attestation"""
    
    def __init__(self, repo_path: str = "."):
        self.repo_path = repo_path
        self.session_id = f"agent_session_{datetime.now().strftime('%Y%m%d_%H%M%S')}"
        
    async def apply_ai_change(self, request: AtomicChangeRequest) -> Dict[str, Any]:
        """Apply an AI-generated change with full attestation"""
        try:
            # Generate TOML representation
            toml_content = self._generate_atomic_toml(request)
            
            # Write to temporary file
            with tempfile.NamedTemporaryFile(mode='w', suffix='.toml', delete=False) as f:
                f.write(toml_content)
                temp_path = f.name
            
            # Apply through Atomic
            result = await self._apply_via_atomic(temp_path, request.auto_apply)
            
            # Clean up
            subprocess.run(['rm', temp_path], check=False)
            
            return {
                'success': True,
                'hash': result['hash'],
                'applied': result['applied'],
                'message': f"Applied change: {request.message}",
                'confidence': request.confidence,
                'session_id': self.session_id
            }
            
        except Exception as e:
            return {
                'success': False,
                'error': str(e),
                'message': f"Failed to apply change: {request.message}"
            }
    
    def _generate_atomic_toml(self, request: AtomicChangeRequest) -> str:
        """Generate Atomic TOML format from change request"""
        timestamp = datetime.utcnow().isoformat()
        
        # Base change metadata
        toml_lines = [
            f'# AI Agent Change - {timestamp}',
            f'message = "{request.message}"',
            f'description = "{request.reasoning}"',
            '',
            '[author]',
            f'name = "{request.model_info.get("provider", "AI")}-{request.model_info.get("model", "Assistant")}"',
            f'email = "ai-agent@{request.model_info.get("provider", "unknown").lower()}.com"',
            '',
            '[metadata.ai_attestation]',
            f'provider = "{request.model_info.get("provider", "unknown")}"',
            f'model = "{request.model_info.get("model", "unknown")}"',
            f'confidence = {request.confidence}',
            f'suggestion_type = "complete"',
            f'interaction_id = "{request.session_id}"',
            f'reasoning = "{request.reasoning}"',
            f'human_modified = false',
            f'review_status = "{"auto_approved" if request.auto_apply and request.confidence > 0.9 else "pending"}"',
            f'applied_via = "agent_tool"',
            f'applied_at = "{timestamp}"',
            ''
        ]
        
        # Agent-specific metadata
        if request.model_info:
            toml_lines.extend([
                '[metadata.ai_attestation.agent_context]',
                f'session_id = "{self.session_id}"',
                f'model_version = "{request.model_info.get("version", "unknown")}"',
                f'temperature = {request.model_info.get("temperature", 0.0)}',
                f'max_tokens = {request.model_info.get("max_tokens", 0)}',
                f'context_window = {request.model_info.get("context_window", 0)}',
                ''
            ])
        
        # Generate change blocks
        for i, change in enumerate(request.changes):
            toml_lines.extend([
                '[[changes]]',
                '[changes.NewVertex]',
                f'up_context = []  # File: {change.file_path}',
                f'down_context = []',
                f'start = {change.line_start}',
                f'end = {change.line_end}',
                f'contents = """{change.new_content}"""',
                '',
                '[changes.NewVertex.inode]',
                f'pos = {1000 + i}',
                ''
            ])
        
        return '\n'.join(toml_lines)
    
    async def _apply_via_atomic(self, toml_path: str, auto_apply: bool) -> Dict[str, Any]:
        """Apply change through Atomic CLI"""
        cmd = ['atomic', 'apply', toml_path]
        if not auto_apply:
            cmd.append('--queue-for-review')
        
        process = await asyncio.create_subprocess_exec(
            *cmd,
            cwd=self.repo_path,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE
        )
        
        stdout, stderr = await process.communicate()
        
        if process.returncode != 0:
            raise Exception(f"Atomic apply failed: {stderr.decode()}")
        
        # Extract hash from output
        output = stdout.decode()
        hash_match = None
        for line in output.split('\n'):
            if 'Hash:' in line:
                hash_match = line.split('Hash:')[1].strip()
                break
        
        return {
            'hash': hash_match or 'unknown',
            'applied': auto_apply,
            'output': output
        }

# Example usage with Claude API
class ClaudeAtomicIntegration:
    def __init__(self, claude_client, repo_path: str = "."):
        self.claude = claude_client
        self.atomic_tool = AtomicAgentTool(repo_path)
    
    async def process_code_request(self, user_prompt: str, context_files: List[str] = None) -> Dict[str, Any]:
        """Process a user's coding request through Claude with Atomic integration"""
        
        # Get Claude's response with change suggestions
        claude_response = await self.claude.messages.create(
            model="claude-3-5-sonnet-20241022",
            max_tokens=4000,
            system="You are a code assistant. When suggesting changes, be specific about file locations and provide confidence levels.",
            messages=[{
                "role": "user", 
                "content": f"Please suggest code changes for: {user_prompt}"
            }]
        )
        
        # Parse Claude's response into structured changes
        changes = self._parse_claude_response(claude_response.content, context_files)
        
        # Create Atomic change request
        request = AtomicChangeRequest(
            message=f"AI Code Changes: {user_prompt}",
            reasoning=claude_response.content,
            confidence=0.85,  # Could be extracted from Claude's response
            changes=changes,
            session_id=self.atomic_tool.session_id,
            model_info={
                "provider": "anthropic",
                "model": "claude-3-5-sonnet",
                "version": "20241022",
                "temperature": 0.0,
                "max_tokens": 4000,
                "context_window": 200000
            },
            auto_apply=False  # Always require review for safety
        )
        
        # Apply through Atomic
        result = await self.atomic_tool.apply_ai_change(request)
        
        return {
            'claude_response': claude_response.content,
            'atomic_result': result,
            'changes_applied': len(changes),
            'review_required': not request.auto_apply
        }
    
    def _parse_claude_response(self, response_content: str, context_files: List[str] = None) -> List[CodeChange]:
        """Parse Claude's response into structured code changes"""
        # This would contain logic to extract specific file changes from Claude's response
        # Implementation would depend on how Claude structures its code suggestions
        changes = []
        
        # Example parsing logic (would need to be more sophisticated)
        if "```" in response_content:
            # Extract code blocks and map to file changes
            # This is a simplified example
            changes.append(CodeChange(
                file_path="src/example.rs",
                line_start=10,
                line_end=15,
                new_content="// Claude's suggested code here",
                change_type="replace"
            ))
        
        return changes

# Usage example
async def main():
    # Initialize Claude client (pseudocode)
    claude_client = anthropic.Anthropic(api_key="your-api-key")
    
    # Create integration
    integration = ClaudeAtomicIntegration(claude_client, "/path/to/atomic/repo")
    
    # Process user request
    result = await integration.process_code_request(
        "Add error handling to the user input validation function",
        context_files=["src/validation.rs", "tests/validation_tests.rs"]
    )
    
    print(f"Applied {result['changes_applied']} changes")
    print(f"Review required: {result['review_required']}")
    if result['atomic_result']['success']:
        print(f"Change hash: {result['atomic_result']['hash']}")

if __name__ == "__main__":
    asyncio.run(main())
```

### MCP/Agent Tool Benefits

**MCP Integration Advantages:**
- **Universal Protocol**: Works with any MCP-compatible AI system
- **Structured Communication**: Standardized input/output format
- **Tool Discoverability**: AI systems can automatically discover Atomic capabilities
- **Error Handling**: Robust error reporting and recovery
- **Session Management**: Maintains context across interactions

**Agent Tool Advantages:**
- **Direct Integration**: No protocol overhead, direct API calls
- **Custom Logic**: Tailored behavior for specific AI systems
- **Rich Context**: Access to full conversation and model metadata  
- **Flexible Parsing**: Custom logic for interpreting AI responses
- **Performance**: Direct execution without protocol translation

**Use Cases:**
- **MCP**: Best for general-purpose AI assistants, IDE plugins, multi-tool workflows
- **Agent Tools**: Best for dedicated AI coding assistants, custom integrations, specialized workflows

## Benefits Summary

### For Development Teams
- **Perfect AI Audit Trail**: Know exactly what every AI assistant did
- **Granular Control**: Approve/reject individual AI suggestions
- **Quality Metrics**: Track AI suggestion quality over time
- **Collaborative Review**: Team can review AI contributions like human PRs

### For AI Research
- **Training Data**: Rich dataset of AI suggestions and human feedback
- **Model Evaluation**: Track which models perform best in real scenarios  
- **Prompt Engineering**: Correlate prompts with successful suggestions
- **Human-AI Interaction**: Study how humans modify AI suggestions

### For Compliance & Auditing
- **Complete Provenance**: Every line of code has full attribution chain
- **Regulatory Compliance**: Meet requirements for AI-generated content tracking
- **Risk Management**: Identify and review high-risk AI suggestions
- **Quality Assurance**: Ensure AI contributions meet standards

## Future Enhancements

### Planned Features
- **Visual diff UI**: Rich editor integration for reviewing AI changes
- **ML-powered review**: AI that learns from human review patterns
- **Cross-repository analysis**: Track AI contributions across projects
- **Real-time collaboration**: Multiple AI agents working together
- **Advanced analytics**: Detailed insights into AI development patterns

### Experimental Ideas
- **Blockchain attestation**: Immutable AI contribution records
- **Federated learning**: Share AI review patterns across organizations
- **Code generation competitions**: AI agents competing on code quality
- **Human-AI pair programming**: Seamless real-time collaboration

This AI streaming change pattern transforms Atomic from a version control system into a comprehensive AI collaboration platform, enabling unprecedented visibility and control over AI-assisted development workflows.