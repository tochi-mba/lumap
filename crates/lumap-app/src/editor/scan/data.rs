//! What a projector's scan knows, how it is kept on disk, and the store that loads scans in the
//! background when a show opens.
//!
//! On disk, beside the show, each projector gets a folder with `photo.png` (the projector's-eye
//! photo), `white.png` and `black.png` (the camera's view lit and dark), `cam2proj.png` (16-bit:
//! for each camera pixel, the projector pixel it sees) and `scan.toml` (sizes, camera, quality).
//! Plain files, so a scan can be inspected and copied with the show.

use lumap_vision::correspondence::{Correspondence, ProjectorMap};
use lumap_vision::graycode::ScanStats;
use lumap_vision::{features, find, photo, snap, Gray, Rgba};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Everything one projector's scan gives.
pub struct ScanData {
    /// The projector.
    pub projector: String,
    /// Folder the scan is kept in.
    pub folder: PathBuf,
    /// Camera pixel → projector pixel.
    pub corr: Correspondence,
    /// Projector pixel → camera pixel.
    pub map: ProjectorMap,
    /// The camera's view with this projector white.
    pub white: Gray,
    /// The camera's view with this projector black.
    pub black: Gray,
    /// What the projector "sees": the camera photo warped into its pixels.
    pub photo: Rgba,
    /// The room's edges in projector pixels (Trace the room, physics).
    pub edges: Gray,
    /// Surfaces worth mapping, best first.
    pub found: Vec<find::Suggestion>,
    /// Real corners and edges to snap to.
    pub snap: snap::SnapIndex,
    /// How good the scan is.
    pub stats: ScanInfo,
}

/// The facts `scan.toml` keeps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanInfo {
    /// Camera that took it.
    pub camera: String,
    /// Projector size scanned.
    pub projector_size: (u32, u32),
    /// Camera picture size.
    pub camera_size: (u32, u32),
    /// Share of camera pixels that decoded.
    pub valid_fraction: f64,
    /// Share of the projector's pixels the camera saw.
    pub coverage: f64,
    /// Average white-minus-black brightness the camera saw.
    pub mean_contrast: f64,
    /// When, in seconds since 1970.
    pub taken: u64,
}

impl ScanData {
    /// Builds everything from a decoded scan and the camera's colour photo with the projector
    /// white. Heavy (seconds at 1080p): call it on a worker thread.
    pub fn build(
        projector: &str,
        folder: PathBuf,
        camera: &str,
        mut corr: Correspondence,
        white: Gray,
        black: Gray,
        color: &Rgba,
        stats: &ScanStats,
    ) -> Self {
        corr.median_filter();
        let map = corr.invert();
        let info = ScanInfo {
            camera: camera.to_string(),
            projector_size: (corr.proj_w, corr.proj_h),
            camera_size: (corr.cam_w, corr.cam_h),
            valid_fraction: stats.valid_fraction,
            coverage: map.coverage(),
            mean_contrast: stats.mean_contrast,
            taken: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        };
        Self::derive(projector, folder, corr, map, white, black, color, info)
    }

    #[allow(clippy::too_many_arguments)]
    fn derive(
        projector: &str,
        folder: PathBuf,
        corr: Correspondence,
        map: ProjectorMap,
        white: Gray,
        black: Gray,
        color: &Rgba,
        stats: ScanInfo,
    ) -> Self {
        let photo = photo::projector_eye(&map, color);
        let grey = Gray::from_rgba(&photo);
        let edges = features::canny(&features::blur(&grey, 1.2), 25.0, 70.0);
        let found = find::find_surfaces(&photo, Some(&map.cam), &find::FindSettings::default());
        let snap = snap::SnapIndex::build(&photo);
        Self {
            projector: projector.to_string(),
            folder,
            corr,
            map,
            white,
            black,
            photo,
            edges,
            found,
            snap,
            stats,
        }
    }

    /// Writes the scan's files into its folder.
    pub fn save(&self, color: &Rgba) -> Result<(), String> {
        std::fs::create_dir_all(&self.folder).map_err(|e| e.to_string())?;
        save_rgba(&self.folder.join("photo.png"), &self.photo)?;
        save_rgba(&self.folder.join("camera.png"), color)?;
        save_gray(&self.folder.join("white.png"), &self.white)?;
        save_gray(&self.folder.join("black.png"), &self.black)?;
        let (w, h) = (self.corr.cam_w, self.corr.cam_h);
        let data = self.corr.to_rgba16();
        let img: image::ImageBuffer<image::Rgba<u16>, Vec<u16>> =
            image::ImageBuffer::from_raw(w, h, data).ok_or("scan map has the wrong size")?;
        img.save(self.folder.join("cam2proj.png"))
            .map_err(|e| e.to_string())?;
        let text = toml::to_string(&self.stats).map_err(|e| e.to_string())?;
        std::fs::write(self.folder.join("scan.toml"), text).map_err(|e| e.to_string())
    }

