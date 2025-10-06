# Changelog

## Unreleased

### Changed

- **Protocol Alignment**: Refactored HTTP API to match SSH protocol patterns exactly
  - Tag downloads now send short version (header only) instead of full tag files
  - Tag uploads now regenerate full tag files from server's channel state (server is authoritative)
  - Eliminates tag file corruption and version mismatch issues
  - Reduces bandwidth usage for tag operations
  - Simplifies client-side tag handling

## 1.1.0 - 2025-10-01

### Fixed

- **Tag File Generation**: Fixed critical issue where tag state files were not being generated after applying changes on the server, causing "Tag file is corrupt" errors during pull operations
  - Server now automatically generates tag files for new states after applying changes
  - Tag creation now generates tag files for both the pre-application state and post-application state
  - Added detailed logging for tag file operations to aid debugging

- **Working Copy Updates**: Fixed issue where server working copy was not updated after receiving pushed changes
  - Server now outputs changes to working copy after applying them, matching SSH protocol behavior
  - Files on server now reflect the latest applied changes automatically
  
- **Version Mismatch**: Updated change file format to version 7 (was version 6)
  - Ensures consistency across repositories
  - Clear version identification for debugging

### Changed

- **Version Bump**: All crates bumped to 1.1.0 for clear version identification
  - Makes it easier to identify which version of Atomic is running
  - Helps distinguish between versions with and without tag file fixes

### New Features

- **Prompt Integration**: Added `atomic prompt` command for shell prompt integration
  - Display current channel in terminal prompt (similar to Git branch display)
  - Configurable format strings with `{channel}` and `{repository}` placeholders
  - Fast performance with 5-second caching
  - Shell integration scripts for Bash, Zsh, and Fish
  - Configuration via `~/.config/atomic/config.toml` or environment variables
  - See `contrib/PROMPT_INTEGRATION.md` for documentation
  - Run `contrib/prompt-demo.sh` for a demonstration

## 1.0.0

### Initial Release

- Initial fork from Pijul 1.0.0-beta.2
- Renamed project from Pijul to Atomic
- Updated all package names and references
- Updated documentation to reflect new project name

## Historical Changes (from Pijul)

### Fixed

- Fixing a bug with name conflicts, where files could end up with 0 alive name.
- Fixing a few panics/unwraps
- Fixing a bug where a zombie file could be deleted by `pijul unrecord`, but its contents would stay zombie.
- CVE-2022-24713
- Fixed a failed assertion in the patch text format.
- Fixed a "merged vertices" bug when moving files and editing them in the same patch, where the new name was "glued" to the new lines inside the file, causing confusion.
- Fixed a performance issue on Windows, where canonicalizing paths can cause a significant slowdown (1ms for each file).

### New features

- Better documentation for `pijul key`.
- `pijul pull` does not open $EDITOR anymore when given a list of changes.
