//! File filtering: the single source of truth for which files get ingested.
//!
//! Every rule here is a **denylist** so files with unknown or uncommon
//! extensions are still embedded (a language without a dedicated code chunker
//! falls back to the generic text splitter). Four independent rule sets are
//! composed by [`IngestFilter`]:
//!
//! * [`is_junk`] — binaries, assets, generated/minified files and build/vendor
//!   directories that are never worth embedding.
//! * [`is_oversized_config_file`] — config-family files (`.json`, `.xml`,
//!   `.yaml`, …) too large to be a hand-written config, i.e. data dumps wearing
//!   a config extension. Always applied.
//! * [`is_test_path`] — test/spec files and directories (applied only when the
//!   caller enables it).
//! * [`is_code_only_ignored`] — data/document/binary types excluded when the
//!   "Code files only" upload option is enabled.
//!
//! The ingestion pipeline calls [`IngestFilter::allows`] once per file.

use std::path::Path;

// ---------------------------------------------------------------------------
// Junk: never worth embedding (applies to every upload).
// ---------------------------------------------------------------------------

/// Extensions that are never embedded: binaries, media, fonts, archives, locks.
const JUNK_EXTS: &[&str] = &[
    "map", "woff", "woff2", "ttf", "otf", "eot", "ico", "svg", "gif", "webp", "bmp", "mp3", "mp4",
    "wav", "zip", "gz", "tar", "7z", "rar", "bin", "exe", "dll", "so", "dylib", "pdb", "lock",
    "sum", "chm", "chw",
];

/// Whole-filename matches that are never embedded.
const JUNK_FILENAMES: &[&str] = &[
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "cargo.lock",
    "composer.lock",
    "go.sum",
    ".ds_store",
    "thumbs.db",
];

/// Directory segments that are never embedded.
const JUNK_DIR_SEGMENTS: &[&str] = &[
    "node_modules",
    ".git",
    "dist",
    "build",
    "target",
    ".vs",
    "bin",
    "obj",
    ".github",
    "coverage",
    ".nyc_output",
    "vendor",
    "third_party",
    "third-party",
];

/// Filename suffixes for generated/minified bundles.
const MINIFIED_SUFFIXES: &[&str] = &[".min.js", ".min.mjs", ".min.cjs", ".bundle.js", ".min.css"];

fn file_name_lower(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn extension_lower(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .as_deref(),
        Some("png") | Some("jpg") | Some("jpeg")
    )
}

/// True if any `/<segment>/` appears in the (separator-normalized, lowercased)
/// path.
fn contains_dir_segment(path: &Path, segments: &[&str]) -> bool {
    let path_l = path
        .to_string_lossy()
        .to_ascii_lowercase()
        .replace('\\', "/");
    let padded = format!("/{path_l}/");
    segments
        .iter()
        .any(|seg| padded.contains(&format!("/{seg}/")))
}

/// Paths we never want in the vector DB (vendor, maps, binaries, minified,
/// build outputs). Used for zip walks and single-file guards. Test files and
/// directories are handled separately by [`is_test_path`], which zip uploads
/// apply only when `filter_test_files` is enabled.
pub fn is_junk(path: &Path) -> bool {
    if is_image(path) {
        return true;
    }
    let name = file_name_lower(path);
    let ext = extension_lower(path);
    if JUNK_EXTS.contains(&ext.as_str()) {
        return true;
    }
    if MINIFIED_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
        return true;
    }
    if JUNK_FILENAMES.contains(&name.as_str()) {
        return true;
    }
    contains_dir_segment(path, JUNK_DIR_SEGMENTS)
}

// ---------------------------------------------------------------------------
// Test files / directories (opt-in via `filter_test_files`).
// ---------------------------------------------------------------------------

/// Directory segments (non-alphanumerics stripped) that mark a test location.
const TEST_DIR_SEGMENTS: &[&str] = &[
    "test",
    "tests",
    "testing",
    "spec",
    "specs",
    "unittest",
    "unittests",
    "integrationtest",
    "integrationtests",
    "e2e",
    "testdata",
    "testutils",
    "testcases",
    "fixture",
    "fixtures",
    "mocks",
    "cypress",
];

