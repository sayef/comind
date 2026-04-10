"""
Code chunker with overlapping context

Splits large files into smaller chunks with overlap to prevent
context loss at chunk boundaries, improving search precision.
"""

from comind.logging_config import get_logger

logger = get_logger(__name__)

# Chunking constants
DEFAULT_CHUNK_SIZE = 512  # tokens per chunk
DEFAULT_OVERLAP = 128  # overlapping tokens
CHARS_PER_TOKEN = 4  # Rough estimate: 1 token ≈ 4 characters


class CodeChunker:
    """Split code into overlapping chunks for better search"""

    def __init__(
        self,
        chunk_size: int = DEFAULT_CHUNK_SIZE,
        overlap: int = DEFAULT_OVERLAP,
        chars_per_token: int = CHARS_PER_TOKEN,
    ):
        """
        Initialize chunker

        Args:
            chunk_size: Target size in tokens per chunk
            overlap: Number of overlapping tokens between chunks
            chars_per_token: Rough character-to-token ratio
        """
        self.chunk_size = chunk_size
        self.overlap = overlap
        self.chars_per_token = chars_per_token

        # Convert to characters
        self.chunk_chars = chunk_size * chars_per_token
        self.overlap_chars = overlap * chars_per_token

    def should_chunk(self, content: str) -> bool:
        """
        Check if content should be chunked

        Args:
            content: Source code content

        Returns:
            True if content is large enough to benefit from chunking
        """
        estimated_tokens = len(content) / self.chars_per_token
        return estimated_tokens > self.chunk_size * 1.5

    def chunk_by_lines(self, content: str, file_path: str = "") -> list[tuple[str, int, int]]:
        """
        Chunk content by lines with overlap

        Preserves line boundaries for better code structure.

        Args:
            content: Source code content
            file_path: Optional file path for logging

        Returns:
            List of (chunk_content, start_line, end_line) tuples
        """
        if not self.should_chunk(content):
            # Content is small enough, return as single chunk
            line_count = content.count("\n") + 1
            return [(content, 1, line_count)]

        lines = content.split("\n")
        chunks = []

        # Calculate lines per chunk
        lines_per_chunk = max(1, self.chunk_chars // 80)  # Assume ~80 chars per line
        overlap_lines = max(1, self.overlap_chars // 80)

        start_line = 0
        while start_line < len(lines):
            # Calculate chunk boundaries
            end_line = min(start_line + lines_per_chunk, len(lines))

            # Extract chunk
            chunk_lines = lines[start_line:end_line]
            chunk_content = "\n".join(chunk_lines)

            # Add chunk with 1-indexed line numbers
            chunks.append((chunk_content, start_line + 1, end_line))

            # Move to next chunk with overlap
            start_line = end_line - overlap_lines

            # Avoid infinite loop on small overlaps
            if start_line >= end_line:
                break

        logger.debug(
            "Chunked file",
            file=file_path,
            total_lines=len(lines),
            chunks=len(chunks),
            chunk_size=lines_per_chunk,
            overlap=overlap_lines,
        )

        return chunks

    def chunk_by_functions(
        self, content: str, symbols: list[dict]
    ) -> list[tuple[str, int, int, str]]:
        """
        Chunk content by function/class boundaries with context

        More intelligent chunking that respects code structure.

        Args:
            content: Source code content
            symbols: List of symbols (functions, classes) with line ranges

        Returns:
            List of (chunk_content, start_line, end_line, symbol_id) tuples
        """
        if not symbols:
            # No symbols, fall back to line-based chunking
            return [(chunk, start, end, "") for chunk, start, end in self.chunk_by_lines(content)]

        lines = content.split("\n")
        chunks = []

        # Sort symbols by line start
        sorted_symbols = sorted(symbols, key=lambda s: s.get("line_start", 0))

        for symbol in sorted_symbols:
            line_start = symbol.get("line_start", 1)
            line_end = symbol.get("line_end", len(lines))
            symbol_id = symbol.get("id", "")

            # Add context lines before and after
            context_lines = self.overlap_chars // 80
            chunk_start = max(1, line_start - context_lines)
            chunk_end = min(len(lines), line_end + context_lines)

            # Extract chunk (convert to 0-indexed for slicing)
            chunk_lines = lines[chunk_start - 1 : chunk_end]
            chunk_content = "\n".join(chunk_lines)

            chunks.append((chunk_content, chunk_start, chunk_end, symbol_id))

        logger.debug("Chunked by functions", symbols=len(symbols), chunks=len(chunks))

        return chunks

    def estimate_tokens(self, content: str) -> int:
        """
        Estimate token count for content

        Args:
            content: Text content

        Returns:
            Estimated token count
        """
        return len(content) // self.chars_per_token


def chunk_file(
    file_path: str,
    content: str,
    symbols: list[dict] | None = None,
    chunk_size: int = DEFAULT_CHUNK_SIZE,
    overlap: int = DEFAULT_OVERLAP,
) -> list[tuple[str, int, int, str]]:
    """
    Convenience function to chunk a file

    Args:
        file_path: Path to the file
        content: File content
        symbols: Optional list of symbols for structure-aware chunking
        chunk_size: Chunk size in tokens
        overlap: Overlap size in tokens

    Returns:
        List of (chunk_content, start_line, end_line, symbol_id) tuples
    """
    chunker = CodeChunker(chunk_size=chunk_size, overlap=overlap)

    if symbols:
        return chunker.chunk_by_functions(content, symbols)
    # Convert line-based chunks to include empty symbol_id
    return [
        (chunk, start, end, "") for chunk, start, end in chunker.chunk_by_lines(content, file_path)
    ]
