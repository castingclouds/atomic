# Atomic API Integration Tests - Phase 5

## Quick Start

```bash
# 1. Start server with existing repo
cd atomic-api
cargo run --release -- /path/to/tenant-data

# 2. Run tests (assumes http://localhost:18080/tenant/1/portfolio/1/project/1/code exists)
cd atomic-api/tests
./integration_test.sh
```

## What It Does

1. Clones repo from server to `/tmp`
2. Makes changes locally
3. Pushes changes back
4. Pulls changes to verify
5. Tests concurrent clones
6. Tests error handling

## Tests

- Health check
- REST API changes list
- Clone from server
- Make changes and push
- Pull changes back
- Concurrent clones
- Invalid operations

## Configuration

```bash
export API_PORT=18080        # Default
export API_HOST=127.0.0.1    # Default
```

## Troubleshooting

**Repository not found**
Ensure `http://localhost:18080/tenant/1/portfolio/1/project/1/code` exists

**Tests fail**
Check logs: `/tmp/tmp.*/`
