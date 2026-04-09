"""
Incremental Indexer for CoMind

Handles smart re-indexing by detecting file changes and only processing
what's necessary. Integrates with DuckDB backend for efficient updates.
"""

import hashlib
import os
from pathlib import Path
from typing import Dict, List, Set, Tuple

from comind.storage.duckdb_backend import DuckDBBackend
from comind.core.graph import Symbol
from comind.logging_config import get_logger

logger = get_logger(__name__)


class IncrementalIndexer:
    """Manages incremental indexing with change detection"""
    
    def __init__(self, backend: DuckDBBackend):
        self.backend = backend
    
    async def detect_changes(
        self,
        repo_id: str,
        repo_path: str,
        file_extensions: List[str] = ['.py']
    ) -> Dict[str, str]:
        """Detect which files have changed since last index
        
        Args:
            repo_id: Repository identifier
            repo_path: Path to repository
            file_extensions: File extensions to track
            
        Returns:
            Dict mapping file_path -> status ('new', 'modified', 'deleted')
        """
        logger.debug(f"Detecting changes in {repo_path}")
        
        # Scan current files and compute hashes
        current_files = {}
        for ext in file_extensions:
            for file_path in Path(repo_path).rglob(f'*{ext}'):
                if self._should_index_file(file_path):
                    rel_path = str(file_path.relative_to(repo_path))
                    current_files[rel_path] = self._compute_file_hash(file_path)
        
        # Compare with stored metadata
        changed_files = await self.backend.detect_changed_files(repo_id, current_files)
        
        logger.debug(
            f"Change detection complete: "
            f"{sum(1 for s in changed_files.values() if s == 'new')} new, "
            f"{sum(1 for s in changed_files.values() if s == 'modified')} modified, "
            f"{sum(1 for s in changed_files.values() if s == 'deleted')} deleted"
        )
        
        return changed_files
    
    def _should_index_file(self, file_path: Path) -> bool:
        """Check if file should be indexed"""
        # Skip common directories
        skip_dirs = {
            '__pycache__', '.git', '.venv', 'venv', 
            'node_modules', '.pytest_cache', 'dist', 'build'
        }
        
        for part in file_path.parts:
            if part in skip_dirs:
                return False
        
        # Skip if file is too large (>10MB)
        try:
            if file_path.stat().st_size > 10 * 1024 * 1024:
                return False
        except:
            return False
        
        return True
    
    def _compute_file_hash(self, file_path: Path) -> str:
        """Compute SHA256 hash of file"""
        try:
            with open(file_path, 'rb') as f:
                return hashlib.sha256(f.read()).hexdigest()
        except Exception as e:
            logger.warning(f"Failed to hash {file_path}: {e}")
            return ""
    
    async def process_changes(
        self,
        repo_id: str,
        repo_path: str,
        changed_files: Dict[str, str],
        indexer_func
    ) -> Tuple[int, int, int]:
        """Process changed files
        
        Args:
            repo_id: Repository identifier
            repo_path: Path to repository
            changed_files: Dict of file_path -> status
            indexer_func: Function to index a file (async)
            
        Returns:
            Tuple of (files_added, files_modified, files_deleted)
        """
        added = 0
        modified = 0
        deleted = 0
        
        for file_path, status in changed_files.items():
            full_path = Path(repo_path) / file_path
            
            if status == 'deleted':
                # Remove symbols from deleted file
                await self.backend.delete_symbols_by_file(file_path)
                deleted += 1
                logger.debug(f"Deleted symbols from {file_path}")
                
            elif status in ('new', 'modified'):
                # Remove old symbols if modified
                if status == 'modified':
                    await self.backend.delete_symbols_by_file(file_path)
                    modified += 1
                else:
                    added += 1
                
                # Index the file
                try:
                    symbols = await indexer_func(full_path, repo_id)
                    
                    # Update file metadata
                    file_hash = self._compute_file_hash(full_path)
                    mtime = int(full_path.stat().st_mtime)
                    size = full_path.stat().st_size
                    
                    await self.backend.update_file_metadata(
                        file_path=file_path,
                        repo_id=repo_id,
                        file_hash=file_hash,
                        mtime=mtime,
                        size=size,
                        symbol_count=len(symbols)
                    )
                    
                    logger.debug(f"Indexed {file_path}: {len(symbols)} symbols")
                    
                except Exception as e:
                    logger.error(f"Failed to index {file_path}: {e}")
        
        return added, modified, deleted
    
    async def should_regenerate_llm_content(
        self,
        symbol: Symbol,
        cache_type: str
    ) -> bool:
        """Check if LLM content needs regeneration
        
        Args:
            symbol: Symbol to check
            cache_type: Type of cache ('wiki' or 'queries')
            
        Returns:
            True if content should be regenerated
        """
        # Compute content hash (signature + docstring + key properties)
        content_parts = [
            symbol.signature or "",
            symbol.docstring or "",
            symbol.name,
            symbol.type.value
        ]
        content_hash = hashlib.sha256(
            '|'.join(content_parts).encode()
        ).hexdigest()
        
        # Check cache
        cache_key = f"{cache_type}:{symbol.id}"
        cached = await self.backend.get_llm_cache(cache_key, content_hash)
        
        return cached is None
    
    async def get_symbols_needing_llm_generation(
        self,
        symbols: List[Symbol],
        cache_type: str
    ) -> List[Symbol]:
        """Filter symbols that need LLM content generation
        
        Args:
            symbols: List of symbols to check
            cache_type: Type of cache ('wiki' or 'queries')
            
        Returns:
            List of symbols needing generation
        """
        needs_generation = []
        
        for symbol in symbols:
            if await self.should_regenerate_llm_content(symbol, cache_type):
                needs_generation.append(symbol)
        
        logger.info(
            f"LLM generation needed for {len(needs_generation)}/{len(symbols)} symbols "
            f"(cache type: {cache_type})"
        )
        
        return needs_generation
    
    async def save_llm_result(
        self,
        symbol: Symbol,
        cache_type: str,
        content: str,
        model: str,
        metadata: Dict = None
    ) -> None:
        """Save LLM generation result to cache
        
        Args:
            symbol: Symbol that was processed
            cache_type: Type of cache ('wiki' or 'queries')
            content: Generated content
            model: Model used for generation
            metadata: Optional metadata
        """
        # Compute content hash
        content_parts = [
            symbol.signature or "",
            symbol.docstring or "",
            symbol.name,
            symbol.type.value
        ]
        content_hash = hashlib.sha256(
            '|'.join(content_parts).encode()
        ).hexdigest()
        
        cache_key = f"{cache_type}:{symbol.id}"
        
        await self.backend.save_llm_cache(
            cache_key=cache_key,
            symbol_id=symbol.id,
            content_hash=content_hash,
            cache_type=cache_type,
            content=content,
            model=model,
            metadata=metadata
        )
    
    async def get_incremental_stats(self, repo_id: str) -> Dict[str, any]:
        """Get statistics about incremental indexing
        
        Args:
            repo_id: Repository identifier
            
        Returns:
            Dict with statistics
        """
        # Get repository stats
        repo_stats = await self.backend.get_repository_stats(repo_id)
        
        # Get cache stats
        cache_stats = self.backend.conn.execute("""
            SELECT 
                cache_type,
                COUNT(*) as entries,
                SUM(access_count) as total_accesses,
                AVG(access_count) as avg_accesses
            FROM llm_cache
            WHERE symbol_id IN (
                SELECT id FROM symbols WHERE repo_id = ?
            )
            GROUP BY cache_type
        """, (repo_id,)).fetchall()
        
        cache_info = {
            row[0]: {
                'entries': row[1],
                'total_accesses': row[2],
                'avg_accesses': row[3]
            }
            for row in cache_stats
        }
        
        # Get file metadata stats
        file_stats = self.backend.conn.execute("""
            SELECT 
                COUNT(*) as total_files,
                SUM(CASE WHEN needs_reindex THEN 1 ELSE 0 END) as needs_reindex,
                AVG(symbol_count) as avg_symbols_per_file
            FROM file_metadata
            WHERE repo_id = ?
        """, (repo_id,)).fetchone()
        
        return {
            'repository': repo_stats,
            'cache': cache_info,
            'files': {
                'total': file_stats[0] if file_stats else 0,
                'needs_reindex': file_stats[1] if file_stats else 0,
                'avg_symbols_per_file': file_stats[2] if file_stats else 0
            }
        }


