//! Contract graph: HTTP routes ↔ clients, queues/topics, buckets and tables, keyed so that
//! providers and consumers meet across repos. Values resolve through constants, settings, env.

use std::collections::HashMap;

use sqlparser::ast::{ObjectName, Statement, TableFactor, TableWithJoins};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

use super::{Binder, Value};
use crate::facts::{Args, ContractFact, FileFacts, ImportItem, TemplatePart, ValueExpr};
use crate::model::{
    EdgeKind, GlobalSymbolId, Language, Position, Range, RepoId, Symbol, SymbolKind,
};

const HTTP_VERBS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];
const HTTP_CLIENTS: &[&str] = &[
    "httpx.Client",
    "httpx.AsyncClient",
    "requests.Session",
    "aiohttp.ClientSession",
];
const HTTP_MODULES: &[&str] = &["httpx", "requests", "axios", "ky", "got"];
const ROUTERS: &[&str] = &["fastapi.APIRouter", "fastapi.FastAPI"];
const DB_READS: &[&str] = &["get", "query", "select", "scalars"];
const DB_WRITES: &[&str] = &["add", "merge", "delete", "insert", "update"];
/// Calls whose value is their first argument's (`Path("/x")`, `str(x)`).
const PASS_THROUGH: &[&str] = &["Path", "PurePath", "PurePosixPath", "str", "URL", "HttpUrl"];
/// Calls building record-like values whose keyword arguments are attributes.
const RECORDS: &[&str] = &["AttrDict", "dict", "SimpleNamespace", "Namespace"];

/// The repo-like owner of contract nodes.
pub const CONTRACTS: &str = "_contracts";

#[derive(Clone)]
enum Client {
    Http,
    Aws(String),
}

pub(super) struct Linked {
    pub symbols: Vec<Symbol>,
    pub edges: Vec<(String, String, EdgeKind)>,
}

pub(super) fn link(b: &Binder, facts: &[FileFacts], calls: &[(String, String)]) -> Linked {
    let mut cx = Cx::new(b, facts);
    cx.performing(facts, calls);
    let mut out = Linked {
        symbols: Vec::new(),
        edges: Vec::new(),
    };
    let mut emit = |src: &str, key: String, kind: EdgeKind| {
        out.edges
            .push((src.to_string(), contract_id(&key).render(), kind));
        if !out.symbols.iter().any(|s: &Symbol| s.name == key) {
            out.symbols.push(contract_symbol(&key));
        }
    };

    for f in facts {
        for c in &f.contracts {
            match c {
                ContractFact::Table { class, name } => {
                    emit(class, format!("table:{name}"), EdgeKind::Defines)
                }
                ContractFact::Route {
                    handler,
                    receiver,
                    method,
                    path,
                    ..
                } => {
                    let Some(p) = cx.value(path, &f.file_id, 0) else {
                        continue;
                    };
                    // File-routed handlers (Next.js) carry their full path and no router.
                    let full = if receiver.is_empty() {
                        p
                    } else {
                        let Some(router) = cx.var_of(receiver, &f.file_id, f) else {
                            continue;
                        };
                        if !cx.routers.contains_key(&router) {
                            continue;
                        }
                        format!("{}{p}", cx.prefix(&router, 0))
                    };
                    emit(
                        handler,
                        format!("http:{method} {}", http_path(&full)),
                        EdgeKind::Serves,
                    );
                }
                ContractFact::Op {
                    from,
                    receiver,
                    method,
                    args,
                    ..
                } => cx.op(f, from, receiver, method, args, &mut emit),
                ContractFact::Construct { .. } => {}
            }
        }
    }
    suffix_match(&mut out);
    out
}

