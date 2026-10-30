//! The coverage gate: every source file outside [`EXCLUDED`] must be fully covered.
//!
//! Reads the JSON export of `cargo llvm-cov --json --summary-only` and checks, per file, lines,
//! regions and (where llvm-cov reports any) branches. Functions are implied by lines.

use anyhow::Context;
use serde_json::Value;

/// Files that need a real window or GPU surface to run and are therefore exempt. Keep logic out
/// of them: anything testable belongs in a covered module. Paths are workspace-relative with
/// forward slashes.
pub const EXCLUDED: &[&str] = &[];

/// One measured metric of one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metric {
    /// Items llvm-cov counted.
    pub count: u64,
    /// Items executed.
    pub covered: u64,
}

/// Coverage of one source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCoverage {
    /// Workspace-relative path with forward slashes.
    pub path: String,
    /// Lines.
    pub lines: Metric,
    /// Regions.
    pub regions: Metric,
    /// Branches (count 0 when not instrumented).
    pub branches: Metric,
}

/// Normalises an absolute llvm-cov filename to a workspace-relative path starting at the last
/// `crates/` or `xtask/` component.
pub fn relative(filename: &str) -> String {
    let f = filename.replace('\\', "/");
    ["/crates/", "/xtask/"]
        .iter()
        .filter_map(|k| f.rfind(k).map(|i| f[i + 1..].to_string()))
        .min_by_key(String::len)
        .unwrap_or(f)
}

fn metric(summary: &Value, key: &str) -> Metric {
    let m = &summary[key];
    Metric {
        count: m["count"].as_u64().unwrap_or(0),
        covered: m["covered"].as_u64().unwrap_or(0),
    }
}

/// Parses the per-file summaries of a llvm-cov JSON export.
pub fn parse(json: &str) -> anyhow::Result<Vec<FileCoverage>> {
    let v: Value = serde_json::from_str(json).context("coverage json is not valid json")?;
    let files = v["data"][0]["files"]
        .as_array()
        .context("no files in coverage json")?;
    Ok(files
        .iter()
        .map(|f| {
            let s = &f["summary"];
            FileCoverage {
                path: relative(f["filename"].as_str().unwrap_or_default()),
                lines: metric(s, "lines"),
                regions: metric(s, "regions"),
                branches: metric(s, "branches"),
            }
        })
        .collect())
}

/// Problems with the report: every gated file short of 100%, and every exclusion that no
/// longer names a measured file (so the list cannot rot).
pub fn problems(files: &[FileCoverage]) -> Vec<String> {
    let mut out = Vec::new();
    for f in files
        .iter()
        .filter(|f| !EXCLUDED.contains(&f.path.as_str()))
    {
        for (name, m) in [
            ("lines", f.lines),
            ("regions", f.regions),
            ("branches", f.branches),
        ] {
            if m.covered < m.count {
                out.push(format!(
                    "{}: {name} {}/{} covered",
                    f.path, m.covered, m.count
                ));
            }
        }
    }
    for e in EXCLUDED {
        if !files.iter().any(|f| f.path == *e) {
            out.push(format!("{e}: excluded but not in the report"));
        }
    }
    out
}

/// Parses `json`, prints a summary and fails on any problem.
pub fn check(json: &str) -> anyhow::Result<()> {
    let files = parse(json)?;
    let (mut covered, mut count) = (0, 0);
    for f in &files {
        covered += f.lines.covered;
        count += f.lines.count;
    }
    println!(
        "coverage: {} files, {covered}/{count} lines overall, {} excluded from the 100% gate",
        files.len(),
        EXCLUDED.len()
    );
    let p = problems(&files);
    anyhow::ensure!(p.is_empty(), "coverage below 100%:\n{}", p.join("\n"));
    println!("every gated file is at 100%");
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn file(path: &str, lines: (u64, u64), branches: (u64, u64)) -> String {
        format!(
            r#"{{"filename":"{path}","summary":{{"lines":{{"count":{},"covered":{}}},"regions":{{"count":2,"covered":2}},"branches":{{"count":{},"covered":{}}}}}}}"#,
            lines.0, lines.1, branches.0, branches.1
        )
    }

    fn report(files: &[String]) -> String {
        format!(r#"{{"data":[{{"files":[{}]}}]}}"#, files.join(","))
    }

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative(r"C:\w\lumap\crates\lumap-core\src\geom.rs"),
            "crates/lumap-core/src/geom.rs"
        );
        assert_eq!(relative("/w/xtask/src/main.rs"), "xtask/src/main.rs");
        assert_eq!(relative("/a/crates/x/crates/y.rs"), "crates/y.rs");
        assert_eq!(relative("/elsewhere/a.rs"), "/elsewhere/a.rs");
    }

    #[test]
    fn full_and_partial_files() {
        let json = report(&[
            file("/w/crates/a/src/full.rs", (10, 10), (0, 0)),
            file("/w/crates/a/src/gap.rs", (10, 9), (4, 3)),
        ]);
        let files = parse(&json).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(
            files[0].regions,
            Metric {
                count: 2,
                covered: 2
            }
        );
        assert_eq!(
            problems(&files),
            vec![
                "crates/a/src/gap.rs: lines 9/10 covered",
                "crates/a/src/gap.rs: branches 3/4 covered",
            ]
        );
        assert!(check(&json).unwrap_err().to_string().contains("gap.rs"));
        let mut ok: Vec<String> = EXCLUDED
            .iter()
            .map(|e| file(&format!("/w/{e}"), (5, 0), (0, 0)))
            .collect();
        ok.push(file("/w/crates/a/src/full.rs", (1, 1), (0, 0)));
        assert!(check(&report(&ok)).is_ok());
    }

    #[test]
    fn missing_fields_and_bad_input() {
        let files = parse(r#"{"data":[{"files":[{"summary":{}}]}]}"#).unwrap();
        assert_eq!(files[0].path, "");
        assert_eq!(
            files[0].lines,
            Metric {
                count: 0,
                covered: 0
            }
        );
        assert!(parse("not json").is_err());
        assert!(parse("{}").is_err());
    }

    #[test]
    fn stale_exclusions_are_reported() {
        assert_eq!(problems(&[]).len(), EXCLUDED.len());
    }
}