/// Filename stems are matched per-token against these.
const TEST_FILE_TOKENS: &[&str] = &["test", "tests", "spec", "specs"];

/// Test file / directory detection covering the common conventions of pytest,
/// unittest/Django, Jest/Vitest/Mocha, Go, Cargo, Maven/Gradle/Spock, .NET,
/// C/C++, RSpec, PHPUnit, XCTest, and Dart. Zip uploads apply this filter only
/// when `filter_test_files` is enabled.
pub fn is_test_path(path: &Path) -> bool {
    let name = file_name_lower(path);
    // Exact special cases that carry no test token.
    if matches!(name.as_str(), "conftest.py") {
        return true;
    }

    // Directory segments match after stripping non-alphanumerics so
    // "__tests__", "unit-test", and "integration_test" all normalize onto the
    // same entries.
    for component in path.components() {
        let Some(seg) = component.as_os_str().to_str() else {
            continue;
        };
        let normalized: String = seg
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if !normalized.is_empty() && TEST_DIR_SEGMENTS.contains(&normalized.as_str()) {
            return true;
        }
    }

    // File stems match per-token so "latest.ts" and "contests.cs" are kept
    // while "foo.test.ts", "test_bar.py", and "FooTests.cs" are filtered.
    let stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
    split_name_tokens(stem)
        .iter()
        .any(|token| TEST_FILE_TOKENS.contains(&token.as_str()))
}