/// A client sees paths below its configured base URL (often `/api/v1`), so a consumer-only
/// HTTP contract links to the one provider route it is a segment-aligned suffix of.
fn suffix_match(out: &mut Linked) {
    let ids: HashMap<String, String> = out
        .symbols
        .iter()
        .map(|s| (s.id.render(), s.name.clone()))
        .collect();
    let served: Vec<&String> = out
        .edges
        .iter()
        .filter(|(_, _, k)| *k == EdgeKind::Serves)
        .filter_map(|(_, d, _)| ids.get(d))
        .collect();
    let mut redirect: HashMap<String, String> = HashMap::new();
    for (id, name) in &ids {
        let Some((verb, path)) = name.strip_prefix("http:").and_then(|r| r.split_once(' ')) else {
            continue;
        };
        if served.contains(&name) || path.matches('/').count() < 2 {
            continue;
        }
        let cands: Vec<&&String> = served
            .iter()
            .filter(|p| {
                p.strip_prefix("http:")
                    .and_then(|r| r.split_once(' '))
                    .is_some_and(|(v, pp)| v == verb && pp != path && pp.ends_with(path))
            })
            .collect();
        let mut uniq: Vec<&String> = cands.into_iter().copied().collect();
        uniq.sort();
        uniq.dedup();
        if let [target] = uniq.as_slice() {
            redirect.insert(id.clone(), contract_id(target).render());
        }
    }
    for e in &mut out.edges {
        if let Some(t) = redirect.get(&e.1) {
            e.1 = t.clone();
        }
    }
    out.symbols
        .retain(|s| !redirect.contains_key(&s.id.render()));
}

struct Cx<'b, 'a> {
    b: &'b Binder<'a>,
    values: HashMap<(&'a str, &'a str), &'a ValueExpr>,
    /// Client registry: (scope, target name) → client kind (+ base URL value).
    clients: HashMap<(String, String), Client>,
    /// Router variable id → own prefix.
    routers: HashMap<String, String>,
    /// Router variable id → (parent app/router id, include prefix).
    mounts: HashMap<String, (String, String)>,
    /// Classes/interfaces whose methods (transitively) perform HTTP, plus their supertypes.
    http_types: std::collections::HashSet<String>,
}

impl<'b, 'a> Cx<'b, 'a> {
    fn new(b: &'b Binder<'a>, facts: &'a [FileFacts]) -> Self {
        let mut cx = Cx {
            b,
            values: HashMap::new(),
            clients: HashMap::new(),
            routers: HashMap::new(),
            mounts: HashMap::new(),
            http_types: std::collections::HashSet::new(),
        };
        for f in facts {
            for v in &f.values {
                cx.values
                    .insert((v.scope.as_str(), v.name.as_str()), &v.value);
            }
        }
        for f in facts {
            for c in &f.contracts {
                let ContractFact::Construct {
                    scope,
                    target,
                    ctor,
                    args,
                } = c
                else {
                    continue;
                };
                let Some(ext) = cx.external(ctor, scope, f) else {
                    continue;
                };
                if ROUTERS.contains(&ext.as_str()) {
                    if let Some(var) = cx.var_of(std::slice::from_ref(target), scope, f) {
                        let prefix = args
                            .get("prefix", usize::MAX)
                            .and_then(|v| cx.value(v, scope, 0))
                            .unwrap_or_default();
                        cx.routers.insert(var, prefix);
                    }
                } else if HTTP_CLIENTS.contains(&ext.as_str()) {
                    cx.clients
                        .insert((scope.clone(), target.clone()), Client::Http);
                } else if ext == "boto3.client" || ext == "boto3.resource" {
                    if let Some(svc) = args
                        .get("service_name", 0)
                        .and_then(|v| cx.value(v, scope, 0))
                    {
                        cx.clients
                            .insert((scope.clone(), target.clone()), Client::Aws(svc));
                    }
                }
            }
        }
        for f in facts {
            for c in &f.contracts {
                let ContractFact::Op {
                    from,
                    receiver,
                    method,
                    args,
                    ..
                } = c
                else {
                    continue;
                };
                if method != "include_router" {
                    continue;
                }
                let parent = cx.var_of(receiver, from, f);
                let router = match args.positional.first() {
                    Some(Some(ValueExpr::Name(p))) => cx.var_of(p, from, f),
                    _ => None,
                };
                if let (Some(parent), Some(router)) = (parent, router) {
                    let prefix = args
                        .get("prefix", usize::MAX)
                        .and_then(|v| cx.value(v, from, 0))
                        .unwrap_or_default();
                    cx.mounts.insert(router, (parent, prefix));
                }
            }
        }
        cx
    }

