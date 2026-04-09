"""
Python code indexer using py-tree-sitter

Parses Python source files and extracts symbols, relationships,
and structural information for the knowledge graph.
"""

import asyncio
from pathlib import Path
from typing import Any, Dict, List, Optional, Set, Tuple
import hashlib
import re

from tree_sitter import Language, Parser, Node
from tree_sitter_python import language

from comind.logging_config import get_logger

from comind.core.graph import Symbol, Relationship, SymbolType, RelationType
from comind.storage.graph_adapter import KnowledgeGraph
from comind.utils.gitignore_handler import GitignoreHandler

logger = get_logger(__name__)

# File filtering constants
MAX_FILE_SIZE = 1 * 1024 * 1024  # 1 MB
MINIFIED_PATTERNS = [
    ".min.js",
    ".min.css",
    ".bundle.js",
    ".bundle.css",
    "-min.js",
    "-min.css",
]


def is_binary_file(file_path: Path) -> bool:
    """
    Check if a file is binary by reading a sample and checking for null bytes
    and UTF-8 validity
    """
    try:
        with open(file_path, 'rb') as f:
            chunk = f.read(8192)  # Read first 8KB
            
            # Check for null bytes (strong indicator of binary)
            if b'\x00' in chunk:
                return True
            
            # Try to decode as UTF-8
            try:
                chunk.decode('utf-8')
                return False
            except UnicodeDecodeError:
                return True
    except Exception:
        return True


def is_minified_file(file_path: Path) -> bool:
    """Check if a file is minified based on naming patterns"""
    name_lower = file_path.name.lower()
    return any(pattern in name_lower for pattern in MINIFIED_PATTERNS)


def should_skip_file(file_path: Path) -> tuple[bool, str | None]:
    """
    Check if a file should be skipped during indexing
    
    Returns:
        (should_skip, reason) tuple
    """
    # Check file size
    try:
        size = file_path.stat().st_size
        if size > MAX_FILE_SIZE:
            return True, f"too large ({size / 1024 / 1024:.1f}MB)"
    except Exception:
        return True, "cannot access"
    
    # Check if minified
    if is_minified_file(file_path):
        return True, "minified"
    
    # Check if binary
    if is_binary_file(file_path):
        return True, "binary"
    
    return False, None


