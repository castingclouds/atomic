# Record Metadata Editing - AI Attribution Support

## Overview

As of this update, `atomic record -e` (interactive editing mode) now includes support for viewing and editing metadata sections, particularly AI attribution information. This allows users to add, edit, or remove AI attribution metadata directly in their editor when recording changes.

## Problem Statement

Previously, when using `atomic record -e` to interactively edit a change before recording:

1. **Metadata was invisible**: AI attribution metadata stored in the change was not displayed in the editor
2. **CLI flags required**: Users had to specify AI attribution via command-line flags (`--ai-assisted`, `--ai-provider`, etc.)
3. **No manual editing**: Users couldn't manually add or edit attribution information during the interactive editing process

## Solution

The change text format now includes a `# Metadata` section that:

- **Displays** existing AI attribution information in human-readable TOML format
- **Allows editing** of attribution fields directly in the editor
- **Preserves** metadata through the edit cycle
- **Merges** with CLI flags (CLI flags take precedence)

## Usage Examples

### Basic Interactive Editing with Metadata

```bash
# Record a change interactively
atomic record -e
```

When you open the editor, you'll now see a metadata section if attribution is present:

```toml
[message]
message = "Add new feature"
timestamp = "2024-01-15T10:30:00Z"

[[authors]]
name = "Jane Developer"
email = "jane@example.com"

# Metadata
# AI Attribution Information
ai_assisted = true
ai_provider = "openai"
ai_model = "gpt-4"
ai_suggestion_type = "Partial"
ai_confidence = 0.85

# Dependencies
[2] ABC123... # Previous change

# Hunks
1. Edit in file.rs:10 ...
```

### Adding Metadata in the Editor

You can manually add a metadata section to any change:

```toml
[message]
message = "Implement authentication"

[[authors]]
name = "John Developer"

# Metadata
# AI Attribution Information
ai_assisted = true
ai_provider = "anthropic"
ai_model = "claude-3-sonnet"
ai_suggestion_type = "Complete"
ai_confidence = 0.95

# Dependencies
...
```

### Editing Existing Metadata

Simply modify the values in the metadata section:

```toml
# Metadata
# AI Attribution Information
ai_assisted = true
ai_provider = "openai"
ai_model = "gpt-4-turbo"  # Changed model name
ai_suggestion_type = "Collaborative"  # Changed from "Complete"
ai_confidence = 0.90  # Adjusted confidence
```

### Removing Metadata

Delete the entire metadata section or set `ai_assisted = false`:

```toml
# Metadata
# AI Attribution Information
ai_assisted = false  # This effectively removes AI attribution
```

## CLI Flag Integration

CLI flags take precedence over metadata in the editor, allowing you to override editor content:

```bash
# Override provider and model from command line
atomic record -e --ai-provider anthropic --ai-model claude-3-opus
```

This will:
1. Open the editor with existing metadata (if any)
2. Let you edit the metadata
3. After saving, override the provider and model with CLI values
4. Keep other metadata fields from the editor

## Metadata Fields

### Available Fields

| Field | Type | Description | Example |
|-------|------|-------------|---------|
| `ai_assisted` | boolean | Whether AI was involved in creating this change | `true` |
| `ai_provider` | string | AI provider name | `"openai"`, `"anthropic"`, `"github"` |
| `ai_model` | string | Specific model used | `"gpt-4"`, `"claude-3-sonnet"` |
| `ai_suggestion_type` | string | Type of AI collaboration | `"Complete"`, `"Partial"`, `"Collaborative"`, `"Inspired"`, `"Review"`, `"Refactor"` |
| `ai_confidence` | float | Confidence score (0.0 to 1.0) | `0.85` |

### Suggestion Types

- **Complete**: AI generated the entire patch
- **Partial**: AI suggested, human modified
- **Collaborative**: Human started, AI completed
- **Inspired**: Human wrote based on AI suggestion
- **Review**: AI reviewed human code
- **Refactor**: AI refactored existing code

## Technical Implementation

### Text Format Changes

**File**: `libatomic/src/change/text_changes.rs`

The `write()` method now includes metadata serialization:

```rust
if !self.metadata.is_empty() {
    writeln!(w)?;
    writeln!(w, "# Metadata")?;
    if let Ok(attribution) = deserialize_attribution_from_metadata(&self.metadata) {
        writeln!(w, "# AI Attribution Information")?;
        writeln!(w, "ai_assisted = {}", attribution.ai_assisted)?;
        // ... write other fields
    }
}
```

The `read_impl()` method now parses metadata:

```rust
// parse metadata section if present
let (i, metadata) = parse_metadata(i).map_err(|e| e.to_owned())?;
```

### Parser Implementation

