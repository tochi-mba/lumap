//! Developer tasks: `cargo xtask ci` runs the same gate as the CI workflow, locally.
//!
//! Steps: file-length check (no source file over 1000 lines), fmt check, clippy (all targets, all features, warnings denied), rustdoc, the website checks, cargo-deny,
//! tests (nextest when installed, `cargo test` otherwise) and, when `cargo-llvm-cov` is
//! installed, the same per-file 100% coverage gate CI enforces (see `coverage.rs`).

#![allow(clippy::print_stdout, clippy::print_stderr)]

use anyhow::{bail, Context};
use std::path::{Path, PathBuf};
use std::process::Command;

mod coverage;

/// Longest a source file may be; longer ones get split into modules.
pub const MAX_FILE_LINES: usize = 1000;

/// Source files (Rust, WGSL, HTML, TOML, Markdown, YAML) under `dir` that exceed
/// [`MAX_FILE_LINES`], as `path: lines`.
pub fn long_files(dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).with_context(|| format!("reading {}", d.display()))? {
            let path = entry?.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if !matches!(name, "target" | ".git" | "node_modules") {
                    stack.push(path);
                }
                continue;
            }
            let counted = matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("rs" | "wgsl" | "html" | "toml" | "md" | "yml" | "yaml")
            ) && name != "Cargo.lock";
            if counted {
                let lines = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?
                    .lines()
                    .count();
                if lines > MAX_FILE_LINES {
                    out.push(format!("{}: {lines}", path.display()));
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

fn check_file_lengths() -> anyhow::Result<()> {
    println!("==> file lengths (max {MAX_FILE_LINES} lines)");
    let long = long_files(&root())?;
    if long.is_empty() {
        Ok(())
    } else {
        bail!("files over {MAX_FILE_LINES} lines: {}", long.join(", "))
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn run(step: &str, mut cmd: Command) -> anyhow::Result<()> {
    println!("==> {step}");
    let status = cmd
        .current_dir(root())
        .status()
        .with_context(|| format!("running {step}"))?;
    if !status.success() {
        bail!("{step} failed ({status})");
    }
    Ok(())
}

fn cargo(args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO"));
    c.args(args);
    c
}

fn has_subcommand(name: &str) -> bool {
    Command::new(env!("CARGO"))
        .args([name, "--version"])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn ci() -> anyhow::Result<()> {
    check_file_lengths()?;
    platforms(false)?;
    run("fmt", cargo(&["fmt", "--all", "--check"]))?;
    run(
        "clippy",
        cargo(&[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ]),
    )?;
    let mut doc = cargo(&["doc", "--workspace", "--no-deps", "--all-features"]);
    doc.env("RUSTDOCFLAGS", "-D warnings");
    run("doc", doc)?;
    site()?;
    if has_subcommand("deny") {
        run("deny", cargo(&["deny", "check"]))?;
    } else {
        println!("==> deny skipped (cargo-deny not installed)");
    }
    if has_subcommand("nextest") {
        run(
            "test",
            cargo(&["nextest", "run", "--workspace", "--all-features"]),
        )?;
        run(
            "doctest",
            cargo(&["test", "--workspace", "--all-features", "--doc"]),
        )?;
    } else {
        run("test", cargo(&["test", "--workspace", "--all-features"]))?;
    }
    if has_subcommand("llvm-cov") {
        coverage()?;
    } else {
        println!("==> coverage skipped (cargo-llvm-cov not installed)");
    }
    println!("==> all green");
    Ok(())
}

/// Runs cargo-llvm-cov and enforces the per-file 100% gate.
fn coverage() -> anyhow::Result<()> {
    println!("==> coverage");
    let output = Command::new(env!("CARGO"))
        .args([
            "llvm-cov",
            "--workspace",
            "--all-features",
            "--json",
            "--summary-only",
        ])
        .current_dir(root())
        .output()
        .context("running cargo llvm-cov")?;
    if !output.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        bail!("cargo llvm-cov failed");
    }
    coverage::check(&String::from_utf8_lossy(&output.stdout))
}

/// First of `names` that runs with `--version`, so `python3` on Linux and `python` on Windows
/// are both found.
fn tool(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find(|name| {
            Command::new(name)
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success())
        })
        .map(ToString::to_string)
}

/// Checks the GitHub Pages site the way the Pages workflow does: the checker, its tests, and
/// the page script's tests. Python is required; Node is used when present.
fn site() -> anyhow::Result<()> {
    let python = tool(&["python3", "python"]).context("python is needed to check the site")?;
    let mut check = Command::new(&python);
    check.arg("scripts/check_site.py").current_dir(root());
    run("site check", check)?;
    let mut tests = Command::new(&python);
    tests
        .args([
            "-m",
            "unittest",
            "discover",
            "-s",
            "tests/site",
            "-p",
            "test_*.py",
        ])
        .current_dir(root());
    run("site checker tests", tests)?;
    if let Some(node) = tool(&["node"]) {
        let mut script = Command::new(node);
        script
            .args([
                "--test",
                "--experimental-test-coverage",
                "--test-coverage-include=site/**",
                "--test-coverage-lines=100",
                "--test-coverage-branches=100",
                "--test-coverage-functions=100",
                "tests/site/app.test.mjs",
            ])
            .current_dir(root());
        run("site script tests", script)?;
    } else {
        println!("==> site script tests skipped (node not installed)");
    }
    Ok(())
}

/// `docs/platforms.json`: what lumap does on each platform.
#[derive(Debug, serde::Deserialize)]
pub struct Platforms {
    /// Column order.
    pub platforms: Vec<String>,
    /// One row per capability.
    pub rows: Vec<PlatformRow>,
}

/// One capability and its state per platform.
#[derive(Debug, serde::Deserialize)]
pub struct PlatformRow {
    /// Short id.
    pub id: String,
    /// What it is.
    pub capability: String,
    /// `supported`, `planned` or `n/a` on Windows.
    pub windows: String,
    /// The same on macOS.
    pub macos: String,
    /// The same on Linux.
    pub linux: String,
    /// The test (a `fn` name) or file that proves a `supported` cell; empty when nothing is
    /// supported yet.
    pub test: String,
}

impl PlatformRow {
    fn cell(&self, platform: &str) -> &str {
        match platform {
            "windows" => &self.windows,
            "macos" => &self.macos,
            "linux" => &self.linux,
            _ => "",
        }
    }
}

/// Renders the Markdown table for the platforms file.
pub fn render_platforms(p: &Platforms) -> String {
    use std::fmt::Write as _;
    let mut out = String::from(concat!(
        "# Platforms

",
        "What lumap does on each platform, generated from `docs/platforms.json` by ",
        "`cargo xtask platforms`; CI fails when this file is stale. A *supported* cell names the ",
        "test or file that proves it; *planned* is on the way; *n/a* does not apply there.

"
    ));
    let totals: Vec<String> = p
        .platforms
        .iter()
        .map(|pl| {
            let n = p.rows.iter().filter(|r| r.cell(pl) == "supported").count();
            format!("{pl}: {n} supported")
        })
        .collect();
    let _ = writeln!(
        out,
        "Totals: {}.
",
        totals.join(", ")
    );
    out.push_str(
        "| Capability | Windows | macOS | Linux | Proof |
|---|---|---|---|---|
",
    );
    for r in &p.rows {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | `{}` |",
            r.capability, r.windows, r.macos, r.linux, r.test
        );
    }
    out
}

/// Every problem with the platforms file: unknown states, a `supported` cell without proof, a
/// proof that is neither a `fn` in the tree nor a file.
pub fn check_platforms(
    p: &Platforms,
    fn_exists: impl Fn(&str) -> bool,
    file_exists: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut problems = Vec::new();
    for r in &p.rows {
        for pl in &p.platforms {
            let cell = r.cell(pl);
            if !matches!(cell, "supported" | "planned" | "n/a") {
                problems.push(format!(
                    "{}: {pl} is {cell:?}, not supported, planned or n/a",
                    r.id
                ));
            }
        }
        let supported = p.platforms.iter().any(|pl| r.cell(pl) == "supported");
        if supported && r.test.is_empty() {
            problems.push(format!("{}: supported somewhere but names no proof", r.id));
        }
        if !r.test.is_empty() && !fn_exists(&r.test) && !file_exists(&r.test) {
            problems.push(format!(
                "{}: proof {:?} is neither a test fn nor a file",
                r.id, r.test
            ));
        }
    }
    problems
}

fn load_platforms() -> anyhow::Result<Platforms> {
    let path = root().join("docs/platforms.json");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Whether some Rust source under crates/ or tests/ defines `fn <name>(`.
fn fn_in_tree(name: &str) -> bool {
    let needle = format!("fn {name}(");
    let mut stack = vec![root().join("crates"), root().join("tests")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n != "target") {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs")
                && std::fs::read_to_string(&path).is_ok_and(|s| s.contains(&needle))
            {
                return true;
            }
        }
    }
    false
}

/// `cargo xtask platforms`: writes docs/platforms.md. `check-platforms`: verifies the json and
/// that the Markdown is current.
fn platforms(write: bool) -> anyhow::Result<()> {
    let p = load_platforms()?;
    let problems = check_platforms(&p, fn_in_tree, |f| root().join(f).exists());
    if !problems.is_empty() {
        bail!("docs/platforms.json: {}", problems.join("; "));
    }
    let md = render_platforms(&p);
    let path = root().join("docs/platforms.md");
    if write {
        std::fs::write(&path, &md).with_context(|| format!("writing {}", path.display()))?;
        println!("wrote {}", path.display());
    } else {
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current.replace("\r\n", "\n") != md {
            bail!("docs/platforms.md is stale; run `cargo xtask platforms`");
        }
        println!("==> platforms table is current ({} rows)", p.rows.len());
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let task = std::env::args().nth(1).unwrap_or_default();
    match task.as_str() {
        "ci" => ci(),
        "file-lengths" => check_file_lengths(),
        "site" => site(),
        "platforms" => platforms(true),
        "check-platforms" => platforms(false),
        "icon" => run(
            "icon",
            cargo(&[
                "run",
                "-q",
                "-p",
                "lumap-app",
                "--",
                "icon",
                "assets/lumap.ico",
            ]),
        ),
        "coverage" => coverage(),
        "check-coverage" => {
            let path = std::env::args()
                .nth(2)
                .context("usage: xtask check-coverage <llvm-cov.json>")?;
            let json = std::fs::read_to_string(&path).with_context(|| format!("reading {path}"))?;
            coverage::check(&json)
        }
        _ => bail!("usage: cargo xtask <ci|file-lengths|site|platforms|check-platforms|icon|coverage|check-coverage FILE>"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn long_files_are_found_and_the_tree_is_within_the_limit() {
        let dir = std::env::temp_dir().join(format!("xtask-long-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join("short.rs"), "fn a() {}\n").unwrap();
        std::fs::write(dir.join("long.rs"), "//\n".repeat(MAX_FILE_LINES + 1)).unwrap();
        std::fs::write(dir.join("long.png"), "x\n".repeat(MAX_FILE_LINES + 1)).unwrap();
        std::fs::write(dir.join("target").join("gen.rs"), "x\n".repeat(2000)).unwrap();
        let found = long_files(&dir).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].ends_with(&format!("long.rs: {}", MAX_FILE_LINES + 1)));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(long_files(&root()).unwrap(), Vec::<String>::new());
    }
}
