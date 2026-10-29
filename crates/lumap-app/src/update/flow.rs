//! Every decision of the update flow, as pure functions over the persisted state: what a check
//! changes, when a download may start, when the app may look at all, what a restart learns,
//! and the words the status bar shows. The worker does the network and the files; the editor
//! keeps the clocks.

use super::offer::{Offer, Version};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How lumap treats a newer release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Say a newer version exists; download only when asked.
    #[default]
    Notify,
    /// Download it in the background and say when it is ready to install.
    Download,
    /// Never look.
    Off,
}

/// Where a pending update is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Known, not downloaded.
    Available,
    /// Being downloaded and verified.
    Downloading,
    /// Downloaded and verified; the installer is on disk.
    Ready,
    /// Handed to the installer; the app is closing.
    Installing,
}

/// A newer version lumap knows about, kept in the settings between runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pending {
    /// `x.y.z`.
    pub version: String,
    /// `lumap-Setup-<version>.exe`.
    pub installer_name: String,
    /// Where the installer is.
    pub installer_url: String,
    /// Where its `.sha256` is.
    pub checksum_url: String,
    /// The release page.
    pub page_url: String,
    /// Installer size in bytes (0 when unknown).
    pub size: u64,
    /// Release notes as plain text.
    pub notes: String,
    /// Where it is.
    pub stage: Stage,
    /// Downloads or installs that failed; the offer is dropped after [`MOST_FAILURES`].
    pub failures: u32,
}

impl Default for Pending {
    fn default() -> Self {
        Self {
            version: String::new(),
            installer_name: String::new(),
            installer_url: String::new(),
            checksum_url: String::new(),
            page_url: String::new(),
            size: 0,
            notes: String::new(),
            stage: Stage::Available,
            failures: 0,
        }
    }
}

impl Pending {
    /// A freshly found offer, downloading at once when the mode says so.
    pub fn from_offer(offer: &Offer, mode: Mode) -> Self {
        Self {
            version: offer.version.to_string(),
            installer_name: offer.installer_name.clone(),
            installer_url: offer.installer_url.clone(),
            checksum_url: offer.checksum_url.clone(),
            page_url: offer.page_url.clone(),
            size: offer.size,
            notes: offer.notes.clone(),
            stage: if mode == Mode::Download {
                Stage::Downloading
            } else {
                Stage::Available
            },
            failures: 0,
        }
    }

    /// The version as a number, when it is one.
    pub fn version(&self) -> Option<Version> {
        Version::parse(&self.version)
    }
}

/// What the update part of the settings remembers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UpdateSettings {
    /// How newer releases are treated.
    pub mode: Mode,
    /// Unix seconds of the last successful check.
    pub last_check: Option<u64>,
    /// Do not offer this version again unless asked.
    pub skipped: Option<String>,
    /// Unix seconds before which the offer stays quiet ("Later").
    pub quiet_until: Option<u64>,
    /// The newer version lumap knows about.
    pub pending: Option<Pending>,
}

/// How long after launch the first check waits, so it never slows a start.
pub const STARTUP_DELAY: Duration = Duration::from_secs(45);
/// How often lumap looks.
pub const CHECK_EVERY: Duration = Duration::from_hours(24);
/// How long "Later" keeps the offer quiet.
pub const LATER: Duration = Duration::from_hours(24);
/// Failed downloads or installs after which the offer is dropped until a later check.
pub const MOST_FAILURES: u32 = 3;

/// What the app is doing, as far as the updater cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    /// Show mode is on.
    pub show_mode: bool,
    /// At least one output window is open on a projector.
    pub outputs_open: bool,
    /// Seconds since launch.
    pub since_launch: Duration,
}

impl Moment {
    /// Whether the updater may do anything at all right now: never during a show, never on a
    /// projector, never in the first moments after launch.
    pub fn allows_update_work(self) -> bool {
        !self.show_mode && !self.outputs_open && self.since_launch >= STARTUP_DELAY
    }
}

/// Whether to ask GitHub now.
pub fn should_check(settings: &UpdateSettings, moment: Moment, due: bool, dev_build: bool) -> bool {
    settings.mode != Mode::Off
        && !dev_build
        && due
        && moment.allows_update_work()
        && !matches!(
            settings.pending,
            Some(Pending {
                stage: Stage::Installing | Stage::Downloading,
                ..
            })
        )
}