    /// Reads a scan folder back. Heavy: call it on a worker thread.
    pub fn load(projector: &str, folder: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(folder.join("scan.toml")).map_err(|e| e.to_string())?;
        let stats: ScanInfo = toml::from_str(&text).map_err(|e| e.to_string())?;
        let raw = image::open(folder.join("cam2proj.png"))
            .map_err(|e| e.to_string())?
            .into_rgba16();
        let (pw, ph) = stats.projector_size;
        let corr = Correspondence::from_rgba16(raw.width(), raw.height(), pw, ph, raw.as_raw())
            .ok_or("the scan map does not match its sizes")?;
        let map = corr.invert();
        let white = load_gray(&folder.join("white.png"))?;
        let black = load_gray(&folder.join("black.png"))?;
        let color = load_rgba(&folder.join("camera.png"))?;
        Ok(Self::derive(
            projector,
            folder.to_path_buf(),
            corr,
            map,
            white,
            black,
            &color,
            stats,
        ))
    }

    /// The projector's-eye photo as a frame for textures.
    pub fn photo_frame(&self) -> lumap_sources::Frame {
        lumap_sources::Frame {
            width: self.photo.width,
            height: self.photo.height,
            rgba: self.photo.data.clone(),
        }
    }
}

fn save_rgba(path: &Path, img: &Rgba) -> Result<(), String> {
    image::RgbaImage::from_raw(img.width, img.height, img.data.clone())
        .ok_or("image has the wrong size")?
        .save(path)
        .map_err(|e| e.to_string())
}

fn save_gray(path: &Path, img: &Gray) -> Result<(), String> {
    image::GrayImage::from_raw(img.width, img.height, img.data.clone())
        .ok_or("image has the wrong size")?
        .save(path)
        .map_err(|e| e.to_string())
}

fn load_gray(path: &Path) -> Result<Gray, String> {
    let g = image::open(path).map_err(|e| e.to_string())?.into_luma8();
    Ok(Gray {
        width: g.width(),
        height: g.height(),
        data: g.into_raw(),
    })
}

fn load_rgba(path: &Path) -> Result<Rgba, String> {
    let g = image::open(path).map_err(|e| e.to_string())?.into_rgba8();
    Ok(Rgba {
        width: g.width(),
        height: g.height(),
        data: g.into_raw(),
    })
}

/// A scan being loaded or ready.
enum Slot {
    Loading(PathBuf),
    Ready(Arc<ScanData>),
    Failed(String),
}

/// The scans of a show's projectors, loaded in the background from their folders.
#[derive(Default)]
pub struct ScanStore {
    slots: HashMap<String, Slot>,
    inbox: Option<(Sender<(String, PathBuf, Result<ScanData, String>)>, Receiver<(String, PathBuf, Result<ScanData, String>)>)>,
}

impl ScanStore {
    /// Makes the store match the show: starts loading scans it names and has not got, and
    /// forgets scans of projectors that are gone or point elsewhere.
    pub fn sync(&mut self, project: &lumap_core::Project, base: &Path) {
        let wanted: HashMap<String, PathBuf> = project
            .outputs
            .iter()
            .filter_map(|o| {
                o.scan
                    .as_deref()
                    .map(|s| (o.id.clone(), lumap_sources::resolve(s, base)))
            })
            .collect();
        self.slots.retain(|id, slot| match (wanted.get(id), slot) {
            (Some(path), Slot::Ready(d)) => &d.folder == path,
            (Some(path), Slot::Loading(p)) => p == path,
            (Some(_), Slot::Failed(_)) => true,
            (None, _) => false,
        });
        for (id, path) in wanted {
            if self.slots.contains_key(&id) {
                continue;
            }
            let (tx, _) = self.inbox.get_or_insert_with(channel);
            let tx = tx.clone();
            self.slots.insert(id.clone(), Slot::Loading(path.clone()));
            std::thread::spawn(move || {
                let result = ScanData::load(&id, &path);
                let _ = tx.send((id, path, result));
            });
        }
        self.receive();
    }

    /// Takes in scans that finished loading.
    pub fn receive(&mut self) {
        let Some((_, rx)) = &self.inbox else { return };
        while let Ok((id, path, result)) = rx.try_recv() {
            let current = matches!(self.slots.get(&id), Some(Slot::Loading(p)) if *p == path);
            if current {
                self.slots.insert(
                    id,
                    match result {
                        Ok(d) => Slot::Ready(Arc::new(d)),
                        Err(e) => Slot::Failed(e),
                    },
                );
            }
        }
    }

    /// Adopts a scan just taken.
    pub fn put(&mut self, data: ScanData) {
        self.slots
            .insert(data.projector.clone(), Slot::Ready(Arc::new(data)));
    }

    /// A projector's scan, when ready.
    pub fn get(&self, projector: &str) -> Option<Arc<ScanData>> {
        match self.slots.get(projector) {
            Some(Slot::Ready(d)) => Some(Arc::clone(d)),
            _ => None,
        }
    }

    /// Why a projector's scan could not load.
    pub fn error(&self, projector: &str) -> Option<&str> {
        match self.slots.get(projector) {
            Some(Slot::Failed(e)) => Some(e),
            _ => None,
        }
    }

    /// True while a scan is still loading.
    pub fn loading(&self, projector: &str) -> bool {
        matches!(self.slots.get(projector), Some(Slot::Loading(_)))
    }

    /// Every ready scan.
    pub fn ready(&self) -> Vec<Arc<ScanData>> {
        self.slots
            .values()
            .filter_map(|s| match s {
                Slot::Ready(d) => Some(Arc::clone(d)),
                _ => None,
            })
            .collect()
    }
}
