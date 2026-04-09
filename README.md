# CoMind

**Graph-powered code intelligence engine with semantic search, execution flow analysis, and LLM-enhanced documentation.**

CoMind indexes your codebase into a knowledge graph, enabling powerful semantic search, impact analysis, and automated documentation generation through an MCP (Model Context Protocol) server.

## Features

- 🔍 **Hybrid Search**: BM25 + semantic embeddings for accurate code search
- 🕸️ **Knowledge Graph**: DuckDB-powered graph with symbols, relationships, and execution flows
- 🤖 **LLM Integration**: Automated wiki generation and query associations
- 📊 **Impact Analysis**: Blast radius analysis for safe refactoring
- 🔄 **Execution Tracing**: Detect and search multi-step execution flows
- 🎯 **MCP Server**: Direct AI agent integration via Model Context Protocol
- ⚡ **Fast Indexing**: Incremental updates with batch processing

## Installation

```bash
# Clone the repository
git clone https://github.com/yourusername/comind.git
cd comind/app

# Install with uv (recommended)
uv sync

# Or with pip
pip install -e .
```

## Quick Start

### 1. Index a Repository

```bash
# Basic indexing
comind analyze --name my-repo /path/to/repo

# With LLM-powered features (requires API key)
export OPENAI_API_KEY=your-key-here
comind analyze --name my-repo /path/to/repo --gen-wiki --gen-queries
```

### 2. Use the MCP Server

Add to your MCP client configuration (e.g., Claude Desktop):

```json
{
  "mcpServers": {
    "comind": {
      "command": "comind",
      "args": ["mcp"]
    }
  }
}
```

### 3. Query via MCP Tools

```python
# Available MCP tools:
- repos()           # List indexed repositories
- find(query, repo) # Semantic search
- flows(query, repo)# Search execution flows
- zoom(symbol, repo)# 360° symbol context
- ripple(symbol)    # Impact analysis
- thread(entry)     # Execution trace
- guide(repo)       # Coding style guide
```

### 4. API Server (Optional)

```bash
# Start the REST API server
comind serve

# Or with uvicorn directly
uvicorn comind.api.server:create_app --host 0.0.0.0 --port 8000

# API documentation at http://localhost:8000/docs
```

API Usage:
```bash
# Index a repository
curl -X POST "http://localhost:8000/repos/index" \
  -H "Content-Type: application/json" \
  -d '{"repo_path": "/path/to/repo", "recursive": true}'

# Search code
curl -X POST "http://localhost:8000/query/search" \
  -H "Content-Type: application/json" \
  -d '{"query": "user authentication", "max_results": 10}'
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Client Applications                      │
│  (IDEs, Web UI, CLI tools, AI Agents via MCP)               │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            │ MCP Protocol / REST API
                            │
┌───────────────────────────▼─────────────────────────────────┐
│                   Query Enhancement Layer                    │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  Wiki-Enhanced Search Engine                         │   │
│  │  • Hybrid search (BM25 + semantic embeddings)        │   │
│  │  • Wiki content as context enrichment                │   │
│  │  • Graph-aware result ranking (RRF)                  │   │
│  │  • Execution flow detection                          │   │
│  └──────────────────────────────────────────────────────┘   │
└───────────────────────────┬─────────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────────┐
│                   Core Processing Layer                      │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │   Graph      │  │    Wiki      │  │   Code Snippet   │  │
│  │   Query      │  │  Generator   │  │    Extractor     │  │
│  │   Engine     │  │   (LLM)      │  │                  │  │
│  └──────────────┘  └──────────────┘  └──────────────────┘  │
└───────────────────────────┬─────────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────────┐
│                      Storage Layer (DuckDB)                  │
│  ┌────────────────┐  ┌──────────────┐  ┌────────────────┐  │
│  │ Knowledge Graph│  │ Vector Search│  │ Full-Text      │  │
│  │  (DuckPGQ)     │  │  (VSS HNSW)  │  │ Search (FTS)   │  │
│  └────────────────┘  └──────────────┘  └────────────────┘  │
│                Single DuckDB file                            │
└─────────────────────────────────────────────────────────────┘
```

### Technical Stack

### Storage
- **DuckDB**: Single-file database with VSS (vector search), FTS (full-text), and graph support
- **Embeddings**: BAAI/bge-small-en-v1.5 for semantic search
- **Incremental**: Smart caching and change detection

### Indexing Pipeline
1. **Parse** → Extract symbols and relationships
2. **Graph** → Build knowledge graph in DuckDB
3. **Embeddings** → Generate semantic vectors
4. **Wiki** → LLM-generated documentation (optional)
5. **Queries** → Natural language associations (optional)
6. **Flows** → Detect execution patterns

### Search Strategy
- **Hybrid**: Combines BM25, semantic, and graph-based search
- **Reciprocal Rank Fusion**: Merges results from multiple strategies
- **Context-aware**: Includes callers, callees, and execution flows

## Configuration

Edit `app/config.yml` or use environment variables:

```yaml
storage:
  data_dir: ~/.comind/data

search:
  embedding_model: BAAI/bge-small-en-v1.5
  
wiki:
  llm_provider: openai
  llm_model: gpt-4o-mini
```

Environment variables:
- `OPENAI_API_KEY` - For wiki and query generation
- `COMIND_DATA_DIR` - Override data directory
- `COMIND_LOG_LEVEL` - Set logging level

## Development

```bash
# Install dev dependencies
uv sync --all-extras

# Run tests
pytest

# Format code
ruff format .

# Lint
ruff check .
```

## Performance

- **Batch Processing**: 30-60x faster query generation (20 symbols/batch)
- **Concurrent Reads**: Read-only MCP server allows queries during indexing
- **Smart Caching**: Incremental updates skip unchanged files
- **HNSW Indexes**: Fast vector similarity search

## License

MIT License - see LICENSE file for details

## Contributing

Contributions welcome! Please read CONTRIBUTING.md for guidelines.

## Acknowledgments

Built with:
- [DuckDB](https://duckdb.org/) - Embedded analytics database
- [FastEmbed](https://github.com/qdrant/fastembed) - Fast embedding generation
- [BM25S](https://github.com/xhluca/bm25s) - BM25 implementation
- [MCP](https://modelcontextprotocol.io/) - Model Context Protocol
