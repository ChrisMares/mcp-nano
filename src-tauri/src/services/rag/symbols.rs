//! Code symbol lookups over the `codebase` collection: definitions,
//! references (callers), callees, and members, plus callee `expand`.
//!
//! Matching is by name, using the `symbol`, `qualified_symbol`, `calls`,
//! `class_name`, and `interface_name` payload fields written at ingestion,
//! plus the chunk's `path`. A qualified name (`Type::name`, `Type.name`)
//! resolves in this order:
//!
//! 1. a member of that type (`qualified_symbol`),
//! 2. a definition of the bare name in a file or folder with that name
//!    (Rust modules such as `public::router`, or `main::main` to pick the
//!    `main` in `src/main.rs`),
//! 3. otherwise every definition of the bare name.
//!
//! Unranked lookups scan every match and return it in file order, so a
//! definition always starts at its first chunk.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use qdrant_client::qdrant::{Condition, Filter};

use super::{vec_db_to_rag_results, RagService, CODE_COLLECTION};
use crate::models::request::SymbolRelation;
use crate::models::RagResult;
use crate::services::ingestion::code_chunker::{language_for_extension, Language};

/// Most related definitions returned by `expand`.
const EXPAND_LIMIT: usize = 5;
/// Most called names followed by a callees lookup or `expand`. Large
/// functions (an app's `main`) easily make 50+ distinct calls.
const MAX_FOLLOWED_CALLS: usize = 100;
/// Most chunks scanned by an unranked lookup before it is sorted into file
/// order and cut to the requested limit.
const LOOKUP_SCAN_LIMIT: usize = 1000;
/// A bare called name with more definitions than this, none of them in the
/// caller's file or class, is treated as a library method on some object
/// (`session.query`, `list.add`) and not followed.
const MAX_AMBIGUOUS_DEFINITIONS: usize = 3;
/// Candidates fetched per requested result for a ranked lookup, so that
/// narrowing and one-chunk-per-definition still leave enough results.
const RANKED_OVERFETCH: usize = 4;
/// Paths listed in an ambiguity note.
const NOTE_MAX_PATHS: usize = 3;
/// Chunk types of C and C++ class and struct declarations. Without a body
/// they are forward declarations (`class Session;`), not definitions.
const C_TYPE_KINDS: [&str; 2] = ["class_specifier", "struct_specifier"];
/// Qualifiers that refer to the current scope rather than a module or type.
const SCOPE_KEYWORDS: [&str; 5] = ["crate", "self", "super", "Self", "this"];

/// Results of one symbol lookup plus an optional hint for the caller.
pub(super) struct Lookup {
    pub results: Vec<RagResult>,
    pub note: Option<String>,
}

/// Definitions found for a requested symbol.
struct Definitions {
    results: Vec<RagResult>,
    /// True when the qualifier picked them (type member or module/file);
    /// false when they are every definition of a bare name.
    precise: bool,
}

impl Definitions {
    /// Ambiguity note for a bare-name match; `None` when the qualifier
    /// picked the results.
    fn note(&self, symbol: &str, suffix: &str) -> Option<String> {
        if self.precise {
            None
        } else {
            ambiguity_note(symbol, &self.results, suffix)
        }
    }
}

/// Files, classes, and languages of the code whose calls are being followed.
struct CallerContext {
    paths: HashSet<String>,
    containers: HashSet<String>,
    families: HashSet<LanguageFamily>,
}

/// Languages that can call each other directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum LanguageFamily {
    CLike,
    CSharp,
    Java,
    JavaScript,
    Python,
    Rust,
    Sql,
}

/// Language family of a chunk, from its path's extension. `None` for files
/// without a grammar (docs, configs).
fn language_family(result: &RagResult) -> Option<LanguageFamily> {
    let path = chunk_location(result).0;
    let file = path.rsplit('/').next().unwrap_or(&path);
    let ext = file.rsplit_once('.')?.1.to_lowercase();
    Some(match language_for_extension(&ext)? {
        Language::C | Language::Cpp => LanguageFamily::CLike,
        Language::CSharp => LanguageFamily::CSharp,
        Language::Java => LanguageFamily::Java,
        Language::JavaScript | Language::TypeScript | Language::TypeScriptTsx => {
            LanguageFamily::JavaScript
        }
        Language::Python => LanguageFamily::Python,
        Language::Rust => LanguageFamily::Rust,
        Language::Sql => LanguageFamily::Sql,
    })
}

impl CallerContext {
    fn new(callers: &[RagResult]) -> Self {
        let mut paths = HashSet::new();
        let mut containers = HashSet::new();
        let mut families = HashSet::new();
        for caller in callers {
            paths.insert(chunk_location(caller).0);
            families.extend(language_family(caller));
            if let Some(container) = container_of(caller) {
                containers.insert(container.to_string());
            }
        }
        Self {
            paths,
            containers,
            families,
        }
    }

    /// False when `result` is in a language the callers cannot call, e.g. a
    /// Python method for a Rust caller. Unknown languages are allowed.
    fn can_call(&self, result: &RagResult) -> bool {
        if self.families.is_empty() {
            return true;
        }
        language_family(result).is_none_or(|family| self.families.contains(&family))
    }

    /// True when `result` is defined in one of the callers' files or classes.
    fn is_local(&self, result: &RagResult) -> bool {
        if self.paths.contains(&chunk_location(result).0) {
            return true;
        }
        container_of(result).is_some_and(|c| self.containers.contains(c))
    }
}