class PythonSymbolExtractor:
    """Extract symbols from Python code using tree-sitter"""

    def __init__(self, file_path: str, source_code: str):
        self.file_path = file_path
        self.source_code = source_code
        self.lines = source_code.split('\n')
        self.symbols: List[Symbol] = []
        self.relationships: List[Relationship] = []
        self.imports: List[Tuple[str, str, int]] = []  # (module, alias, line)
        # Unresolved calls: (source_symbol_id, bare_call_name) — resolved after all files indexed
        self.unresolved_calls: List[Tuple[str, str]] = []

        # Initialize tree-sitter parser
        self.parser = Parser(Language(language()))
        self.python_language = Language(language())

    def extract(self) -> Tuple[List[Symbol], List[Relationship], List[Tuple[str, str]]]:
        """Extract symbols, relationships, and unresolved call pairs from Python code.

        Returns:
            (symbols, relationships, unresolved_calls)
            unresolved_calls: list of (source_symbol_id, callee_bare_name)
        """
        try:
            tree = self.parser.parse(bytes(self.source_code, "utf8"))
            self._extract_from_node(tree.root_node)

            # Process imports after all symbols are extracted
            self._process_imports()

            logger.debug("Symbol extraction complete", file=self.file_path, symbols=len(self.symbols), relationships=len(self.relationships))

            return self.symbols, self.relationships, self.unresolved_calls
        except Exception as e:
            logger.error("Symbol extraction failed", file=self.file_path, error=str(e))
            return [], [], []
    
    def _get_signature_from_source(self, node: Node, include_decorators: bool = True) -> str:
        """
        Extract original signature text from source code.
        Language-agnostic: works for any language by extracting the actual source text.
        
        Args:
            node: Tree-sitter node (function_definition, class_definition, etc.)
            include_decorators: Whether to include decorators in the signature
        
        Returns:
            Original signature text from source
        """
        start_line = node.start_point[0]
        end_line = node.end_point[0]
        
        # For functions/classes, we want just the definition line(s), not the body
        # Find the colon that ends the signature
        signature_lines = []
        for line_num in range(start_line, min(start_line + 10, len(self.lines))):  # Limit to 10 lines
            line = self.lines[line_num]
            signature_lines.append(line)
            # Stop at the colon (end of signature)
            if ':' in line:
                break
        
        signature = '\n'.join(signature_lines).strip()
        
        # If decorators should be excluded, remove them
        if not include_decorators and signature.startswith('@'):
            # Find the actual def/class line
            lines = signature.split('\n')
            for i, line in enumerate(lines):
                if line.strip().startswith(('def ', 'class ', 'async def ')):
                    signature = '\n'.join(lines[i:])
                    break
        
        return signature
    
    def _extract_from_node(self, node: Node, parent_symbol: Optional[Symbol] = None):
        """Recursively extract symbols from tree-sitter nodes"""
        
        if node.type == "module":
            # Create module symbol
            module_name = Path(self.file_path).stem
            module_symbol = Symbol(
                id=self._generate_symbol_id(module_name, "module"),
                name=module_name,
                type=SymbolType.MODULE,
                file_path=self.file_path,
                line_start=1,
                line_end=len(self.lines),
                signature=f"module {module_name}"
            )
            self.symbols.append(module_symbol)
            
            # Process all children
            for child in node.children:
                self._extract_from_node(child, module_symbol)
        
        elif node.type == "class_definition":
            class_symbol = self._extract_class(node, parent_symbol)
            if class_symbol:
                self.symbols.append(class_symbol)
                
                # Add contains relationship if has parent
                if parent_symbol:
                    self.relationships.append(Relationship(
                        source_id=parent_symbol.id,
                        target_id=class_symbol.id,
                        type=RelationType.CONTAINS
                    ))
                
                # Process class body
                body_node = node.child_by_field_name("body")
                if body_node:
                    for child in body_node.children:
                        self._extract_from_node(child, class_symbol)
        
        elif node.type == "function_definition":
            func_symbol = self._extract_function(node, parent_symbol, is_async=False)
            if func_symbol:
                self.symbols.append(func_symbol)
                
                # Add contains relationship if has parent
                if parent_symbol:
                    self.relationships.append(Relationship(
                        source_id=parent_symbol.id,
                        target_id=func_symbol.id,
                        type=RelationType.CONTAINS
                    ))
                
                # Process function body for nested definitions
                body_node = node.child_by_field_name("body")
                if body_node:
                    for child in body_node.children:
                        if child.type in ["function_definition", "class_definition"]:
                            self._extract_from_node(child, func_symbol)
        
        elif node.type == "decorated_definition":
            # Extract decorators and then process the definition
            decorators = self._extract_decorators(node)
            definition_node = node.child_by_field_name("definition")
            if definition_node:
                if definition_node.type == "function_definition":
                    func_symbol = self._extract_function(definition_node, parent_symbol, is_async=False, decorators=decorators)
                    if func_symbol:
                        self.symbols.append(func_symbol)
                        if parent_symbol:
                            self.relationships.append(Relationship(
                                source_id=parent_symbol.id,
                                target_id=func_symbol.id,
                                type=RelationType.CONTAINS
                            ))
                elif definition_node.type == "class_definition":
                    class_symbol = self._extract_class(definition_node, parent_symbol, decorators=decorators)
                    if class_symbol:
                        self.symbols.append(class_symbol)
                        if parent_symbol:
                            self.relationships.append(Relationship(
                                source_id=parent_symbol.id,
                                target_id=class_symbol.id,
                                type=RelationType.CONTAINS
                            ))
                        # Process class body
                        body_node = definition_node.child_by_field_name("body")
                        if body_node:
                            for child in body_node.children:
                                self._extract_from_node(child, class_symbol)
        
        # Handle imports
        elif node.type == "import_statement":
            self._extract_import(node)
        elif node.type == "import_from_statement":
            self._extract_import_from(node)
        
        # Recursively process other nodes
        else:
            for child in node.children:
                self._extract_from_node(child, parent_symbol)
    
    def _extract_class(self, node: Node, parent: Optional[Symbol] = None, decorators: List[str] = None) -> Symbol:
        """Extract class symbol from tree-sitter node"""
        if decorators is None:
            decorators = []
            
        # Get class name
        name_node = node.child_by_field_name("name")
        class_name = name_node.text.decode('utf8') if name_node else "UnknownClass"
        
        # Get base classes (for metadata)
        bases = []
        superclasses_node = node.child_by_field_name("superclasses")
        if superclasses_node:
            for child in superclasses_node.children:
                if child.type == "identifier":
                    bases.append(child.text.decode('utf8'))
                elif child.type == "attribute":
                    bases.append(child.text.decode('utf8'))
        
        # Extract original signature from source (language-agnostic)
        signature = self._get_signature_from_source(node, include_decorators=True)
        
        # Get docstring
        docstring = self._get_docstring(node)
        
        symbol = Symbol(
            id=self._generate_symbol_id(class_name, "class"),
            name=class_name,
            type=SymbolType.CLASS,
            file_path=self.file_path,
            line_start=node.start_point[0] + 1,
            line_end=node.end_point[0] + 1,
            signature=signature,
            docstring=docstring,
            properties={
                "bases": bases,
                "decorators": decorators,
                "parent_id": parent.id if parent else None
            }
        )
        
        # Add inheritance relationships
        for base_name in bases:
            self.relationships.append(Relationship(
                source_id=symbol.id,
                target_id=self._generate_symbol_id(base_name, "class"),
                type=RelationType.INHERITS,
                confidence=0.8
            ))
        
        return symbol
    
    def _get_docstring(self, node: Node) -> Optional[str]:
        """Extract docstring from a tree-sitter node"""
        body_node = node.child_by_field_name("body")
        if body_node and body_node.child_count > 0:
            first_stmt = body_node.children[0]
            if first_stmt.type == "expression_statement":
                expr = first_stmt.children[0] if first_stmt.child_count > 0 else None
                if expr and expr.type == "string":
                    # Remove quotes from docstring
                    text = expr.text.decode('utf8')
                    if text.startswith('"""') or text.startswith("'''"):
                        return text[3:-3].strip()
                    elif text.startswith('"') or text.startswith("'"):
                        return text[1:-1].strip()
        return None
    
    def _extract_function(self, node: Node, parent: Optional[Symbol] = None, is_async: bool = False, decorators: List[str] = None) -> Symbol:
        """Extract function/method symbol from tree-sitter node"""
        if decorators is None:
            decorators = []
            
        # Get function name
        name_node = node.child_by_field_name("name")
        func_name = name_node.text.decode('utf8') if name_node else "unknown_function"
        
        # Extract original signature from source (language-agnostic, always accurate)
        signature = self._get_signature_from_source(node, include_decorators=True)
        
        # Parse parameters for metadata (structured data for queries)
        params = []
        param_types = {}
        params_node = node.child_by_field_name("parameters")
        if params_node:
            for child in params_node.children:
                if child.type == "identifier":
                    # Simple parameter: def foo(x)
                    param_name = child.text.decode('utf8')
                    params.append(param_name)
                elif child.type == "typed_parameter":
                    # Typed parameter: def foo(x: int)
                    param_name_node = child.child_by_field_name("name")
                    param_type_node = child.child_by_field_name("type")
                    if param_name_node:
                        param_name = param_name_node.text.decode('utf8')
                        params.append(param_name)
                        if param_type_node:
                            param_types[param_name] = param_type_node.text.decode('utf8')
                elif child.type == "default_parameter":
                    # Default parameter: def foo(x=10)
                    param_name_node = child.child_by_field_name("name")
                    if param_name_node:
                        param_name = param_name_node.text.decode('utf8')
                        params.append(param_name)
                elif child.type == "typed_default_parameter":
                    # Typed default parameter: def foo(x: int = 10)
                    param_name_node = child.child_by_field_name("name")
                    param_type_node = child.child_by_field_name("type")
                    if param_name_node:
                        param_name = param_name_node.text.decode('utf8')
                        params.append(param_name)
                        if param_type_node:
                            param_types[param_name] = param_type_node.text.decode('utf8')
                elif child.type in ("list_splat_pattern", "dictionary_splat_pattern"):
                    # *args or **kwargs
                    param_name_node = child.child_by_field_name("name") or (child.children[0] if child.child_count > 0 else None)
                    if param_name_node:
                        param_name = param_name_node.text.decode('utf8')
                        params.append(param_name)
        
        # Get return type for metadata
        return_type = None
        return_type_node = node.child_by_field_name("return_type")
        if return_type_node:
            return_type = return_type_node.text.decode('utf8')
        
        # Determine symbol type
        symbol_type = SymbolType.METHOD if parent and parent.type == SymbolType.CLASS else SymbolType.FUNCTION
        
        # Get docstring
        docstring = self._get_docstring(node)
        
        # Extract conditional branches (if/else, try/except)
        conditional_branches = self._extract_conditional_branches(node)
        
        symbol = Symbol(
            id=self._generate_symbol_id(func_name, symbol_type.value),
            name=func_name,
            type=symbol_type,
            file_path=self.file_path,
            line_start=node.start_point[0] + 1,
            line_end=node.end_point[0] + 1,
            signature=signature,
            docstring=docstring,
            properties={
                "args": params,
                "param_types": param_types,
                "returns": return_type,
                "decorators": decorators,
                "parent_id": parent.id if parent else None,
                "is_async": is_async,
                "conditional_branches": conditional_branches
            }
        )
        
        # Extract function calls within the function
        self._extract_function_calls(symbol, node)
        
        return symbol
    
    def _extract_conditional_branches(self, func_node: Node) -> List[Dict[str, Any]]:
        """Extract conditional branches from a function using ConditionalFlowExtractor"""
        extractor = ConditionalFlowExtractor(self.source_code, self.lines)
        return extractor.extract_conditional_flows(func_node)
    
    def _extract_decorators(self, node: Node) -> List[str]:
        """Extract decorators from a decorated_definition node"""
        decorators = []
        for child in node.children:
            if child.type == "decorator":
                # Get the decorator name (skip the @ symbol)
                decorator_text = child.text.decode('utf8')
                if decorator_text.startswith('@'):
                    decorator_text = decorator_text[1:].strip()
                decorators.append(decorator_text)
        return decorators
    
    def _extract_import(self, node: Node) -> None:
        """Extract import statements from tree-sitter node"""
        # import foo, bar, baz
        for child in node.children:
            if child.type == "dotted_name" or child.type == "identifier":
                module_name = child.text.decode('utf8')
                self.imports.append((module_name, module_name, node.start_point[0] + 1))
    
    def _extract_import_from(self, node: Node) -> None:
        """Extract from-import statements from tree-sitter node"""
        # from foo import bar
        module_name = ""
        module_node = node.child_by_field_name("module_name")
        if module_node:
            module_name = module_node.text.decode('utf8')
        
        # Get imported names
        for child in node.children:
            if child.type == "dotted_name" or child.type == "identifier":
                if child != module_node:  # Skip the module name itself
                    imported_name = child.text.decode('utf8')
                    full_name = f"{module_name}.{imported_name}" if module_name else imported_name
                    self.imports.append((full_name, imported_name, node.start_point[0] + 1))
    
    def _extract_function_calls(self, func_symbol: Symbol, node: Node):
        """Extract function calls within a function using tree-sitter"""
        def bare_name(func_node) -> str | None:
            """Return just the function name, stripping attribute access (self.foo → foo)."""
            text = func_node.text.decode("utf8")
            # attribute access: self.method, cls.method, obj.method
            if func_node.type == "attribute":
                attr = func_node.child_by_field_name("attribute")
                if attr:
                    return attr.text.decode("utf8")
            # plain identifier
            if "." not in text:
                return text
            # dotted (e.g. module.func) — take the last segment
            return text.rsplit(".", 1)[-1]

        def find_calls(n: Node):
            if n.type == "call":
                func_node = n.child_by_field_name("function")
                if func_node:
                    name = bare_name(func_node)
                    call_line = n.start_point[0] + 1  # 1-based
                    # Capture the full call expression text as the call site snippet
                    call_text = n.text.decode("utf8").split("\n")[0].strip()  # first line only
                    if name and name != func_symbol.name:
                        self.unresolved_calls.append((func_symbol.id, name, call_line, call_text))
            for child in n.children:
                find_calls(child)

        find_calls(node)
    
    def _process_imports(self):
        """Process extracted imports and create relationships"""
        for module_name, alias_name, line in self.imports:
            # Create import symbol
            import_symbol = Symbol(
                id=self._generate_symbol_id(alias_name, "import"),
                name=alias_name,
                type=SymbolType.IMPORT,
                file_path=self.file_path,
                line_start=line,
                line_end=line,
                signature=f"import {module_name}",
                properties={"module": module_name}
            )
            self.symbols.append(import_symbol)
            
            # Add import relationship to module
            module_id = self._generate_symbol_id(Path(self.file_path).stem, "module")
            self.relationships.append(Relationship(
                source_id=module_id,
                target_id=import_symbol.id,
                type=RelationType.IMPORTS
            ))
    
    def _generate_symbol_id(self, name: str, symbol_type: str) -> str:
        """Generate a unique symbol ID"""
        file_hash = hashlib.md5(self.file_path.encode()).hexdigest()[:8]
        return f"{file_hash}_{symbol_type}_{name}"