**File**: `libatomic/src/change/text_changes.rs`

New `parse_metadata()` function using nom parser combinators:

```rust
fn parse_metadata(input: &str) -> Result<(&str, Vec<u8>), nom::Err<nom::error::Error<&str>>> {
    // Parses the metadata section and converts to binary format
    // Handles: ai_assisted, ai_provider, ai_model, ai_suggestion_type, ai_confidence
}
```

### Header Parser Update

**File**: `libatomic/src/change/parse.rs`

Updated to stop before metadata section:

```rust
pub fn parse_header(input: &str) -> IResult<&str, Result<ChangeHeader, toml::de::Error>> {
    map(
        alt((
            take_until("# Metadata"),      // NEW: Stop before metadata
            take_until("# Dependencies"),
            take_until("# Hunks"),
        )),
        |s| toml::de::from_str(s),
    )(input)
}
```

### Record Command Integration

**File**: `atomic/src/commands/record.rs`

Enhanced CLI flag merging logic:

```rust
// Merge CLI-provided attribution flags with editor content
// CLI flags take precedence over what was in the editor
if self.ai_assisted || self.ai_provider.is_some() || ... {
    let existing_attribution = deserialize_attribution_from_metadata(&change.hashed.metadata).ok();
    
    if let Some(mut attr) = existing_attribution {
        // Override with CLI flags
        if self.ai_assisted { attr.ai_assisted = true; }
        // ... merge other fields
        change.hashed.metadata = serialize_attribution_for_metadata(&attr)?;
    } else {
        // Create from CLI flags only
        change.hashed.metadata = create_attribution_from_env()?;
    }
}
```

## Workflows

### Workflow 1: Recording with Environment Detection

```bash
# Set environment variables
export ATOMIC_AI_ENABLED=true
export ATOMIC_AI_PROVIDER=openai
export ATOMIC_AI_MODEL=gpt-4

# Record with interactive editing
atomic record -e
# Metadata section will show environment-detected values
# Edit as needed and save
```

### Workflow 2: Adding Attribution After the Fact

```bash
# Record change first
atomic record -m "Implement feature"

# Later, amend to add attribution
atomic record -e --amend --ai-assisted --ai-provider openai
# Edit the metadata section in the editor
```

### Workflow 3: Reviewing and Adjusting Attribution

```bash
# Record with initial attribution
atomic record -e --ai-assisted --ai-provider github --ai-confidence 0.5

# In editor, review the AI contribution and adjust:
# - Change suggestion_type from "Complete" to "Partial"
# - Increase confidence from 0.5 to 0.8
# Save and the adjusted attribution is recorded
```

## Best Practices

1. **Be Accurate**: Attribution should accurately reflect the AI's contribution
2. **Use Confidence Scores**: Lower confidence for extensively modified suggestions
3. **Document Provider/Model**: Helps with reproducibility and auditing
4. **Choose Correct Type**: Select the suggestion type that best describes the collaboration
5. **Review Before Recording**: Double-check attribution metadata in the editor

## Migration Notes

### Backward Compatibility

- Changes recorded before this feature have no metadata section (empty `metadata` field)
- Existing changes can be amended to add metadata: `atomic record -e --amend`
- Binary metadata format is preserved for non-attribution data

### Version Information

- **Attribution Version**: 1
- **Change Format Version**: 7 (includes tag field)
- **Metadata Serialization**: bincode format

## Troubleshooting

### Metadata Not Showing in Editor

**Possible causes:**
1. Change has no metadata (not an AI-assisted change)
2. Metadata is in binary format that can't be deserialized as attribution

**Solution:**
- Add metadata manually in the editor
- Use CLI flags: `atomic record -e --ai-assisted --ai-provider <provider>`

### Metadata Lost After Editing

**Possible causes:**
1. Syntax error in metadata section
2. Invalid field values (e.g., confidence > 1.0)

**Solution:**
- Check for TOML syntax errors
- Ensure field values are valid (boolean, string, or float as appropriate)
- Check editor output for error messages

### CLI Flags Not Overriding Editor Metadata

**Expected behavior:**
- CLI flags should override corresponding editor fields
- Other metadata fields from editor are preserved

**Verification:**
```bash
atomic attribution --hash <change-hash>
# Check that CLI-provided values are present
```

## Related Commands

- `atomic attribution` - View AI attribution statistics
- `atomic attribution --hash <hash>` - View attribution for specific change
- `atomic record --help` - See all available attribution flags

## See Also

- [AI Attribution System Documentation](ATTRIBUTION.md)
- [HTTP API Protocol Alignment](HTTP-API-PROTOCOL-COMPARISON.md)
- [Agents Development Guide](AGENTS.md)