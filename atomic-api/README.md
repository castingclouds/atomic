# Atomic API

A focused REST API server with WebSocket support for Atomic VCS repository operations, designed to be used behind a Fastify reverse proxy for multi-tenant SaaS deployments.

## Architecture Overview

This crate follows the AGENTS.md principles with a **single responsibility**: exposing Atomic VCS operations via REST API and WebSocket for a single repository. The multi-tenant control plane (tenant/portfolio/project routing) is handled by Fastify, which proxies to this Rust API server.

```
React Frontend (Port 3000)
    ↓ HTTP Requests & WebSocket Connections
Node.js Fastify (Port 3001) - Multi-tenant routing & reverse proxy
    ↓ HTTP Proxy & WebSocket Proxy per tenant/project
Rust Atomic API (Port 8080+) - Direct Atomic VCS operations
Rust WebSocket Server (Port 8081+) - Real-time workflow communication
    ↓ Direct Rust Crate Usage
Atomic VCS Libraries (libatomic, atomic-repository, etc.)
    ↓ File System Access
.atomic/ Directory (Database Files)
```

## Key Design Principles

Following AGENTS.md architectural guidelines:

- **Single Responsibility**: Only handles Atomic VCS API operations
- **Direct Rust Integration**: No FFI/DLL overhead, pure Rust crate usage
- **Minimal Dependencies**: Only essential dependencies for API functionality
- **Error Handling Strategy**: Comprehensive error types with context
- **Configuration-Driven**: Environment variable configuration
- **Factory Patterns**: Repository management with validation

## API Endpoints

### Health Check
- `GET /health` - Server health status

### Tenant/Project Changes
- `GET /tenant/{tenant_id}/portfolio/{portfolio_id}/project/{project_id}/code/changes?limit=50&offset=0` - List repository changes
- `GET /tenant/{tenant_id}/portfolio/{portfolio_id}/project/{project_id}/code/changes/{change_id}?include_diff=true` - Get specific change with full diff content

#### Query Parameters
- `limit` - Maximum number of changes to return (default: 50)
- `offset` - Number of changes to skip (default: 0) 
- `include_diff` - Include full diff content in individual change response (default: false)

#### Change ID Format
Changes use **cryptographic hashes as IDs** to ensure global uniqueness across distributed systems:
- **ID Format**: Base32-encoded hash (e.g., `MNYNGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC`)
- **Length**: 53 characters
- **Uniqueness**: Cryptographically guaranteed to be unique across all repositories
- **Deterministic**: Same change content always produces the same ID
- **Distributed-Safe**: No ID conflicts when syncing between repositories

### WebSocket Endpoints

The server also provides WebSocket endpoints for real-time communication:

- `ws://localhost:8081/` - WebSocket connection for real-time workflow updates
- Message types: `health_check`, `state_transition`, `repository_status`, `change_status_update`
- Configuration-driven workflow support (workflows loaded from external configuration)

#### WebSocket Message Format
```json
{
  "id": "uuid",
  "timestamp": "2025-01-15T15:40:04.688518+00:00",
  "sender": "client_id",
  "payload": {
    "type": "health_check"
  }
}
```

### Future Endpoints (Planned)
- `GET /tenant/{tenant_id}/portfolio/{portfolio_id}/project/{project_id}/code/files/{path}` - Get file content
- `GET /tenant/{tenant_id}/portfolio/{portfolio_id}/project/{project_id}/code/channels` - List repository channels
- `GET /tenant/{tenant_id}/portfolio/{portfolio_id}/project/{project_id}/code/attribution/stats` - AI attribution statistics

## Usage

### Standalone Server

