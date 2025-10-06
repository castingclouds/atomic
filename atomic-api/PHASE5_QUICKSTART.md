# Phase 5 Integration Testing - Quick Start Guide

## 🚀 Ready to Test!

We've completed Phases 1-3 of the atomic-api HTTP protocol implementation. Now it's time to test everything with the real atomic CLI!

## What We're Testing

- ✅ **Phase 1**: Tag upload (tagup) implementation
- ✅ **Phase 2**: Dependency validation before apply
- ✅ **Phase 3**: Clean URL routing (no `.atomic` in URLs)
- 🧪 **Phase 5**: Full clone/push/pull with real atomic CLI

## Quick Start (30 seconds)

```bash
cd atomic-api/tests
./integration_test.sh
```

That's it! The script will:
1. Build atomic-api (release mode)
2. Create test repositories
3. Start the server on port 18080
4. Run 11 integration tests
5. Show you a summary

## What Gets Tested

### 11 Automated Tests

1. **Health Check** - Server responds correctly
2. **REST API** - Changes endpoint works
3. **Protocol Discovery** - Atomic protocol detection
4. **Clone Operation** - `atomic clone http://...`
5. **Push Operation** - `atomic push`
6. **Pull Operation** - `atomic pull`
7. **Dependency Push** - Multi-change with deps
8. **Concurrent Operations** - 3 parallel clones
9. **Large Repository** - 100 changes performance test
10. **Invalid Operations** - Error handling
11. **Server Logs** - No unexpected errors

## Expected Output

```
========================================
  Atomic API Integration Tests
  Phase 5: Testing with Real Atomic CLI
========================================

[INFO] Checking prerequisites...
[PASS] All prerequisites found
[INFO] Building atomic-api...
[PASS] Build complete
[INFO] Setting up test environment...
[PASS] Test environment ready
[INFO] Starting atomic-api server on 127.0.0.1:18080...
[PASS] Server started successfully (PID: 12345)

Running integration tests...

[TEST] Health check endpoint
[PASS] Health check returned healthy status
[TEST] REST API - List changes
[PASS] REST API returned changes list
[TEST] Protocol discovery
[PASS] Protocol discovery successful
[TEST] Clone operation
[PASS] Clone operation successful
[PASS] Cloned content verified
[TEST] Push operation
[PASS] Push operation successful
[TEST] Pull operation
[PASS] Pull operation successful
[TEST] Push with dependencies
[PASS] Dependency push successful
[TEST] Concurrent pull operations
[PASS] Concurrent operations successful
[TEST] Large repository handling (100 changes)
[PASS] Large repository push successful (8 seconds)
[TEST] Invalid operation handling
[PASS] Invalid tenant returns error (HTTP 404)
[PASS] Malformed request returns error (HTTP 500)
[TEST] Server logs verification
[PASS] No unexpected errors in server logs
[PASS] Server logged change applications

========================================
  Integration Test Summary
========================================
Tests Run:    11
Tests Passed: 11
Tests Failed: 0
========================================
✓ All tests passed!

Next steps:
  1. Review server logs: /tmp/tmp.XXXXXXX/server.log
  2. Test with production data
  3. Run performance benchmarks
```

## URL Structure (Important!)

We're using **numeric IDs from database sequences**, not names:

```
✅ Correct:   http://localhost:18080/tenant/1/portfolio/1/project/1/code
❌ Wrong:     http://localhost:18080/tenant/acme/portfolio/main/project/demo/code
```

### Filesystem Mapping

```
URL:        /tenant/1/portfolio/1/project/1/code
Filesystem: /tenant-data/1/1/1/.atomic/
```

**Key Point**: `.atomic` is a filesystem detail, NOT part of the URL!

## Manual Testing (Optional)

If you want to test manually:

```bash
# 1. Build
cd atomic-api
cargo build --release

# 2. Setup test data with numeric IDs
mkdir -p /tmp/test-data/1/1/1
cd /tmp/test-data/1/1/1
atomic init
echo "Hello World" > README.md
atomic add README.md
atomic record -m "Initial commit"

# 3. Start server
ATOMIC_API_BIND=127.0.0.1:18080 ./target/release/atomic-api /tmp/test-data

# 4. In another terminal - Test clone
cd /tmp
atomic clone http://127.0.0.1:18080/tenant/1/portfolio/1/project/1/code my-test

# 5. Test push
cd my-test
echo "More content" >> README.md
atomic add README.md
atomic record -m "Update"
atomic push http://127.0.0.1:18080/tenant/1/portfolio/1/project/1/code

# 6. Test pull
atomic pull http://127.0.0.1:18080/tenant/1/portfolio/1/project/1/code
```

## Troubleshooting

### "atomic: command not found"
```bash
# Install atomic CLI first
cd atomic
cargo install --path .
```

### "Port 18080 already in use"
```bash
# Change port
API_PORT=9000 ./integration_test.sh
```

### "Server failed to start"
```bash
# Check what's wrong
cat /tmp/tmp.*/server.log
```

### Tests fail
```bash
# The script creates logs in /tmp/tmp.XXXXXXX/
# Check:
ls /tmp/tmp.*/
cat /tmp/tmp.*/server.log
cat /tmp/tmp.*/clone.log
cat /tmp/tmp.*/push.log
```

## Configuration

Environment variables:
- `API_PORT` - Server port (default: 18080)
- `API_HOST` - Server host (default: 127.0.0.1)

Example:
```bash
API_PORT=9000 API_HOST=0.0.0.0 ./integration_test.sh
```

## What Happens Next?

After Phase 5 passes:

1. ✅ Verify all tests pass
2. 📊 Review performance metrics
3. 📝 Update progress checklist
4. 🎯 Optional: Phase 4 (Archive support) - if needed
5. 📚 Phase 6: Documentation and examples

## Success Criteria

- ✅ All 11 tests pass
- ✅ Clone works with atomic CLI
- ✅ Push uploads changes correctly
- ✅ Pull downloads changes correctly
- ✅ Dependencies validated
- ✅ Performance <1s for typical operations
- ✅ No errors in server logs

## Time Estimate

- **Automated test run**: ~2-3 minutes
- **Manual testing** (optional): ~5 minutes
- **Review and debugging**: 15-30 minutes if issues found

## Questions?

- See `tests/README.md` for detailed documentation
- See `docs/progress-checklist.md` for implementation status
- See `PUSH_PULL_INTEGRATION.md` for architecture details

## Let's Go! 🚀

```bash
cd atomic-api/tests
./integration_test.sh
```

Good luck! The implementation is solid and should pass all tests. If you hit any issues, check the logs and let me know!