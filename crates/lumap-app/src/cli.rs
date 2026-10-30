//! Headless subcommands.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use anyhow::{bail, Context};
use clap::Args;
use lumap_core::project::{demo_project, Project};
use lumap_render::gpu::{GpuContext, GpuRenderer};
use lumap_render::scene::{Scene, DEFAULT_MESH_DENSITY};
use lumap_render::software;
use lumap_sources::SourceSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Sizes in the `.ico`: the ones Explorer, the taskbar and the installer wizard ask for.
pub const ICON_SIZES: [u32; 5] = [16, 24, 32, 48, 256];

/// Arguments for `lumap render`.
#[derive(Debug, Args)]
pub struct RenderArgs {
    /// Project file.
    pub project: PathBuf,
    /// Output ids to render; defaults to every output.
    #[arg(long = "output", short = 'o')]
    pub outputs: Vec<String>,
    /// Destination: a `.png` file for a single output, otherwise a directory that receives
    /// `<output>.png` per output. Defaults to the current directory.
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Render size as `WxH`; defaults to the output's own size.
    #[arg(long, value_parser = parse_size)]
    pub size: Option<(u32, u32)>,
    /// Use the CPU reference renderer instead of the GPU.
    #[arg(long)]
    pub software: bool,
}

/// Parses `1920x1080`.
pub fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("expected WxH, got {s:?}"))?;
    let w: u32 = w
        .trim()
        .parse()
        .map_err(|_| format!("bad width in {s:?}"))?;
    let h: u32 = h
        .trim()
        .parse()
        .map_err(|_| format!("bad height in {s:?}"))?;
    if w == 0 || h == 0 {
        return Err("size must be at least 1x1".into());
    }
    Ok((w, h))
}

fn load(path: &Path) -> anyhow::Result<Project> {
    Project::load(path).with_context(|| format!("loading {}", path.display()))
}