```bash
# Start both REST API and WebSocket servers
atomic-api /tenant-data

# With custom bind addresses
ATOMIC_API_BIND=0.0.0.0:9000 ATOMIC_WS_BIND=0.0.0.0:9001 atomic-api /tenant-data

# Expected filesystem structure:
# /tenant-data/
# ├── tenant-123/
# │   ├── portfolio-456/
# │   │   ├── project-789/
# │   │   │   └── .atomic/     # Atomic repository database
# │   │   └── project-790/
# │   │       └── .atomic/
# │   └── portfolio-457/
# │       └── project-001/
# │           └── .atomic/
# └── tenant-124/
#     └── portfolio-001/
#         └── project-001/
#             └── .atomic/
```

### Behind Fastify Proxy

```javascript
// fastify-proxy.js
const fastify = require('fastify')({ logger: true });

// Proxy tenant/project requests to Rust API
fastify.register(require('@fastify/http-proxy'), {
  upstream: 'http://localhost:8080', // Rust API server
  prefix: '/api',
  rewritePrefix: '', // Remove /api prefix when forwarding
});

// Route mapping:
// Frontend: /api/tenant/123/portfolio/456/project/789/changes
// Proxies to: http://localhost:8080/tenant/123/portfolio/456/project/789/changes
// Rust API maps to: /tenant-data/123/456/789/.atomic/

fastify.get('/health', async (request, reply) => {
  return { status: 'ok', service: 'fastify-proxy' };
});
```

### Library Usage

```rust
use atomic_api::ApiServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = ApiServer::new("/tenant-data").await?;
    server.serve("127.0.0.1:8080").await?;
    Ok(())
}
```

## Environment Variables

- `ATOMIC_API_BIND` - REST API server bind address (default: `127.0.0.1:8080`)
- `ATOMIC_WS_BIND` - WebSocket server bind address (default: `127.0.0.1:8081`)

## Development

### Building

```bash
cargo build --release
```

### Testing

```bash
cargo test
```

### Running

```bash
# Development mode
cargo run -- /tenant-data

# Production mode
./target/release/atomic-api /tenant-data

# Test REST API with curl:
curl http://localhost:8080/health
curl http://localhost:8080/tenant/123/portfolio/456/project/789/changes
curl http://localhost:8080/tenant/123/portfolio/456/project/789/changes/MNYNGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC
curl "http://localhost:8080/tenant/123/portfolio/456/project/789/changes/MNYNGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC?include_diff=true"

# Test WebSocket with wscat:
npm install -g wscat
wscat -c ws://localhost:8081
> {"id":"test","timestamp":"2025-01-15T15:40:04Z","payload":{"type":"health_check"}}
```

## Integration with Fastify

The Fastify server handles:
- Frontend serving and static assets
- Authentication and authorization
- Rate limiting and request validation
- Reverse proxy to Rust API and WebSocket servers

This Rust API server handles:
- Tenant/portfolio/project repository routing
- Direct Atomic VCS operations
- Repository data access and validation
- AI attribution tracking
- Change management and file operations
- Path security validation
- Real-time WebSocket communication for workflows

## Repository Structure

```
atomic-api/
├── src/
│   ├── lib.rs          # Public API and re-exports
│   ├── main.rs         # Standalone binary
│   ├── server.rs       # Core API server implementation
│   ├── websocket.rs    # WebSocket server implementation
│   ├── message.rs      # Message types for WebSocket communication
│   └── error.rs        # Error handling following AGENTS.md patterns
├── Cargo.toml          # Minimal dependencies
└── README.md           # This file
```

## Error Handling

Following AGENTS.md error handling strategy with hierarchical error types:

```rust
#[derive(Debug, Error)]
pub enum ApiError {
    Repository(RepositoryError),  // Wraps Atomic VCS errors
    Io(std::io::Error),          // I/O operations
    Internal { message: String }, // Server errors
}
```

All errors are automatically converted to appropriate HTTP status codes and JSON responses.

## API Response Format

### Changes List Response
```json
[
  {
    "id": "MNYNGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC",
    "hash": "MNYNGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC",
    "message": "Add new feature implementation",
    "author": "Lee Faus (username) <lee@example.com>",
    "timestamp": "2025-01-15T15:40:04.688518+00:00"
  }
]
```

