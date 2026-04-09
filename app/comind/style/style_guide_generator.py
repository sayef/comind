"""
Style Guide Markdown Generator

Generates comprehensive style guide markdown files from extracted patterns,
following the SCAN_SPEC.md format.
"""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any

from comind.style.style_extractor import StylePatterns
from comind.logging_config import get_logger

logger = get_logger(__name__)


class StyleGuideGenerator:
    """Generate markdown style guide from extracted patterns"""
    
    def __init__(self, patterns: StylePatterns, repo_name: str):
        self.patterns = patterns
        self.repo_name = repo_name
    
    def generate(self) -> str:
        """Generate complete style guide markdown"""
        sections = [
            self._generate_header(),
            self._generate_environment_section(),
            self._generate_coding_style_section(),
            self._generate_documentation_section(),
            self._generate_error_handling_section(),
            self._generate_logging_section(),
            self._generate_architecture_section(),
            self._generate_library_usage_section(),
            self._generate_micro_idioms_section(),
            self._generate_consistency_assessment(),
        ]
        
        return "\n\n".join(sections)
    
    def _generate_header(self) -> str:
        """Generate document header"""
        return f"""# {self.repo_name.upper().replace('-', '_')}_STYLE_GUIDE

**Auto-generated from repository analysis**

This style guide documents the dominant coding patterns, conventions, and practices
found in the `{self.repo_name}` repository. Follow these patterns to maintain consistency."""
    
    def _generate_environment_section(self) -> str:
        """Generate Environment & Tooling section"""
        return f"""## 1. Environment & Tooling Assumptions

### 1.1 Python Version & Syntax

**Inferred Target Python Version:** {self.patterns.python_version}

**Modern Syntax Usage:**
- Type hints with `|` union operator: {self._format_prevalence(self.patterns.advanced_typing.get("Union", None))}
- Async/await patterns: {self._format_prevalence(self.patterns.async_usage)}

### 1.2 Package & Dependency Management

**Primary package manager:** {self.patterns.package_manager}

**Lockfile presence:** {"Yes" if self.patterns.has_lockfile else "No"}

### 1.3 Linter & Formatter Footprints

**Max line length:** {self.patterns.max_line_length} characters

**Code formatting:** Likely using Black/Ruff with default settings

**Prevalence:** High"""
    
    def _generate_coding_style_section(self) -> str:
        """Generate General Coding Style section"""
        
        # Get dominant function naming
        func_naming = "snake_case"
        if self.patterns.function_naming:
            func_naming = self.patterns.function_naming.most_common(1)[0][0]
        
        # Get dominant class naming
        class_naming = "PascalCase"
        if self.patterns.class_naming:
            class_naming = self.patterns.class_naming.most_common(1)[0][0]
        
        return f"""## 2. General Coding Style

### 2.1 Function Structure

**Naming style:** {func_naming}

**Side-effect vs pure preference:** Mixed (context-dependent)

### 2.2 Naming Conventions

**Function naming:** `{func_naming}`
- Example: `analyze_repository()`, `get_context()`, `build_index()`

**Class naming:** `{class_naming}`
- Example: `KnowledgeGraph`, `StyleExtractor`, `QueryEngine`

**Constants:** `UPPER_SNAKE_CASE`
- Example: `MAX_RESULTS`, `DEFAULT_TIMEOUT`

**Private functions/variables:** Single underscore `_private`
- Prevalence: {self._format_counter_prevalence(self.patterns.private_naming, "single_underscore")}

**Canonical Example:**

```python
class RepositoryAnalyzer:
    MAX_DEPTH = 10  # Constant
    
    def __init__(self):
        self._cache = {{}}  # Private attribute
    
    def analyze_code(self, repo_path: str) -> dict:
        \"\"\"Public method using snake_case\"\"\"
        return self._process_files(repo_path)
    
    def _process_files(self, path: str) -> dict:
        \"\"\"Private helper method\"\"\"
        pass
```

**Prevalence:** High

### 2.3 Typing Practices

**Type hints required:** {self._format_prevalence(self.patterns.type_hints_usage)}

**Return types specified:** {self._format_prevalence(self.patterns.return_type_usage)}

**Advanced Typing Usage:**
- `TypeVar` / `Generic`: {self._format_prevalence(self.patterns.advanced_typing.get("TypeVar", None))}
- `Protocol`: {self._format_prevalence(self.patterns.advanced_typing.get("Protocol", None))}
- `Callable`: {self._format_prevalence(self.patterns.advanced_typing.get("Callable", None))}
- `Optional` / `Union`: {self._format_prevalence(self.patterns.advanced_typing.get("Optional", None))}

**Canonical Example:**

```python
from typing import Any, Optional

async def search(
    query: str,
    repo_id: str,
    max_results: int = 10,
    include_wiki: bool = True
) -> dict[str, Any]:
    \"\"\"All parameters and return types are annotated\"\"\"
    results = await self._execute_search(query)
    return {{"results": results, "total": len(results)}}
```

**Prevalence:** {self.patterns.type_hints_usage.prevalence}"""
    
    def _generate_documentation_section(self) -> str:
        """Generate Documentation Style section"""
        return f"""## 3. Documentation Style

### 3.1 Docstring Format

**Format:** {self.patterns.docstring_format}

**Coverage:** {self.patterns.docstring_coverage.percentage:.1f}% of functions documented

**Required sections:** Description, Args, Returns

**Type hints in docstrings:** Omitted (types in signature)

**Canonical Docstring Example:**

```python
async def analyze_repository(request: IndexRequest) -> dict[str, Any]:
    \"\"\"
    Complete repository analysis pipeline.
    
    Analyzes a repository (local or Git URL) and creates:
    - Knowledge graph of symbols and relationships
    - BM25S and semantic search indexes
    - LLM-powered wiki documentation
    
    Args:
        request: Analysis request with repo path/URL and options
        
    Returns:
        Analysis results with phase status and statistics
        
    Raises:
        HTTPException: If git clone or indexing fails
    \"\"\"
    pass
```

**Prevalence:** {self.patterns.docstring_coverage.prevalence}

### 3.2 Inline Comments & Metadata

**Comment density:** Medium

**TODO format:** `# TODO: description` or `# FIXME: issue`

**Comment tone:** Explanatory - focus on "why" over "what"

**Prevalence:** Medium"""
    
    def _generate_error_handling_section(self) -> str:
        """Generate Error Handling section"""
        return f"""## 4. Error Handling & Validation

**Custom exception hierarchies:** HTTPException from FastAPI

**Error handling style:** Raise and log, don't swallow

**Exception chaining:** `raise ... from exc` used when appropriate

**Validation style:** Pydantic models for request validation

**Canonical Example:**

```python
try:
    result = subprocess.run(cmd, capture_output=True, timeout=300)
    
    if result.returncode != 0:
        raise HTTPException(
            status_code=400,
            detail=f"Git clone failed: {{result.stderr}}"
        )
except subprocess.TimeoutExpired:
    raise HTTPException(
        status_code=408,
        detail="Git clone timeout (exceeded 5 minutes)"
    )
except Exception as e:
    logger.error("Unexpected error", error=str(e))
    raise HTTPException(
        status_code=500,
        detail=f"Failed to clone repository: {{str(e)}}"
    )
```

**Prevalence:** High"""
    
    def _generate_logging_section(self) -> str:
        """Generate Logging section"""
        structured = "Yes" if self.patterns.structured_logging else "No"
        
        return f"""## 5. Logging & Observability

**Logger initialization:** `{self.patterns.logger_init_pattern}`

**Structured logging:** {structured}

**Log format:** {"JSON structured logs with Rich console formatting" if self.patterns.structured_logging else "Standard text logs"}

**Contextual logging:** Yes - using structlog with context variables

**Log level usage:**
- `DEBUG`: Detailed diagnostic information
- `INFO`: General informational messages (default)
- `WARNING`: Suppressed third-party library noise
- `ERROR`: Error events that need attention

**Canonical Example:**

```python
from comind.logging_config import get_logger

logger = get_logger(__name__)

# Structured logging with context
logger.info(
    "Starting repository analysis",
    repo_path=repo_path,
    repo_name=repo_name,
    is_git=is_git
)

# Error logging
logger.error("Indexing failed", error=index_result["error"])

# Warning for edge cases
logger.warning("Repository not found", repo_id=repo_id)
```

**Prevalence:** High"""
    
    def _generate_architecture_section(self) -> str:
        """Generate Architecture & Concurrency section"""
        return f"""## 6. Architecture & Concurrency Patterns

### 6.1 Async / Await Usage

**Heavy use of asyncio:** {self.patterns.async_usage.prevalence}

**Async functions:** {self.patterns.async_usage.percentage:.1f}% of functions are async

**Mix of sync/async:** Yes - async for I/O operations, sync for CPU-bound tasks

**Canonical Example:**

```python
async def analyze_repository(request: IndexRequest) -> dict[str, Any]:
    \"\"\"Async endpoint for long-running analysis\"\"\"
    
    # Async I/O operations
    index_result = await indexer.index_repository(repo_path=repo_path)
    await query_engine.save_repo_index(repo_id, settings.storage.indexes_dir)
    
    # Async wiki generation
    wiki_result = await generate_wiki_func(
        repo_path=repo_path,
        storage_path=str(wiki_storage),
        graph=graph
    )
    
    return {{"status": "success", "phases": {{...}}}}
```

**Prevalence:** {self.patterns.async_usage.prevalence}

### 6.2 Parallelism

**Threading vs Multiprocessing:** Async I/O preferred over threading

**Batch processing:** Not heavily used

**Prevalence:** Low"""
    
    def _generate_library_usage_section(self) -> str:
        """Generate Library & Framework Usage section"""
        
        # Get top imports
        top_imports = self.patterns.common_imports.most_common(10)
        imports_list = "\n".join([f"- `{name}` ({count} usages)" for name, count in top_imports])
        
        return f"""## 7. Library & Framework Usage Patterns

### 7.1 HTTP & API Clients

**Primary HTTP library:** FastAPI for server, httpx/requests for clients

**Timeout enforcement:** Yes - explicit timeouts on subprocess calls

**Retry mechanism:** Not heavily used

### 7.2 Common Libraries

**Most frequently imported:**

{imports_list if imports_list else "- Standard library modules"}

### 7.3 Date, Time, & Localization

**Timezone aware:** Yes - using `utc=False` for local time in logs

**ISO format:** Yes - `TimeStamper(fmt="iso")`

### 7.4 Environment & Secrets

**Environment variables:** `os.getenv()` for secrets, Pydantic Settings for config

**Secret handling:** Environment variables (e.g., `GITLAB_API_PRIVATE_TOKEN`)

**Canonical Example:**

```python
# Environment variable for secrets
gitlab_token = os.getenv("GITLAB_API_PRIVATE_TOKEN")

# Pydantic for configuration
from comind.config import get_settings
settings = get_settings()
```

**Prevalence:** High"""
    
    def _generate_micro_idioms_section(self) -> str:
        """Generate Common Micro-Idioms section"""
        return f"""## 8. Common Micro-Idioms & Performance

**Recurring patterns:**

- **`if __name__ == "__main__"`:** Used in entry points
- **Context managers:** Heavy use of `with` statements and `try/finally`
- **F-strings:** Preferred for string formatting
- **Type checking:** `isinstance()` checks before operations
- **Explicit None checks:** `if x is not None:` over `if x:`
- **Dict `.get()`:** Preferred over direct access for optional keys
- **Pathlib:** `Path` objects over string paths
- **Comprehensions:** List/dict comprehensions over loops

**Canonical Examples:**

```python
# Context managers with cleanup
try:
    # Clone repository
    cloned_repo = clone_git_repo(url, branch)
    # ... do work ...
finally:
    if cloned_repo:
        shutil.rmtree(cloned_repo, ignore_errors=True)

# F-strings for formatting
logger.info(f"Loaded {{len(repos)}} repositories")

# Type checking before operations
if isinstance(symbol_data, Symbol):
    symbol = symbol_data
elif isinstance(symbol_data, dict):
    symbol = Symbol(**symbol_data)

# Path objects
graph_file = settings.storage.graphs_dir / f"{{repo_name}}.pkl"

# Dict .get() with defaults
repo_name = metadata.get("repo_name") or metadata.get("repo_id")
```

**Prevalence:** High"""
    
    def _generate_consistency_assessment(self) -> str:
        """Generate Style Consistency Assessment"""
        return f"""## 9. Style Consistency Assessment

**Overall consistency level:** High

**Areas highly standardized:**
- Type hints on all public functions
- Async/await for I/O operations
- Structured logging with context
- Error handling with HTTPException
- Pydantic models for validation
- Snake_case for functions, PascalCase for classes

**Areas with some variation:**
- Docstring coverage (not all private functions documented)
- Comment density varies by module complexity

**Recommendations for enforcement:**

1. **Enable Ruff** with strict type checking rules
2. **Require docstrings** on all public functions (pydocstyle)
3. **Pre-commit hooks** for formatting (black/ruff format)
4. **Mypy strict mode** for type checking
5. **Enforce async** for all I/O operations

**Suggested `.ruff.toml` rules:**

```toml
[tool.ruff]
line-length = {self.patterns.max_line_length}
target-version = "py311"

[tool.ruff.lint]
select = [
    "E",   # pycodestyle errors
    "W",   # pycodestyle warnings
    "F",   # pyflakes
    "I",   # isort
    "N",   # pep8-naming
    "UP",  # pyupgrade
    "ANN", # flake8-annotations
    "ASYNC", # flake8-async
    "B",   # flake8-bugbear
]

[tool.ruff.lint.pydocstyle]
convention = "{self.patterns.docstring_format.lower()}"
```

---

**Generated by CoMind Style Guide Extractor**  
*Last updated: Auto-generated from latest repository analysis*"""
    
    def _format_prevalence(self, stats: Any) -> str:
        """Format prevalence statistics"""
        if stats is None:
            return "Unknown"
        if hasattr(stats, 'prevalence'):
            return f"{stats.prevalence} ({stats.percentage:.1f}%)"
        return "Unknown"
    
    def _format_counter_prevalence(self, counter: Any, key: str) -> str:
        """Format prevalence from Counter"""
        if not counter:
            return "Low"
        total = sum(counter.values())
        count = counter.get(key, 0)
        if total == 0:
            return "Low"
        ratio = count / total
        if ratio >= 0.7:
            return f"High ({ratio*100:.1f}%)"
        elif ratio >= 0.3:
            return f"Medium ({ratio*100:.1f}%)"
        return f"Low ({ratio*100:.1f}%)"


async def generate_style_guide_markdown(
    patterns: StylePatterns,
    repo_name: str,
    output_path: str | None = None
) -> str:
    """Generate style guide markdown from patterns
    
    Args:
        patterns: Extracted style patterns
        repo_name: Repository name for the guide
        output_path: Optional path to save the markdown file
        
    Returns:
        Generated markdown content
    """
    generator = StyleGuideGenerator(patterns, repo_name)
    markdown = generator.generate()
    
    # Save to file if path provided
    if output_path:
        output_file = Path(output_path)
        output_file.parent.mkdir(parents=True, exist_ok=True)
        output_file.write_text(markdown)
        logger.info("Style guide saved", path=output_path)
    
    return markdown
