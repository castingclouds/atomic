#!/bin/bash
# Atomic VCS Prompt Integration Demo
#
# This script demonstrates the prompt integration feature for Atomic VCS.
# Run this script to see various prompt configurations in action.

set -e

# Colors for demo output
DEMO_COLOR_GREEN='\033[0;32m'
DEMO_COLOR_BLUE='\033[0;34m'
DEMO_COLOR_YELLOW='\033[0;33m'
DEMO_COLOR_CYAN='\033[0;36m'
DEMO_COLOR_RESET='\033[0m'

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Check if atomic binary exists
ATOMIC_BIN="$PROJECT_ROOT/target/release/atomic"
if [ ! -f "$ATOMIC_BIN" ]; then
    ATOMIC_BIN="$PROJECT_ROOT/target/debug/atomic"
fi

if [ ! -f "$ATOMIC_BIN" ]; then
    echo "Error: atomic binary not found. Please run 'cargo build' first."
    exit 1
fi

# Print section header
print_header() {
    echo ""
    printf "${DEMO_COLOR_CYAN}===================================================${DEMO_COLOR_RESET}\n"
    printf "${DEMO_COLOR_CYAN}$1${DEMO_COLOR_RESET}\n"
    printf "${DEMO_COLOR_CYAN}===================================================${DEMO_COLOR_RESET}\n"
    echo ""
}

# Print command being executed
print_command() {
    printf "${DEMO_COLOR_YELLOW}\$ $1${DEMO_COLOR_RESET}\n"
}

# Print output
print_output() {
    printf "${DEMO_COLOR_GREEN}$1${DEMO_COLOR_RESET}\n"
}

# Create a temporary repository for testing
TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

print_header "Atomic VCS Prompt Integration Demo"

echo "This demo shows how the atomic prompt command works."
echo "We'll create a temporary repository and demonstrate various prompt formats."
echo ""

# Initialize repository
print_header "1. Setting up a test repository"

print_command "cd $TEMP_DIR"
cd "$TEMP_DIR"

print_command "atomic init"
"$ATOMIC_BIN" init > /dev/null 2>&1
print_output "✓ Repository initialized"

print_command "atomic channel"
"$ATOMIC_BIN" channel
echo ""

# Basic usage
print_header "2. Basic prompt command"

print_command "atomic prompt"
output=$("$ATOMIC_BIN" prompt)
print_output "$output"
echo ""
echo "This is the default format: [channel]"
echo ""

# Channel only
print_header "3. Channel name only"

print_command "atomic prompt --channel-only"
output=$("$ATOMIC_BIN" prompt --channel-only)
print_output "$output"
echo ""
echo "Perfect for minimal prompts or custom formatting"
echo ""

# Custom format
print_header "4. Custom format strings"

print_command "atomic prompt --format '({channel})'"
output=$("$ATOMIC_BIN" prompt --format "({channel})")
print_output "$output"
echo ""

print_command "atomic prompt --format 'on {channel}'"
output=$("$ATOMIC_BIN" prompt --format "on {channel}")
print_output "$output"
echo ""

print_command "atomic prompt --format '⚛ {channel}'"
output=$("$ATOMIC_BIN" prompt --format "⚛ {channel}")
print_output "$output"
echo ""

# With repository name
print_header "5. Including repository name"

print_command "atomic prompt --show-repository --format '[{repository}:{channel}]'"
output=$("$ATOMIC_BIN" prompt --show-repository --format "[{repository}:{channel}]")
print_output "$output"
echo ""

# Create a channel and switch
print_header "6. Multiple channels"

print_command "atomic channel new feature-branch"
"$ATOMIC_BIN" channel new feature-branch > /dev/null 2>&1
print_output "✓ Created feature-branch"

print_command "atomic channel"
"$ATOMIC_BIN" channel

print_command "atomic prompt"
output=$("$ATOMIC_BIN" prompt)
print_output "$output"
echo ""

print_command "atomic channel switch main"
"$ATOMIC_BIN" channel switch main > /dev/null 2>&1
print_output "✓ Switched to main"

print_command "atomic prompt"
output=$("$ATOMIC_BIN" prompt)
print_output "$output"
echo ""

# Shell integration
print_header "7. Shell integration examples"

echo "To use in your shell, add to ~/.bashrc or ~/.zshrc:"
echo ""
print_output "source $SCRIPT_DIR/atomic-prompt.sh"
print_output "PS1='\$(atomic_prompt)\\w\\\$ '"
echo ""
echo "Example prompts:"
echo ""
print_output "[main] ~/my-project \$"
print_output "[feature-branch] ~/my-project \$"
print_output "[main] user@host:~/my-project \$"
echo ""

# Performance test
print_header "8. Performance"

echo "The atomic prompt command is designed to be fast:"
echo ""

print_command "time atomic prompt"
time "$ATOMIC_BIN" prompt > /dev/null
echo ""

echo "Typical execution time: 5-15ms (first call)"
echo "Cached execution time: <1ms (subsequent calls within 5 seconds)"
echo ""

# Configuration
print_header "9. Configuration"

echo "You can configure the prompt in ~/.config/atomic/config.toml:"
echo ""
cat << 'EOF'
[prompt]
enabled = true
format = "[{channel}]"
show_repository = false
EOF
echo ""
echo "Environment variables:"
echo "  ATOMIC_PROMPT_FORMAT     - Custom format string"
echo "  ATOMIC_PROMPT_COLOR      - auto, always, never"
echo "  ATOMIC_PROMPT_SHOW_REPO  - true, false"
echo ""

# Outside repository
print_header "10. Behavior outside repository"

cd /tmp
print_command "cd /tmp"
print_command "atomic prompt"
"$ATOMIC_BIN" prompt
echo "(no output - silently exits when not in a repository)"
echo ""

# Final notes
print_header "Demo Complete!"

echo "Key features:"
echo "  ✓ Fast execution (<15ms)"
echo "  ✓ Automatic repository detection"
echo "  ✓ Customizable format strings"
echo "  ✓ Color support (auto-detected)"
echo "  ✓ Caching for performance"
echo "  ✓ Silent failure outside repositories"
echo ""
echo "For more information, see:"
echo "  $SCRIPT_DIR/PROMPT_INTEGRATION.md"
echo "  $SCRIPT_DIR/atomic-prompt.sh (Bash/Zsh)"
echo "  $SCRIPT_DIR/atomic-prompt.fish (Fish shell)"
echo ""
echo "Quick start:"
printf "  ${DEMO_COLOR_GREEN}source $SCRIPT_DIR/atomic-prompt.sh${DEMO_COLOR_RESET}\n"
printf "  ${DEMO_COLOR_GREEN}PS1='\$(atomic_prompt)\\w\\\$ '${DEMO_COLOR_RESET}\n"
echo ""

print_output "Happy hacking with Atomic VCS! ⚛"
echo ""
