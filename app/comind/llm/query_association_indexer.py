"""
Query Association Indexer

Post-processing step that generates query associations for symbols after initial indexing.
This runs after the graph is built so we have caller/callee information.
"""

import asyncio
from typing import Dict, Any, List
from pathlib import Path

from comind.core.graph import Symbol, SymbolType
from comind.storage.graph_adapter import KnowledgeGraph
from comind.search.query_generator import QueryGenerator
from comind.llm.llm_client import LLMConfig
from comind.utils.snippet_extractor import CodeSnippetExtractor
from comind.logging_config import get_logger

logger = get_logger(__name__)


class QueryAssociationIndexer:
    """Generate query associations for symbols in the graph"""
    
    def __init__(
        self,
        graph: KnowledgeGraph,
        llm_config: LLMConfig,
        repo_root: Path = None,
        incremental_indexer = None
    ):
        self.graph = graph
        self.query_generator = QueryGenerator(graph, llm_config)
        self.snippet_extractor = CodeSnippetExtractor(repo_root=repo_root)
        self.incremental_indexer = incremental_indexer
        self.llm_config = llm_config
    
    async def generate_queries_for_repo(
        self,
        repo_id: str,
        concurrency: int = 5,
        skip_types: List[str] = None,
        batch_size: int = 20,
        progress_callback: callable = None
    ) -> Dict[str, Any]:
        """
        Generate query associations for all symbols in a repository using batch processing.
        
        Args:
            repo_id: Repository identifier
            concurrency: Number of concurrent batch LLM calls
            skip_types: Symbol types to skip (default: ["import", "module"])
            batch_size: Number of symbols to process in one LLM call
            progress_callback: Optional callback(current, total) for progress updates
            
        Returns:
            Statistics about query generation
        """
        skip_types = skip_types or ["import", "module"]
        
        logger.info("Starting query association generation", repo_id=repo_id)
        
        # Get all symbols for this repo
        all_symbols = await self._get_repo_symbols(repo_id)
        
        # Filter symbols and check cache
        symbols_to_process = []
        cached_count = 0
        total_queries = 0
        
        for s in all_symbols:
            if s.type.value in skip_types:
                continue
                
            # Check cache if incremental indexer is available
            if self.incremental_indexer:
                should_regenerate = await self.incremental_indexer.should_regenerate_llm_content(
                    s, 'queries'
                )
                if not should_regenerate and s.associated_queries:
                    # Use cached queries
                    cached_count += 1
                    total_queries += len(s.associated_queries)
                    continue
            
            symbols_to_process.append(s)
        
        logger.info(
            "Symbols to process",
            total=len(all_symbols),
            cached=cached_count,
            to_generate=len(symbols_to_process)
        )
        
        if not symbols_to_process:
            return {
                "repo_id": repo_id,
                "symbols_processed": cached_count,
                "symbols_failed": 0,
                "total_queries_generated": total_queries,
                "avg_queries_per_symbol": total_queries / cached_count if cached_count > 0 else 0
            }
        
        # Extract code snippets for all symbols
        code_snippets = {}
        for symbol in symbols_to_process:
            snippet = await self.snippet_extractor.extract_snippet(symbol)
            if snippet:
                code_snippets[symbol.id] = snippet.get("code", "")
        
        # Process in batches with concurrency
        processed = 0
        failed = 0
        batches_completed = 0
        semaphore = asyncio.Semaphore(concurrency)
        
        async def process_batch(batch_idx: int, batch_symbols: List[Symbol]):
            nonlocal total_queries, processed, failed, batches_completed
            
            async with semaphore:
                try:
                    # Generate queries for entire batch in one LLM call
                    batch_results = await self.query_generator.generate_queries_batch(
                        symbols=batch_symbols,
                        code_snippets=code_snippets,
                        batch_size=len(batch_symbols)  # Process entire batch in one call
                    )
                    
                    # Update symbols with results
                    import json
                    for symbol in batch_symbols:
                        queries = batch_results.get(symbol.id, [])
                        if queries:
                            symbol.associated_queries = queries
                            await self.graph.add_symbol(symbol)
                            
                            # Save to cache
                            if self.incremental_indexer:
                                await self.incremental_indexer.save_llm_result(
                                    symbol=symbol,
                                    cache_type='queries',
                                    content=json.dumps(queries),
                                    model=self.llm_config.model,
                                    metadata={'query_count': len(queries)}
                                )
                            
                            total_queries += len(queries)
                            processed += 1
                        else:
                            failed += 1
                    
                    batches_completed += 1
                    
                    # Update progress callback if provided
                    if progress_callback:
                        progress_callback(batches_completed, len(batches))
                    
                    # Log every 10% or at completion (DEBUG to avoid interfering with Rich progress)
                    pct_complete = int((processed / len(symbols_to_process)) * 100)
                    prev_pct = int(((processed - len(batch_symbols)) / len(symbols_to_process)) * 100)
                    
                    if pct_complete // 10 > prev_pct // 10 or processed == len(symbols_to_process):
                        logger.debug(
                            f"Query generation: {processed}/{len(symbols_to_process)} symbols ({pct_complete}%), {total_queries} queries generated"
                        )
                    
                except Exception as e:
                    logger.error(f"Batch {batch_idx + 1} processing failed: {e}")
                    failed += len(batch_symbols)
                    batches_completed += 1
                    if progress_callback:
                        progress_callback(batches_completed, len(batches))
        
        # Split into batches and process concurrently
        batches = [
            symbols_to_process[i:i + batch_size]
            for i in range(0, len(symbols_to_process), batch_size)
        ]
        
        logger.info(f"Processing {len(batches)} batches with concurrency={concurrency}")
        
        tasks = [process_batch(idx, batch) for idx, batch in enumerate(batches)]
        await asyncio.gather(*tasks)
        
        total_processed = processed + cached_count
        total_queries_final = total_queries
        
        logger.info(
            "Query association generation complete",
            repo_id=repo_id,
            processed=total_processed,
            failed=failed,
            total_queries=total_queries_final,
            avg_queries_per_symbol=total_queries_final / total_processed if total_processed > 0 else 0
        )
        
        return {
            "repo_id": repo_id,
            "symbols_processed": total_processed,
            "symbols_failed": failed,
            "total_queries_generated": total_queries_final,
            "avg_queries_per_symbol": total_queries_final / total_processed if total_processed > 0 else 0
        }
    
    async def _get_repo_symbols(self, repo_id: str) -> List[Symbol]:
        """Get all symbols for a repository"""
        # Get all symbols from the graph using the DuckDB backend
        all_symbols = await self.graph.get_all_symbols(repo_id=repo_id)
        return all_symbols