/// Split a file stem into lowercase word tokens on non-alphanumeric characters
/// and on lowercase/digit → UPPERCASE boundaries.
fn split_name_tokens(stem: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut prev_lower_or_digit = false;
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            if c.is_ascii_uppercase() && prev_lower_or_digit {
                tokens.push(std::mem::take(&mut current));
            }
            current.push(c.to_ascii_lowercase());
            prev_lower_or_digit = !c.is_ascii_uppercase();
        } else {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            prev_lower_or_digit = false;
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

// ---------------------------------------------------------------------------
// "Code files only": data, documents, media and binaries to exclude.
//
// Deliberately **omitted** because the extension is also used by real code:
// `ts` (TypeScript / MPEG-TS), `m` (Objective-C / MATLAB), `pl` (Perl /
// Prolog), `r` (R), `d` (D), `v` (Verilog / V), `cls` (LaTeX / Apex),
// `sc` (Scala / SuperCollider), `obj` / `stl` (ObjectScript / Wavefront /
// 3D meshes), and `ipynb` (notebooks contain code).
// ---------------------------------------------------------------------------

/// Lowercase extensions (no dot) excluded by "Code files only".
pub const CODE_ONLY_IGNORE_LIST: &[&str] = &[
    // Data / tabular / serialized
    "csv", "tsv", "psv", "jsonl", "ndjson", "json-seq", "parquet", "avro", "orc", "arrow",
    "feather", "hdf5", "h5", "hdf", "npy", "npz", "nwb", "mat", "rdata", "rds", "sas7bdat", "sav",
    "dta", "pkl", "pickle", "joblib", "dbf", "msgpack", "cbor", "bson", "pbf",
    // Databases & dumps
    "db", "sqlite", "sqlite3", "sqlite-journal", "wal", "shm", "mdb", "accdb", "frm", "myd", "myi",
    "ibd", "dump", "bak", "backup",
    // Office / rich documents
    "pdf", "doc", "docm", "docx", "dot", "dotx", "ppt", "pptm", "pptx", "pps", "ppsx", "pot",
    "odt", "odp", "ods", "rtf", "wpd", "wps", "pages", "numbers", "one", "onetoc2", "vsd", "vsdx",
    "xls", "xlsx", "xlsb", "xlsm",
    // Ebooks / subtitles
    "epub", "mobi", "azw", "azw3", "fb2", "djvu", "cbz", "cbr", "srt", "vtt", "ass", "ssa", "sub",
    "sbv",
    // Images / graphics
    "heic", "heif", "avif", "jxl", "psd", "psb", "ai", "eps", "xcf", "sketch", "fig", "afdesign",
    "afphoto", "dds", "ktx", "basis", "raw", "cr2", "nef", "dng", "arw", "orf",
    // Audio / video
    "aac", "oga", "m4a", "wma", "opus", "aiff", "aif", "mid", "midi", "amr", "flac", "mov", "avi",
    "mkv", "webm", "wmv", "flv", "mpg", "mpeg", "3gp", "ogv", "mts", "m2ts", "m4v",
    // Fonts
    "ttc", "otc", "type1", "pfm", "afm",
    // Archives / packages / installers
    "tgz", "tbz2", "xz", "txz", "zst", "lz", "lzma", "cab", "iso", "dmg", "jar", "war", "ear",
    "whl", "egg", "apk", "aab", "ipa", "deb", "rpm", "pkg", "msix", "appx", "nupkg", "vsix", "xpi",
    "crx",
    // Compiled binaries / bytecode
    "o", "a", "lib", "ko", "elf", "class", "dex", "pyc", "pyo", "pyd", "wasm", "luac", "rbc",
    "msi", "msp",
    // ML / AI artifacts
    "pt", "pth", "ckpt", "onnx", "tflite", "safetensors", "gguf", "ggml", "nii", "dcm", "mha",
    "mhd",
    // Scientific / geo
    "shp", "shx", "prj", "gpkg", "geojson", "topojson", "kml", "kmz", "mbtiles", "osm", "nc",
    "nc4", "grib", "grib2", "grb",
    // CAD / 3D / design
    "dwg", "dxf", "step", "stp", "iges", "igs", "fbx", "blend", "3ds", "skp", "glb", "gltf",
    "usdz", "usd", "usda", "usdc",
    // Secrets / keys / certificates (never embed)
    "pem", "key", "crt", "cer", "der", "p12", "pfx", "jks", "keystore", "truststore", "gpg", "asc",
    "pgp", "sig", "crl", "p7b", "p7s", "kdbx", "keychain",
    // Logs / dumps / crash reports / coverage
    "log", "dmp", "core", "etl", "evtx", "crash", "stackdump", "mdmp", "trace", "lcov", "snap",
    // Email / calendar / contacts / misc OS formats
    "eml", "msg", "mbox", "vcf", "ics", "vcs", "reg", "inf", "lnk", "url", "scf", "swf",
];

/// Whole-filename matches (lowercase) excluded by "Code files only".
pub const CODE_ONLY_IGNORE_FILENAMES: &[&str] = &[
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
    "cargo.lock",
    "composer.lock",
    "gemfile.lock",
    "poetry.lock",
    "pipfile.lock",
    "go.sum",
    "mix.lock",
    ".ds_store",
    "thumbs.db",
];

/// Directory segments excluded by "Code files only".
pub const CODE_ONLY_IGNORE_DIRS: &[&str] = &[
    "__pycache__",
    ".venv",
    "venv",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    ".eggs",
    "site-packages",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".cache",
    ".angular",
    ".idea",
    ".gradle",
    ".terraform",
    ".dart_tool",
    "pods",
    "deriveddata",
    "bower_components",
];

/// True when `path` is a non-code type excluded by the "Code files only"
/// option. Complements [`is_junk`] (which runs first and catches media /
/// binaries / build dirs).
pub fn is_code_only_ignored(path: &Path) -> bool {
    let name = file_name_lower(path);
    if MINIFIED_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
        return true;
    }
    if CODE_ONLY_IGNORE_FILENAMES.contains(&name.as_str()) {
        return true;
    }
    let ext = extension_lower(path);
    if !ext.is_empty() && CODE_ONLY_IGNORE_LIST.contains(&ext.as_str()) {
        return true;
    }
    contains_dir_segment(path, CODE_ONLY_IGNORE_DIRS)
}

// ---------------------------------------------------------------------------
// Oversized config-family files: data dumps wearing a config extension.
//
// `xml`, `json`, `yaml`, `toml`, `html` and `css` are kept because they are
// usually hand-written config. A data dump saved with one of those extensions
// is indistinguishable by name — but it is never small. Hand-written config
// tops out well under 128 KiB, while a data dump passes 200 KiB almost
// immediately. Applied unconditionally, independent of the "Code files only"
// toggle, because the file is a data dump either way.
// ---------------------------------------------------------------------------