/// What a successful check changes. The latest release is the truth: a pending version it no
/// longer offers (withdrawn, superseded, or already running) goes, a newer one replaces it, and
/// a version the user skipped is not offered again.
pub fn after_check(settings: &mut UpdateSettings, offer: Option<&Offer>) -> Option<String> {
    let Some(offer) = offer else {
        let had = settings.pending.take().is_some();
        return had.then(|| "That update is no longer offered.".to_string());
    };
    let version = offer.version.to_string();
    if settings.skipped.as_deref() == Some(&version) {
        settings.pending = None;
        return None;
    }
    match &settings.pending {
        Some(p) if p.version == version && p.stage != Stage::Available => None,
        Some(p) if p.version == version => None,
        _ => {
            settings.pending = Some(Pending::from_offer(offer, settings.mode));
            Some(format!("lumap {version} is available."))
        }
    }
}

/// A download that is wanted now: the pending update is in the downloading stage and the
/// moment allows it.
pub fn should_download(settings: &UpdateSettings, moment: Moment) -> bool {
    moment.allows_update_work()
        && matches!(
            settings.pending,
            Some(Pending {
                stage: Stage::Downloading,
                ..
            })
        )
}

/// The user asked for the download (Notify mode), or the installer file went missing.
pub fn request_download(settings: &mut UpdateSettings) {
    if let Some(p) = &mut settings.pending {
        if p.stage == Stage::Available {
            p.stage = Stage::Downloading;
        }
    }
}

/// The download finished and verified.
pub fn download_succeeded(settings: &mut UpdateSettings) -> Option<String> {
    let p = settings.pending.as_mut()?;
    p.stage = Stage::Ready;
    Some(format!("lumap {} is ready to install.", p.version))
}

/// The download failed or did not verify. Three strikes drop the offer until a later check.
pub fn download_failed(settings: &mut UpdateSettings, why: &str) -> String {
    let Some(p) = settings.pending.as_mut() else {
        return String::new();
    };
    p.failures += 1;
    if p.failures >= MOST_FAILURES {
        let v = p.version.clone();
        settings.pending = None;
        return format!("Could not download lumap {v} ({why}); it will be offered again later.");
    }
    p.stage = Stage::Available;
    format!(
        "Download of lumap {} failed ({why}); try again from the status bar.",
        p.version
    )
}

/// "Later": nothing is shown for a day.
pub fn remind_later(settings: &mut UpdateSettings, now_unix: u64) {
    settings.quiet_until = Some(now_unix + LATER.as_secs());
}

/// "Skip": this version is never offered again unless the user asks.
pub fn skip(settings: &mut UpdateSettings) {
    if let Some(p) = settings.pending.take() {
        settings.skipped = Some(p.version);
    }
}

/// The user chose to install: the installer is handed over and the app closes.
pub fn start_install(settings: &mut UpdateSettings) -> bool {
    match settings.pending.as_mut() {
        Some(p) if p.stage == Stage::Ready => {
            p.stage = Stage::Installing;
            true
        }
        _ => false,
    }
}

/// What a fresh start learns about the update the last run handed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupNews {
    /// Nothing was pending.
    None,
    /// The handed-over version is the one now running.
    Installed(String),
    /// The installer was cancelled or failed; the update is offered again.
    InstallFailed(String),
    /// The installer failed too often; the offer is dropped until a later check.
    Abandoned(String),
}

/// Reconciles the pending update with the version now running.
pub fn on_startup(settings: &mut UpdateSettings, running: Option<Version>) -> StartupNews {
    let Some(p) = settings.pending.as_mut() else {
        return StartupNews::None;
    };
    if running.is_some() && p.version() == running {
        let v = p.version.clone();
        settings.pending = None;
        return StartupNews::Installed(v);
    }
    if p.stage != Stage::Installing {
        return StartupNews::None;
    }
    p.failures += 1;
    let v = p.version.clone();
    if p.failures >= MOST_FAILURES {
        settings.pending = None;
        return StartupNews::Abandoned(v);
    }
    p.stage = Stage::Ready;
    StartupNews::InstallFailed(v)
}

/// What the status bar offers right now, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Banner {
    /// Nothing to show.
    None,
    /// A newer version exists; Download, Later, Skip.
    Available(String),
    /// Downloading; nothing to click.
    Downloading(String),
    /// Ready; Install and restart, Later, Skip.
    Ready(String),
}