impl RagService {
    pub(super) async fn symbol_lookup(
        &self,
        symbol: &str,
        relation: SymbolRelation,
        query: &str,
        scope: Option<Filter>,
        limit: usize,
    ) -> Result<Lookup> {
        match relation {
            SymbolRelation::Definition => {
                let found = self
                    .find_definitions(symbol, scope, query, limit * RANKED_OVERFETCH)
                    .await?;
                let note = found.note(symbol, "");
                let mut results = if query.is_empty() {
                    type_files_first(found.results, split_symbol(symbol).1)
                } else {
                    found.results
                };
                results.truncate(limit);
                Ok(Lookup { results, note })
            }
            SymbolRelation::References => {
                let filter = Filter::must([Condition::matches("calls", reference_names(symbol))]);
                let mut results = self.fetch(with_scope(scope, filter), query, limit).await?;
                results.truncate(limit);
                Ok(Lookup {
                    results,
                    note: None,
                })
            }
            SymbolRelation::Callees => {
                let found = self.find_definitions(symbol, scope.clone(), "", 0).await?;
                let note = found.note(symbol, " The callees of all of them are combined.");
                let results = self
                    .callee_definitions(&found.results, scope, query, limit)
                    .await?;
                Ok(Lookup { results, note })
            }
            SymbolRelation::Members => self.members(symbol, scope, query, limit).await,
        }
    }

    /// Definitions of the functions called by `hits`, capped at
    /// [`EXPAND_LIMIT`].
    pub(super) async fn expand_callees(
        &self,
        hits: &[RagResult],
        scope: Option<Filter>,
    ) -> Result<Vec<RagResult>> {
        self.callee_definitions(hits, scope, "", EXPAND_LIMIT).await
    }

    /// Every match in file order when `query` is empty, otherwise the top
    /// `ranked_n` hybrid-search hits.
    async fn fetch(&self, filter: Filter, query: &str, ranked_n: usize) -> Result<Vec<RagResult>> {
        if query.is_empty() {
            self.scan_matching(filter).await
        } else {
            let results = self
                .semantic_search(CODE_COLLECTION, query, Some(filter), ranked_n)
                .await?;
            Ok(without_forward_declarations(results))
        }
    }

    /// Every chunk matching `filter` (up to [`LOOKUP_SCAN_LIMIT`]), in file
    /// order.
    async fn scan_matching(&self, filter: Filter) -> Result<Vec<RagResult>> {
        let vec_result = self
            .qdrant
            .scroll_items(CODE_COLLECTION, filter, LOOKUP_SCAN_LIMIT)
            .await
            .context("symbol lookup")?;
        let mut results = without_forward_declarations(vec_db_to_rag_results(&vec_result));
        results.sort_by_key(chunk_location);
        Ok(results)
    }

    /// Definitions of `symbol`, resolved as described in the module docs.
    async fn find_definitions(
        &self,
        symbol: &str,
        scope: Option<Filter>,
        query: &str,
        ranked_n: usize,
    ) -> Result<Definitions> {
        let (container, bare) = split_symbol(symbol);
        if container.is_some() {
            let exact = Filter::must([Condition::matches(
                "qualified_symbol",
                qualified_variants(symbol),
            )]);
            let results = self
                .fetch(with_scope(scope.clone(), exact), query, ranked_n)
                .await?;
            if !results.is_empty() {
                return Ok(Definitions {
                    results,
                    precise: true,
                });
            }
        }
        let results = self
            .fetch(
                with_scope(scope, name_filter(&[bare.to_string()])),
                query,
                ranked_n,
            )
            .await?;
        if let Some(module) = container {
            let narrowed: Vec<RagResult> =
                in_module(&results, module).into_iter().cloned().collect();
            if !narrowed.is_empty() {
                return Ok(Definitions {
                    results: narrowed,
                    precise: true,
                });
            }
        }
        Ok(Definitions {
            results,
            precise: false,
        })
    }

