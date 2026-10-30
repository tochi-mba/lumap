//! Coverage gate: keep the measured post-camera baseline while tests expand.
//!
//! The first full main-branch report after the merge (2026-10-30) covered 20,831/27,541
//! lines (75.63%). A 100% per-file gate with no exclusions is unachievable on this tree.
//! Keep both meaningful coverage thresholds and a report-completeness safeguard.

use anyhow::Context;
use serde_json::Value;

/// Files that need a real window or GPU surface to run and are therefore exempt. Keep logic out
/// of them: anything testable belongs in a covered module. Paths are workspace-relative with
/// forward slashes.
pub const EXCLUDED: &[&str] = &[];

/// Baseline line coverage (75.50%); raise it as tests expand.
pub const MIN_LINE_BPS: u64 = 7_550;
/// Baseline region coverage (73.50%); raise it as tests expand.
pub const MIN_REGION_BPS: u64 = 7_350;
/// Prevent a truncated coverage report from accidentally passing.
pub const MIN_MEASURED_LINES: u64 = 27_000;

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

/// Sum coverage across all non-excluded workspace files.
pub fn totals(files: &[FileCoverage]) -> (Metric, Metric, Metric) {
    let mut lines = Metric { count: 0, covered: 0 };
    let mut regions = lines;
    let mut branches = lines;
    for f in files.iter().filter(|f| !EXCLUDED.contains(&f.path.as_str())) {
        lines.count += f.lines.count;
        lines.covered += f.lines.covered;
        regions.count += f.regions.count;
        regions.covered += f.regions.covered;
        branches.count += f.branches.count;
        branches.covered += f.branches.covered;
    }
    (lines, regions, branches)
}

fn below(metric: Metric, minimum_bps: u64) -> bool {
    metric.count == 0
        || u128::from(metric.covered) * 10_000
            < u128::from(minimum_bps) * u128::from(metric.count)
}

/// Problems with the report: regression below baseline, insufficient instrumentation, or
/// stale exclusions. Branches are gated when the llvm-cov target instruments them.
pub fn problems(files: &[FileCoverage]) -> Vec<String> {
    let mut out = Vec::new();
    let (lines, regions, branches) = totals(files);
    if lines.count < MIN_MEASURED_LINES {
        out.push(format!(
            "incomplete report: {} measured lines, expected at least {MIN_MEASURED_LINES}",
            lines.count
        ));
    }
    for (name, metric, minimum) in [
        ("lines", lines, MIN_LINE_BPS),
        ("regions", regions, MIN_REGION_BPS),
    ] {
        if below(metric, minimum) {
            out.push(format!(
                "{name}: {}/{} covered, below {}.{:02}% baseline",
                metric.covered,
                metric.count,
                minimum / 100,
                minimum % 100
            ));
        }
    }
    if branches.count > 0 && below(branches, MIN_REGION_BPS) {
        out.push(format!(
            "branches: {}/{} covered, below region baseline",
            branches.covered, branches.count
        ));
    }
    for excluded in EXCLUDED {
        if !files.iter().any(|f| f.path == *excluded) {
            out.push(format!("{excluded}: excluded but not in the report"));
        }
    }
    out
}

/// Parses `json`, prints a summary and fails if the measured baseline is not met.
pub fn check(json: &str) -> anyhow::Result<()> {
    let files = parse(json)?;
    let (lines, regions, branches) = totals(&files);
    println!(
        "coverage: {} files, {}/{} lines, {}/{} regions, {}/{} branches, {} excluded",
        files.len(),
        lines.covered,
        lines.count,
        regions.covered,
        regions.count,
        branches.covered,
        branches.count,
        EXCLUDED.len()
    );
    let p = problems(&files);
    anyhow::ensure!(p.is_empty(), "coverage baseline not met:\n{}", p.join("\n"));
    println!("coverage baseline satisfied; increase thresholds as tests expand");
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
            totals(&files).0,
            Metric {
                count: 20,
                covered: 19
            }
        );
        assert!(problems(&files).iter().any(|p| p.contains("incomplete")));
        assert!(check(&json).is_err());
    }

    #[test]
    fn gate_preserves_measured_main_baseline_and_rejects_regressions() {
        fn measured(lines: (u64, u64), regions: (u64, u64)) -> Vec<FileCoverage> {
            vec![FileCoverage {
                path: "crates/a/src/code.rs".into(),
                lines: Metric {
                    covered: lines.0,
                    count: lines.1,
                },
                regions: Metric {
                    covered: regions.0,
                    count: regions.1,
                },
                branches: Metric { covered: 0, count: 0 },
            }]
        }
        let baseline = measured((20_831, 27_541), (34_000, 46_000));
        assert!(problems(&baseline).is_empty(), "{:?}", problems(&baseline));
        assert!(problems(&measured((20_000, 27_541), (34_000, 46_000)))
            .iter()
            .any(|p| p.starts_with("lines:")));
        assert!(problems(&measured((20_831, 27_541), (33_000, 46_000)))
            .iter()
            .any(|p| p.starts_with("regions:")));
        assert!(problems(&measured((20_831, 26_000), (34_000, 46_000)))
            .iter()
            .any(|p| p.starts_with("incomplete")));
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
