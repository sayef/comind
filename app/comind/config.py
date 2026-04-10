"""
Configuration management for CoMind using pydantic-settings.

Supports YAML config files and environment variables.
Priority: Environment variables > config.yml
"""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any, Literal

import yaml
from pydantic import Field, field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict


class SearchSettings(BaseSettings):
    """Search engine configuration"""

    model_config = SettingsConfigDict(
        env_prefix="COMIND_SEARCH_",
        extra="ignore",
    )

    enable_text_search: bool = Field(default=True, description="Enable text search with Whoosh")
    enable_semantic_search: bool = Field(
        default=True,
        description="Enable semantic search with embeddings",
    )
    embedding_model: str = Field(
        default="BAAI/bge-small-en-v1.5",
        description="FastEmbed model for embeddings",
    )
    index_dir: Path = Field(
        default=Path("search_index"),
        description="Directory for search indexes",
    )
    max_results: int = Field(default=10, ge=1, le=100, description="Maximum search results")


class WikiSettings(BaseSettings):
    """Wiki generation configuration"""

    model_config = SettingsConfigDict(
        env_prefix="COMIND_WIKI_",
        extra="ignore",
    )

    enable_llm: bool = Field(default=False, description="Enable LLM for wiki generation")
    llm_provider: Literal["openai", "anthropic", "local"] = Field(
        default="openai",
        description="LLM provider",
    )
    llm_model: str = Field(default="gpt-4o-mini", description="LLM model name")
    llm_api_key: str | None = Field(default=None, description="LLM API key")
    llm_base_url: str | None = Field(default=None, description="LLM base URL for custom endpoints")
    output_dir: Path = Field(default=Path("wiki_output"), description="Wiki output directory")


class IndexingSettings(BaseSettings):
    """Code indexing configuration"""

    model_config = SettingsConfigDict(
        env_prefix="COMIND_INDEX_",
        extra="ignore",
    )

    max_file_size_mb: int = Field(default=10, ge=1, description="Maximum file size to index (MB)")
    exclude_patterns: list[str] = Field(
        default_factory=lambda: [
            "**/__pycache__/**",
            "**/.git/**",
            "**/.venv/**",
            "**/venv/**",
            "**/node_modules/**",
            "**/.pytest_cache/**",
            "**/*.pyc",
        ],
        description="Glob patterns to exclude from indexing",
    )
    max_workers: int = Field(default=4, ge=1, le=16, description="Maximum parallel workers")


class ProcessDetectionSettings(BaseSettings):
    """Process detection configuration"""

    model_config = SettingsConfigDict(
        env_prefix="COMIND_PROCESS_",
        extra="ignore",
    )

    max_trace_depth: int = Field(default=10, ge=1, le=50, description="Maximum trace depth")
    max_branching: int = Field(default=4, ge=1, le=10, description="Maximum branching factor")
    max_processes: int = Field(default=75, ge=1, le=500, description="Maximum processes to detect")
    min_steps: int = Field(default=3, ge=2, le=10, description="Minimum steps for a process")


class StorageSettings(BaseSettings):
    """Centralized storage configuration"""

    model_config = SettingsConfigDict(
        env_prefix="COMIND_STORAGE_",
        extra="ignore",
    )

    data_dir: Path = Field(
        default=Path.home() / ".comind" / "data",
        description="Base directory for all CoMind data storage",
    )

    @property
    def indexes_dir(self) -> Path:
        """Directory for search indexes"""
        return self.data_dir / "indexes"

    @property
    def graphs_dir(self) -> Path:
        """Directory for knowledge graphs"""
        return self.data_dir / "graphs"

    @property
    def wiki_dir(self) -> Path:
        """Directory for wiki content"""
        return self.data_dir / "wiki"

    @property
    def cache_dir(self) -> Path:
        """Directory for cache"""
        return self.data_dir / "cache"

    @property
    def repos_dir(self) -> Path:
        """Directory for persistent git repository clones"""
        return self.data_dir / "repos"

    @property
    def duckdb_path(self) -> Path:
        """Path to single shared DuckDB database file"""
        return self.data_dir / "comind.duckdb"


class ServerSettings(BaseSettings):
    """API server configuration"""

    model_config = SettingsConfigDict(
        env_prefix="COMIND_SERVER_",
        extra="ignore",
    )

    host: str = Field(default="0.0.0.0", description="Server host")
    port: int = Field(default=8000, ge=1024, le=65535, description="Server port")
    reload: bool = Field(default=False, description="Enable auto-reload in development")
    workers: int = Field(default=1, ge=1, le=16, description="Number of worker processes")
    log_level: Literal["debug", "info", "warning", "error", "critical"] = Field(
        default="info",
        description="Logging level",
    )
    cors_origins: list[str] = Field(
        default_factory=lambda: ["*"],
        description="CORS allowed origins",
    )


