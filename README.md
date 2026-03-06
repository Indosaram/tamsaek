# Tamsaek

[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)

A local file search engine with full-text capabilities. Built with [Tantivy](https://github.com/quickwit-oss/tantivy) for blazing-fast search. Available as both an **MCP server** and a standalone **CLI tool**.

## Features

- **Full-Text Search**: Powered by Tantivy with phrases, boolean operators, and field-specific queries
- **Regex Search**: Find patterns using regular expressions (term-level)
- **Metadata Filtering**: Filter by file extension, source type
- **Incremental Indexing**: Add documents without rebuilding the index
- **Zero External Dependencies**: No separate database or search server required
- **Shared Index**: CLI and MCP server share the same index — index once, search from anywhere
- **MCP Standard Compliant**: Proper error handling, tool annotations

## Installation

### From Source

```bash
git clone https://github.com/Indosaram/tamsaek.git
cd tamsaek
cargo build --release
```

Binaries:
- `target/release/tamsaek-mcp` — MCP server
- `target/release/tamsaek` — CLI tool

### Using Cargo

```bash
# MCP server
cargo install tamsaek-mcp

# CLI tool
cargo install tamsaek-cli
```

---

## CLI Usage

### Global Options

| Option | Description |
|--------|-------------|
| `-i, --index-path <PATH>` | Custom index path (default: OS data directory) |
| `-h, --help` | Print help |
| `-V, --version` | Print version |

---

### `tamsaek index` — Index a directory

Recursively scans a directory and adds files to the search index.

```bash
# Index all files in a directory
tamsaek index /path/to/project

# Index only specific extensions
tamsaek index /path/to/project -e rs,toml,md

# Non-recursive indexing (current directory only)
tamsaek index /path/to/project --recursive false
```

| Option | Description |
|--------|-------------|
| `<PATH>` | Path to the directory to index (required) |
| `-e, --extensions <EXT>` | Comma-separated list of file extensions to include (e.g. `rs,md,txt`) |
| `-r, --recursive` | Index recursively (default: `true`) |

---

### `tamsaek search` — Full-text search

Search across indexed documents using natural language queries.

```bash
tamsaek search "error handling"
tamsaek search "TamsaekIndex" --limit 5
```

| Option | Description |
|--------|-------------|
| `<QUERY>` | The search query (required) |
| `-l, --limit <N>` | Maximum results to return (default: `10`) |

**Output example:**
```
Found 4 results for 'TamsaekIndex':
[1] /path/to/lib.rs (Score: 3.42)
  ID: /path/to/lib.rs
  Snippet: use tamsaek_core::{TamsaekIndex, Document};
```

---

### `tamsaek search-regex` — Regex search

Search documents using regular expression patterns. Regex operates at the **term level** (individual words), not across entire lines.

```bash
tamsaek search-regex "tamsaek.*"
tamsaek search-regex "inde[x]" --limit 3
```

| Option | Description |
|--------|-------------|
| `<PATTERN>` | Regex pattern to search for (required) |
| `-l, --limit <N>` | Maximum results to return (default: `10`) |

> **Note**: Tantivy regex matches individual tokenized terms. Multi-word patterns like `fn.*open` won't match across token boundaries. Use patterns like `doc.*ment` (within a single word) instead.

---

### `tamsaek filter` — Filter documents

Filter indexed documents by file extension or source type.

```bash
# List all .rs files
tamsaek filter --extension rs

# Filter by source
tamsaek filter --source local --limit 20

# Combine filters
tamsaek filter --extension md --source local
```

| Option | Description |
|--------|-------------|
| `-e, --extension <EXT>` | File extension to filter by (e.g. `rs`) |
| `-s, --source <SOURCE>` | Source to filter by (e.g. `local`) |
| `-l, --limit <N>` | Maximum results to return (default: `10`) |

---

### `tamsaek get-document` — Retrieve a document

Fetch a specific document from the index by its ID (typically the file path).

```bash
# Preview (truncated to 500 chars)
tamsaek get-document "/path/to/file.rs"

# Full content
tamsaek get-document "/path/to/file.rs" --full
```

| Option | Description |
|--------|-------------|
| `<ID>` | Document ID to retrieve (required) |
| `--full` | Show full content instead of 500-char preview |

---

### `tamsaek stats` — Index statistics

Display information about the current index.

```bash
tamsaek stats
```

**Output example:**
```
Index Statistics:
  Path: /Users/you/Library/Application Support/tamsaek/index
  Documents: 27
```

---

### `tamsaek remove` — Remove a document

Remove a single document from the index by its ID.

```bash
tamsaek remove "/path/to/old-file.rs"
```

| Option | Description |
|--------|-------------|
| `<ID>` | Document ID to remove (required) |

---

### `tamsaek clear` — Clear the index

Remove **all** documents from the index. Prompts for confirmation unless `--force` is used.

```bash
# With confirmation prompt
tamsaek clear

# Skip confirmation
tamsaek clear --force
```

| Option | Description |
|--------|-------------|
| `-f, --force` | Skip the confirmation prompt |

---

## MCP Server Usage

### Setup with Claude Desktop

Add to `~/Library/Application Support/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "tamsaek": {
      "command": "/path/to/tamsaek-mcp"
    }
  }
}
```

### Available MCP Tools

| Tool | Description | Read-Only |
|------|-------------|-----------|
| `search` | Full-text search | Yes |
| `search-regex` | Regex pattern search | Yes |
| `filter` | Filter by extension/source | Yes |
| `get-document` | Retrieve document by ID | Yes |
| `index-directory` | Index files from a directory | No |
| `remove-document` | Remove a document | No |
| `get-stats` | Index statistics | Yes |
| `clear-index` | Clear all documents | No (destructive) |

### MCP Tool Examples

**search:**
```json
{
  "query": "rust programming",
  "limit": 20
}
```

**index-directory:**
```json
{
  "path": "/path/to/documents",
  "extensions": ["txt", "md", "rs"],
  "recursive": true
}
```

---

## Architecture

```
tamsaek/
├── crates/
│   ├── tamsaek-core/   # Search engine library (shared)
│   ├── tamsaek-mcp/    # MCP server binary
│   └── tamsaek-cli/    # CLI tool binary
```

`tamsaek-core` provides the indexing and search engine, shared by both the MCP server and CLI. This means they operate on the **same index** — documents indexed via CLI are immediately searchable from the MCP server and vice versa.

## Configuration

### Index Location

Default index directory by platform:

| Platform | Path |
|----------|------|
| macOS | `~/Library/Application Support/tamsaek/index` |
| Linux | `~/.local/share/tamsaek/index` |
| Windows | `%APPDATA%\tamsaek\index` |

Override with the `--index-path` flag:
```bash
tamsaek --index-path /custom/path search "query"
```

## Development

```bash
# Check compilation
cargo check

# Run tests (includes integration tests for CLI)
cargo test

# Run with logging
RUST_LOG=info cargo run -p tamsaek-mcp

# Build release
cargo build --release
```

## License

MIT License - see [LICENSE](LICENSE)