    /// First chunk of each definition called by `callers`, in the order the
    /// calls are made (ranked by `query` instead when one is given).
    async fn callee_definitions(
        &self,
        callers: &[RagResult],
        scope: Option<Filter>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RagResult>> {
        let callees = called_names(callers);
        if callees.is_empty() {
            return Ok(Vec::new());
        }
        let mut names: Vec<String> = Vec::new();
        for call in &callees {
            for form in reference_names(call) {
                push_unique(&mut names, form);
            }
        }
        let candidates = first_chunk_per_symbol(
            self.scan_matching(with_scope(scope, name_filter(&names)))
                .await?,
        );
        let resolved = resolve_callees(&candidates, &callees, &CallerContext::new(callers));
        if query.is_empty() {
            let mut resolved = resolved;
            resolved.truncate(limit);
            Ok(resolved)
        } else {
            self.rerank_results(query, resolved, limit, false)
        }
    }

    /// The type and its members. When one file holds the type's body (a C++
    /// class declared in a header, defined in a `.cpp`), every member is
    /// listed on that declaration, which comes first, followed by the other
    /// files in file order. When several files declare a type of that name
    /// (separate C# controllers), members are grouped by file with the file
    /// that defines the most members first, and each file's type chunk
    /// lists only its own members.
    async fn members(
        &self,
        symbol: &str,
        scope: Option<Filter>,
        query: &str,
        limit: usize,
    ) -> Result<Lookup> {
        let filter = with_scope(scope, members_filter(symbol));
        let all = first_chunk_per_symbol(self.scan_matching(filter.clone()).await?);
        let type_name = split_symbol(symbol).1;
        let mut lists = member_lists(&all, type_name);
        let bodies = type_paths(&all, type_name);
        let merged = bodies.len() == 1;
        if merged {
            lists = merge_member_lists(&all, &lists, &bodies[0]);
        }
        let paths = distinct_paths(&all);
        let mut results = if query.is_empty() {
            group_by_member_count(all, &lists)
        } else {
            first_chunk_per_symbol(self.fetch(filter, query, limit * RANKED_OVERFETCH).await?)
        };
        results.truncate(limit);
        attach_member_lists(&mut results, &lists, type_name);
        let note = if !merged && paths.len() > 1 {
            Some(format!(
                "'{symbol}' is defined in {} files; members are grouped by file ({}).",
                paths.len(),
                path_summary(&paths)
            ))
        } else {
            None
        };
        Ok(Lookup { results, note })
    }
}

/// Split `Type::name` / `Type.name` at the last separator into the last
/// container segment and the name. Unqualified input, and qualifiers such as
/// `self` or `crate`, have no container.
fn split_symbol(symbol: &str) -> (Option<&str>, &str) {
    let colon = symbol.rfind("::").map(|i| (i, 2));
    let dot = symbol.rfind('.').map(|i| (i, 1));
    let split = match (colon, dot) {
        (Some(c), Some(d)) => Some(if c.0 > d.0 { c } else { d }),
        (c, d) => c.or(d),
    };
    let Some((index, len)) = split else {
        return (None, symbol);
    };
    if index == 0 || index + len >= symbol.len() {
        return (None, symbol);
    }
    let name = &symbol[index + len..];
    let container = &symbol[..index];
    let container = container.rsplit("::").next().unwrap_or(container);
    let container = container.rsplit('.').next().unwrap_or(container);
    if SCOPE_KEYWORDS.contains(&container) {
        return (None, name);
    }
    (Some(container), name)
}

/// Forms a qualified member can be stored as: `qualified_symbol` uses `::`
/// for Rust/C++ and `.` elsewhere, so both are tried. Empty when unqualified.
fn qualified_variants(symbol: &str) -> Vec<String> {
    let (Some(container), name) = split_symbol(symbol) else {
        return Vec::new();
    };
    let mut variants = Vec::new();
    for candidate in [
        symbol.to_string(),
        format!("{container}::{name}"),
        format!("{container}.{name}"),
    ] {
        push_unique(&mut variants, candidate);
    }
    variants
}

/// Append `item` unless `list` already holds it.
fn push_unique(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

/// Chunks whose `symbol` or `qualified_symbol` is one of `names`.
fn name_filter(names: &[String]) -> Filter {
    Filter::should([
        Condition::matches("symbol", names.to_vec()),
        Condition::matches("qualified_symbol", names.to_vec()),
    ])
}

/// Names a call site of `symbol` can be recorded under. Member calls
/// (`obj.name()`) only store the bare name, so it is always included.
fn reference_names(symbol: &str) -> Vec<String> {
    let mut names = qualified_variants(symbol);
    push_unique(&mut names, symbol.to_string());
    push_unique(&mut names, split_symbol(symbol).1.to_string());
    names
}

/// The type itself and every member defined inside it.
fn members_filter(symbol: &str) -> Filter {
    let type_name = split_symbol(symbol).1.to_string();
    Filter::should([
        Condition::matches("class_name", type_name.clone()),
        Condition::matches("interface_name", type_name),
    ])
}

/// Combine an optional tool/UI scope filter with a lookup filter.
fn with_scope(scope: Option<Filter>, lookup: Filter) -> Filter {
    match scope {
        Some(scope) => Filter::must([Condition::from(scope), Condition::from(lookup)]),
        None => lookup,
    }
}

fn meta_str<'a>(result: &'a RagResult, key: &str) -> Option<&'a str> {
    result
        .metadata
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

/// `qualified_symbol`, else `symbol`, of a chunk.
fn defined_name(result: &RagResult) -> Option<&str> {
    meta_str(result, "qualified_symbol").or_else(|| meta_str(result, "symbol"))
}

/// The type or module a chunk's `qualified_symbol` places it in.
fn container_of(result: &RagResult) -> Option<&str> {
    meta_str(result, "qualified_symbol").and_then(|q| split_symbol(q).0)
}

/// File name without its extension.
fn file_stem(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

/// The strings of a list field (`calls`, `members`) of a chunk.
pub fn metadata_strings<'a>(result: &'a RagResult, key: &str) -> Vec<&'a str> {
    let mut values = Vec::new();
    if let Some(list) = result.metadata.get(key).and_then(|v| v.as_array()) {
        for value in list {
            if let Some(text) = value.as_str() {
                values.push(text);
            }
        }
    }
    values
}

/// `calls` without bare names whose qualified form is also listed: a call to
/// `Fl::copy` is stored as both `Fl::copy` and `copy`, and only the qualified
/// one should be shown or followed.
pub fn distinct_calls<'a>(calls: &[&'a str]) -> Vec<&'a str> {
    let mut covered: HashSet<&str> = HashSet::new();
    for call in calls {
        if let (Some(_), name) = split_symbol(call) {
            covered.insert(name);
        }
    }
    let mut out = Vec::with_capacity(calls.len());
    for call in calls {
        let qualified = split_symbol(call).0.is_some();
        if qualified || !covered.contains(call) {
            out.push(*call);
        }
    }
    out
}

