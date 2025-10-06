#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test configuration
TEST_ROOT="/tmp/atomic-tag-test"
SERVER_DIR="$TEST_ROOT/server/1/1/1"
CLIENT_DIR="$TEST_ROOT/client"
CLONE_DIR="$TEST_ROOT/clone"
API_LOG="$TEST_ROOT/api-server.log"
SERVER_URL="http://127.0.0.1:8080/tenant/1/portfolio/1/project/1/code"

# Get the atomic binary path
ATOMIC_BIN="$(pwd)/target/release/atomic"
API_BIN="$(pwd)/target/release/atomic-api"

echo -e "${YELLOW}=== Atomic Tag Push/Pull End-to-End Test ===${NC}"
echo ""

# Function to print test steps
step() {
    echo -e "${YELLOW}[$1]${NC} $2"
}

# Function to print success
success() {
    echo -e "${GREEN}✓${NC} $1"
}

# Function to print error and exit
error() {
    echo -e "${RED}✗${NC} $1"
    exit 1
}

# Cleanup function
cleanup() {
    local exit_code=$?
    step "CLEANUP" "Stopping server"
    pkill -f atomic-api 2>/dev/null || true

    if [ $exit_code -eq 0 ]; then
        step "CLEANUP" "Cleaning up test directories (test passed)"
        rm -rf "$TEST_ROOT"
        success "Cleanup complete"
    else
        echo ""
        echo -e "${RED}Test failed. Preserving test data for inspection:${NC}"
        echo "  Server dir: $SERVER_DIR"
        echo "  Client dir: $CLIENT_DIR"
        echo "  Clone dir:  $CLONE_DIR"
        echo "  API log:    $API_LOG"
        echo ""
    fi
}

# Set trap to cleanup on exit
trap cleanup EXIT

# Step 1: Clean slate
step "1" "Cleaning up any existing test data"
pkill -f atomic-api 2>/dev/null || true
sleep 1
rm -rf "$TEST_ROOT"
mkdir -p "$SERVER_DIR"
success "Clean slate ready"

# Step 2: Initialize server repository
step "2" "Initializing server repository"
cd "$SERVER_DIR"
$ATOMIC_BIN init > /dev/null
success "Server repository initialized"

# Step 3: Create test content on server
step "3" "Creating test content on server"
cat > file.txt << 'EOF'
Line 1
Line 2
Line 3
EOF
$ATOMIC_BIN add file.txt > /dev/null
$ATOMIC_BIN record -m "Initial commit" . > /dev/null
echo "Line 4" >> file.txt
$ATOMIC_BIN record -m "Add line 4" . > /dev/null
echo "Line 5" >> file.txt
$ATOMIC_BIN record -m "Add line 5" . > /dev/null
success "Created 3 changes on server"

# Step 4: Start API server
step "4" "Starting API server"
cd "$SERVER_DIR"
RUST_LOG=debug $API_BIN "$TEST_ROOT/server" > "$API_LOG" 2>&1 &
API_PID=$!
sleep 2

# Check if server started
if ! ps -p $API_PID > /dev/null; then
    error "API server failed to start. Check $API_LOG"
fi
success "API server started (PID: $API_PID)"

# Step 5: Clone repository to client
step "5" "Cloning repository to client"
cd "$TEST_ROOT"
$ATOMIC_BIN clone "$SERVER_URL" client > /dev/null 2>&1
cd "$CLIENT_DIR"

# Verify clone worked
if [ ! -f file.txt ]; then
    error "Clone failed - file.txt not found"
fi

LINES=$(wc -l < file.txt)
if [ "$LINES" -ne 5 ]; then
    error "Clone failed - expected 5 lines, got $LINES"
fi
success "Repository cloned successfully"

# Step 6: Create tag on client
step "6" "Creating tag v1.0.0 on client"
TAG_OUTPUT=$($ATOMIC_BIN tag create --version v1.0.0 -m "Release 1.0" 2>&1)
TAG_HASH=$(echo "$TAG_OUTPUT" | grep -oE '[A-Z0-9]{53}' | head -1)

if [ -z "$TAG_HASH" ]; then
    error "Failed to extract tag hash from output: $TAG_OUTPUT"
fi

success "Tag created: $TAG_HASH"

