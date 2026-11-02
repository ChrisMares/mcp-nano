//! Call-site extraction for code chunks.
//!
//! Each chunk's code is re-parsed with its language grammar and every call
//! site is reduced to the called name. The names are stored in the chunk's
//! `calls` payload field so the MCP and UI can answer "who calls X" and
//! "what does X call". Matching is by name only (no type resolution), so it
//! is roughly as precise as grepping for call sites.

use std::collections::HashSet;

use tree_sitter::{Node, Parser};

use super::helpers::{node_text, walk_preorder};
use super::Language;

/// Upper bound on stored call names per chunk.
pub const MAX_CALLS_PER_CHUNK: usize = 50;

/// Names so common they would match most of a codebase. Qualified forms
/// (for example `Foo::new`) are still kept.
const COMMON_NAMES: &[&str] = &[
    // Rust
    "new",
    "default",
    "from",
    "into",
    "clone",
    "to_string",
    "to_owned",
    "unwrap",
    "unwrap_or",
    "unwrap_or_default",
    "unwrap_or_else",
    "expect",
    "ok",
    "err",
    "is_some",
    "is_none",
    "is_ok",
    "is_err",
    "as_str",
    "as_ref",
    "as_deref",
    "iter",
    "into_iter",
    "collect",
    "len",
    "is_empty",
    "push",
    "pop",
    "insert",
    "remove",
    "contains",
    "get",
    "get_mut",
    "map",
    "map_err",
    "and_then",
    "ok_or",
    "ok_or_else",
    "Some",
    "Ok",
    "Err",
    "Box",
    "Vec",
    "String",
    "format",
    "join",
    "split",
    "trim",
    "await",
    "now",
    "context",
    "with_context",
    "chain",
    "iter_mut",
    "clear",
    "is_null",
    "as_object",
    "as_array",
    "as_i64",
    "as_u64",
    "as_f64",
    "as_bool",
    "to_vec",
    "to_lowercase",
    "to_uppercase",
    "sort",
    "sort_by",
    "sort_by_key",
    "take",
    "skip",
    "max",
    "min",
    "cloned",
    "copied",
    "any",
    "all",
    "first",
    "last",
    "rev",
    "flatten",
    "starts_with",
    "ends_with",
    "replace",
    "parse",
    "lock",
    "read",
    "write",
    "send",
    "spawn",
    "display",
    "extend",
    "enumerate",
    "zip",
    "next",
    "with_capacity",
    "lines",
    "chars",
    "bytes",
    "entry",
    "or_insert",
    // JavaScript / TypeScript / React
    "toString",
    "forEach",
    "filter",
    "reduce",
    "find",
    "some",
    "every",
    "includes",
    "slice",
    "splice",
    "concat",
    "keys",
    "values",
    "entries",
    "then",
    "catch",
    "finally",
    "parseInt",
    "parseFloat",
    "log",
    "error",
    "warn",
    "require",
    "useState",
    "useEffect",
    "useCallback",
    "useMemo",
    "useRef",
    "useContext",
    "set",
    "has",
    "add",
    "delete",
    "Number",
    "Boolean",
    "Array",
    "Object",
    "Promise",
    "Error",
    "Date",
    "Fragment",
    "React.Fragment",
    "Suspense",
    "React.Suspense",
    "StrictMode",
    "React.StrictMode",
    // Python
    "print",
    "range",
    "str",
    "int",
    "float",
    "bool",
    "list",
    "dict",
    "tuple",
    "isinstance",
    "super",
    "append",
    "items",
    "update",
    "open",
    "sorted",
    // Java / C# / C / C++
    "equals",
    "hashCode",
    "size",
    "ToString",
    "Equals",
    "printf",
    "malloc",
    "free",
    "sizeof",
    "strlen",
    "memcpy",
    "std::move",
];

/// Standard-library types whose associated functions (`Vec::new`,
/// `Instant::now`, ...) are dropped even in qualified form.
const COMMON_TYPES: &[&str] = &[
    "Vec",
    "String",
    "HashMap",
    "HashSet",
    "BTreeMap",
    "BTreeSet",
    "VecDeque",
    "Box",
    "Arc",
    "Rc",
    "Option",
    "Result",
    "Some",
    "Ok",
    "Err",
    "Duration",
    "Instant",
    "SystemTime",
    "Path",
    "PathBuf",
    "Mutex",
    "RwLock",
    "Cell",
    "RefCell",
    "std",
    "fs",
    "io",
    "mem",
    "Object",
    "Array",
    "JSON",
    "Math",
    "Promise",
    "console",
    "Date",
    "Number",
];

/// Called names in `code`, de-duplicated in first-seen order and capped at
/// [`MAX_CALLS_PER_CHUNK`]. SQL has no meaningful call graph and returns none.
pub fn extract_calls(lang: Language, code: &str) -> Vec<String> {
    if lang == Language::Sql || code.is_empty() {
        return Vec::new();
    }
    let mut parser = Parser::new();
    if parser.set_language(&lang.tree_sitter_language()).is_err() {
        return Vec::new();
    }
    let source = code.as_bytes();
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };

    let mut names: Vec<String> = Vec::new();
    walk_preorder(tree.root_node(), |node| {
        if let Some(callee) = callee_node(&node) {
            push_callee_names(&callee, source, &mut names);
        } else if let Some(tag) = jsx_component_tag(&node, source) {
            push_component_names(&tag, &mut names);
        }
    });

    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for name in names {
        if out.len() >= MAX_CALLS_PER_CHUNK {
            break;
        }
        if name.is_empty() || COMMON_NAMES.contains(&name.as_str()) {
            continue;
        }
        if seen.insert(name.clone()) {
            out.push(name);
        }
    }
    out
}

