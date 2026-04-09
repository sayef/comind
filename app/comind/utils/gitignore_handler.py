"""
Gitignore handler for proper .gitignore and .comindignore support

Uses pathspec library to parse and match gitignore patterns,
supporting nested .gitignore files and custom ignore patterns.
"""

from pathlib import Path
from typing import List, Set
import pathspec

from comind.logging_config import get_logger

logger = get_logger(__name__)


class GitignoreHandler:
    """Handle .gitignore and .comindignore pattern matching"""
    
    def __init__(self, repo_root: Path):
        """
        Initialize gitignore handler
        
        Args:
            repo_root: Root directory of the repository
        """
        self.repo_root = repo_root
        self.patterns: List[pathspec.PathSpec] = []
        self.ignore_files: List[Path] = []
        
        # Load all gitignore files
        self._load_gitignore_files()
    
    def _load_gitignore_files(self):
        """Load all .gitignore and .comindignore files in the repository"""
        # Find all .gitignore and .comindignore files
        gitignore_files = list(self.repo_root.rglob(".gitignore"))
        comindignore_files = list(self.repo_root.rglob(".comindignore"))
        
        all_ignore_files = gitignore_files + comindignore_files
        
        logger.debug(
            "Loading ignore files",
            gitignore_count=len(gitignore_files),
            comindignore_count=len(comindignore_files)
        )
        
        # Load patterns from each file
        for ignore_file in all_ignore_files:
            try:
                with open(ignore_file, 'r', encoding='utf-8') as f:
                    patterns = f.read().splitlines()
                
                # Filter out comments and empty lines
                patterns = [
                    p.strip() for p in patterns 
                    if p.strip() and not p.strip().startswith('#')
                ]
                
                if patterns:
                    # Create PathSpec for this file
                    spec = pathspec.PathSpec.from_lines('gitwildmatch', patterns)
                    self.patterns.append(spec)
                    self.ignore_files.append(ignore_file)
                    
                    logger.debug(
                        "Loaded ignore file",
                        file=str(ignore_file.relative_to(self.repo_root)),
                        pattern_count=len(patterns)
                    )
            except Exception as e:
                logger.warning(
                    "Failed to load ignore file",
                    file=str(ignore_file),
                    error=str(e)
                )
    
    def should_ignore(self, file_path: Path) -> bool:
        """
        Check if a file should be ignored based on gitignore patterns
        
        Args:
            file_path: Absolute path to the file
            
        Returns:
            True if the file should be ignored
        """
        try:
            # Get relative path from repo root
            rel_path = file_path.relative_to(self.repo_root)
            rel_path_str = str(rel_path)
            
            # Check against all loaded patterns
            for spec in self.patterns:
                if spec.match_file(rel_path_str):
                    return True
            
            return False
        except ValueError:
            # File is not under repo root
            return False
    
    def should_ignore_directory(self, dir_path: Path) -> bool:
        """
        Check if a directory should be ignored
        
        Args:
            dir_path: Absolute path to the directory
            
        Returns:
            True if the directory should be ignored
        """
        try:
            # Get relative path from repo root
            rel_path = dir_path.relative_to(self.repo_root)
            rel_path_str = str(rel_path) + "/"  # Add trailing slash for directory matching
            
            # Check against all loaded patterns
            for spec in self.patterns:
                if spec.match_file(rel_path_str):
                    return True
            
            return False
        except ValueError:
            # Directory is not under repo root
            return False
    
    def get_default_patterns(self) -> List[str]:
        """Get default ignore patterns for Python projects"""
        return [
            # Python
            "__pycache__/",
            "*.py[cod]",
            "*$py.class",
            "*.so",
            ".Python",
            "build/",
            "develop-eggs/",
            "dist/",
            "downloads/",
            "eggs/",
            ".eggs/",
            "lib/",
            "lib64/",
            "parts/",
            "sdist/",
            "var/",
            "wheels/",
            "*.egg-info/",
            ".installed.cfg",
            "*.egg",
            
            # Virtual environments
            "venv/",
            "ENV/",
            "env/",
            ".venv/",
            
            # IDEs
            ".vscode/",
            ".idea/",
            "*.swp",
            "*.swo",
            "*~",
            
            # OS
            ".DS_Store",
            "Thumbs.db",
            
            # Git
            ".git/",
            ".gitignore",
            
            # Node
            "node_modules/",
            
            # Build artifacts
            "*.min.js",
            "*.min.css",
            "*.bundle.js",
            "*.bundle.css",
        ]


def create_default_comindignore(repo_root: Path):
    """
    Create a default .comindignore file if it doesn't exist
    
    Args:
        repo_root: Root directory of the repository
    """
    comindignore_path = repo_root / ".comindignore"
    
    if comindignore_path.exists():
        logger.debug(".comindignore already exists")
        return
    
    handler = GitignoreHandler(repo_root)
    default_patterns = handler.get_default_patterns()
    
    try:
        with open(comindignore_path, 'w', encoding='utf-8') as f:
            f.write("# CoMind ignore patterns\n")
            f.write("# This file uses gitignore syntax\n\n")
            for pattern in default_patterns:
                f.write(f"{pattern}\n")
        
        logger.info("Created default .comindignore", path=str(comindignore_path))
    except Exception as e:
        logger.warning("Failed to create .comindignore", error=str(e))