/// Config-family extensions: kept by default, but size-capped by
/// [`is_oversized_config_file`].
pub const CONFIG_FAMILY_EXTS: &[&str] =
    &["xml", "json", "yaml", "yml", "toml", "html", "htm", "css"];

/// Largest a [`CONFIG_FAMILY_EXTS`] file may be and still be treated as a
/// hand-written config rather than a data dump.
pub const MAX_CONFIG_FILE_BYTES: u64 = 128 * 1024;

/// True when `path` is a config-family file too large to plausibly be a config
/// (a data dump that happens to use a config extension). Only config-family
/// paths are stat'd, so the zip walk stays cheap. A missing/unreadable file is
/// kept — the loader reports the real error if there is one.
pub fn is_oversized_config_file(path: &Path) -> bool {
    let ext = extension_lower(path);
    if !CONFIG_FAMILY_EXTS.contains(&ext.as_str()) {
        return false;
    }
    path.metadata()
        .map(|m| m.len() > MAX_CONFIG_FILE_BYTES)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Composite filter used by the ingestion pipeline.
// ---------------------------------------------------------------------------

/// Upload-time toggles that decide which files are ingested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestFilter {
    /// Enable the "Code files only" denylist ([`is_code_only_ignored`]).
    pub code_only: bool,
    /// Skip test/spec files and directories ([`is_test_path`]).
    pub filter_tests: bool,
}

impl IngestFilter {
    pub fn new(code_only: bool, filter_tests: bool) -> Self {
        Self {
            code_only,
            filter_tests,
        }
    }