    /// Mark types whose methods reach `fetch`/`axios` through the call graph (TS/JS wrappers).
    fn performing(&mut self, facts: &[FileFacts], calls: &[(String, String)]) {
        let mut doing: std::collections::HashSet<String> = std::collections::HashSet::new();
        for f in facts
            .iter()
            .filter(|f| matches!(f.language, Language::TypeScript | Language::JavaScript))
        {
            for c in &f.contracts {
                if let ContractFact::Op {
                    from,
                    receiver,
                    method,
                    ..
                } = c
                {
                    if (receiver.is_empty() && method == "fetch")
                        || self.direct_http(receiver, from, f)
                    {
                        doing.insert(from.clone());
                    }
                }
            }
        }
        let mut callers: HashMap<&str, Vec<&str>> = HashMap::new();
        for (src, dst) in calls {
            callers.entry(dst.as_str()).or_default().push(src.as_str());
        }
        let mut queue: Vec<String> = doing.iter().cloned().collect();
        while let Some(n) = queue.pop() {
            for c in callers.get(n.as_str()).into_iter().flatten() {
                if doing.len() < 100_000 && doing.insert(c.to_string()) {
                    queue.push(c.to_string());
                }
            }
        }
        let mut types: Vec<String> = doing
            .iter()
            .filter_map(|m| self.b.parent.get(m))
            .filter(|p| self.b.is_type(p))
            .cloned()
            .collect();
        while let Some(t) = types.pop() {
            if self.http_types.insert(t.clone()) {
                types.extend(self.b.bases.borrow().get(&t).cloned().unwrap_or_default());
            }
        }
    }

    fn direct_http(&self, receiver: &[String], from: &str, f: &FileFacts) -> bool {
        receiver.len() == 1
            && self
                .external(receiver, from, f)
                .is_some_and(|m| HTTP_MODULES.contains(&m.as_str()))
    }

    /// Full mount prefix of a router: parents' include prefixes, then its own prefix.
    fn prefix(&self, router: &str, depth: u32) -> String {
        let own = self.routers.get(router).cloned().unwrap_or_default();
        match self.mounts.get(router) {
            Some((parent, p)) if depth < 8 => format!("{}{p}{own}", self.prefix(parent, depth + 1)),
            _ => own,
        }
    }

    /// Dotted external name of a path whose head is imported from a third-party module.
    fn external(&self, path: &[String], scope: &str, f: &FileFacts) -> Option<String> {
        let (head, rest) = path.split_first()?;
        let base = match self.b.resolve_name(head, scope, f, 0) {
            Some(Value::Module { name, .. })
                if !self
                    .b
                    .modules
                    .contains_key(&(super::Family::Py, name.as_str())) =>
            {
                name
            }
            Some(_) => return None,
            // `from fastapi import APIRouter`: bound by an import the corpus can't resolve.
            None => f.imports.iter().find_map(|imp| {
                imp.items.iter().find_map(|item| match item {
                    // A default import names the module itself (`import axios from "axios"`).
                    ImportItem::Name { name, alias }
                        if imp.level == 0
                            && name == "default"
                            && alias.as_deref() == Some(head) =>
                    {
                        Some(imp.module.clone())
                    }
                    ImportItem::Name { name, alias }
                        if imp.level == 0 && alias.as_deref().unwrap_or(name) == head =>
                    {
                        Some(format!("{}.{name}", imp.module))
                    }
                    ImportItem::Module { alias: Some(a) } if a == head => Some(imp.module.clone()),
                    _ => None,
                })
            })?,
        };
        let mut parts = vec![base];
        parts.extend(rest.iter().cloned());
        Some(parts.join("."))
    }

