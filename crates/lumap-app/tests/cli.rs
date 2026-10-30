//! End-to-end tests of the headless subcommands, run as a real subprocess.

#![allow(clippy::unwrap_used)]

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;

/// `LUMAP_REQUIRE_GPU` demands the GPU path; an empty value counts as unset.
fn require_gpu() -> bool {
    std::env::var("LUMAP_REQUIRE_GPU").is_ok_and(|v| !v.trim().is_empty())
}

fn lumap() -> Command {
    let mut c = Command::cargo_bin("lumap").unwrap();
    c.env("RUST_LOG", "warn");
    c
}

fn demo(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("demo.lumap.toml");
    lumap()
        .args(["new", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("wrote"));
    path
}

#[test]
fn new_refuses_to_overwrite_without_force() {
    let dir = tempfile::tempdir().unwrap();
    let path = demo(dir.path());
    lumap()
        .args(["new", path.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--force"));
    lumap()
        .args(["new", path.to_str().unwrap(), "--force"])
        .assert()
        .success();
}

#[test]
fn validate_reports_and_exits_by_severity() {
    let dir = tempfile::tempdir().unwrap();
    let path = demo(dir.path());
    lumap()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("0 error(s)"));
    let broken = dir.path().join("broken.lumap.toml");
    std::fs::write(
        &broken,
        "schema_version = 1\n[[surfaces]]\nid = \"s\"\noutput = \"ghost\"\nsource = \"ghost\"\n",
    )
    .unwrap();
    lumap()
        .args(["validate", broken.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(
            predicate::str::contains("unknown output \"ghost\"")
                .and(predicate::str::contains("2 error(s)")),
        );
    lumap()
        .args([
            "validate",
            dir.path().join("missing.toml").to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("loading"));
    let junk = dir.path().join("junk.toml");
    std::fs::write(&junk, "schema_version = 99\n").unwrap();
    lumap()
        .args(["validate", junk.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("newer"));
}

#[test]
fn migrate_rewrites_legacy_files() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("old.toml");
    std::fs::write(&legacy, "[[outputs]]\nid = \"a\"\nsize = [640, 360]\n").unwrap();
    let out = dir.path().join("new.toml");
    lumap()
        .args([
            "migrate",
            legacy.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("schema 1"));
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("schema_version = 1") && text.contains("width = 640"));
    assert!(
        std::fs::read_to_string(&legacy).unwrap().contains("size"),
        "original untouched with --out"
    );
    lumap()
        .args(["migrate", legacy.to_str().unwrap()])
        .assert()
        .success();
    assert!(std::fs::read_to_string(&legacy)
        .unwrap()
        .contains("schema_version = 1"));
}

#[test]
fn render_writes_pngs_with_the_software_renderer() {
    let dir = tempfile::tempdir().unwrap();
    let path = demo(dir.path());
    let out = dir.path().join("frames");
    lumap()
        .args([
            "render",
            path.to_str().unwrap(),
            "--software",
            "--size",
            "128x72",
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("left.png")
                .and(predicate::str::contains("right.png"))
                .and(predicate::str::contains("software")),
        );
    let img = image::open(out.join("left.png")).unwrap();
    assert_eq!((img.width(), img.height()), (128, 72));
    // Single output to a file path, at native size.
    let single = dir.path().join("right.png");
    lumap()
        .args([
            "render",
            path.to_str().unwrap(),
            "--software",
            "-o",
            "right",
            "--out",
            single.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(image::open(&single).unwrap().width(), 1920);
    lumap()
        .args([
            "render",
            path.to_str().unwrap(),
            "--software",
            "-o",
            "ghost",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unknown output"));
    lumap()
        .args(["render", path.to_str().unwrap(), "--size", "nope"])
        .assert()
        .failure();
}

#[test]
fn render_uses_the_gpu_when_one_is_available() {
    let dir = tempfile::tempdir().unwrap();
    let path = demo(dir.path());
    let out = dir.path().join("gpu.png");
    let assert = lumap()
        .args([
            "render",
            path.to_str().unwrap(),
            "-o",
            "left",
            "--size",
            "64x36",
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    if require_gpu() {
        assert!(stdout.contains("(64x36, gpu)"), "{stdout}");
    }
    assert!(out.exists());
}

#[test]
fn render_refuses_invalid_projects_and_empty_ones() {
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("broken.lumap.toml");
    std::fs::write(
        &broken,
        "schema_version = 1\n[[surfaces]]\nid = \"s\"\noutput = \"ghost\"\nsource = \"ghost\"\n",
    )
    .unwrap();
    lumap()
        .args(["render", broken.to_str().unwrap(), "--software"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("lumap validate"));
    let empty = dir.path().join("empty.lumap.toml");
    std::fs::write(&empty, "schema_version = 1\n").unwrap();
    lumap()
        .args(["render", empty.to_str().unwrap(), "--software"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no outputs"));
}

#[test]
fn help_and_version() {
    lumap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("render").and(predicate::str::contains("validate")));
    lumap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("lumap"));
}

#[test]
fn new_with_a_look_puts_it_on_every_surface_and_looks_lists_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fire.lumap.toml");
    lumap()
        .args(["new", path.to_str().unwrap(), "--look", "campfire"])
        .assert()
        .success();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("look-campfire"), "{text}");
    assert!(text.contains("pattern = \"fire\""), "{text}");
    lumap()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success();
    lumap()
        .args([
            "new",
            dir.path().join("x.lumap.toml").to_str().unwrap(),
            "--look",
            "nope",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("lumap looks"));
    lumap()
        .arg("looks")
        .assert()
        .success()
        .stdout(predicate::str::contains("Campfire").and(predicate::str::contains("Edges")));
}
