"""
Query Generator for Symbol-to-Query Association

Generates natural language queries that developers might use to find symbols.
Uses LLM to create query associations during indexing.
"""

import json
from typing import List, Dict, Any, Optional

from comind.core.graph import Symbol, GraphBackend
from comind.llm.llm_client import LLMConfig, LLMResponse, call_llm
from comind.prompts import QUERY_GENERATION_PROMPT
from comind.logging_config import get_logger

logger = get_logger(__name__)


class QueryGenerator:
    """Generate natural language queries associated with code symbols"""
    
    def __init__(self, graph: GraphBackend, llm_config: LLMConfig):
        self.graph = graph
        self.llm_config = llm_config
    
    async def generate_queries_for_symbol(
        self,
        symbol: Symbol,
        code_snippet: str = "",
        max_queries: int = 10
    ) -> List[str]:
        """
        Generate natural language queries for a symbol using LLM with structured output.
        
        Args:
            symbol: The symbol to generate queries for
            code_snippet: Code snippet showing the symbol's implementation
            max_queries: Maximum number of queries to generate
            
        Returns:
            List of natural language query strings
        """
        # Skip generating queries for imports and modules
        if symbol.type.value in ["import", "module"]:
            return []
        
        # Get callers and callees for context
        callers = await self.graph.get_callers(symbol.id)
        callees = await self.graph.get_callees(symbol.id)
        
        callers_str = ", ".join([c.name for c in callers[:5]]) if callers else "none"
        callees_str = ", ".join([c.name for c in callees[:5]]) if callees else "none"
        
        # Prepare prompt
        prompt = QUERY_GENERATION_PROMPT.format(
            symbol_name=symbol.name,
            symbol_type=symbol.type.value,
            signature=symbol.signature or "N/A",
            docstring=symbol.docstring or "No docstring",
            file_path=symbol.file_path,
            code_snippet=code_snippet[:500] if code_snippet else "N/A",  # Limit snippet size
            callees=callees_str,
            callers=callers_str
        )
        
        # Define structured output schema
        response_format = {
            "type": "json_schema",
            "json_schema": {
                "name": "query_list",
                "strict": True,
                "schema": {
                    "type": "object",
                    "properties": {
                        "queries": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "List of natural language search queries"
                        }
                    },
                    "required": ["queries"],
                    "additionalProperties": False
                }
            }
        }
        
        try:
            # Call LLM with structured output
            llm_response = await call_llm(
                prompt=prompt,
                system_prompt="You are a code analysis assistant that generates search queries.",
                config=self.llm_config,
                response_format=response_format
            )
            
            # Parse structured JSON response
            result = json.loads(llm_response.content)
            queries = result.get("queries", [])
            
            # Filter and limit
            queries = [q.strip() for q in queries if q.strip()][:max_queries]
            
            logger.debug(
                "Generated queries for symbol",
                symbol=symbol.name,
                query_count=len(queries)
            )
            
            return queries
            
        except Exception as e:
            logger.error(
                "Failed to generate queries for symbol",
                symbol=symbol.name,
                error=str(e)
            )
            # Fallback: generate basic queries from symbol name
            return self._generate_fallback_queries(symbol)
    
    def _generate_fallback_queries(self, symbol: Symbol) -> List[str]:
        """Generate basic queries when LLM fails"""
        queries = []
        
        # Add symbol name variations
        name_lower = symbol.name.lower()
        queries.append(name_lower)
        
        # Add with type
        queries.append(f"{name_lower} {symbol.type.value}")
        
        # Add "how does X work"
        queries.append(f"how does {name_lower} work")
        
        # Add variations based on name patterns
        if "_" in symbol.name:
            # snake_case: add words
            words = symbol.name.split("_")
            queries.append(" ".join(words))
        
        # Add type-specific queries
        if symbol.type.value == "function":
            queries.append(f"{name_lower} function")
        elif symbol.type.value == "class":
            queries.append(f"{name_lower} class")
        elif symbol.type.value == "method":
            queries.append(f"{name_lower} method")
        
        return queries[:5]  # Limit fallback queries
    
    async def generate_queries_batch(
        self,
        symbols: List[Symbol],
        code_snippets: Dict[str, str] = None,
        batch_size: int = 20
    ) -> Dict[str, List[str]]:
        """
        Generate queries for multiple symbols in batch (true batching with one LLM call per batch).
        
        Args:
            symbols: List of symbols to process
            code_snippets: Optional dict mapping symbol_id to code snippet
            batch_size: Number of symbols to process in one LLM call
            
        Returns:
            Dict mapping symbol_id to list of queries
        """
        code_snippets = code_snippets or {}
        results = {}
        
        total_batches = (len(symbols) + batch_size - 1) // batch_size
        logger.debug(f"Generating queries for {len(symbols)} symbols in {total_batches} batches")
        
        # Process in batches
        for batch_num, i in enumerate(range(0, len(symbols), batch_size), 1):
            batch = symbols[i:i + batch_size]
            
            logger.debug(f"Processing batch {batch_num}/{total_batches} ({len(batch)} symbols)")
            
            # Build batch prompt with all symbols
            symbols_data = []
            for idx, symbol in enumerate(batch):
                snippet = code_snippets.get(symbol.id, "")
                
                # Get callers and callees for context
                callers = await self.graph.get_callers(symbol.id)
                callees = await self.graph.get_callees(symbol.id)
                
                callers_str = ", ".join([c.name for c in callers[:3]]) if callers else "none"
                callees_str = ", ".join([c.name for c in callees[:3]]) if callees else "none"
                
                symbols_data.append({
                    "index": idx,
                    "name": symbol.name,
                    "type": symbol.type.value,
                    "signature": symbol.signature or "N/A",
                    "docstring": symbol.docstring or "No docstring",
                    "file_path": symbol.file_path,
                    "code_snippet": snippet[:300] if snippet else "N/A",
                    "callers": callers_str,
                    "callees": callees_str
                })
            
            # Create batch prompt
            batch_prompt = self._create_batch_prompt(symbols_data)
            
            # Define structured output schema for batch
            response_format = {
                "type": "json_schema",
                "json_schema": {
                    "name": "batch_query_list",
                    "strict": True,
                    "schema": {
                        "type": "object",
                        "properties": {
                            "results": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "index": {"type": "integer"},
                                        "queries": {
                                            "type": "array",
                                            "items": {"type": "string"}
                                        }
                                    },
                                    "required": ["index", "queries"],
                                    "additionalProperties": False
                                }
                            }
                        },
                        "required": ["results"],
                        "additionalProperties": False
                    }
                }
            }
            
            try:
                # Call LLM with batch
                llm_response = await call_llm(
                    prompt=batch_prompt,
                    system_prompt="You are a code analysis assistant that generates search queries for multiple code symbols at once.",
                    config=self.llm_config,
                    response_format=response_format
                )
                
                # Parse batch response
                result = json.loads(llm_response.content)
                batch_results = result.get("results", [])
                
                # Map results back to symbols
                for item in batch_results:
                    idx = item.get("index")
                    queries = item.get("queries", [])
                    if idx is not None and idx < len(batch):
                        symbol = batch[idx]
                        results[symbol.id] = [q.strip() for q in queries if q.strip()][:10]
                
                # Fill in any missing results with fallback
                for idx, symbol in enumerate(batch):
                    if symbol.id not in results:
                        results[symbol.id] = self._generate_fallback_queries(symbol)
                        
            except Exception as e:
                logger.error(f"Batch query generation failed: {e}")
                # Fallback for entire batch
                for symbol in batch:
                    results[symbol.id] = self._generate_fallback_queries(symbol)
        
        return results
    
    def _create_batch_prompt(self, symbols_data: List[Dict]) -> str:
        """Create a prompt for batch query generation"""
        symbols_text = ""
        for s in symbols_data:
            symbols_text += f"""
Symbol {s['index']}:
- Name: {s['name']}
- Type: {s['type']}
- Signature: {s['signature']}
- Docstring: {s['docstring']}
- File: {s['file_path']}
- Code: {s['code_snippet']}
- Called by: {s['callers']}
- Calls: {s['callees']}
"""
        
        return f"""Generate natural language search queries for the following code symbols. For each symbol, generate 3-8 queries that a developer might use to find this symbol.

{symbols_text}

Return a JSON object with a "results" array, where each item has:
- "index": the symbol index (0-based)
- "queries": array of query strings

Focus on:
1. What the symbol does (functionality)
2. Common use cases
3. Problem it solves
4. Natural language variations of the name
5. Related concepts

Keep queries concise and natural."""