    /// The module-variable symbol a path names (routers and apps are module variables).
    fn var_of(&self, path: &[String], scope: &str, f: &FileFacts) -> Option<String> {
        let found = match path {
            [name] => self.b.module_member(&f.file_id, name, true, 0),
            _ => self.b.resolve_path(path, scope, f, 0),
        };
        match found? {
            Value::Sym(id) => Some(id),
            _ => None,
        }
    }

    fn client(&self, receiver: &[String], from: &str, f: &FileFacts) -> Option<Client> {
        if let ([recv, attr], true) = (receiver, receiver.first().is_some_and(|r| r == "self")) {
            let class = self.b.enclosing_class(from)?;
            return self
                .clients
                .get(&(class, format!("{recv}.{attr}")))
                .cloned();
        }
        if let [name] = receiver {
            for s in self.b.scope_chain(from) {
                if let Some(c) = self.clients.get(&(s, name.clone())) {
                    return Some(c.clone());
                }
            }
        }
        // A module-level client imported from elsewhere.
        let id = self.var_of(receiver, from, f)?;
        let var = self.b.sym(&id)?;
        let scope = self.b.parent.get(&id)?;
        self.clients
            .get(&(scope.clone(), var.name.clone()))
            .cloned()
    }

    fn op(
        &self,
        f: &FileFacts,
        from: &str,
        receiver: &[String],
        method: &str,
        args: &Args,
        emit: &mut impl FnMut(&str, String, EdgeKind),
    ) {
        if method == "text" && receiver.is_empty() {
            if let Some(ValueExpr::Lit(sql)) = args.positional.first().and_then(Option::as_ref) {
                for (table, write) in sql_tables(sql) {
                    let kind = if write {
                        EdgeKind::Writes
                    } else {
                        EdgeKind::Reads
                    };
                    emit(from, format!("table:{table}"), kind);
                }
            }
            return;
        }
        // pypika: `Query.from_(t)` / `.join(t)` read, `Query.into(t)` / `Query.update(t)` write.
        if matches!(method, "from_" | "join" | "into" | "update") {
            if let Some(table) = args
                .positional
                .first()
                .and_then(Option::as_ref)
                .and_then(|v| self.pypika_table(v, from))
            {
                let kind = if matches!(method, "into" | "update") {
                    EdgeKind::Writes
                } else {
                    EdgeKind::Reads
                };
                emit(from, format!("table:{table}"), kind);
                return;
            }
        }
        if let Some(table) = self.model_table(args, from, f) {
            if DB_READS.contains(&method) {
                emit(from, format!("table:{table}"), EdgeKind::Reads);
                return;
            }
            if DB_WRITES.contains(&method) {
                emit(from, format!("table:{table}"), EdgeKind::Writes);
                return;
            }
        }
        if receiver.is_empty() && method == "fetch" {
            let url = args.positional.first().and_then(Option::as_ref);
            let verb = match args.positional.get(1).and_then(Option::as_ref) {
                Some(init) => self
                    .field(init, from, "method", 0)
                    .and_then(|(v, at)| self.value(&v, &at, 0))
                    .unwrap_or_else(|| "GET".into()),
                None => "GET".into(),
            };
            if let Some(url) = url.and_then(|u| self.value(u, from, 0)) {
                emit(
                    from,
                    format!("http:{} {}", verb.to_uppercase(), http_path(&url)),
                    EdgeKind::Consumes,
                );
            }
            return;
        }
        let direct_http = self.direct_http(receiver, from, f);
        let client = if direct_http {
            Some(Client::Http)
        } else {
            self.client(receiver, from, f)
                .or_else(|| self.wrapper(receiver, method, from, f))
        };
        match client {
            Some(Client::Http) if HTTP_VERBS.contains(&method) || method == "request" => {
                let (verb, url) = if method == "request" {
                    (
                        args.get("method", 0).and_then(|v| self.value(v, from, 0)),
                        args.get("url", 1),
                    )
                } else {
                    (Some(method.to_string()), args.get("url", 0))
                };
                if let (Some(verb), Some(url)) = (verb, url.and_then(|u| self.value(u, from, 0))) {
                    emit(
                        from,
                        format!("http:{} {}", verb.to_uppercase(), http_path(&url)),
                        EdgeKind::Consumes,
                    );
                }
            }
            Some(Client::Aws(svc)) => {
                if let Some((key, kind)) = self.aws(&svc, method, args, from) {
                    emit(from, key, kind);
                }
            }
            _ => {}
        }
    }

