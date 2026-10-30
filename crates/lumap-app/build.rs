//! Explorer, the taskbar and Start menu use the icon embedded in lumap.exe.
//! Compiled with the Windows SDK resource compiler without a runtime dependency.
use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=../../assets/lumap.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"));
    let icon = manifest.join("../../assets/lumap.ico");
    let source = out.join("lumap.rc");
    let escaped = icon.display().to_string().replace('\\', "\\\\");
    fs::write(&source, format!("1 ICON \"{escaped}\"\n")).expect("write icon resource");
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "msvc" {
        let res = out.join("lumap.res");
        let status = Command::new("rc.exe")
            .arg("/nologo")
            .arg(format!("/fo{}", res.display()))
            .arg(&source)
            .status()
            .expect("Windows SDK rc.exe is required to embed lumap's icon");
        assert!(status.success(), "failed to compile lumap's icon resource");
        println!("cargo:rustc-link-arg-bin=lumap={}", res.display());
    } else {
        let obj = out.join("lumap-icon.o");
        let status = Command::new("windres")
            .arg(&source)
            .arg("-O")
            .arg("coff")
            .arg("-o")
            .arg(&obj)
            .status()
            .expect("windres is required for Windows GNU builds");
        assert!(status.success(), "failed to compile lumap's icon resource");
        println!("cargo:rustc-link-arg-bin=lumap={}", obj.display());
    }
}