    /// Whether `path` should be ingested.
    ///
    /// `path` is used for the junk/extension checks (absolute is fine).
    /// `relative` is the path within the extraction root and is used for test
    /// detection so unrelated parent directories never match a test segment.
    pub fn allows(&self, path: &Path, relative: &Path) -> bool {
        if is_junk(path) {
            return false;
        }
        // Unconditional: an oversized config-family file is a data dump
        // regardless of the "Code files only" toggle.
        if is_oversized_config_file(path) {
            return false;
        }
        if self.code_only && is_code_only_ignored(path) {
            return false;
        }
        if self.filter_tests && is_test_path(relative) {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junk_skips_minified_assets_and_media() {
        assert!(is_junk(Path::new("wwwroot/chart.umd.min.js.map")));
        assert!(is_junk(Path::new("bootstrap.min.css.map")));
        assert!(is_junk(Path::new("open-iconic.woff")));
        assert!(is_junk(Path::new("favicon.ico")));
        assert!(is_junk(Path::new("lib/jquery.min.js")));
        assert!(is_junk(Path::new("styles.min.css")));
        assert!(is_junk(Path::new("assets/logo.png")));
        assert!(is_junk(Path::new("assets/photo.JPG")));
    }

    #[test]
    fn junk_skips_build_vendor_and_lockfiles() {
        assert!(is_junk(Path::new("proj/node_modules/react/index.js")));
        assert!(is_junk(Path::new("repo/.git/HEAD")));
        assert!(is_junk(Path::new("repo/target/debug/foo")));
        assert!(is_junk(Path::new("repo/coverage/lcov.info")));
        assert!(is_junk(Path::new("proj/.github/workflows/ci.yml")));
        assert!(is_junk(Path::new("pkg/package-lock.json")));
        assert!(is_junk(Path::new("Cargo.lock")));
    }

    #[test]
    fn junk_keeps_source_and_docs() {
        assert!(!is_junk(Path::new("src/OptionsService.cs")));
        assert!(!is_junk(Path::new("README.md")));
        assert!(!is_junk(Path::new("app.js")));
        assert!(!is_junk(Path::new("src/services/ingestion_service.rs")));
        // Test paths are not junk-skipped; `is_test_path` handles them.
        assert!(!is_junk(Path::new("repo/tests/unit/foo.rs")));
    }

    #[test]
    fn test_path_matches_directories() {
        assert!(is_test_path(Path::new("repo/tests/unit/foo.rs")));
        assert!(is_test_path(Path::new("repo/test/foo.rs")));
        assert!(is_test_path(Path::new("pkg/__tests__/button.tsx")));
        assert!(is_test_path(Path::new("lib/spec/helpers.js")));
        assert!(is_test_path(Path::new("src/unit-tests/thing.cs")));
        assert!(is_test_path(Path::new("src/unit_tests/thing.cs")));
        assert!(is_test_path(Path::new("src/integration-test/api.go")));
        assert!(is_test_path(Path::new("web/cypress/support.js")));
        assert!(is_test_path(Path::new("web/e2e/login.ts")));
        assert!(is_test_path(Path::new(r"repo\test\fixtures\a.js")));
        // Real source outside test directories is kept.
        assert!(!is_test_path(Path::new("src/services/ingestion_service.rs")));
    }

    #[test]
    fn test_path_matches_filenames() {
        assert!(is_test_path(Path::new("src/foo.test.ts")));
        assert!(is_test_path(Path::new("src/foo.spec.jsx")));
        assert!(is_test_path(Path::new("src/test_bar.py")));
        assert!(is_test_path(Path::new("src/bar_test.py")));
        assert!(is_test_path(Path::new("app/conftest.py")));
        assert!(is_test_path(Path::new("pkg/bar_test.go")));
        assert!(is_test_path(Path::new("src/main/FooTests.cs")));
        assert!(is_test_path(Path::new("app/models/user_spec.rb")));
        assert!(is_test_path(Path::new("lib/widget_test.dart")));
        // Substring false positives must be kept.
        assert!(!is_test_path(Path::new("src/latest.ts")));
        assert!(!is_test_path(Path::new("src/contests.cs")));
        assert!(!is_test_path(Path::new("src/protest.py")));
        assert!(!is_test_path(Path::new("src/attestation.java")));
        assert!(!is_test_path(Path::new("src/main.rs")));
    }

    #[test]
    fn code_only_excludes_data_documents_and_secrets() {
        assert!(is_code_only_ignored(Path::new(
            "crawl-findings/data/hole_scores.csv"
        )));
        assert!(is_code_only_ignored(Path::new("data/rows.jsonl")));
        assert!(is_code_only_ignored(Path::new("data/table.parquet")));
        assert!(is_code_only_ignored(Path::new("reports/book.pdf")));
        assert!(is_code_only_ignored(Path::new("docs/spec.docx")));
        assert!(is_code_only_ignored(Path::new("sheets/budget.xlsx")));
        assert!(is_code_only_ignored(Path::new("db/app.sqlite")));
        assert!(is_code_only_ignored(Path::new("certs/server.pem")));
        assert!(is_code_only_ignored(Path::new("logs/app.log")));
        assert!(is_code_only_ignored(Path::new("models/embed.pt")));
    }

    #[test]
    fn code_only_keeps_code_docs_config_and_specials() {
        assert!(!is_code_only_ignored(Path::new("src/main.rs")));
        assert!(!is_code_only_ignored(Path::new("web/app.tsx")));
        assert!(!is_code_only_ignored(Path::new("README.md")));
        assert!(!is_code_only_ignored(Path::new("docs/guide.mdx")));
        assert!(!is_code_only_ignored(Path::new("notes.txt")));
        assert!(!is_code_only_ignored(Path::new("package.json")));
        assert!(!is_code_only_ignored(Path::new("config.yaml")));
        assert!(!is_code_only_ignored(Path::new("Cargo.toml")));
        assert!(!is_code_only_ignored(Path::new("index.html")));
        assert!(!is_code_only_ignored(Path::new("styles.css")));
        assert!(!is_code_only_ignored(Path::new("Makefile")));
        assert!(!is_code_only_ignored(Path::new("Dockerfile")));
    }

    #[test]
    fn code_only_keeps_dual_use_extensions() {
        // These are real code in some language and must never be filtered.
        for p in [
            "src/app.ts",
            "src/legacy.m",
            "script.pl",
            "analysis.r",
            "source.d",
            "hw.v",
            "paper.cls",
            "tune.sc",
            "mesh.obj",
            "model.stl",
            "notebook.ipynb",
        ] {
            assert!(!is_code_only_ignored(Path::new(p)), "{p} should be kept");
        }
    }

    #[test]
    fn code_only_still_allows_test_code_when_tests_kept() {
        // code_only filters by file *type*, not by test location, so enabling
        // it must never block test code when the user opts to keep tests.
        let keep_tests = IngestFilter::new(true, false);
        for p in [
            "src/foo.test.ts",
            "src/bar_test.py",
            "app/models/user_spec.rb",
            "tests/unit/thing.rs",
            "src/__tests__/button.test.tsx",
        ] {
            let path = Path::new(p);
            assert!(keep_tests.allows(path, path), "{p} should be embedded");
        }
    }

    #[test]
    fn ingest_filter_composes_rule_sets() {
        let strict = IngestFilter::new(true, true);
        let rel = Path::new("crawl-findings/data/hole_scores.csv");
        // code-only excludes the CSV.
        assert!(!strict.allows(rel, rel));
        // disabling code-only re-includes it.
        assert!(IngestFilter::new(false, true).allows(rel, rel));

        let test_file = Path::new("src/foo.test.ts");
        assert!(!strict.allows(test_file, test_file));
        assert!(IngestFilter::new(true, false).allows(test_file, test_file));

        // Junk is always excluded regardless of the toggles.
        let junk = Path::new("node_modules/react/index.js");
        assert!(!strict.allows(junk, junk));
        assert!(!IngestFilter::new(false, false).allows(junk, junk));

        // Test detection uses the relative path (extraction-relative).
        let abs = Path::new("/home/user/contest-uploads/src/app.ts");
        let rel = Path::new("src/app.ts");
        assert!(strict.allows(abs, rel));
    }

    fn write_bytes(path: &Path, len: u64) {
        std::fs::write(path, vec![b'x'; len as usize]).unwrap();
    }

    #[test]
    fn oversized_config_file_is_skipped_and_small_one_kept() {
        let dir = tempfile::tempdir().unwrap();
        let dump = dir.path().join("showdetails_2024.json");
        write_bytes(&dump, MAX_CONFIG_FILE_BYTES + 1);
        assert!(is_oversized_config_file(&dump));

        let config = dir.path().join("tsconfig.json");
        write_bytes(&config, 512);
        assert!(!is_oversized_config_file(&config));
    }

    #[test]
    fn config_size_boundary_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let at_cap = dir.path().join("at-cap.xml");
        write_bytes(&at_cap, MAX_CONFIG_FILE_BYTES);
        assert!(!is_oversized_config_file(&at_cap), "exactly the cap is kept");

        let over_cap = dir.path().join("over-cap.yml");
        write_bytes(&over_cap, MAX_CONFIG_FILE_BYTES + 1);
        assert!(is_oversized_config_file(&over_cap));
    }

    #[test]
    fn size_guard_only_applies_to_config_family() {
        let dir = tempfile::tempdir().unwrap();
        let big_code = dir.path().join("generated.rs");
        write_bytes(&big_code, MAX_CONFIG_FILE_BYTES * 4);
        assert!(!is_oversized_config_file(&big_code));

        // Non-config extensions are not size-guarded; the code-only list
        // handles them by type instead.
        let big_csv = dir.path().join("rows.csv");
        write_bytes(&big_csv, MAX_CONFIG_FILE_BYTES * 4);
        assert!(!is_oversized_config_file(&big_csv));
    }

    #[test]
    fn oversized_config_is_excluded_even_without_code_only() {
        let dir = tempfile::tempdir().unwrap();
        let dump = dir.path().join("showdetails_2024.json");
        write_bytes(&dump, MAX_CONFIG_FILE_BYTES + 1);

        let permissive = IngestFilter::new(false, false);
        assert!(
            !permissive.allows(&dump, Path::new("crawl-findings/raw/showdetails_2024.json")),
            "oversized config is skipped even with every toggle off"
        );
    }

    #[test]
    fn missing_config_file_is_kept() {
        assert!(!is_oversized_config_file(Path::new(
            "/nonexistent/nope.json"
        )));
    }
}