    /// A verb method on a class that owns an HTTP client (directly or via its bases) is an
    /// HTTP call: shared service clients are recognised without per-wrapper rules.
    fn wrapper(
        &self,
        receiver: &[String],
        method: &str,
        from: &str,
        f: &FileFacts,
    ) -> Option<Client> {
        if !(HTTP_VERBS.contains(&method) || method == "request") || receiver.is_empty() {
            return None;
        }
        let mut full = receiver.to_vec();
        full.push(method.to_string());
        if matches!(f.language, Language::TypeScript | Language::JavaScript) {
            // The receiver's static type (often an interface) is implemented by an HTTP class.
            let Value::Instance(t) = self.b.resolve_path(receiver, from, f, 0)? else {
                return None;
            };
            return self.http_types.contains(&t).then_some(Client::Http);
        }
        let Value::Sym(m) = self.b.resolve_path(&full, from, f, 0)? else {
            return None;
        };
        let owner = self.b.parent.get(&m)?;
        self.owns_http(owner, 0).then_some(Client::Http)
    }

    fn owns_http(&self, class: &str, depth: u32) -> bool {
        if depth > 8 {
            return false;
        }
        let own = self.clients.iter().any(|((scope, target), c)| {
            scope == class && target.starts_with("self.") && matches!(c, Client::Http)
        });
        own || self
            .b
            .bases
            .borrow()
            .get(class)
            .is_some_and(|bs| bs.iter().any(|b| self.owns_http(b, depth + 1)))
    }

    fn aws(&self, svc: &str, method: &str, args: &Args, from: &str) -> Option<(String, EdgeKind)> {
        let v = |kw: &str, pos: usize| args.get(kw, pos).and_then(|x| self.value(x, from, 0));
        Some(match (svc, method) {
            ("sqs", "send_message" | "send_message_batch") => (
                format!("sqs:{}", queue_name(&v("QueueUrl", usize::MAX)?)),
                EdgeKind::Publishes,
            ),
            ("sqs", "receive_message") => (
                format!("sqs:{}", queue_name(&v("QueueUrl", usize::MAX)?)),
                EdgeKind::Subscribes,
            ),
            ("sns", "publish" | "publish_batch") => {
                let arn = v("TopicArn", usize::MAX).or_else(|| v("TargetArn", usize::MAX))?;
                (format!("sns:{}", arn_name(&arn)), EdgeKind::Publishes)
            }
            ("s3", "put_object" | "delete_object" | "copy_object") => (
                format!("s3:{}", bucket(&v("Bucket", usize::MAX)?)),
                EdgeKind::Writes,
            ),
            ("s3", "upload_file" | "upload_fileobj") => {
                (format!("s3:{}", bucket(&v("Bucket", 1)?)), EdgeKind::Writes)
            }
            ("s3", "get_object" | "head_object" | "list_objects_v2") => (
                format!("s3:{}", bucket(&v("Bucket", usize::MAX)?)),
                EdgeKind::Reads,
            ),
            ("s3", "download_file" | "download_fileobj") => {
                (format!("s3:{}", bucket(&v("Bucket", 0)?)), EdgeKind::Reads)
            }
            _ => return None,
        })
    }