/// Union of the distinct calls of `results`, in first-seen order, excluding
/// the results' own symbols and capped at [`MAX_FOLLOWED_CALLS`].
fn called_names(results: &[RagResult]) -> Vec<String> {
    let mut own: HashSet<&str> = HashSet::new();
    for r in results {
        for key in ["symbol", "qualified_symbol"] {
            if let Some(name) = meta_str(r, key) {
                own.insert(name);
            }
        }
    }
    let mut seen: HashSet<&str> = HashSet::new();
    let mut names: Vec<String> = Vec::new();
    for r in results {
        for name in distinct_calls(&metadata_strings(r, "calls")) {
            if names.len() >= MAX_FOLLOWED_CALLS {
                return names;
            }
            if !own.contains(name) && seen.insert(name) {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// `(path, chunk_index)` of a chunk, used for file ordering. Chunks indexed
/// before `path` was recorded fall back to the bare `file_name`.
fn chunk_location(result: &RagResult) -> (String, i64) {
    let path = meta_str(result, "path")
        .or_else(|| meta_str(result, "file_name"))
        .unwrap_or("")
        .to_string();
    let index = result
        .metadata
        .get("chunk_index")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    (path, index)
}

/// Distinct paths of `results`, in first-seen order.
fn distinct_paths<'a>(results: impl IntoIterator<Item = &'a RagResult>) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for r in results {
        push_unique(&mut paths, chunk_location(r).0);
    }
    paths
}

/// True when a folder of `path`, or the file name without extension, is
/// `module`: `src/routes/public.rs` is in `public` and in `routes`. Matching
/// is case-sensitive so `Fl::` does not match the `FL/` folder.
fn path_in_module(path: &str, module: &str) -> bool {
    for segment in path.split('/') {
        if segment == module || file_stem(segment) == module {
            return true;
        }
    }
    false
}

/// File stems that are the root file of the folder they are in.
const MODULE_ROOT_STEMS: [&str; 3] = ["mod", "index", "__init__"];

/// How closely `path` is the module `module`: 3 for a file named after it
/// (`public.rs`), 2 for the root file of a folder named after it
/// (`routes/mod.rs`), 1 for any other file in such a folder, 0 otherwise.
fn module_match(path: &str, module: &str) -> u8 {
    let segments: Vec<&str> = path.split('/').collect();
    let Some((file, folders)) = segments.split_last() else {
        return 0;
    };
    let stem = file_stem(file);
    if stem == module {
        return 3;
    }
    let parent_matches = folders.last().is_some_and(|f| *f == module);
    if parent_matches && MODULE_ROOT_STEMS.contains(&stem) {
        return 2;
    }
    if folders.contains(&module) {
        return 1;
    }
    0
}

/// Results defined in the module `module`, keeping only the closest match
/// tier: `routes::router` prefers `routes.rs` or `routes/mod.rs` over other
/// files under `routes/`.
fn in_module<'a, I>(results: I, module: &str) -> Vec<&'a RagResult>
where
    I: IntoIterator<Item = &'a RagResult>,
{
    let mut best = 0u8;
    let mut out: Vec<&RagResult> = Vec::new();
    for r in results {
        let tier = module_match(&chunk_location(r).0, module);
        if tier == 0 || tier < best {
            continue;
        }
        if tier > best {
            best = tier;
            out.clear();
        }
        out.push(r);
    }
    out
}

/// True for a C/C++ `class X;` / `struct X;` chunk: a declaration with no
/// body. A continuation chunk of a large class still has `;` or braces.
fn is_forward_declaration(result: &RagResult) -> bool {
    let is_type = meta_str(result, "type").is_some_and(|t| C_TYPE_KINDS.contains(&t));
    let code = meta_str(result, "code").unwrap_or(&result.document);
    is_type && !code.contains(['{', '}', ';'])
}

fn without_forward_declarations(mut results: Vec<RagResult>) -> Vec<RagResult> {
    results.retain(|r| !is_forward_declaration(r));
    results
}

/// True for the chunk declaring the type `type_name` itself, as opposed to
/// one of its members (a constructor `T::T` is a member) or a free function
/// of the same name.
fn is_type_chunk(result: &RagResult, type_name: &str) -> bool {
    let declares = meta_str(result, "class_name") == Some(type_name)
        || meta_str(result, "interface_name") == Some(type_name);
    if !declares || meta_str(result, "symbol") != Some(type_name) {
        return false;
    }
    container_of(result).is_none()
}

/// Distinct paths of the chunks that declare the type `type_name`.
fn type_paths(results: &[RagResult], type_name: &str) -> Vec<String> {
    distinct_paths(results.iter().filter(|r| is_type_chunk(r, type_name)))
}

/// Move the files that declare the type `name` ahead of the others (a C++
/// class header before its constructors in the `.cpp`), keeping file order
/// otherwise.
fn type_files_first(results: Vec<RagResult>, name: &str) -> Vec<RagResult> {
    let types = type_paths(&results, name);
    let (mut first, rest): (Vec<RagResult>, Vec<RagResult>) = results
        .into_iter()
        .partition(|r| types.contains(&chunk_location(r).0));
    first.extend(rest);
    first
}

/// Keep the first chunk of each `(path, symbol)` definition, preserving
/// order. Chunks without a symbol are kept as-is.
fn first_chunk_per_symbol(results: Vec<RagResult>) -> Vec<RagResult> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut out = Vec::with_capacity(results.len());
    for r in results {
        if let Some(symbol) = defined_name(&r) {
            let key = (chunk_location(&r).0, symbol.to_string());
            if !seen.insert(key) {
                continue;
            }
        }
        out.push(r);
    }
    out
}

/// Definitions among `candidates` for one called name:
///
/// * qualified (`Type::name`): the member of that type, else the bare name
///   defined in a file or folder named `Type` (Rust modules), else nothing;
/// * bare: the definitions in the caller's file or class when there are
///   any, otherwise all of them unless there are more than
///   [`MAX_AMBIGUOUS_DEFINITIONS`].
fn resolve_call<'a>(
    candidates: &'a [RagResult],
    call: &str,
    callers: &CallerContext,
) -> Vec<&'a RagResult> {
    let (container, bare) = split_symbol(call);
    let callable: Vec<&RagResult> = candidates.iter().filter(|r| callers.can_call(r)).collect();
    if let Some(container) = container {
        let variants = qualified_variants(call);
        let mut exact = Vec::new();
        let mut same_name = Vec::new();
        for r in &callable {
            if meta_str(r, "qualified_symbol").is_some_and(|q| variants.iter().any(|v| v == q)) {
                exact.push(*r);
            } else if meta_str(r, "symbol") == Some(bare) {
                same_name.push(*r);
            }
        }
        return if exact.is_empty() {
            in_module(same_name, container)
        } else {
            exact
        };
    }

    let mut defs = Vec::new();
    for r in callable {
        if meta_str(r, "symbol") == Some(bare) || meta_str(r, "qualified_symbol") == Some(bare) {
            defs.push(r);
        }
    }
    let local: Vec<&RagResult> = defs
        .iter()
        .copied()
        .filter(|r| callers.is_local(r))
        .collect();
    if !local.is_empty() {
        return local;
    }
    if defs.len() > MAX_AMBIGUOUS_DEFINITIONS {
        return Vec::new();
    }
    defs
}

