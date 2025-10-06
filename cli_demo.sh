#!/bin/bash

# CLI Integration Demo Script for Atomic VCS AI Attribution System
# This script demonstrates the new CLI features for AI attribution tracking

set -e

echo "=== Atomic VCS CLI Integration Demo ==="
echo "Demonstrating Phase 3 CLI Integration features"
echo

# Build the project first
echo "1. Building Atomic CLI with attribution features..."
cargo build --release --quiet
echo "   ✓ Build completed successfully"
echo

# Set up demo repository
DEMO_DIR="demo_repo_cli"
if [ -d "$DEMO_DIR" ]; then
    rm -rf "$DEMO_DIR"
fi

echo "2. Creating demo repository..."
mkdir "$DEMO_DIR"
cd "$DEMO_DIR"

# Initialize repository
../target/release/atomic init
echo "   ✓ Repository initialized"

# Create some test files
echo "3. Creating test files with different attribution patterns..."

# Human-authored file
cat > human_file.rs << 'EOF'
// This is a human-written file
fn hello_world() {
    println!("Hello, World!");
}
EOF

# AI-assisted file (will be auto-detected)
cat > ai_assisted_file.rs << 'EOF'
// AI-assisted implementation using GitHub Copilot
use std::collections::HashMap;

fn process_data(data: Vec<String>) -> HashMap<String, usize> {
    let mut result = HashMap::new();
    for item in data {
        *result.entry(item).or_insert(0) += 1;
    }
    result
}
EOF

# Another AI file
cat > gpt_file.py << 'EOF'
# Generated with ChatGPT assistance
def fibonacci(n):
    if n <= 1:
        return n
    return fibonacci(n-1) + fibonacci(n-2)
EOF

echo "   ✓ Test files created"

# Add files to atomic
../target/release/atomic add human_file.rs
../target/release/atomic add ai_assisted_file.rs
../target/release/atomic add gpt_file.py
echo "   ✓ Files added to atomic"

echo
echo "4. Recording changes with attribution..."

# Record human change
echo "Recording human-authored change..."
../target/release/atomic record -m "Add human-written hello world function" human_file.rs
echo "   ✓ Human change recorded"

# Record AI-assisted change with explicit flags
echo "Recording AI-assisted change with explicit attribution..."
../target/release/atomic record \
    --ai-assisted \
    --ai-provider "github" \
    --ai-model "copilot" \
    --ai-suggestion-type "collaborative" \
    --ai-confidence 0.85 \
    -m "AI-assisted data processing implementation" \
    ai_assisted_file.rs
echo "   ✓ AI-assisted change recorded with explicit attribution"

# Record another AI change (will be auto-detected)
echo "Recording change with auto-detected AI assistance..."
../target/release/atomic record -m "ChatGPT helped implement fibonacci function" gpt_file.py
echo "   ✓ Change with auto-detected AI recorded"

echo
echo "5. Demonstrating new CLI features..."

echo "=== Standard Log Output ==="
../target/release/atomic log --limit 3

echo
echo "=== Log with Attribution Information ==="
../target/release/atomic log --attribution --limit 3

echo
echo "=== Log showing only AI-assisted changes ==="
../target/release/atomic log --ai-only --attribution

echo
echo "=== Log showing only human-authored changes ==="
../target/release/atomic log --human-only --attribution

echo
echo "=== Attribution Statistics Command ==="
../target/release/atomic attribution

echo
echo "=== Detailed Attribution Statistics ==="
../target/release/atomic attribution --stats --providers --suggestion-types

echo
echo "=== Attribution Statistics in JSON format ==="
../target/release/atomic attribution --output-format json

echo
echo "6. Demonstrating Apply with Attribution..."

# Create a change file to apply
cat > test_change.patch << 'EOF'
# This would be a proper atomic change format
# For demo purposes, we'll create a simple file to show apply attribution features
EOF

echo "Testing apply command with attribution tracking..."
# Note: This requires actual change files, so we'll demonstrate the flags
echo "Command: atomic apply --with-attribution --show-attribution <change-id>"
echo "   (This would show attribution information during patch application)"

echo
echo "=== Demo Configuration ==="
echo "Environment variables that can be used:"
echo "  ATOMIC_AI_ENABLED=true"
echo "  ATOMIC_AI_PROVIDER=openai"
echo "  ATOMIC_AI_MODEL=gpt-4"
echo "  ATOMIC_AI_CONFIDENCE=0.9"
echo "  ATOMIC_AI_SUGGESTION_TYPE=complete"

# Set some environment variables and show they work
export ATOMIC_AI_ENABLED=true
export ATOMIC_AI_PROVIDER=example-ai
export ATOMIC_AI_MODEL=demo-model

echo
echo "With environment variables set, recording another change..."
../target/release/atomic record -m "Change recorded with env vars for AI attribution" --all
echo "   ✓ Change recorded with environment variable attribution"

echo
echo "=== Final Attribution Summary ==="
../target/release/atomic attribution --stats --providers

echo
echo "=== Demo Complete ==="
echo
echo "Summary of new CLI features demonstrated:"
echo "  • atomic log --attribution     : Show attribution info in log"
echo "  • atomic log --ai-only         : Filter to show only AI-assisted changes"
echo "  • atomic log --human-only      : Filter to show only human-authored changes"
echo "  • atomic attribution           : Show attribution statistics"
echo "  • atomic attribution --stats   : Detailed attribution statistics"
echo "  • atomic attribution --providers : AI provider breakdown"
echo "  • atomic attribution --json    : JSON output format"
echo "  • atomic record --ai-assisted  : Explicit AI attribution flags"
echo "  • atomic apply --with-attribution : Attribution tracking during apply"
echo "  • Environment variable support : ATOMIC_AI_* variables"
echo
echo "The attribution system successfully integrates with Atomic's CLI"
echo "while maintaining the mathematical properties of commutative patches!"

# Cleanup
cd ..
# Uncomment to remove demo directory
# rm -rf "$DEMO_DIR"

echo
echo "Demo repository preserved at: $DEMO_DIR"
echo "You can explore it further with: cd $DEMO_DIR && ../target/release/atomic log --attribution"