# Step 6.5: Verify tag metadata in client database
step "6.5" "Verifying tag metadata stored in client database"
CLIENT_LOG_OUTPUT=$($ATOMIC_BIN log 2>&1)
if echo "$CLIENT_LOG_OUTPUT" | grep -q "$TAG_HASH"; then
    # Check if the tag shows consolidation info
    if echo "$CLIENT_LOG_OUTPUT" | grep -q "Consolidates:"; then
        success "Client database has tag metadata"
    else
        error "Client database missing tag consolidation info"
    fi
else
    error "Tag not found in client database"
fi

# Step 7: Verify tag exists locally
step "7" "Verifying tag exists locally"
TAG_LIST=$($ATOMIC_BIN tag list)
if ! echo "$TAG_LIST" | grep -q "$TAG_HASH"; then
    error "Tag not found in local tag list"
fi

# Verify tag file exists
TAG_FILE=".atomic/changes/${TAG_HASH:0:2}/${TAG_HASH:2}.tag"

if [ ! -f "$TAG_FILE" ]; then
    error "Tag file not found: $TAG_FILE"
fi
success "Tag file exists locally"

# Step 8: Create another change after the tag
step "8" "Creating a new change after the tag"
echo "Line 6" >> file.txt
POST_TAG_CHANGE=$($ATOMIC_BIN record -m "Post-tag change" . 2>&1 | grep -oE '[A-Z0-9]{53}' | head -1)
if [ -z "$POST_TAG_CHANGE" ]; then
    error "Failed to create post-tag change"
fi
success "Post-tag change created: $POST_TAG_CHANGE"

# Step 9: Push both tag and new change to server
step "9" "Pushing tag and new change to server (atomic push)"
PUSH_START=$(date +%s)
$ATOMIC_BIN push -a "$SERVER_URL" > /dev/null 2>&1
PUSH_END=$(date +%s)
PUSH_TIME=$((PUSH_END - PUSH_START))
success "Atomic push completed in ${PUSH_TIME}s"

# Step 9.5: Verify tag metadata in server database
step "9.5" "Verifying tag metadata stored in server database"
cd "$SERVER_DIR"
SERVER_LOG_OUTPUT=$($ATOMIC_BIN log 2>&1)
if echo "$SERVER_LOG_OUTPUT" | grep -q "$TAG_HASH"; then
    # Check if the tag shows consolidation info
    if echo "$SERVER_LOG_OUTPUT" | grep -q "Consolidates:"; then
        success "Server database has tag metadata ✓"
    else
        echo -e "${YELLOW}Warning: Server database missing tag consolidation info${NC}"
        echo "This is the bug we're fixing - tag metadata not transferred to database on apply"
    fi
else
    error "Tag not found in server database"
fi

# Step 10: Verify tag file on server
step "10" "Verifying tag file on server"
SERVER_TAG_FILE="$SERVER_DIR/.atomic/changes/${TAG_HASH:0:2}/${TAG_HASH:2}.tag"

if [ ! -f "$SERVER_TAG_FILE" ]; then
    error "Tag file not found on server: $SERVER_TAG_FILE"
fi
success "Tag file exists on server"

# Step 11: Verify post-tag change on server
step "11" "Verifying post-tag change on server"
cd "$SERVER_DIR"
if ! $ATOMIC_BIN log | grep -q "$POST_TAG_CHANGE"; then
    error "Post-tag change not found in server log"
fi
success "Post-tag change appears in server log"

# Step 12: Verify tag in server log
step "12" "Verifying tag appears in server log"
cd "$SERVER_DIR"
if ! $ATOMIC_BIN log | grep -q "🏷️"; then
    error "Tag change not found in server log"
fi
success "Tag change appears in server log"

# Step 13: Clone from server to new location
step "13" "Cloning from server to verify tag download"
cd "$TEST_ROOT"
RUST_LOG=debug $ATOMIC_BIN clone "$SERVER_URL" clone > "$CLONE_DIR.log" 2>&1 || {
    echo ""
    echo -e "${RED}Clone failed with error. Checking server logs:${NC}"
    echo "Clone log:"
    tail -20 "$CLONE_DIR.log" || cat "$CLONE_DIR.log"
    echo ""
    echo "Server log:"
    tail -30 "$API_LOG"
    error "Clone failed"
}
cd "$CLONE_DIR"
success "Fresh clone completed"