/// Resolve every called name in call order, without duplicates.
fn resolve_callees(
    candidates: &[RagResult],
    callees: &[String],
    callers: &CallerContext,
) -> Vec<RagResult> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    for call in callees {
        for r in resolve_call(candidates, call, callers) {
            if seen.insert(r.id.as_str()) {
                out.push(r.clone());
            }
        }
    }
    out
}

/// Bare names of the members defined in each file, excluding the type.
fn member_lists(all: &[RagResult], type_name: &str) -> HashMap<String, Vec<String>> {
    let mut lists: HashMap<String, Vec<String>> = HashMap::new();
    for r in all {
        let Some(defined) = defined_name(r) else {
            continue;
        };
        let name = split_symbol(defined).1;
        if name != type_name {
            let list = lists.entry(chunk_location(r).0).or_default();
            push_unique(list, name.to_string());
        }
    }
    lists
}

/// All members of `all`, the declaring file's first and then the other
/// files' in file order, as one list keyed by `type_path`.
fn merge_member_lists(
    all: &[RagResult],
    lists: &HashMap<String, Vec<String>>,
    type_path: &str,
) -> HashMap<String, Vec<String>> {
    let mut paths = vec![type_path.to_string()];
    for path in distinct_paths(all) {
        if path != type_path {
            paths.push(path);
        }
    }
    let mut merged: Vec<String> = Vec::new();
    for path in &paths {
        for name in lists.get(path).into_iter().flatten() {
            push_unique(&mut merged, name.clone());
        }
    }
    HashMap::from([(type_path.to_string(), merged)])
}

/// Reorder file-ordered results so the file with the most members comes
/// first (ties keep file order); a stray same-named symbol elsewhere sorts
/// last.
fn group_by_member_count(
    all: Vec<RagResult>,
    lists: &HashMap<String, Vec<String>>,
) -> Vec<RagResult> {
    let paths = distinct_paths(&all);
    let mut order: Vec<(usize, usize)> = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let count = lists.get(path).map(Vec::len).unwrap_or(0);
        order.push((index, count));
    }
    order.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut grouped = Vec::with_capacity(all.len());
    for (index, _) in order {
        let path = &paths[index];
        for r in &all {
            if &chunk_location(r).0 == path {
                grouped.push(r.clone());
            }
        }
    }
    grouped
}

/// Put each file's member list on that file's type chunk (or its first
/// result) as `members`.
fn attach_member_lists(
    results: &mut [RagResult],
    lists: &HashMap<String, Vec<String>>,
    type_name: &str,
) {
    let mut targets: HashMap<String, usize> = HashMap::new();
    for (index, r) in results.iter().enumerate() {
        let path = chunk_location(r).0;
        let is_type = is_type_chunk(r, type_name);
        match targets.get(&path) {
            Some(&existing) if !is_type || is_type_chunk(&results[existing], type_name) => {}
            _ => {
                targets.insert(path, index);
            }
        }
    }
    for (path, index) in targets {
        let Some(names) = lists.get(&path).filter(|n| !n.is_empty()) else {
            continue;
        };
        if let Some(map) = results[index].metadata.as_object_mut() {
            map.insert("members".into(), serde_json::json!(names));
        }
    }
}

/// Up to [`NOTE_MAX_PATHS`] paths, comma-separated, with `…` when cut.
fn path_summary(paths: &[String]) -> String {
    let mut text = paths[..paths.len().min(NOTE_MAX_PATHS)].join(", ");
    if paths.len() > NOTE_MAX_PATHS {
        text.push_str(", …");
    }
    text
}