/// `lumap validate`.
pub fn validate(path: &Path) -> anyhow::Result<ExitCode> {
    let project = load(path)?;
    let issues = project.validate();
    for issue in &issues {
        println!("{issue}");
    }
    let errors = issues.iter().filter(|i| i.error).count();
    println!(
        "{}: {} source(s), {} output(s), {} surface(s); {} error(s), {} warning(s)",
        path.display(),
        project.sources.len(),
        project.outputs.len(),
        project.surfaces.len(),
        errors,
        issues.len() - errors
    );
    Ok(if errors == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// `lumap migrate`.
pub fn migrate(path: &Path, out: Option<&Path>) -> anyhow::Result<ExitCode> {
    let project = load(path)?;
    let target = out.unwrap_or(path);
    project
        .save(target)
        .with_context(|| format!("writing {}", target.display()))?;
    println!(
        "wrote {} (schema {})",
        target.display(),
        lumap_core::project::SCHEMA_VERSION
    );
    Ok(ExitCode::SUCCESS)
}

/// `lumap icon OUT`: the app icon, drawn by [`crate::theme::icon`], as a multi-size `.ico`.
pub fn icon(out: &Path) -> anyhow::Result<ExitCode> {
    use image::codecs::ico::{IcoEncoder, IcoFrame};
    let frames = ICON_SIZES
        .iter()
        .map(|&size| {
            IcoFrame::as_png(
                &crate::theme::icon(size),
                size,
                size,
                image::ExtendedColorType::Rgba8,
            )
            .with_context(|| format!("encoding the {size} px icon"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let file = std::fs::File::create(out).with_context(|| format!("creating {}", out.display()))?;
    IcoEncoder::new(std::io::BufWriter::new(file))
        .encode_images(&frames)
        .with_context(|| format!("writing {}", out.display()))?;
    println!("wrote {} ({} sizes)", out.display(), frames.len());
    Ok(ExitCode::SUCCESS)
}

/// `lumap new`.
pub fn new_project(path: &Path, force: bool, look: Option<&str>) -> anyhow::Result<ExitCode> {
    if path.exists() && !force {
        bail!("{} exists; pass --force to overwrite", path.display());
    }
    let mut project = demo_project();
    if let Some(name) = look {
        let look = lumap_core::looks::find(name)
            .with_context(|| format!("no look called {name:?}; `lumap looks` lists them"))?;
        let surfaces: Vec<String> = project.surfaces.iter().map(|s| s.id.clone()).collect();
        for s in &surfaces {
            look.apply(&mut project, s);
        }
    }
    project
        .save(path)
        .with_context(|| format!("writing {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(ExitCode::SUCCESS)
}

/// `lumap looks`: every built-in look with its category and blurb.
#[allow(clippy::unnecessary_wraps)]
pub fn list_looks() -> anyhow::Result<ExitCode> {
    for look in lumap_core::looks::LOOKS {
        println!(
            "{:<18} {:<8} {}",
            look.name,
            look.category().label(),
            look.blurb
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Where one rendered output goes.
pub fn output_path(out: Option<&Path>, output_id: &str, single: bool) -> PathBuf {
    match out {
        Some(p) if single && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) => {
            p.to_path_buf()
        }
        Some(dir) => dir.join(format!("{output_id}.png")),
        None => PathBuf::from(format!("{output_id}.png")),
    }
}

/// `lumap render`.
pub fn render(args: &RenderArgs) -> anyhow::Result<ExitCode> {
    let project = load(&args.project)?;
    if !project.is_valid() {
        for issue in project.validate().iter().filter(|i| i.error) {
            eprintln!("{issue}");
        }
        bail!("project has errors; run `lumap validate`");
    }
    let base_dir = args.project.parent().unwrap_or(Path::new("."));
    let (mut sources, errors) = SourceSet::open(&project, base_dir);
    for e in &errors {
        eprintln!("warning: {e}");
    }
    sources.poll();

    let ids: Vec<String> = if args.outputs.is_empty() {
        project.outputs.iter().map(|o| o.id.clone()).collect()
    } else {
        for id in &args.outputs {
            if project.output(id).is_none() {
                bail!("unknown output {id:?}");
            }
        }
        args.outputs.clone()
    };
    if ids.is_empty() {
        bail!("project has no outputs");
    }
    let mut gpu = if args.software {
        None
    } else {
        match GpuContext::headless() {
            Ok(ctx) => Some(GpuRenderer::new(ctx.device, ctx.queue)),
            Err(e) => {
                eprintln!("warning: {e}; falling back to the software renderer");
                None
            }
        }
    };
    let single = ids.len() == 1;
    for id in &ids {
        let scene = Scene::build(&project, id, &sources, DEFAULT_MESH_DENSITY)?;
        let (w, h) = args.size.unwrap_or((scene.width, scene.height));
        let image = match gpu.as_mut() {
            Some(g) => g.render_to_image(&scene, w, h)?,
            None => software::render_at(&scene, w, h),
        };
        let path = output_path(args.out.as_deref(), id, single);
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        image
            .save(&path)
            .with_context(|| format!("writing {}", path.display()))?;
        println!(
            "wrote {} ({w}x{h}, {})",
            path.display(),
            if gpu.is_some() { "gpu" } else { "software" }
        );
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn size_parsing() {
        assert_eq!(parse_size("1920x1080").unwrap(), (1920, 1080));
        assert_eq!(parse_size(" 8 X 4 ").unwrap(), (8, 4));
        assert!(parse_size("1920").is_err());
        assert!(parse_size("ax2").is_err());
        assert!(parse_size("2xb").is_err());
        assert!(parse_size("0x2").is_err());
    }

    #[test]
    fn output_paths() {
        assert_eq!(output_path(None, "left", true), PathBuf::from("left.png"));
        assert_eq!(
            output_path(Some(Path::new("frame.png")), "left", true),
            PathBuf::from("frame.png")
        );
        assert_eq!(
            output_path(Some(Path::new("frame.PNG")), "left", true),
            PathBuf::from("frame.PNG")
        );
        assert_eq!(
            output_path(Some(Path::new("frame.png")), "left", false),
            PathBuf::from("frame.png/left.png")
        );
        assert_eq!(
            output_path(Some(Path::new("dir")), "left", true),
            PathBuf::from("dir/left.png")
        );
    }
}
