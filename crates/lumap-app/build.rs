//! Explorer, the taskbar and Start menu use the icon embedded in lumap.exe.
//! Compiled with the Windows SDK resource compiler without a runtime dependency.
use std::{env, fs, path::PathBuf, process::Command};

/// The Windows SDK resource compiler isn't necessarily on the CI runner's PATH.
fn resource_compiler() -> PathBuf {
    if let Some(rc) = env::var_os("RC") {
        return PathBuf::from(rc);
    }
    let sdk = env::var_os("WindowsSdkDir").map(PathBuf::from).or_else(|| {
        env::var_os("ProgramFiles(x86)")
            .map(|dir| PathBuf::from(dir).join("Windows Kits").join("10"))
    });
    if let Some(sdk) = sdk {
        if let Ok(entries) = fs::read_dir(sdk.join("bin")) {
            let mut versions: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
            versions.sort();
            for version in versions.into_iter().rev() {
                let rc = version.join("x64").join("rc.exe");
                if rc.is_file() {
                    return rc;
                }
            }
        }
    }
    PathBuf::from("rc.exe")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../../assets/lumap.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }
    let out = PathBuf::from(env::var("OUT_DIR")?);
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let icon = manifest.join("../../assets/lumap.ico");
    let source = out.join("lumap.rc");
    let escaped = icon.display().to_string().replace('\\', "\\\\");
    fs::write(&source, format!("1 ICON \"{escaped}\"\n"))?;
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "msvc" {
        let res = out.join("lumap.res");
        let status = Command::new(resource_compiler())
            .arg("/nologo")
            .arg(format!("/fo{}", res.display()))
            .arg(&source)
            .status()?;
        if !status.success() {
            return Err("failed to compile lumap's Windows icon resource".into());
        }
        println!("cargo:rustc-link-arg-bin=lumap={}", res.display());
    } else {
        let obj = out.join("lumap-icon.o");
        let status = Command::new("windres")
            .arg(&source)
            .arg("-O")
            .arg("coff")
            .arg("-o")
            .arg(&obj)
            .status()?;
        if !status.success() {
            return Err("failed to compile lumap's Windows icon resource".into());
        }
        println!("cargo:rustc-link-arg-bin=lumap={}", obj.display());
    }
    Ok(())
}