class ConditionalFlowExtractor:
    """Extract conditional execution branches using tree-sitter"""
    
    def __init__(self, source_code: str, lines: List[str]):
        self.source_code = source_code
        self.lines = lines
    
    def extract_conditional_flows(self, func_node: Node) -> List[Dict[str, Any]]:
        """
        Extract all conditional branches (if/else, try/except) from a function.
        
        Returns list of:
        {
            "condition": "if user_exists",
            "branch_type": "if_true" | "if_false" | "try" | "except" | "finally",
            "calls": ["function_name", ...],
            "line_start": 10,
            "line_end": 15
        }
        """
        branches = []
        
        body_node = func_node.child_by_field_name("body")
        if not body_node:
            return branches
        
        # Walk the function body
        self._extract_branches_recursive(body_node, branches)
        
        return branches
    
    def _extract_branches_recursive(self, node: Node, branches: List[Dict[str, Any]]):
        """Recursively extract conditional branches"""
        
        if node.type == "if_statement":
            # Extract condition
            condition_node = node.child_by_field_name("condition")
            condition_text = self._get_node_text(condition_node) if condition_node else "unknown"
            
            # Extract "if" branch
            consequence_node = node.child_by_field_name("consequence")
            if consequence_node:
                if_calls = self._extract_calls_from_block(consequence_node)
                branches.append({
                    "condition": condition_text,
                    "branch_type": "if_true",
                    "calls": if_calls,
                    "line_start": consequence_node.start_point[0] + 1,
                    "line_end": consequence_node.end_point[0] + 1
                })
            
            # Extract "else" or "elif" branches
            alternative_node = node.child_by_field_name("alternative")
            if alternative_node:
                if alternative_node.type == "else_clause":
                    # Pure else
                    body = alternative_node.child_by_field_name("body")
                    if body:
                        else_calls = self._extract_calls_from_block(body)
                        branches.append({
                            "condition": f"not ({condition_text})",
                            "branch_type": "if_false",
                            "calls": else_calls,
                            "line_start": body.start_point[0] + 1,
                            "line_end": body.end_point[0] + 1
                        })
                elif alternative_node.type == "elif_clause":
                    # Recursively handle elif as another if_statement
                    self._extract_branches_recursive(alternative_node, branches)
        
        elif node.type == "try_statement":
            # Extract try block
            body_node = node.child_by_field_name("body")
            if body_node:
                try_calls = self._extract_calls_from_block(body_node)
                branches.append({
                    "condition": "normal_execution",
                    "branch_type": "try",
                    "calls": try_calls,
                    "line_start": body_node.start_point[0] + 1,
                    "line_end": body_node.end_point[0] + 1
                })
            
            # Extract except handlers
            for child in node.children:
                if child.type == "except_clause":
                    # Get exception type
                    exc_type = "Exception"
                    for exc_child in child.children:
                        if exc_child.type in ["identifier", "attribute", "dotted_name"]:
                            exc_type = self._get_node_text(exc_child)
                            break
                    
                    # Get handler body
                    handler_body = child.child_by_field_name("body")
                    if not handler_body:
                        # Fallback: find block node
                        for exc_child in child.children:
                            if exc_child.type == "block":
                                handler_body = exc_child
                                break
                    
                    if handler_body:
                        except_calls = self._extract_calls_from_block(handler_body)
                        branches.append({
                            "condition": f"raises {exc_type}",
                            "branch_type": "except",
                            "exception_type": exc_type,
                            "calls": except_calls,
                            "line_start": handler_body.start_point[0] + 1,
                            "line_end": handler_body.end_point[0] + 1
                        })
            
            # Extract finally block
            for child in node.children:
                if child.type == "finally_clause":
                    finally_body = child.child_by_field_name("body")
                    if finally_body:
                        finally_calls = self._extract_calls_from_block(finally_body)
                        branches.append({
                            "condition": "always",
                            "branch_type": "finally",
                            "calls": finally_calls,
                            "line_start": finally_body.start_point[0] + 1,
                            "line_end": finally_body.end_point[0] + 1
                        })
        
        # Recurse into children for nested conditions
        for child in node.children:
            if child.type in ["if_statement", "try_statement"]:
                self._extract_branches_recursive(child, branches)
    
    def _extract_calls_from_block(self, block_node: Node) -> List[str]:
        """Extract function call names from a code block"""
        calls = []
        
        def find_calls(n: Node):
            if n.type == "call":
                func_node = n.child_by_field_name("function")
                if func_node:
                    call_name = self._get_bare_name(func_node)
                    if call_name and call_name not in calls:
                        calls.append(call_name)
            
            for child in n.children:
                find_calls(child)
        
        find_calls(block_node)
        return calls
    
    def _get_bare_name(self, func_node: Node) -> str | None:
        """Extract bare function name (strips self.method → method)"""
        text = func_node.text.decode("utf8")
        
        if func_node.type == "attribute":
            attr = func_node.child_by_field_name("attribute")
            if attr:
                return attr.text.decode("utf8")
        
        if "." not in text:
            return text
        
        return text.rsplit(".", 1)[-1]
    
    def _get_node_text(self, node: Node) -> str:
        """Get text content of a node"""
        if not node:
            return ""
        return node.text.decode("utf8")