    /// The table name of a pypika `Table("name", schema=…)` an expression evaluates to.
    fn pypika_table(&self, v: &ValueExpr, scope: &str) -> Option<String> {
        let (expr, at) = match v {
            ValueExpr::Name(p) => self.locate(p, scope, 0)?,
            other => (other.clone(), scope.to_string()),
        };
        let ValueExpr::Call { func, args, kwargs } = &expr else {
            return None;
        };
        if func.last().map(String::as_str) != Some("Table") {
            return None;
        }
        let name = kwargs
            .iter()
            .find(|(k, _)| k == "name")
            .map(|(_, v)| v)
            .or(args.first())?;
        self.value(name, &at, 0)
    }

    /// Table of the ORM model an operation's first argument names or constructs.
    fn model_table(&self, args: &Args, from: &str, f: &FileFacts) -> Option<String> {
        let path = match args.positional.first() {
            Some(Some(ValueExpr::Name(p))) => p.clone(),
            _ => args.calls.iter().find(|(i, _)| *i == 0)?.1.clone(),
        };
        let Value::Sym(class) = self.b.resolve_path(&path, from, f, 0)? else {
            return None;
        };
        self.b.facts.values().find_map(|ff| {
            ff.contracts.iter().find_map(|c| match c {
                ContractFact::Table { class: c, name } if *c == class => Some(name.clone()),
                _ => None,
            })
        })
    }

    /// The string a value expression evaluates to; unknown parts become `{}`.
    fn value(&self, v: &ValueExpr, scope: &str, depth: u32) -> Option<String> {
        if depth > 12 {
            return None;
        }
        match v {
            ValueExpr::Lit(s) => Some(s.clone()),
            ValueExpr::Env(k) => Some(format!("{{env:{k}}}")),
            ValueExpr::Template(parts) => Some(
                parts
                    .iter()
                    .map(|p| match p {
                        TemplatePart::Lit(s) => s.clone(),
                        TemplatePart::Expr(Some(path)) => self
                            .value(&ValueExpr::Name(path.clone()), scope, depth + 1)
                            .filter(|s| !s.contains("{env:"))
                            .unwrap_or_else(|| "{}".into()),
                        TemplatePart::Expr(None) => "{}".into(),
                    })
                    .collect(),
            ),
            ValueExpr::Name(path) => {
                let (expr, at) = self.locate(path, scope, depth + 1)?;
                self.value(&expr, &at, depth + 1)
            }
            ValueExpr::Join { sep, parts } => {
                let vals: Vec<String> = parts
                    .iter()
                    .map(|p| {
                        self.value(p, scope, depth + 1)
                            .unwrap_or_else(|| "{}".into())
                    })
                    .collect();
                Some(if sep == "/" {
                    join_path(&vals)
                } else {
                    vals.concat()
                })
            }
            ValueExpr::Call { func, args, .. } => {
                let last = func.last()?.as_str();
                if PASS_THROUGH.contains(&last) {
                    self.value(args.first()?, scope, depth + 1)
                } else {
                    None
                }
            }
            ValueExpr::Record(_) => None,
        }
    }