/// The banner for the state, quiet during a show, on a projector, or while "Later" holds.
pub fn banner(settings: &UpdateSettings, moment: Moment, now_unix: u64) -> Banner {
    if moment.show_mode || moment.outputs_open {
        return Banner::None;
    }
    if settings.quiet_until.is_some_and(|until| now_unix < until) {
        return Banner::None;
    }
    match &settings.pending {
        None
        | Some(Pending {
            stage: Stage::Installing,
            ..
        }) => Banner::None,
        Some(p) => {
            let size = if p.size > 0 {
                format!(" ({} MB)", (p.size / 1_048_576).max(1))
            } else {
                String::new()
            };
            match p.stage {
                Stage::Available => Banner::Available(format!("lumap {}{size}", p.version)),
                Stage::Downloading => {
                    Banner::Downloading(format!("Downloading lumap {}{size}", p.version))
                }
                Stage::Ready => Banner::Ready(format!("lumap {} is ready", p.version)),
                Stage::Installing => Banner::None,
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn offer(v: &str) -> Offer {
        Offer {
            version: Version::parse(v).unwrap(),
            installer_name: format!("lumap-Setup-{v}.exe"),
            installer_url: format!("https://github.com/tochi-mba/lumap/releases/download/v{v}/lumap-Setup-{v}.exe"),
            checksum_url: format!("https://github.com/tochi-mba/lumap/releases/download/v{v}/lumap-Setup-{v}.exe.sha256"),
            page_url: "https://github.com/tochi-mba/lumap/releases/latest".into(),
            size: 52 * 1_048_576,
            notes: "notes".into(),
        }
    }

    fn calm() -> Moment {
        Moment {
            show_mode: false,
            outputs_open: false,
            since_launch: STARTUP_DELAY,
        }
    }

    #[test]
    fn the_app_only_looks_when_it_is_safe_and_due() {
        let mut s = UpdateSettings::default();
        assert!(should_check(&s, calm(), true, false));
        assert!(!should_check(&s, calm(), false, false), "not due");
        assert!(!should_check(&s, calm(), true, true), "developer build");
        assert!(!should_check(
            &s,
            Moment {
                show_mode: true,
                ..calm()
            },
            true,
            false
        ));
        assert!(!should_check(
            &s,
            Moment {
                outputs_open: true,
                ..calm()
            },
            true,
            false
        ));
        assert!(!should_check(
            &s,
            Moment {
                since_launch: Duration::from_secs(1),
                ..calm()
            },
            true,
            false
        ));
        s.mode = Mode::Off;
        assert!(!should_check(&s, calm(), true, false));
        s.mode = Mode::Notify;
        s.pending = Some(Pending {
            stage: Stage::Downloading,
            ..Pending::default()
        });
        assert!(
            !should_check(&s, calm(), true, false),
            "a download is in flight"
        );
    }

    #[test]
    fn a_check_replaces_drops_or_keeps_the_pending_update() {
        let mut s = UpdateSettings::default();
        assert_eq!(
            after_check(&mut s, Some(&offer("0.4.0"))),
            Some("lumap 0.4.0 is available.".into())
        );
        assert_eq!(s.pending.as_ref().unwrap().stage, Stage::Available);
        assert_eq!(
            after_check(&mut s, Some(&offer("0.4.0"))),
            None,
            "same offer again says nothing"
        );
        s.pending.as_mut().unwrap().stage = Stage::Ready;
        assert_eq!(
            after_check(&mut s, Some(&offer("0.4.0"))),
            None,
            "a ready download is kept"
        );
        assert_eq!(
            after_check(&mut s, Some(&offer("0.5.0"))),
            Some("lumap 0.5.0 is available.".into())
        );
        assert_eq!(
            after_check(&mut s, None),
            Some("That update is no longer offered.".into())
        );
        assert!(s.pending.is_none());
        assert_eq!(after_check(&mut s, None), None);
        s.skipped = Some("0.5.0".into());
        assert_eq!(after_check(&mut s, Some(&offer("0.5.0"))), None, "skipped");
        assert!(s.pending.is_none());
        s.mode = Mode::Download;
        after_check(&mut s, Some(&offer("0.6.0")));
        assert_eq!(s.pending.as_ref().unwrap().stage, Stage::Downloading);
        assert!(should_download(&s, calm()));
        assert!(!should_download(
            &s,
            Moment {
                show_mode: true,
                ..calm()
            }
        ));
    }

    #[test]
    fn downloads_succeed_fail_and_give_up() {
        let mut s = UpdateSettings::default();
        assert!(download_failed(&mut s, "x").is_empty());
        assert!(download_succeeded(&mut s).is_none());
        after_check(&mut s, Some(&offer("0.4.0")));
        assert!(!should_download(&s, calm()));
        request_download(&mut s);
        assert!(should_download(&s, calm()));
        request_download(&mut s);
        assert_eq!(s.pending.as_ref().unwrap().stage, Stage::Downloading);
        assert_eq!(
            download_succeeded(&mut s),
            Some("lumap 0.4.0 is ready to install.".into())
        );
        assert_eq!(s.pending.as_ref().unwrap().stage, Stage::Ready);
        assert!(start_install(&mut s));
        assert_eq!(s.pending.as_ref().unwrap().stage, Stage::Installing);
        assert!(!start_install(&mut s), "only a ready update installs");

        let mut s = UpdateSettings::default();
        after_check(&mut s, Some(&offer("0.4.0")));
        request_download(&mut s);
        assert_eq!(
            download_failed(&mut s, "offline"),
            "Download of lumap 0.4.0 failed (offline); try again from the status bar."
        );
        assert_eq!(s.pending.as_ref().unwrap().stage, Stage::Available);
        request_download(&mut s);
        download_failed(&mut s, "offline");
        request_download(&mut s);
        assert_eq!(
            download_failed(&mut s, "bad hash"),
            "Could not download lumap 0.4.0 (bad hash); it will be offered again later."
        );
        assert!(s.pending.is_none());
    }

    #[test]
    fn later_and_skip() {
        let mut s = UpdateSettings::default();
        after_check(&mut s, Some(&offer("0.4.0")));
        remind_later(&mut s, 1000);
        assert_eq!(s.quiet_until, Some(1000 + LATER.as_secs()));
        assert_eq!(banner(&s, calm(), 1000), Banner::None);
        assert_eq!(
            banner(&s, calm(), 1000 + LATER.as_secs()),
            Banner::Available("lumap 0.4.0 (52 MB)".into())
        );
        skip(&mut s);
        assert_eq!(s.skipped.as_deref(), Some("0.4.0"));
        assert!(s.pending.is_none());
        skip(&mut s);
    }

    #[test]
    fn a_restart_learns_what_happened() {
        let mut s = UpdateSettings::default();
        assert_eq!(
            on_startup(&mut s, Version::parse("0.3.0")),
            StartupNews::None
        );
        after_check(&mut s, Some(&offer("0.4.0")));
        assert_eq!(
            on_startup(&mut s, Version::parse("0.3.0")),
            StartupNews::None,
            "not handed over yet"
        );
        request_download(&mut s);
        download_succeeded(&mut s);
        start_install(&mut s);
        let mut handed = s.clone();
        assert_eq!(
            on_startup(&mut handed, Version::parse("0.4.0")),
            StartupNews::Installed("0.4.0".into())
        );
        assert!(handed.pending.is_none());
        assert_eq!(
            on_startup(&mut s, Version::parse("0.3.0")),
            StartupNews::InstallFailed("0.4.0".into())
        );
        assert_eq!(s.pending.as_ref().unwrap().stage, Stage::Ready);
        start_install(&mut s);
        on_startup(&mut s, Version::parse("0.3.0"));
        start_install(&mut s);
        assert_eq!(
            on_startup(&mut s, Version::parse("0.3.0")),
            StartupNews::Abandoned("0.4.0".into())
        );
        assert!(s.pending.is_none());
        let mut dev = UpdateSettings::default();
        after_check(&mut dev, Some(&offer("0.4.0")));
        assert_eq!(on_startup(&mut dev, None), StartupNews::None);
    }

    #[test]
    fn the_banner_is_quiet_during_a_show_and_names_each_stage() {
        let mut s = UpdateSettings::default();
        assert_eq!(banner(&s, calm(), 0), Banner::None);
        after_check(&mut s, Some(&offer("0.4.0")));
        assert_eq!(
            banner(&s, calm(), 0),
            Banner::Available("lumap 0.4.0 (52 MB)".into())
        );
        assert_eq!(
            banner(
                &s,
                Moment {
                    show_mode: true,
                    ..calm()
                },
                0
            ),
            Banner::None
        );
        assert_eq!(
            banner(
                &s,
                Moment {
                    outputs_open: true,
                    ..calm()
                },
                0
            ),
            Banner::None
        );
        s.pending.as_mut().unwrap().size = 0;
        request_download(&mut s);
        assert_eq!(
            banner(&s, calm(), 0),
            Banner::Downloading("Downloading lumap 0.4.0".into())
        );
        download_succeeded(&mut s);
        assert_eq!(
            banner(&s, calm(), 0),
            Banner::Ready("lumap 0.4.0 is ready".into())
        );
        start_install(&mut s);
        assert_eq!(banner(&s, calm(), 0), Banner::None);
    }

    #[test]
    fn settings_round_trip_through_toml() {
        let mut s = UpdateSettings {
            mode: Mode::Download,
            ..UpdateSettings::default()
        };
        after_check(&mut s, Some(&offer("0.4.0")));
        let text = toml::to_string(&s).unwrap();
        assert!(text.contains("mode = \"download\""));
        let back: UpdateSettings = toml::from_str(&text).unwrap();
        assert_eq!(back, s);
        let sparse: UpdateSettings = toml::from_str("mode = \"off\"").unwrap();
        assert_eq!(sparse.mode, Mode::Off);
        assert!(sparse.pending.is_none());
        assert_eq!(Pending::default().version(), None);
    }
}