/// The node naming the called function, for any supported call node kind.
fn callee_node<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    match node.kind() {
        // Rust, JS/TS, C, C++, Python (`call`), C# (`invocation_expression`).
        "call_expression" | "call" | "invocation_expression" => {
            node.child_by_field_name("function")
        }
        // Java: `obj.method(...)` keeps the method name in `name`.
        "method_invocation" => node.child_by_field_name("name"),
        // `new Foo(...)` in JS/TS, Java, and C#.
        "new_expression" => node.child_by_field_name("constructor"),
        "object_creation_expression" => node.child_by_field_name("type"),
        _ => None,
    }
}

/// Tag name of a JSX element that renders a component (`<Foo />`,
/// `<Foo.Bar>`). Lowercase tags are HTML elements and return `None`.
fn jsx_component_tag(node: &Node, source: &[u8]) -> Option<String> {
    if !matches!(
        node.kind(),
        "jsx_opening_element" | "jsx_self_closing_element"
    ) {
        return None;
    }
    let text = node_text(&node.child_by_field_name("name")?, source);
    let starts_upper = text.chars().next().is_some_and(char::is_uppercase);
    (starts_upper && text.split('.').all(is_identifier)).then_some(text)
}

/// Rendering `<Foo.Bar>` references both `Foo.Bar` and the `Bar` component.
fn push_component_names(tag: &str, out: &mut Vec<String>) {
    out.push(tag.to_string());
    if let Some((_, last)) = tag.rsplit_once('.') {
        out.push(last.to_string());
    }
}

/// Push the bare name of a callee and, for `Type::name` paths, the
/// two-segment qualified form as well.
fn push_callee_names(callee: &Node, source: &[u8], out: &mut Vec<String>) {
    let kind = callee.kind();
    if kind == "scoped_identifier" || kind == "qualified_identifier" {
        let text = node_text(callee, source);
        let segments: Vec<&str> = text.split("::").map(str::trim).collect();
        if segments.len() >= 2 {
            let last_two = &segments[segments.len() - 2..];
            let owner = last_two[0];
            if COMMON_TYPES.contains(&owner) {
                return;
            }
            if !owner.is_empty() && is_identifier(owner) {
                out.push(last_two.join("::"));
            }
        }
    }
    if let Some(name) = last_name(callee, source) {
        out.push(name);
    }
}

/// Reduce a callee expression to its final identifier: `a.b.c` -> `c`,
/// `Foo::bar` -> `bar`, `make::<T>` -> `make`.
fn last_name(node: &Node, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" | "property_identifier" | "field_identifier" | "type_identifier" | "name" => {
            Some(node_text(node, source)).filter(|text| is_identifier(text))
        }
        _ => {
            for field in ["name", "field", "property", "attribute", "function", "type"] {
                if let Some(child) = node.child_by_field_name(field) {
                    return last_name(&child, source);
                }
            }
            None
        }
    }
}

fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 80
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_calls_keep_names_and_qualified_paths_and_drop_common_ones() {
        let code = "fn run(&self) { let v = Vec::new(); let s = QdrantService::new(c); s.query_items(x).await.unwrap(); helper(1); }";
        let calls = extract_calls(Language::Rust, code);
        assert_eq!(calls, vec!["QdrantService::new", "query_items", "helper"]);
    }

    #[test]
    fn typescript_calls_include_members_and_constructors() {
        let code =
            "function f() { const x = new JobTracker(); api.getFiles(); load(); x.forEach(g); }";
        let calls = extract_calls(Language::TypeScript, code);
        assert_eq!(calls, vec!["JobTracker", "getFiles", "load"]);
    }

    #[test]
    fn jsx_components_are_recorded_as_calls_and_html_tags_are_not() {
        let code = "function Page() { return (<div><Header title=\"x\" /><Tabs.Panel>{load()}</Tabs.Panel><React.Fragment /></div>); }";
        let calls = extract_calls(Language::TypeScriptTsx, code);
        assert_eq!(calls, vec!["Header", "Tabs.Panel", "Panel", "load"]);
    }

    #[test]
    fn jsx_in_plain_javascript_is_recorded() {
        let code = "const App = () => <Layout><Sidebar /></Layout>;";
        let calls = extract_calls(Language::JavaScript, code);
        assert_eq!(calls, vec!["Layout", "Sidebar"]);
    }

    #[test]
    fn python_calls_use_attribute_names() {
        let code =
            "def f(self):\n    self.client.upsert(points)\n    build_filter(x)\n    print(x)\n";
        let calls = extract_calls(Language::Python, code);
        assert_eq!(calls, vec!["upsert", "build_filter"]);
    }

    #[test]
    fn java_method_invocations_and_constructors() {
        let code = "class A { void f() { repo.save(item); Widget w = new Widget(); compute(); } }";
        let calls = extract_calls(Language::Java, code);
        assert_eq!(calls, vec!["save", "Widget", "compute"]);
    }

    #[test]
    fn duplicates_are_removed_and_sql_has_no_calls() {
        let calls = extract_calls(Language::Rust, "fn f() { go(); go(); }");
        assert_eq!(calls, vec!["go"]);
        assert!(extract_calls(Language::Sql, "SELECT count(*) FROM t").is_empty());
    }
}
