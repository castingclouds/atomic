#!/bin/bash
# Phase 5: Integration Testing for atomic-api
# Tests atomic-api server with real atomic CLI operations

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
API_PORT=${API_PORT:-8080}
API_HOST=${API_HOST:-127.0.0.1}
TEST_DIR=$(mktemp -d)
TENANT_DATA=${TENANT_DATA:-"/tmp/atomic-test-data"}

# Test tracking
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

# Cleanup function
cleanup() {
    echo -e "${YELLOW}Cleaning up...${NC}"
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[PASS]${NC} $1"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

log_error() {
    echo -e "${RED}[FAIL]${NC} $1"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

log_test() {
    echo -e "${YELLOW}[TEST]${NC} $1"
    TESTS_RUN=$((TESTS_RUN + 1))
}

# Check prerequisites
check_prerequisites() {
    log_info "Checking prerequisites..."

    # Check atomic is in PATH
    if ! command -v atomic &> /dev/null; then
        log_error "atomic CLI not found in PATH"
        echo "Make sure atomic is in your PATH: export PATH=\"\$HOME/Projects/personal/atomic/target/release:\$PATH\""
        exit 1
    fi

    # Check if server is running
    if ! curl -sf "http://$API_HOST:$API_PORT/health" > /dev/null 2>&1; then
        log_error "Server not responding at http://$API_HOST:$API_PORT"
        echo "Please start server first"
        exit 1
    fi

    log_success "Prerequisites found and server is running"
}

# Clone and setup test environment
setup_test_env() {
    log_info "Cloning repository from server..."

    cd "$TEST_DIR"
    local remote_url="http://$API_HOST:$API_PORT/tenant/1/portfolio/1/project/1/code"

    atomic clone "$remote_url" "work-repo" 2>&1 | tee "$TEST_DIR/setup-clone.log"
    local clone_result=$?

    # Check for errors in output (atomic CLI sometimes returns 0 even on error)
    if grep -q "Error:" "$TEST_DIR/setup-clone.log"; then
        log_error "Failed to clone repository from $remote_url"
        log_error "Ensure repository exists at that URL"
        echo ""
        echo "Clone output:"
        cat "$TEST_DIR/setup-clone.log"
        exit 1
    fi

    if [ -d "$TEST_DIR/work-repo/.atomic" ]; then
        log_success "Repository cloned successfully"
    else
        log_error "Failed to clone repository - .atomic directory not created"
        log_error "Clone exit code: $clone_result"
        cat "$TEST_DIR/setup-clone.log"
        exit 1
    fi
}

# Test 1: Health check
test_health_check() {
    log_test "Health check endpoint"

    local response=$(curl -sf "http://$API_HOST:$API_PORT/health")
    if [[ "$response" == *"healthy"* ]]; then
        log_success "Health check returned healthy status"
    else
        log_error "Health check failed: $response"
    fi
}

# Test 2: REST API - List changes
test_rest_api_changes() {
    log_test "REST API - List changes"

    local url="http://$API_HOST:$API_PORT/tenant/1/portfolio/1/project/1/code/changes"
    local response=$(curl -sf "$url" 2>&1)
    local status=$(curl -s -o /dev/null -w "%{http_code}" "$url")

    if [[ "$status" == "200" ]] && [[ -n "$response" ]]; then
        log_success "REST API returned changes list"
    else
        log_error "REST API changes list failed (HTTP $status)"
    fi
}

# Test 3: Make changes and push
test_make_changes_and_push() {
    log_test "Make changes and push back to server"

    cd "$TEST_DIR/work-repo"

    # Create new file
    echo "Test content $(date)" > test-file.txt
    atomic add test-file.txt
    atomic record -m "Add test file"

    # Modify existing file if it exists
    if [ -f "README.md" ]; then
        echo "Modified $(date)" >> README.md
        atomic add README.md
        atomic record -m "Update README"
    fi

    # Push changes
    local remote_url="http://$API_HOST:$API_PORT/tenant/1/portfolio/1/project/1/code"
    if atomic push "$remote_url" 2>&1 | tee "$TEST_DIR/push.log"; then
        log_success "Push operation successful"
    else
        log_error "Push operation failed"
        cat "$TEST_DIR/push.log"
    fi
}

# Test 4: Pull changes back
test_pull_changes() {
    log_test "Pull changes from server"

    cd "$TEST_DIR"
    local pull_repo="$TEST_DIR/pull-repo"
    local remote_url="http://$API_HOST:$API_PORT/tenant/1/portfolio/1/project/1/code"

    # Clone a fresh copy
    if atomic clone "$remote_url" "$pull_repo" 2>&1 | tee "$TEST_DIR/pull-clone.log"; then
        cd "$pull_repo"

        # Verify our pushed changes exist
        if [ -f "test-file.txt" ]; then
            log_success "Pull operation verified - changes present"
        else
            log_error "Pull operation failed - changes not found"
        fi
    else
        log_error "Pull clone failed"
        cat "$TEST_DIR/pull-clone.log"
    fi
}

# Test 5: Concurrent clones
test_concurrent_clones() {
    log_test "Concurrent clone operations"

    cd "$TEST_DIR"
    local remote_url="http://$API_HOST:$API_PORT/tenant/1/portfolio/1/project/1/code"

    local clone1="$TEST_DIR/concurrent-1"
    local clone2="$TEST_DIR/concurrent-2"
    local clone3="$TEST_DIR/concurrent-3"

    (atomic clone "$remote_url" "$clone1" > "$TEST_DIR/concurrent1.log" 2>&1) &
    local pid1=$!
    (atomic clone "$remote_url" "$clone2" > "$TEST_DIR/concurrent2.log" 2>&1) &
    local pid2=$!
    (atomic clone "$remote_url" "$clone3" > "$TEST_DIR/concurrent3.log" 2>&1) &
    local pid3=$!

    wait $pid1
    local result1=$?
    wait $pid2
    local result2=$?
    wait $pid3
    local result3=$?

    if [ $result1 -eq 0 ] && [ $result2 -eq 0 ] && [ $result3 -eq 0 ]; then
        log_success "Concurrent clones successful"
    else
        log_error "Some concurrent clones failed"
    fi
}

# Test 6: Invalid operations
test_invalid_operations() {
    log_test "Invalid operation handling"

    local invalid_url="http://$API_HOST:$API_PORT/tenant/99999/portfolio/99999/project/99999/code/changes"
    local response=$(curl -s -w "%{http_code}" -o /dev/null "$invalid_url")

    if [ "$response" == "404" ] || [ "$response" == "500" ]; then
        log_success "Invalid tenant returns error (HTTP $response)"
    else
        log_error "Invalid tenant should return error, got HTTP $response"
    fi
}



# Print summary
print_summary() {
    echo ""
    echo "========================================"
    echo "  Integration Test Summary"
    echo "========================================"
    echo -e "Tests Run:    ${BLUE}$TESTS_RUN${NC}"
    echo -e "Tests Passed: ${GREEN}$TESTS_PASSED${NC}"
    echo -e "Tests Failed: ${RED}$TESTS_FAILED${NC}"
    echo "========================================"

    if [ $TESTS_FAILED -eq 0 ]; then
        echo -e "${GREEN}✓ All tests passed!${NC}"
        echo ""
        echo "Next steps:"
        echo "  1. Test with production data"
        echo "  2. Run performance benchmarks"
        return 0
    else
        echo -e "${RED}✗ Some tests failed${NC}"
        echo ""
        echo "Debug information:"
        echo "  - Test directory: $TEST_DIR"
        echo "  - Test logs: $TEST_DIR/*.log"
        return 1
    fi
}

# Main execution
main() {
    echo "========================================"
    echo "  Atomic API Integration Tests"
    echo "  Phase 5: Testing with Real Atomic CLI"
    echo "========================================"
    echo ""

    check_prerequisites
    setup_test_env

    echo ""
    echo "Running integration tests..."
    echo ""

    # Core tests
    test_health_check
    test_rest_api_changes
    test_make_changes_and_push
    test_pull_changes
    test_concurrent_clones
    test_invalid_operations

    echo ""
    print_summary
}

# Run main
main
exit $?
