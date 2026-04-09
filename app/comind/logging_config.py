"""
Structured logging configuration using structlog.

Provides consistent, structured logging across the application.
"""

from __future__ import annotations

import logging
import sys
from typing import Any

import structlog
from rich.console import Console
from rich.logging import RichHandler

from comind.config import get_settings


def _format_event_message(logger, method_name, event_dict):
    """Format event as message with remaining fields as key-value pairs"""
    # Remove redundant fields
    event_dict.pop("level", None)
    event_dict.pop("timestamp", None)
    
    # Extract event as the main message
    event_msg = event_dict.pop("event", "")
    
    # If there are remaining fields, format them as key=value
    if event_dict:
        kv_pairs = " ".join(f"{k}={repr(v)}" for k, v in event_dict.items())
        return f"{event_msg} {kv_pairs}"
    
    return event_msg


def configure_logging() -> None:
    """Configure structured logging for the application"""
    settings = get_settings()

    # Create Rich handler with custom formatting
    rich_handler = RichHandler(
        console=Console(stderr=True),
        rich_tracebacks=True,
        tracebacks_show_locals=settings.debug,
        markup=True,
        show_path=False,
        show_time=True,
        omit_repeated_times=False,
    )
    
    # Configure standard library logging
    logging.basicConfig(
        format="%(message)s",
        level=getattr(logging, settings.server.log_level.upper()),
        handlers=[rich_handler],
        force=True,
    )
    
    # Suppress verbose third-party library logs
    logging.getLogger("sentence_transformers").setLevel(logging.WARNING)
    logging.getLogger("httpx").setLevel(logging.WARNING)
    logging.getLogger("httpcore").setLevel(logging.WARNING)
    logging.getLogger("transformers").setLevel(logging.WARNING)
    logging.getLogger("torch").setLevel(logging.WARNING)
    logging.getLogger("uvicorn.access").setLevel(logging.WARNING)
    logging.getLogger("uvicorn.error").setLevel(logging.INFO)

    # Configure structlog to render as plain text for Rich
    structlog.configure(
        processors=[
            structlog.contextvars.merge_contextvars,
            structlog.processors.add_log_level,
            structlog.processors.StackInfoRenderer(),
            structlog.dev.set_exc_info,
            # Format event message with context as key-value pairs
            _format_event_message,
        ],
        wrapper_class=structlog.stdlib.BoundLogger,
        context_class=dict,
        logger_factory=structlog.stdlib.LoggerFactory(),
        cache_logger_on_first_use=True,
    )


def get_logger(name: str | None = None) -> structlog.BoundLogger:
    """Get a structured logger instance"""
    return structlog.get_logger(name)


# Convenience function for adding context
def bind_context(**kwargs: Any) -> None:
    """Bind context variables to all subsequent log messages"""
    structlog.contextvars.bind_contextvars(**kwargs)


def clear_context() -> None:
    """Clear all context variables"""
    structlog.contextvars.clear_contextvars()