### Individual Change Response (with diff)
```json
{
  "id": "MNYNGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC",
  "hash": "MNYNGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC",
  "message": "Add new feature implementation",
  "author": "Lee Faus (username) <lee@example.com>",
  "timestamp": "2025-01-15T15:40:04.688518+00:00",
  "description": "Optional longer description",
  "diff": "Change MNYNGT2VGEQZX4QA43FWBDVYQY7CGXN4J2CGE5FDFIHOWQFKFIJQC\nDate: Mon, 15 Jan 2025 15:40:04 +0000\nAuthor: Lee Faus (username) <lee@example.com>\n\n    Add new feature implementation\n\n# Hunks\n\nfile 1. \"src/main.rs\"\n\n  up 1:1, new 4:1\n+fn new_feature() {\n+    println!(\"Hello from new feature!\");\n+}\n\n  up 4:1, new 7:1/8:1\n-    println!(\"Hello, world!\");\n+    println!(\"Hello, world!\");\n+    new_feature();",
  "files_changed": ["2 change(s) found"]
}
```

**Note**: 
- Both `id` and `hash` fields contain the same value (the change hash) for distributed system compatibility
- Use `?include_diff=true` to get full diff content - without it, `diff` and `files_changed` will be `null`
- The `diff` field contains the same format as `atomic change <hash>` command output

## Implementation Status

### Current Status ✅ COMPLETE
- ✅ REST API (100% complete)
- ✅ WebSocket Server (100% complete)
- ✅ Protocol GET operations (100% complete)
- ✅ Protocol POST apply (100% complete)
- ✅ Protocol POST tagup (100% complete - **Phase 1**)
- ✅ Dependency validation (100% complete - **Phase 2**)
- ✅ Clean URL routing (100% complete - **Phase 3**)
- ✅ Integration testing (100% complete - **Phase 5**)
- ⏸️ Archive operations (skipped - not immediately needed)

### HTTP Protocol Implementation - COMPLETE

**Completed 2025-09-30** - Full distributed push/pull capabilities are now functional and tested. See implementation documentation in the `docs/` directory:

- **[docs/implementation-plan.md](docs/implementation-plan.md)** - Complete implementation overview and status
- **[docs/progress-checklist.md](docs/progress-checklist.md)** - Detailed progress tracking (5/6 phases complete)
- **[tests/README.md](tests/README.md)** - Integration test guide
- **[tests/integration_test.sh](tests/integration_test.sh)** - Automated test suite (6/6 tests passing)

**Key Achievements:**
- ✅ Clone/push/pull operations fully functional via HTTP
- ✅ Tag synchronization working (Phase 1)
- ✅ Dependency validation preventing out-of-order applies (Phase 2)
- ✅ Clean `/code/*` URL structure without `.atomic` (Phase 3)
- ✅ All integration tests passing with real atomic CLI (Phase 5)
- ✅ Separate atomic-api and atomic-remote crates (follows AGENTS.md)
- ✅ Maintains REST API backward compatibility

## Future Enhancements

1. **File Operations**: Add endpoints for browsing and reading tenant/portfolio/project files
2. **Repository Management**: Add endpoints for creating/managing tenant portfolios and projects
3. **Workflow Integration**: Integration with atomic-workflow crate for configuration-driven workflows
4. **Attribution Analytics**: Enhanced AI contribution tracking per tenant/portfolio/project
5. **Performance Optimization**: Repository connection pooling and caching
6. **Security**: Enhanced tenant isolation and request validation
7. **Change Relationships**: Add dependency tracking and change graph visualization
8. **WebSocket Authentication**: Add authentication and authorization for WebSocket connections
9. **Message Broadcasting**: Support for broadcasting workflow updates to multiple clients

## License

GPL-3.0 - Same as the main Atomic VCS project.