    /// The expression a dotted name denotes and the scope to read it in, following module
    /// constants, class attributes (incl. bases), instance types and record fields.
    fn locate(&self, path: &[String], scope: &str, depth: u32) -> Option<(ValueExpr, String)> {
        if depth > 12 {
            return None;
        }
        let f = self.b.file_facts_of(scope)?;
        let head = path.first()?;
        let mut found = None;
        for s in self.b.scope_chain(scope) {
            if let Some(v) = self.values.get(&(s.as_str(), head.as_str())) {
                found = Some(((*v).clone(), s, 1));
                break;
            }
        }
        if found.is_none() {
            for k in (1..=path.len()).rev() {
                let Some(v) = self.b.resolve_path(&path[..k], scope, f, 0) else {
                    continue;
                };
                let owner = match v {
                    Value::Instance(c) => Some(c),
                    Value::Sym(id) if self.b.is_type(&id) => Some(id),
                    Value::Sym(id) => {
                        // A module variable: the instance it holds (when an attribute follows),
                        // else its own value.
                        let var = self.b.sym(&id)?;
                        let parent = self.b.parent.get(&id)?.clone();
                        let instance = self
                            .b
                            .types
                            .get(&(parent.as_str(), var.name.as_str()))
                            .and_then(|src| self.b.type_of(src, &parent, 0))
                            .and_then(|v| match v {
                                Value::Instance(c) => Some(c),
                                _ => None,
                            });
                        if let (Some(c), Some(attr)) = (&instance, path.get(k)) {
                            if let Some((v, at)) = self.class_value(c, attr, 0) {
                                found = Some((v, at, k + 1));
                                break;
                            }
                        }
                        if let Some(v) = self.values.get(&(parent.as_str(), var.name.as_str())) {
                            found = Some(((*v).clone(), parent, k));
                            break;
                        }
                        instance
                    }
                    _ => None,
                };
                let (Some(class), Some(attr)) = (owner, path.get(k)) else {
                    continue;
                };
                if let Some((v, at)) = self.class_value(&class, attr, 0) {
                    found = Some((v, at, k + 1));
                    break;
                }
            }
        }
        let (mut expr, mut at, used) = found?;
        for name in &path[used..] {
            let (e, a) = self.field(&expr, &at, name, depth + 1)?;
            expr = e;
            at = a;
        }
        Some((expr, at))
    }

    /// `X.name` where `X` evaluates to a record-like value.
    fn field(
        &self,
        v: &ValueExpr,
        scope: &str,
        name: &str,
        depth: u32,
    ) -> Option<(ValueExpr, String)> {
        match v {
            ValueExpr::Record(fields) => fields
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| (v.clone(), scope.to_string())),
            ValueExpr::Call { func, kwargs, .. }
                if func.last().is_some_and(|f| RECORDS.contains(&f.as_str())) =>
            {
                kwargs
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| (v.clone(), scope.to_string()))
            }
            ValueExpr::Name(p) => {
                let mut full = p.clone();
                full.push(name.to_string());
                self.locate(&full, scope, depth + 1)
            }
            _ => None,
        }
    }

    /// A class attribute's value expression, searching base classes.
    fn class_value(&self, class: &str, attr: &str, depth: u32) -> Option<(ValueExpr, String)> {
        if depth > 8 {
            return None;
        }
        if let Some(v) = self.values.get(&(class, attr)) {
            return Some(((*v).clone(), class.to_string()));
        }
        let bases = self
            .b
            .bases
            .borrow()
            .get(class)
            .cloned()
            .unwrap_or_default();
        bases
            .iter()
            .find_map(|b| self.class_value(b, attr, depth + 1))
    }
}

/// Join path parts with single slashes, keeping a leading slash.
fn join_path(parts: &[String]) -> String {
    let mut out = String::new();
    for (i, p) in parts.iter().enumerate() {
        let p = if i == 0 {
            p.trim_end_matches('/')
        } else {
            p.trim_matches('/')
        };
        if p.is_empty() {
            continue;
        }
        if i > 0 {
            out.push('/');
        }
        out.push_str(p);
    }
    out
}

/// `/v1/skills/{skill_id}?x=1` → `/v1/skills/{}`; scheme and host dropped.
pub fn http_path(url: &str) -> String {
    let mut p = url;
    for scheme in ["https://", "http://"] {
        if let Some(rest) = p.strip_prefix(scheme) {
            p = rest.find('/').map_or("/", |i| &rest[i..]);
        }
    }
    // A leading unresolved base (`{}/x`, `{env:BASE}/x`) is a host, not a path segment.
    while p.starts_with('{') {
        p = p.find('}').map_or("", |i| &p[i + 1..]);
    }
    let p = p.split(['?', '#']).next().unwrap_or_default();
    let mut segs: Vec<String> = p
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.starts_with('{') && s.ends_with('}') {
                "{}".to_string()
            } else {
                s.to_string()
            }
        })
        .collect();
    // `/docs${query}`: a placeholder glued to the last segment is a query string.
    if let Some(last) = segs.last_mut() {
        if last.len() > 2 && last.ends_with("{}") {
            last.truncate(last.len() - 2);
        }
    }
    format!("/{}", segs.join("/"))
}