class MCPSettings(BaseSettings):
    """MCP server configuration"""

    model_config = SettingsConfigDict(
        env_prefix="COMIND_MCP_",
        extra="ignore",
    )

    server_name: str = Field(default="comind-python", description="MCP server name")
    server_version: str = Field(default="0.1.0", description="MCP server version")
    enable_stdio: bool = Field(default=True, description="Enable stdio transport")
    enable_http: bool = Field(default=False, description="Enable HTTP/SSE transport")


class Settings(BaseSettings):
    """Main application settings"""

    model_config = SettingsConfigDict(
        env_file=".env",
        env_file_encoding="utf-8",
        case_sensitive=False,
        extra="ignore",
    )

    # HuggingFace token (optional, suppresses warnings)
    hf_token: str | None = None

    # Application metadata
    app_name: str = Field(default="CoMind", description="Application name")
    app_version: str = Field(default="0.1.0", description="Application version")
    debug: bool = Field(default=False, description="Debug mode")
    environment: Literal["development", "staging", "production"] = Field(
        default="development",
        description="Environment",
    )

    # Component settings
    storage: StorageSettings = Field(default_factory=StorageSettings)
    search: SearchSettings = Field(default_factory=SearchSettings)
    wiki: WikiSettings = Field(default_factory=WikiSettings)
    indexing: IndexingSettings = Field(default_factory=IndexingSettings)
    process_detection: ProcessDetectionSettings = Field(default_factory=ProcessDetectionSettings)
    server: ServerSettings = Field(default_factory=ServerSettings)
    mcp: MCPSettings = Field(default_factory=MCPSettings)

    @field_validator("search", mode="before")
    @classmethod
    def validate_search_settings(cls, v: SearchSettings | dict | None) -> SearchSettings:
        """Validate and create search settings"""
        if v is None:
            return SearchSettings()
        if isinstance(v, dict):
            return SearchSettings(**v)
        return v

    @field_validator("wiki", mode="before")
    @classmethod
    def validate_wiki_settings(cls, v: WikiSettings | dict | None) -> WikiSettings:
        """Validate and create wiki settings"""
        if v is None:
            return WikiSettings()
        if isinstance(v, dict):
            return WikiSettings(**v)
        return v


def _load_yaml_config(config_path: Path | None = None) -> dict[str, Any]:
    """Load configuration from YAML file"""
    if config_path is None:
        # Look for config.yml in standard locations
        search_paths = [
            Path.cwd() / "config.yml",
            Path(__file__).parent.parent / "config.yml",
            Path.home() / ".comind" / "config.yml",
        ]

        for path in search_paths:
            if path.exists():
                config_path = path
                break

    if config_path is None or not config_path.exists():
        return {}

    with open(config_path) as f:
        return yaml.safe_load(f) or {}


def _expand_path(value: Any) -> Any:
    """Recursively expand paths with tilde and environment variables"""
    if isinstance(value, str):
        # Expand tilde and environment variables
        return os.path.expandvars(str(Path(value).expanduser()))
    if isinstance(value, dict):
        return {k: _expand_path(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_expand_path(item) for item in value]
    return value


def _merge_yaml_with_settings(yaml_config: dict[str, Any]) -> dict[str, Any]:
    """Merge YAML config into pydantic settings format with path expansion"""
    merged = {}

    # Map YAML structure to pydantic settings
    if "app" in yaml_config:
        merged["app_name"] = yaml_config["app"].get("name", "CoMind")
        merged["app_version"] = yaml_config["app"].get("version", "0.1.0")
        merged["debug"] = yaml_config["app"].get("debug", False)
        merged["environment"] = yaml_config["app"].get("environment", "development")

    # Component settings - expand paths in all values
    for component in [
        "storage",
        "search",
        "wiki",
        "indexing",
        "process_detection",
        "server",
        "mcp",
    ]:
        if component in yaml_config:
            merged[component] = _expand_path(yaml_config[component])

    return merged


# Global settings instance
_settings: Settings | None = None


def get_settings(config_path: Path | None = None) -> Settings:
    """
    Get global settings instance (singleton pattern)

    Loads configuration from:
    1. config.yml (if found)
    2. Environment variables (overrides YAML)
    """
    global _settings
    if _settings is None:
        # Load YAML config first
        yaml_config = _load_yaml_config(config_path)
        merged_config = _merge_yaml_with_settings(yaml_config)

        # Create settings with YAML defaults, then env vars override
        _settings = Settings(**merged_config)
    return _settings


def reload_settings(config_path: Path | None = None) -> Settings:
    """Reload settings from YAML and environment"""
    global _settings

    # Load YAML config first
    yaml_config = _load_yaml_config(config_path)
    merged_config = _merge_yaml_with_settings(yaml_config)

    # Create settings with YAML defaults, then env vars override
    _settings = Settings(**merged_config)
    return _settings
