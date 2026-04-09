"""
LLM Client for Wiki Generation

OpenAI-compatible API client using httpx.
Supports OpenAI, Azure, LiteLLM, Ollama, and any OpenAI-compatible endpoint.

Config priority: env vars > defaults
"""

import os
import httpx
from typing import Optional, Dict, Any, Callable
from dataclasses import dataclass


@dataclass
class LLMConfig:
    """LLM configuration"""
    api_key: str
    base_url: str = "https://api.openai.com/v1"
    model: str = "gpt-4o-mini"
    max_tokens: int = 16384
    temperature: float = 0.0
    provider: str = "openai"
    api_version: Optional[str] = None
    is_reasoning_model: bool = False


@dataclass
class LLMResponse:
    """LLM response"""
    content: str
    prompt_tokens: Optional[int] = None
    completion_tokens: Optional[int] = None


def resolve_llm_config(overrides: Optional[Dict[str, Any]] = None) -> LLMConfig:
    """
    Resolve LLM configuration from env vars and optional overrides.
    Priority: overrides > env vars > defaults
    """
    overrides = overrides or {}
    
    api_key = (
        overrides.get("api_key") or
        os.getenv("GITNEXUS_API_KEY") or
        os.getenv("OPENAI_API_KEY") or
        ""
    )
    
    base_url = (
        overrides.get("base_url") or
        os.getenv("GITNEXUS_LLM_BASE_URL") or
        "https://api.openai.com/v1"
    )
    
    model = (
        overrides.get("model") or
        os.getenv("GITNEXUS_MODEL") or
        "gpt-4o-mini"
    )
    
    return LLMConfig(
        api_key=api_key,
        base_url=base_url,
        model=model,
        max_tokens=overrides.get("max_tokens", 16384),
        temperature=overrides.get("temperature", 0.0),
        provider=overrides.get("provider", "openai"),
        api_version=overrides.get("api_version") or os.getenv("GITNEXUS_AZURE_API_VERSION"),
        is_reasoning_model=overrides.get("is_reasoning_model", False)
    )


def estimate_tokens(text: str) -> int:
    """Estimate token count from text (rough heuristic: ~4 chars per token)"""
    return len(text) // 4


def is_azure_provider(base_url: str) -> bool:
    """Returns true if the given base URL is an Azure OpenAI endpoint"""
    return ".openai.azure.com" in base_url or ".services.ai.azure.com" in base_url


def is_reasoning_model(model: str, override: Optional[bool] = None) -> bool:
    """
    Returns true if the model name matches a known reasoning model pattern.
    Match known bare reasoning models (o1, o3) and any o-series with -mini/-preview suffix
    """
    if override is not None:
        return override
    import re
    return bool(re.match(r"^o[1-9]\d*(-mini|-preview)$|^o1$|^o3$", model, re.IGNORECASE))


def build_request_url(base_url: str, api_version: Optional[str]) -> str:
    """Build the full chat completions URL, appending ?api-version when provided"""
    base = base_url.rstrip("/") + "/chat/completions"
    if api_version:
        base += f"?api-version={api_version}"
    return base


async def call_llm(
    prompt: str,
    config: LLMConfig,
    system_prompt: Optional[str] = None,
    on_chunk: Optional[Callable[[int], None]] = None,
    response_format: Optional[Dict[str, Any]] = None
) -> LLMResponse:
    """
    Call an OpenAI-compatible LLM API.
    Uses streaming when on_chunk callback is provided for real-time progress.
    Retries up to 3 times on transient failures (429, 5xx, network errors).
    """
    messages = []
    if system_prompt:
        messages.append({"role": "system", "content": system_prompt})
    messages.append({"role": "user", "content": prompt})
    
    # Detect Azure endpoint
    azure = config.provider == "azure" or is_azure_provider(config.base_url)
    
    # Detect reasoning model
    reasoning = is_reasoning_model(config.model, config.is_reasoning_model)
    
    url = build_request_url(config.base_url, config.api_version if azure else None)
    use_stream = on_chunk is not None
    
    # Build request body - reasoning models reject temperature and use max_completion_tokens
    body: Dict[str, Any] = {
        "model": config.model,
        "messages": messages,
        "stream": use_stream
    }
    
    if reasoning:
        body["max_completion_tokens"] = config.max_tokens
    else:
        body["max_tokens"] = config.max_tokens
        body["temperature"] = config.temperature
    
    # Add structured output format if provided
    if response_format:
        body["response_format"] = response_format
    
    # Build headers
    headers = {
        "Content-Type": "application/json"
    }
    
    if config.provider == "openrouter":
        headers["Authorization"] = f"Bearer {config.api_key}"
        headers["HTTP-Referer"] = "https://github.com/gitnexus"
    elif azure:
        headers["api-key"] = config.api_key
    else:
        headers["Authorization"] = f"Bearer {config.api_key}"
    
    # Retry logic
    max_retries = 3
    for attempt in range(max_retries):
        try:
            async with httpx.AsyncClient(timeout=300.0) as client:
                if use_stream:
                    # Streaming mode
                    content_parts = []
                    chars_received = 0
                    
                    async with client.stream("POST", url, json=body, headers=headers) as response:
                        response.raise_for_status()
                        
                        async for line in response.aiter_lines():
                            if not line.strip() or line.strip() == "data: [DONE]":
                                continue
                            
                            if line.startswith("data: "):
                                try:
                                    import json
                                    chunk_data = json.loads(line[6:])
                                    delta = chunk_data.get("choices", [{}])[0].get("delta", {})
                                    content = delta.get("content", "")
                                    
                                    if content:
                                        content_parts.append(content)
                                        chars_received += len(content)
                                        if on_chunk:
                                            on_chunk(chars_received)
                                except:
                                    continue
                    
                    full_content = "".join(content_parts)
                    return LLMResponse(content=full_content)
                
                else:
                    # Non-streaming mode
                    response = await client.post(url, json=body, headers=headers)
                    response.raise_for_status()
                    
                    result = response.json()
                    content = result["choices"][0]["message"]["content"]
                    usage = result.get("usage", {})
                    
                    return LLMResponse(
                        content=content,
                        prompt_tokens=usage.get("prompt_tokens"),
                        completion_tokens=usage.get("completion_tokens")
                    )
        
        except httpx.HTTPStatusError as e:
            if e.response.status_code == 429 or e.response.status_code >= 500:
                if attempt < max_retries - 1:
                    import asyncio
                    await asyncio.sleep(2 ** attempt)
                    continue
            raise
        except (httpx.ConnectError, httpx.ReadTimeout) as e:
            if attempt < max_retries - 1:
                import asyncio
                await asyncio.sleep(2 ** attempt)
                continue
            raise
    
    raise Exception("Max retries exceeded")


class LLMClient:
    """Thin wrapper around call_llm exposing a .generate() interface.

    This adapter lets GraphWikiGenerator use the same LLM backend as the
    module wiki generator without changing either caller.
    """

    def __init__(self, config: LLMConfig, system_prompt: Optional[str] = None) -> None:
        self._config = config
        self._system_prompt = system_prompt

    async def generate(self, prompt: str) -> str:
        response = await call_llm(prompt, self._config, self._system_prompt)
        return response.content