async def smart_reindex(
    repo_id: str,
    repo_path: str,
    backend: DuckDBBackend,
    indexer_func,
    force: bool = False
) -> Dict[str, any]:
    """Smart re-indexing with change detection
    
    Args:
        repo_id: Repository identifier
        repo_path: Path to repository
        backend: DuckDB backend instance
        indexer_func: Function to index a file
        force: Force full re-index
        
    Returns:
        Dict with indexing statistics
    """
    incremental = IncrementalIndexer(backend)
    
    if force:
        logger.info("Force re-index: processing all files")
        # Mark all files for reindex
        all_files = []
        for file_path in Path(repo_path).rglob('*.py'):
            if incremental._should_index_file(file_path):
                rel_path = str(file_path.relative_to(repo_path))
                all_files.append(rel_path)
        
        await backend.mark_files_for_reindex(all_files)
        changed_files = {f: 'modified' for f in all_files}
    else:
        # Detect changes
        changed_files = await incremental.detect_changes(repo_id, repo_path)
    
    if not changed_files:
        logger.info("No changes detected, skipping re-index")
        return {
            'changed': False,
            'files_processed': 0,
            'symbols_added': 0
        }
    
    # Process changes
    added, modified, deleted = await incremental.process_changes(
        repo_id=repo_id,
        repo_path=repo_path,
        changed_files=changed_files,
        indexer_func=indexer_func
    )
    
    return {
        'changed': True,
        'files_added': added,
        'files_modified': modified,
        'files_deleted': deleted,
        'total_files_processed': added + modified + deleted
    }