/// A folder or file name that tells `path` apart from `others`: the file
/// stem when it is unique, otherwise the nearest unique parent folder.
fn distinguishing_segment<'a>(path: &'a str, others: &[String]) -> &'a str {
    let segments: Vec<&str> = path.split('/').collect();
    let mut candidates: Vec<&str> = Vec::new();
    if let Some(file) = segments.last() {
        candidates.push(file_stem(file));
    }
    for segment in segments.iter().rev().skip(1) {
        candidates.push(segment);
    }
    for candidate in &candidates {
        let shared = others
            .iter()
            .filter(|other| other.as_str() != path)
            .any(|other| path_in_module(other, candidate));
        if !shared {
            return candidate;
        }
    }
    candidates.first().copied().unwrap_or(path)
}

/// True for `T::T` / `T.T`: part of the type, not another definition of
/// its name.
fn is_constructor(result: &RagResult) -> bool {
    let Some(qualified) = meta_str(result, "qualified_symbol") else {
        return false;
    };
    let (container, name) = split_symbol(qualified);
    container == Some(name)
}

/// Hint for a bare name defined in several files, telling the caller how to
/// qualify it. `None` when the matches are all in one file.
fn ambiguity_note(symbol: &str, results: &[RagResult], suffix: &str) -> Option<String> {
    let definitions = results
        .iter()
        .filter(|r| meta_str(r, "symbol").is_some() && !is_constructor(r));
    let paths = distinct_paths(definitions);
    if paths.len() < 2 {
        return None;
    }
    let bare = split_symbol(symbol).1;
    let example = distinguishing_segment(&paths[0], &paths);
    Some(format!(
        "'{symbol}' is defined in {} files ({}). Qualify it with a type, file, or folder name to pick one, e.g. '{example}::{bare}'.{suffix}",
        paths.len(),
        path_summary(&paths)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(path: &str, symbol: Option<&str>, index: i64) -> RagResult {
        let mut metadata = serde_json::json!({ "path": path, "chunk_index": index });
        if let Some(symbol) = symbol {
            metadata["symbol"] = serde_json::json!(symbol);
        }
        RagResult {
            id: format!("{path}-{index}"),
            metadata,
            ..Default::default()
        }
    }

    fn qualified(path: &str, symbol: &str, qualified: &str, index: i64) -> RagResult {
        let mut r = chunk(path, Some(symbol), index);
        r.metadata["qualified_symbol"] = serde_json::json!(qualified);
        r
    }

    fn ids(results: Vec<RagResult>) -> Vec<String> {
        results.into_iter().map(|r| r.id).collect()
    }

    fn no_callers() -> CallerContext {
        CallerContext::new(&[])
    }

    #[test]
    fn split_symbol_uses_last_separator_and_ignores_scope_keywords() {
        assert_eq!(split_symbol("run"), (None, "run"));
        assert_eq!(split_symbol("Svc::run"), (Some("Svc"), "run"));
        assert_eq!(split_symbol("ns::Svc::run"), (Some("Svc"), "run"));
        assert_eq!(split_symbol("App.Svc.Run"), (Some("Svc"), "Run"));
        assert_eq!(split_symbol("::run"), (None, "::run"));
        assert_eq!(split_symbol("self::helper"), (None, "helper"));
        assert_eq!(split_symbol("crate::db::open"), (Some("db"), "open"));
    }

    #[test]
    fn qualified_variants_cover_both_separators() {
        assert!(qualified_variants("run").is_empty());
        assert_eq!(qualified_variants("Ctl::Get"), vec!["Ctl::Get", "Ctl.Get"]);
        assert_eq!(qualified_variants("Ctl.Get"), vec!["Ctl.Get", "Ctl::Get"]);
    }

    #[test]
    fn reference_names_include_qualified_forms_and_bare_name() {
        assert_eq!(
            reference_names("Fl_Widget::damage"),
            vec!["Fl_Widget::damage", "Fl_Widget.damage", "damage"]
        );
        assert_eq!(reference_names("damage"), vec!["damage"]);
    }

    #[test]
    fn distinct_calls_drops_bare_names_covered_by_qualified_ones() {
        let calls = ["Fl::copy", "copy", "event_x", "Fl::event_x", "draw"];
        assert_eq!(
            distinct_calls(&calls),
            vec!["Fl::copy", "Fl::event_x", "draw"]
        );
    }

    #[test]
    fn called_names_follow_distinct_calls_and_skip_own_symbols() {
        let mut a = chunk("a.c", Some("a"), 0);
        a.metadata["calls"] = serde_json::json!(["Fl::copy", "copy", "b"]);
        let mut b = chunk("a.c", Some("b"), 1);
        b.metadata["calls"] = serde_json::json!(["c"]);
        assert_eq!(called_names(&[a, b]), vec!["Fl::copy", "c"]);
    }

    #[test]
    fn path_in_module_matches_folders_and_file_stems() {
        assert!(path_in_module("src/routes/public.rs", "public"));
        assert!(path_in_module("src/routes/public.rs", "routes"));
        assert!(path_in_module("src/main.rs", "main"));
        assert!(path_in_module("static/js/scorecard.js", "scorecard"));
        assert!(!path_in_module("FL/Fl_Widget.H", "Fl"));
        assert!(!path_in_module("src/routes/admin.rs", "public"));
    }

    #[test]
    fn module_match_prefers_file_then_folder_root_then_folder() {
        assert_eq!(module_match("src/routes/public.rs", "public"), 3);
        assert_eq!(module_match("src/routes/mod.rs", "routes"), 2);
        assert_eq!(module_match("src/routes/admin.rs", "routes"), 1);
        assert_eq!(module_match("src/main.rs", "main"), 3);
        assert_eq!(module_match("src/routes/admin.rs", "public"), 0);

        let results = vec![
            chunk("src/routes/admin.rs", Some("router"), 0),
            chunk("src/routes/mod.rs", Some("router"), 0),
            chunk("src/routes/public.rs", Some("router"), 0),
        ];
        let picked: Vec<&str> = in_module(&results, "routes")
            .into_iter()
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(picked, vec!["src/routes/mod.rs-0"]);
    }

    #[test]
    fn callees_stay_in_the_callers_language_family() {
        let caller = chunk("src/main.rs", Some("main"), 0);
        let callers = CallerContext::new(&[caller]);
        let candidates = vec![
            qualified("scripts/build.py", "run", "Builder.run", 1),
            chunk("src/server.rs", Some("run"), 2),
        ];
        let resolved = ids(resolve_callees(&candidates, &["run".to_string()], &callers));
        assert_eq!(resolved, vec!["src/server.rs-2"]);

        let tsx = CallerContext::new(&[chunk("ui/App.tsx", Some("App"), 0)]);
        assert!(tsx.can_call(&chunk("ui/api.js", Some("get"), 0)));
        assert!(!tsx.can_call(&chunk("api/app.py", Some("get"), 0)));
        assert!(tsx.can_call(&chunk("notes.md", None, 0)));
    }

    #[test]
    fn qualified_call_resolves_to_member_then_module_then_nothing() {
        let candidates = vec![
            chunk("src/routes/admin.rs", Some("router"), 0),
            chunk("src/routes/public.rs", Some("router"), 0),
            qualified("src/Fl.cxx", "copy", "Fl::copy", 3),
            chunk("src/other.cxx", Some("copy"), 1),
            chunk("src/lib.rs", Some("new"), 1),
        ];
        let resolve = |call: &str| -> Vec<String> {
            resolve_call(&candidates, call, &no_callers())
                .into_iter()
                .map(|r| r.id.clone())
                .collect()
        };
        assert_eq!(resolve("public::router"), vec!["src/routes/public.rs-0"]);
        assert_eq!(resolve("Fl::copy"), vec!["src/Fl.cxx-3"]);
        assert!(resolve("Router::new").is_empty());
    }

    #[test]
    fn bare_call_prefers_caller_file_or_class_and_drops_widely_defined_names() {
        let caller = qualified("src/Ctl.cs", "Get", "Ctl.Get", 0);
        let callers = CallerContext::new(&[caller]);
        let mut candidates = vec![
            qualified("src/Other.cs", "Helper", "Other.Helper", 1),
            qualified("src/Ctl.Partial.cs", "Helper", "Ctl.Helper", 2),
        ];
        for i in 0..4 {
            candidates.push(qualified(
                &format!("m{i}.py"),
                "query",
                &format!("C{i}.query"),
                1,
            ));
        }
        candidates.push(chunk("src/util.cs", Some("Format"), 1));
        let resolved = ids(resolve_callees(
            &candidates,
            &[
                "Format".to_string(),
                "Helper".to_string(),
                "query".to_string(),
            ],
            &callers,
        ));
        assert_eq!(resolved, vec!["src/util.cs-1", "src/Ctl.Partial.cs-2"]);
    }

    #[test]
    fn file_order_uses_path_so_same_named_files_do_not_interleave() {
        let mut results = vec![
            chunk("b/Ctl.cs", None, 0),
            chunk("a/Ctl.cs", None, 1),
            chunk("a/Ctl.cs", None, 0),
        ];
        results.sort_by_key(chunk_location);
        assert_eq!(ids(results), vec!["a/Ctl.cs-0", "a/Ctl.cs-1", "b/Ctl.cs-0"]);
    }

    #[test]
    fn chunk_location_falls_back_to_file_name() {
        let r = RagResult {
            metadata: serde_json::json!({ "file_name": "x.rs", "chunk_index": 3 }),
            ..Default::default()
        };
        assert_eq!(chunk_location(&r), ("x.rs".to_string(), 3));
    }

    #[test]
    fn first_chunk_per_symbol_keeps_one_chunk_per_definition() {
        let results = vec![
            chunk("a.c", Some("parse"), 3),
            chunk("a.c", Some("parse"), 4),
            chunk("a.c", Some("load"), 9),
            chunk("b.c", Some("parse"), 1),
            chunk("a.c", None, 12),
        ];
        assert_eq!(
            ids(first_chunk_per_symbol(results)),
            vec!["a.c-3", "a.c-9", "b.c-1", "a.c-12"]
        );
    }

    #[test]
    fn members_are_listed_per_file_and_largest_file_comes_first() {
        let all = vec![
            chunk("panels/codeview.cxx", Some("Ctl"), 0),
            qualified("api/Mobile/Ctl.cs", "Ctl", "Ctl", 0),
            qualified("api/Mobile/Ctl.cs", "Get", "Ctl.Get", 1),
            qualified("api/OData/Ctl.cs", "Ctl", "Ctl", 0),
            qualified("api/OData/Ctl.cs", "Get", "Ctl.Get", 1),
            qualified("api/OData/Ctl.cs", "GetMinimal", "Ctl.GetMinimal", 2),
        ];
        let lists = member_lists(&all, "Ctl");
        assert_eq!(lists["api/Mobile/Ctl.cs"], vec!["Get"]);
        assert_eq!(lists["api/OData/Ctl.cs"], vec!["Get", "GetMinimal"]);

        let mut grouped = group_by_member_count(all, &lists);
        assert_eq!(
            ids(grouped.clone()),
            vec![
                "api/OData/Ctl.cs-0",
                "api/OData/Ctl.cs-1",
                "api/OData/Ctl.cs-2",
                "api/Mobile/Ctl.cs-0",
                "api/Mobile/Ctl.cs-1",
                "panels/codeview.cxx-0",
            ]
        );
        attach_member_lists(&mut grouped, &lists, "Ctl");
        assert_eq!(
            grouped[0].metadata["members"],
            serde_json::json!(["Get", "GetMinimal"])
        );
        assert_eq!(grouped[3].metadata["members"], serde_json::json!(["Get"]));
        assert!(grouped[5].metadata.get("members").is_none());
    }

    fn typed(path: &str, symbol: &str, kind: &str, code: &str, index: i64) -> RagResult {
        let mut r = chunk(path, Some(symbol), index);
        r.metadata["type"] = serde_json::json!(kind);
        r.metadata["code"] = serde_json::json!(code);
        r
    }

    #[test]
    fn forward_declarations_are_dropped_but_class_bodies_are_kept() {
        let results = vec![
            typed("fwd.h", "Session", "class_specifier", "class Session", 0),
            typed(
                "s.h",
                "Session",
                "class_specifier",
                "class Session : public Base {",
                0,
            ),
            typed(
                "s.h",
                "Session",
                "class_specifier",
                "  int port_;\n  bool ok_;",
                1,
            ),
            typed("x.h", "stat", "struct_specifier", "struct stat", 0),
            typed("s.cpp", "run", "function_definition", "run", 0),
        ];
        assert_eq!(
            ids(without_forward_declarations(results)),
            vec!["s.h-0", "s.h-1", "s.cpp-0"]
        );
    }

    fn class(path: &str, name: &str, index: i64) -> RagResult {
        let mut r = chunk(path, Some(name), index);
        r.metadata["class_name"] = serde_json::json!(name);
        r
    }

    #[test]
    fn definitions_put_the_type_declaration_before_its_constructors() {
        let results = vec![
            qualified("src/session.cpp", "Session", "Session::Session", 4),
            class("src/session.h", "Session", 1),
        ];
        assert_eq!(
            ids(type_files_first(results.clone(), "Session")),
            vec!["src/session.h-1", "src/session.cpp-4"]
        );
        assert!(ambiguity_note("Session", &results, "").is_none());

        let functions = vec![
            qualified("src/svc.rs", "run", "Svc::run", 0),
            chunk("src/tool.rs", Some("run"), 0),
        ];
        assert_eq!(
            ids(type_files_first(functions, "run")),
            vec!["src/svc.rs-0", "src/tool.rs-0"]
        );
    }

    #[test]
    fn cpp_members_in_other_files_are_merged_onto_the_one_declaration() {
        let all = vec![
            class("FL/Fl_Button.H", "Fl_Button", 0),
            qualified("FL/Fl_Button.H", "down_box", "Fl_Button::down_box", 1),
            qualified("src/Fl_Button.cxx", "value", "Fl_Button::value", 0),
            qualified("src/Fl_Button.cxx", "handle", "Fl_Button::handle", 1),
            qualified("src/Fl_Button.cxx", "Fl_Button", "Fl_Button::Fl_Button", 2),
        ];
        let bodies = type_paths(&all, "Fl_Button");
        assert_eq!(bodies, vec!["FL/Fl_Button.H"]);
        let lists = merge_member_lists(&all, &member_lists(&all, "Fl_Button"), &bodies[0]);
        let mut grouped = group_by_member_count(all, &lists);
        assert_eq!(
            ids(grouped.clone()),
            vec![
                "FL/Fl_Button.H-0",
                "FL/Fl_Button.H-1",
                "src/Fl_Button.cxx-0",
                "src/Fl_Button.cxx-1",
                "src/Fl_Button.cxx-2",
            ]
        );
        attach_member_lists(&mut grouped, &lists, "Fl_Button");
        assert_eq!(
            grouped[0].metadata["members"],
            serde_json::json!(["down_box", "value", "handle"])
        );
        assert!(grouped[2].metadata.get("members").is_none());
    }

    #[test]
    fn ambiguity_note_suggests_a_distinguishing_qualifier() {
        let results = vec![
            chunk("src/main.rs", Some("main"), 1),
            chunk("build.rs", Some("main"), 0),
            chunk("scripts/load.py", Some("main"), 2),
        ];
        let note = ambiguity_note("main", &results, "").unwrap();
        assert!(note.contains("3 files"));
        assert!(note.contains("e.g. 'main::main'"), "{note}");

        let same_stem = vec![
            chunk("api/Mobile/Ctl.cs", Some("Ctl"), 0),
            chunk("api/OData/Ctl.cs", Some("Ctl"), 0),
        ];
        let note = ambiguity_note("Ctl", &same_stem, "").unwrap();
        assert!(note.contains("e.g. 'Mobile::Ctl'"), "{note}");

        assert!(ambiguity_note("x", &[chunk("a.rs", Some("x"), 0)], "").is_none());
    }

    #[test]
    fn members_filter_matches_class_or_interface_by_last_segment() {
        let filter = members_filter("Api.CustomerController");
        assert_eq!(filter.should.len(), 2);
    }

    #[test]
    fn with_scope_wraps_both_filters_in_must() {
        let scope = Filter::must([Condition::matches("repo_name", "r".to_string())]);
        let combined = with_scope(Some(scope), name_filter(&["x".to_string()]));
        assert_eq!(combined.must.len(), 2);
        let unscoped = with_scope(None, name_filter(&["x".to_string()]));
        assert_eq!(unscoped.should.len(), 2);
    }
}