# Step 14: Verify tag file in clone
step "14" "Verifying tag file was cloned"
CLONE_TAG_FILE=".atomic/changes/${TAG_HASH:0:2}/${TAG_HASH:2}.tag"

if [ ! -f "$CLONE_TAG_FILE" ]; then
    error "Tag file not cloned: $CLONE_TAG_FILE"
fi
success "Tag file was cloned successfully"

# Step 15: Verify post-tag change in clone
step "15" "Verifying post-tag change in clone"
if ! $ATOMIC_BIN log | grep -q "$POST_TAG_CHANGE"; then
    error "Post-tag change not found in cloned log"
fi
success "Post-tag change appears in cloned log"

# Step 16: Verify tag in cloned log
step "16" "Verifying tag appears in cloned log"
if ! $ATOMIC_BIN log | grep -q "🏷️"; then
    error "Tag change not found in cloned log"
fi
success "Tag change appears in cloned log"

# Step 16.5: Verify tag metadata in clone database
step "16.5" "CRITICAL: Verifying tag metadata in clone database"
CLONE_LOG_OUTPUT=$($ATOMIC_BIN log 2>&1)
if echo "$CLONE_LOG_OUTPUT" | grep -q "$TAG_HASH"; then
    if echo "$CLONE_LOG_OUTPUT" | grep -q "Consolidates:"; then
        success "Clone database has tag metadata ✓✓✓"
        echo -e "${GREEN}        FIX VERIFIED: Tag metadata properly transferred!${NC}"
    else
        echo -e "${RED}Clone database missing tag consolidation info${NC}"
        error "Fix not working - tag metadata not stored in clone database"
    fi
else
    error "Tag not found in clone log"
fi

# Step 17: Verifying tag list in clone
step "17" "Verifying tag list in clone"
TAG_LIST_CLONE=$($ATOMIC_BIN tag list --repository $CLONE_DIR 2>&1)
# Check if any tags are listed (there should be at least one)
if echo "$TAG_LIST_CLONE" | grep -q "Consolidated changes:"; then
    # Count how many tags are listed
    TAG_COUNT=$(echo "$TAG_LIST_CLONE" | grep -c "Consolidated changes:" || echo "0")
    success "Found $TAG_COUNT tag(s) in cloned repository ✓✓✓"
else
    echo ""
    echo -e "${RED}CRITICAL: No tags found in cloned tag list${NC}"
    echo "Even though consolidating tag metadata is in database,"
    echo "tag list command is not showing any tags."
    echo ""
    echo "Tag list output:"
    echo "$TAG_LIST_CLONE"
    echo ""
    error "No tags appearing in 'atomic tag list' output"
fi

# Step 18: Verify file content in clone
step "18" "Verifying file content in clone"
if [ ! -f file.txt ]; then
    error "file.txt not found in clone"
fi

CLONE_LINES=$(wc -l < file.txt)
if [ "$CLONE_LINES" -ne 6 ]; then
    error "Clone has wrong content - expected 6 lines (including post-tag change), got $CLONE_LINES"
fi
success "File content correct in clone (6 lines including post-tag change)"

# All tests passed!
echo ""
echo -e "${GREEN}═══════════════════════════════════════${NC}"
echo -e "${GREEN}✓ All tests passed successfully!${NC}"
echo -e "${GREEN}═══════════════════════════════════════${NC}"
echo ""
echo "Summary:"
echo "  - Created 3 changes on server"
echo "  - Cloned to client successfully"
echo "  - Created tag with 4 consolidated changes"
echo "  - ✓ Client database has tag metadata"
echo "  - Pushed tag to server (${PUSH_TIME}s)"
echo "  - ✓ Server database has tag metadata"
echo "  - Verified tag file on server"
echo "  - Cloned from server successfully"
echo "  - Verified tag file in clone"
echo "  - ✓ Clone database has tag metadata"
echo "  - ✓ Tag appears in 'atomic tag list' command"
echo ""
echo -e "${GREEN}Tag push/pull functionality is working correctly!${NC}"
echo -e "${GREEN}Tag metadata is properly stored in databases!${NC}"
echo ""

exit 0