class PythonIndexer:
    """Indexes Python code into the knowledge graph"""
    
    def __init__(self, graph: KnowledgeGraph, query_engine=None):
        self.graph = graph
        self.query_engine = query_engine  # Reference to query engine for building indexes
        self.current_repo_id: Optional[str] = None  # Track current repository being indexed
        self.repo_root: Optional[Path] = None  # Root directory of the repository being indexed
        self.gitignore_handler: Optional[GitignoreHandler] = None  # Gitignore pattern matcher
        
    async def index_file(
        self,
        file_path: str,
        collect_unresolved: list | None = None,
    ) -> Dict[str, Any]:
        """Index a single Python file.

        Args:
            collect_unresolved: If provided, unresolved (source_id, callee_name) pairs
                are appended to this list for later bulk resolution.
        """
        path = Path(file_path)

        if not path.exists() or path.suffix != '.py':
            return {"error": "File not found or not a Python file"}

        with open(path, 'r', encoding='utf-8') as f:
            source_code = f.read()

        # Convert to relative path if we have a repo root
        relative_path = file_path
        if self.repo_root:
            try:
                relative_path = str(Path(file_path).relative_to(self.repo_root))
            except ValueError:
                pass

        # Extract symbols (use relative path)
        extractor = PythonSymbolExtractor(relative_path, source_code)
        symbols, relationships, unresolved_calls = extractor.extract()

        # Set repo_id on all symbols if we're indexing a repository
        if self.current_repo_id:
            for symbol in symbols:
                symbol.repo_id = self.current_repo_id

        # Add to graph
        for symbol in symbols:
            await self.graph.add_symbol(symbol)

        for relationship in relationships:
            await self.graph.add_relationship(relationship)

        # Accumulate unresolved calls for later bulk resolution
        if collect_unresolved is not None:
            collect_unresolved.extend(unresolved_calls)
        
        # Add symbols to repo-specific search index
        if self.query_engine and self.current_repo_id:
            for symbol in symbols:
                # Get file content for better search
                content = source_code if symbol.type != SymbolType.MODULE else ""
                await self.query_engine.add_symbol_to_index(
                    self.current_repo_id,
                    symbol,
                    content
                )
        
        return {
            "file": file_path,
            "symbols_extracted": len(symbols),
            "relationships_extracted": len(relationships)
        }
    
    async def index_directory(
        self,
        directory_path: str,
        recursive: bool = True,
        progress_callback: Optional[callable] = None
    ) -> Dict[str, Any]:
        """Index all Python files in a directory"""
        path = Path(directory_path)
        
        logger.debug("Starting directory indexing", path=str(path), recursive=recursive)
        
        if not path.exists() or not path.is_dir():
            logger.error("Directory not found", path=str(path))
            return {"error": "Directory not found"}
        
        # Find Python files
        if recursive:
            python_files = list(path.rglob("*.py"))
        else:
            python_files = list(path.glob("*.py"))
        
        logger.debug("Found Python files before filtering", count=len(python_files))
        
        # Filter using gitignore patterns if available
        if self.gitignore_handler:
            python_files = [
                f for f in python_files 
                if not self.gitignore_handler.should_ignore(f)
            ]
            logger.debug("Python files after gitignore filtering", count=len(python_files))
        else:
            # Fallback to basic filtering if no gitignore handler
            python_files = [
                f for f in python_files 
                if not any(part.startswith('.') for part in f.relative_to(path).parts)
                and '__pycache__' not in f.parts
            ]
            logger.debug("Python files after basic filtering", count=len(python_files))
        
        # Apply file-level filtering (size, binary, minified)
        skipped_files = []
        filtered_files = []
        for file_path in python_files:
            should_skip, reason = should_skip_file(file_path)
            if should_skip:
                skipped_files.append((str(file_path.relative_to(path)), reason))
            else:
                filtered_files.append(file_path)
        
        if skipped_files:
            logger.info("Skipped files", count=len(skipped_files), reasons={r: sum(1 for _, reason in skipped_files if reason == r) for _, r in skipped_files})
        
        logger.debug("Python files after all filtering", count=len(filtered_files))
        
        results = []
        total_symbols = 0
        total_relationships = 0
        errors = []
        all_unresolved_calls: list[tuple[str, str]] = []
        
        # Collect all symbols and relationships for batch insert
        all_symbols = []
        all_relationships = []

        logger.debug("Starting file indexing", total_files=len(filtered_files))

        # Process files in parallel batches for speed
        import asyncio
        batch_size = 10  # Process 10 files at a time
        
        async def process_file(file_path):
            """Process a single file and return results"""
            try:
                # Parse file without writing to DB yet
                path = Path(file_path)
                if not path.exists() or path.suffix != '.py':
                    return None, "File not found or not a Python file"
                
                with open(path, 'r', encoding='utf-8') as f:
                    source_code = f.read()
                
                relative_path = str(file_path)
                if self.repo_root:
                    try:
                        relative_path = str(Path(file_path).relative_to(self.repo_root))
                    except ValueError:
                        pass
                
                from comind.indexing.indexer import PythonSymbolExtractor
                extractor = PythonSymbolExtractor(relative_path, source_code)
                symbols, relationships, unresolved_calls = extractor.extract()
                
                # Set repo_id on all symbols
                if self.current_repo_id:
                    for symbol in symbols:
                        symbol.repo_id = self.current_repo_id
                
                return {
                    'symbols': symbols,
                    'relationships': relationships,
                    'unresolved_calls': unresolved_calls,
                    'file_path': relative_path
                }, None
            except Exception as e:
                return None, str(e)
        
        # Process files in parallel batches with progress updates
        total_files = len(filtered_files)
        files_processed = 0
        
        for i in range(0, len(filtered_files), batch_size):
            batch = filtered_files[i:i + batch_size]
            batch_results = await asyncio.gather(
                *[process_file(str(fp)) for fp in batch],
                return_exceptions=True
            )
            
            for result, error in batch_results:
                files_processed += 1
                
                if isinstance(result, Exception):
                    errors.append(str(result))
                elif error:
                    errors.append(error)
                elif result:
                    all_symbols.extend(result['symbols'])
                    all_relationships.extend(result['relationships'])
                    all_unresolved_calls.extend(result['unresolved_calls'])
                    total_symbols += len(result['symbols'])
                    total_relationships += len(result['relationships'])
                    results.append(result)
                
                # Update progress every 10 files or on completion
                if files_processed % 10 == 0 or files_processed == total_files:
                    progress_pct = int((files_processed / total_files) * 100)
                    
                    # Call progress callback if provided
                    if progress_callback:
                        progress_callback(
                            phase="parsing",
                            current=files_processed,
                            total=total_files,
                            symbols=total_symbols,
                            relationships=total_relationships
                        )
                    
                    logger.debug(
                        f"Parsing progress: {files_processed}/{total_files} files ({progress_pct}%) - "
                        f"{total_symbols} symbols, {total_relationships} relationships"
                    )
        
        # Bulk insert all symbols and relationships at once
        if progress_callback:
            progress_callback(phase="bulk_insert", current=0, total=1, symbols=total_symbols, relationships=total_relationships)
        
        logger.debug("Bulk inserting symbols and relationships", symbols=len(all_symbols), relationships=len(all_relationships))
        
        if all_symbols:
            await self.graph.add_symbols_batch(all_symbols)
        
        if all_relationships:
            await self.graph.add_relationships_batch(all_relationships)

        if errors:
            logger.warning("Indexing completed with errors", error_count=len(errors), total_files=len(filtered_files))

        logger.debug("File indexing complete", files_processed=len(results), total_symbols=total_symbols, total_relationships=total_relationships)

        # Resolve CALLS relationships now that all symbols are in the graph
        if progress_callback:
            progress_callback(phase="resolving_calls", current=0, total=1, symbols=total_symbols, relationships=total_relationships)
        
        resolved_calls = await self._resolve_calls(all_unresolved_calls)
        total_relationships += resolved_calls
        logger.debug("Resolved CALLS relationships", count=resolved_calls)

        # Detect communities after indexing (optional, can be slow)
        if hasattr(self.graph.backend, 'detect_communities'):
            self.graph.backend.detect_communities()
        
        # Process detection - now enabled by default with query generation
        processes_count = 0
        detect_processes = True  # Enabled to generate searchable queries
        
        if detect_processes:
            if progress_callback:
                progress_callback(phase="detecting_processes", current=0, total=1, symbols=total_symbols, relationships=total_relationships)
            
            from comind.indexing.process_detector import ProcessDetector
            detector = ProcessDetector(self.graph)
            
            # Detect processes and get traces for query generation
            logger.debug("Detecting execution processes...")
            processes = await detector.detect_processes()
            processes_count = len(processes)
            logger.debug(f"Detected {processes_count} processes")
        else:
            processes = []
        
        if processes:
            process_dicts = []
            all_process_queries = []
            
            for p in processes:
                entry_sym = await self.graph.get_symbol(p.entry_point_id)
                terminal_sym = await self.graph.get_symbol(p.terminal_id)
                steps = []
                if entry_sym:
                    steps.append({
                        "id": entry_sym.id,
                        "step": 1,
                        "name": entry_sym.name,
                        "file_path": entry_sym.file_path,
                    })
                if terminal_sym and terminal_sym.id != p.entry_point_id:
                    steps.append({
                        "id": terminal_sym.id,
                        "step": len(steps) + 1,
                        "name": terminal_sym.name,
                        "file_path": terminal_sym.file_path,
                    })
                process_dicts.append({
                    "id": p.id,
                    "repo_id": self.current_repo_id,
                    "name": p.label,
                    "entry_point": p.entry_point_id,
                    "label": p.label,
                    "type": p.process_type,
                    "steps": steps,
                    "priority": p.priority,
                    "step_count": p.step_count,
                })
                
                # Generate queries for this process using stored traces
                trace = detector.process_traces.get(p.id, [])
                if trace:
                    queries = detector.generate_queries_for_process(p, trace)
                    for query in queries:
                        all_process_queries.append({
                            "process_id": p.id,
                            "query": query
                        })
            
            # Store processes
            await self.graph.store_processes(process_dicts)
            
            # Store process queries with embeddings
            if all_process_queries and self.query_engine:
                if progress_callback:
                    progress_callback(phase="generating_queries", current=0, total=1, symbols=total_symbols, relationships=total_relationships)
                
                logger.debug(f"Generating embeddings for {len(all_process_queries)} process queries...")
                await self._store_process_queries(all_process_queries)
                logger.debug("Process queries stored with embeddings")
        
        # Build repo-specific search index if we're indexing a repository
        if self.query_engine and self.current_repo_id:
            await self.query_engine.build_repo_index(self.current_repo_id)
            
            # Save indexes to centralized storage (will be set by caller)
            # The server will call save_repo_index with the correct storage path
        
        return {
            "directory": directory_path,
            "files_processed": len(results),
            "total_files": len(python_files),
            "total_symbols": total_symbols,
            "total_relationships": total_relationships,
            "errors": errors,
            "processes_detected": processes_count
        }
    
    async def _store_process_queries(self, process_queries: List[Dict[str, str]]) -> None:
        """Store process queries with embeddings for semantic search"""
        if not process_queries:
            return
        
        # Generate embeddings for all queries
        from fastembed import TextEmbedding
        from comind.config import get_settings
        
        settings = get_settings()
        model = TextEmbedding(model_name=settings.search.embedding_model)
        
        # Batch embed all queries
        query_texts = [pq["query"] for pq in process_queries]
        embeddings = list(model.embed(query_texts))
        
        # Store in database
        for i, pq in enumerate(process_queries):
            embedding_list = embeddings[i].tolist()
            self.graph.backend.conn.execute("""
                INSERT INTO process_queries (process_id, query, embedding)
                VALUES (?, ?, ?)
            """, (pq["process_id"], pq["query"], embedding_list))
    
    async def _resolve_calls(self, unresolved: list[tuple[str, str]]) -> int:
        """Resolve unresolved (source_id, callee_name) pairs into CALLS edges.

        Builds a name → [symbol_id] map from all indexed symbols, then adds
        CALLS relationships for every (source, target) pair found.
        Returns the number of edges added.
        """
        if not unresolved:
            return 0

        all_symbols = await self.graph.get_all_symbols()
        # name → list of symbol ids (multiple symbols can share a name)
        name_to_ids: dict[str, list[str]] = {}
        for sym in all_symbols:
            name_to_ids.setdefault(sym.name, []).append(sym.id)

        added = 0
        for entry in unresolved:
            source_id, callee_name = entry[0], entry[1]
            call_line = entry[2] if len(entry) > 2 else None
            call_text = entry[3] if len(entry) > 3 else None
            targets = name_to_ids.get(callee_name, [])
            for target_id in targets:
                if target_id == source_id:
                    continue
                props: dict = {}
                if call_line is not None:
                    props["call_line"] = call_line
                if call_text:
                    props["call_text"] = call_text
                await self.graph.add_relationship(Relationship(
                    source_id=source_id,
                    target_id=target_id,
                    type=RelationType.CALLS,
                    confidence=0.7,
                    properties=props,
                ))
                added += 1
        return added

    async def index_repository(
        self,
        repo_path: str,
        repo_id: str | None = None,
        force: bool = False,
        progress_callback: Optional[callable] = None
    ) -> Dict[str, Any]:
        """Index a Python repository
        
        Args:
            repo_path: Path to repository on disk
            repo_id: Optional custom repository identifier (defaults to absolute path)
            force: If True, force full re-index (ignored for now, handled by incremental indexer)
            progress_callback: Optional callback for progress updates
        """
        path = Path(repo_path)
        
        if not path.exists():
            return {"error": "Repository path not found"}
        
        # Set current repo_id (use custom ID or absolute path as unique identifier)
        self.current_repo_id = repo_id or str(path.absolute())
        
        # Set repo root for relative path conversion
        self.repo_root = path.absolute()
        
        # Initialize gitignore handler for proper file filtering
        self.gitignore_handler = GitignoreHandler(self.repo_root)
        logger.debug("Initialized gitignore handler", repo_root=str(self.repo_root))
        
        # Get repository metadata
        metadata = await self._get_repository_metadata(path)
        
        # Register repository BEFORE indexing (required for foreign key constraints)
        await self.graph.register_repository(repo_path, metadata, repo_id=self.current_repo_id)
        
        # Index the directory with progress callback
        result = await self.index_directory(str(path), progress_callback=progress_callback)
        
        # Clear repo_id after indexing
        self.current_repo_id = None
        
        return {
            "repository": repo_path,
            "repo_id": str(path.absolute()),
            "metadata": metadata,
            **result
        }
    
    async def _get_repository_metadata(self, repo_path: Path) -> Dict[str, Any]:
        """Get repository metadata"""
        metadata = {
            "indexed_at": asyncio.get_event_loop().time(),
            "path": str(repo_path),
        }
        
        # Get git information if available
        import subprocess
        result = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=repo_path,
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.returncode == 0:
            metadata["last_commit"] = result.stdout.strip()
        
        # Count Python files
        python_files = list(repo_path.rglob("*.py"))
        metadata["python_files"] = len(python_files)
        
        return metadata