fn queue_name(url: &str) -> String {
    if url.starts_with("{env:") {
        return url.to_string();
    }
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

fn arn_name(arn: &str) -> String {
    if arn.starts_with("{env:") {
        return arn.to_string();
    }
    arn.rsplit(':').next().unwrap_or(arn).to_string()
}

fn bucket(b: &str) -> String {
    b.trim_start_matches("s3://")
        .split('/')
        .next()
        .unwrap_or(b)
        .to_string()
}

/// Tables a SQL string reads or writes (`true` = write).
fn sql_tables(sql: &str) -> Vec<(String, bool)> {
    let Ok(stmts) = Parser::parse_sql(&GenericDialect {}, sql) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let name = |n: &ObjectName| {
        n.to_string()
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_string()
    };
    let from = |twj: &[TableWithJoins], out: &mut Vec<(String, bool)>| {
        for t in twj {
            for f in std::iter::once(&t.relation).chain(t.joins.iter().map(|j| &j.relation)) {
                if let TableFactor::Table { name: n, .. } = f {
                    out.push((name(n), false));
                }
            }
        }
    };
    for s in stmts {
        match s {
            Statement::Query(q) => {
                if let sqlparser::ast::SetExpr::Select(sel) = q.body.as_ref() {
                    from(&sel.from, &mut out);
                }
            }
            Statement::Insert(i) => {
                let t = i.table.to_string();
                out.push((t.rsplit('.').next().unwrap_or(&t).to_string(), true));
            }
            Statement::Update(u) => {
                if let TableFactor::Table { name: n, .. } = &u.table.relation {
                    out.push((name(n), true));
                }
            }
            Statement::Delete(d) => {
                let tables = match &d.from {
                    sqlparser::ast::FromTable::WithFromKeyword(t)
                    | sqlparser::ast::FromTable::WithoutKeyword(t) => t,
                };
                for t in tables {
                    if let TableFactor::Table { name: n, .. } = &t.relation {
                        out.push((name(n), true));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

pub fn contract_id(key: &str) -> GlobalSymbolId {
    GlobalSymbolId {
        scheme: "comind-contract".into(),
        package_manager: ".".into(),
        package: CONTRACTS.into(),
        version: ".".into(),
        descriptor: key.into(),
    }
}

fn contract_symbol(key: &str) -> Symbol {
    let zero = Position { line: 0, column: 0 };
    Symbol {
        id: contract_id(key),
        name: key.to_string(),
        kind: SymbolKind::Contract,
        language: Language::Other("contract".into()),
        repo: RepoId(CONTRACTS.into()),
        file_path: String::new(),
        range: Range {
            start: zero,
            end: zero,
        },
        signature: None,
        docstring: None,
        fingerprint: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_paths_normalize() {
        assert_eq!(http_path("/v1/skills/{skill_id}"), "/v1/skills/{}");
        assert_eq!(
            http_path("http://skills-svc/v1/skills/{}?x=1"),
            "/v1/skills/{}"
        );
        assert_eq!(http_path("{}/v1/a//b/"), "/v1/a/b");
        assert_eq!(http_path("{env:BASE}/x"), "/x");
        assert_eq!(http_path("/api/documents{}"), "/api/documents");
    }

    #[test]
    fn sql_tables_by_statement() {
        assert_eq!(
            sql_tables("SELECT count(*) FROM skills"),
            vec![("skills".into(), false)]
        );
        assert_eq!(
            sql_tables("SELECT * FROM a JOIN b ON a.id = b.id"),
            vec![("a".into(), false), ("b".into(), false)]
        );
        assert_eq!(
            sql_tables("INSERT INTO skills (id) VALUES (1)"),
            vec![("skills".into(), true)]
        );
        assert_eq!(
            sql_tables("UPDATE x.skills SET a = 1"),
            vec![("skills".into(), true)]
        );
    }
}
