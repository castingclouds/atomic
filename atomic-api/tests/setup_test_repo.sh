#!/bin/bash
# Setup test repository for integration testing

set -e

TENANT_DATA=${1:-"/tmp/atomic-test-data"}
TENANT_ID=${2:-"1"}
PORTFOLIO_ID=${3:-"1"}
PROJECT_ID=${4:-"1"}

# Check atomic is in PATH
if ! command -v atomic &> /dev/null; then
    echo "Error: atomic CLI not found in PATH"
    echo "Make sure atomic is in your PATH: export PATH=\"\$HOME/Projects/personal/atomic/target/release:\$PATH\""
    exit 1
fi

REPO_PATH="$TENANT_DATA/$TENANT_ID/$PORTFOLIO_ID/$PROJECT_ID"

echo "Setting up test repository at: $REPO_PATH"

# Create directory structure
mkdir -p "$REPO_PATH"
cd "$REPO_PATH"

# Remove old repo if exists
if [ -d ".atomic" ]; then
    echo "Removing existing repository..."
    rm -rf .atomic
fi

# Initialize new atomic repository
echo "Initializing atomic repository..."
atomic init

# Create initial content
echo "# Test Repository" > README.md
echo "" >> README.md
echo "This is a test repository for atomic-api integration tests." >> README.md
echo "Created: $(date)" >> README.md

atomic add README.md
atomic record -m "Initial commit"

# Create second change
echo "" >> README.md
echo "## Getting Started" >> README.md
echo "This repository is ready for testing." >> README.md

atomic add README.md
atomic record -m "Add getting started section"

# Create third change with multiple files
echo "print('Hello from atomic')" > example.py
mkdir -p src
echo "fn main() { println!(\"Hello\"); }" > src/main.rs

atomic add example.py
atomic add src/main.rs
atomic record -m "Add example files"

echo ""
echo "✓ Test repository created successfully!"
echo ""
echo "Repository location: $REPO_PATH"
echo "Changes created: 3"
echo ""
echo "Start server with:"
echo "  cd atomic-api"
echo "  cargo run --release -- $TENANT_DATA"
echo ""
echo "Test URL:"
echo "  http://localhost:8080/tenant/$TENANT_ID/portfolio/$PORTFOLIO_ID/project/$PROJECT_ID/code"
