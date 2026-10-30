//! Keeping lumap current, modelled on rexplayer's updater and made show-safe: it asks GitHub
//! for the latest release once a day, downloads by default on a background worker,
//! verifies it against the published SHA-256, and installs only when the user clicks
//! "Install and restart". Nothing happens while show mode is on or an output window is open,
//! and nothing happens in the first 45 seconds after launch.
//!
//! [`flow`] holds every decision as pure functions over [`flow::UpdateSettings`], [`offer`]
//! parses releases and checks hashes, [`worker`] does the network and the files, and
//! [`Updater`] ties them to the editor.

pub mod flow;
pub mod offer;
pub mod worker;

use flow::{Banner, Moment, Stage, StartupNews, UpdateSettings};
use offer::{check_due, Version};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use worker::{Event, Fetcher, Worker};

/// What the user asked from the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Download the available version now.
    Download,
    /// Hand the verified installer to Windows and close.
    Install,
    /// Keep quiet for a day.
    Later,
    /// Never offer this version again.
    Skip,
    /// Look now, whatever the schedule says.
    CheckNow,
}

/// State shown by the manual update controls (the worker never blocks the UI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckUi {
    /// A manual or automatic check is queued or running.
    pub checking: bool,
    /// Checking is safe right now and the worker is free.
    pub enabled: bool,
    /// Human-readable result or reason for being unavailable.
    pub detail: String,
}

/// The updater as the editor sees it.
pub struct Updater {
    worker: Worker,
    dir: PathBuf,
    launched: Instant,
    running: Option<Version>,
    /// Set when the installer has been started: the app should close.
    pub closing: bool,
    check_requested: bool,
    check_in_flight: bool,
    manual_in_flight: bool,
    check_feedback: Option<String>,
}

impl std::fmt::Debug for Updater {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Updater")
            .field("running", &self.running)
            .field("closing", &self.closing)
            .finish_non_exhaustive()
    }
}

/// Where downloaded installers are kept: `%LOCALAPPDATA%\lumap\updates` (or the config
/// directory's sibling when `LUMAP_CONFIG_DIR` is set, which the tests use).
pub fn downloads_dir() -> PathBuf {
    match std::env::var_os("LUMAP_CONFIG_DIR") {
        Some(d) => PathBuf::from(d).join("updates"),
        None => dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("lumap")
            .join("updates"),
    }
}

/// Seconds since the Unix epoch, now.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Updater {
    /// An updater for the running build, fetching with `curl`.
    pub fn new() -> Self {
        Self::with(
            worker::Curl,
            downloads_dir(),
            Version::running(),
            Instant::now(),
        )
    }

    /// An updater with every outside dependency passed in.
    pub fn with(
        fetcher: impl Fetcher + Sync,
        dir: PathBuf,
        running: Option<Version>,
        launched: Instant,
    ) -> Self {
        Self {
            worker: Worker::new(fetcher),
            dir,
            launched,
            running,
            closing: false,
            check_requested: false,
            check_in_flight: false,
            manual_in_flight: false,
            check_feedback: None,
        }
    }

    /// The version this build runs as, when it is a release.
    #[cfg(test)]
    pub fn running(&self) -> Option<Version> {
        self.running
    }

    /// Reconciles the settings with what the last run handed over. Returns the line for the
    /// status bar, if there is news.
    pub fn startup(&self, settings: &mut UpdateSettings) -> Option<String> {
        let news = flow::on_startup(settings, self.running);
        match news {
            StartupNews::None => None,
            StartupNews::Installed(v) => {
                worker::discard_downloads(&self.dir);
                Some(format!("Updated to lumap {v}."))
            }
            StartupNews::InstallFailed(v) => Some(format!(
                "The lumap {v} update did not finish; it is ready to try again."
            )),
            StartupNews::Abandoned(v) => {
                worker::discard_downloads(&self.dir);
                Some(format!(
                    "The lumap {v} update failed too often; it will be offered again later."
                ))
            }
        }
    }

    /// One step: starts the work the state asks for, applies what the worker finished, and
    /// returns a line for the status bar when something changed. Cheap when idle.
    pub fn tick(
        &mut self,
        settings: &mut UpdateSettings,
        moment: Moment,
        now_unix: u64,
    ) -> Option<String> {
        let mut message = None;
        if let Some(event) = self.worker.poll() {
            message = self.apply(settings, event, now_unix);
        }
        if self.worker.busy() || self.closing {
            return message;
        }
        // A click is an explicit request: it overrides automatic-update preferences and the
        // startup delay, but never the live-show/projector safety restriction.
        if self.check_requested {
            if self.running.is_none() {
                self.check_requested = false;
                let detail = "This is a development build; updates come from the repository.";
                self.check_feedback = Some(detail.into());
                return Some(detail.into());
            }
            if !moment.show_mode && !moment.outputs_open {
                self.check_requested = false;
                self.manual_in_flight = true;
                self.check_in_flight = true;
                if let Some(current) = self.running {
                    self.worker.start_check(current);
                }
            }
            return message;
        }
        let due = check_due(
            settings
                .last_check
                .map(|s| UNIX_EPOCH + Duration::from_secs(s)),
            UNIX_EPOCH + Duration::from_secs(now_unix),
            flow::CHECK_EVERY,
        );
        if let Some(current) = self.running {
            if flow::should_check(settings, moment, due, false) {
                self.check_in_flight = true;
                self.worker.start_check(current);
                return message;
            }
        }
        if flow::should_download(settings, moment) {
            if let Some(p) = &settings.pending {
                self.worker.start_download(p.clone(), self.dir.clone());
            }
        }
        message
    }

    fn apply(
        &mut self,
        settings: &mut UpdateSettings,
        event: Event,
        now_unix: u64,
    ) -> Option<String> {
        match event {
            Event::Checked(Ok(offer)) => {
                self.check_in_flight = false;
                let manual = std::mem::take(&mut self.manual_in_flight);
                settings.last_check = Some(now_unix);
                let news = flow::after_check(settings, offer.as_ref());
                if settings.pending.is_none() {
                    worker::discard_downloads(&self.dir);
                }
                let result = settings.pending.as_ref().map_or_else(
                    || "No newer installable release found. You're up to date.".to_string(),
                    |p| format!("lumap {} is available.", p.version),
                );
                self.check_feedback = Some(result.clone());
                news.or_else(|| manual.then_some(result))
            }
            Event::Checked(Err(why)) => {
                self.check_in_flight = false;
                let manual = std::mem::take(&mut self.manual_in_flight);
                log::info!("update check failed: {why}");
                let result = format!("Couldn't check for updates: {why}");
                self.check_feedback = Some(result.clone());
                manual.then_some(result)
            }
            Event::Downloaded(Ok(_)) => flow::download_succeeded(settings),
            Event::Downloaded(Err(why)) => Some(flow::download_failed(settings, &why)),
        }
    }

    /// Acts on a click in the status bar. Returns a line for the status bar.
    pub fn request(
        &mut self,
        settings: &mut UpdateSettings,
        request: Request,
        now_unix: u64,
    ) -> Option<String> {
        match request {
            Request::Download => {
                flow::request_download(settings);
                None
            }
            Request::Install => {
                let p = settings.pending.clone()?;
                if p.stage != Stage::Ready {
                    return None;
                }
                let installer = worker::installer_path(&self.dir, &p);
                if !installer.is_file() {
                    if let Some(p) = settings.pending.as_mut() {
                        p.stage = Stage::Downloading;
                    }
                    return Some("The installer went missing; downloading it again.".into());
                }
                match worker::launch_installer(&installer) {
                    Ok(()) => {
                        flow::start_install(settings);
                        self.closing = true;
                        Some(format!(
                            "Installing lumap {}; lumap will reopen.",
                            p.version
                        ))
                    }
                    Err(why) => Some(flow::download_failed(settings, &why)),
                }
            }
            Request::Later => {
                flow::remind_later(settings, now_unix);
                None
            }
            Request::Skip => {
                flow::skip(settings);
                worker::discard_downloads(&self.dir);
                None
            }
            Request::CheckNow => {
                self.check_requested = true;
                self.check_feedback = None;
                settings.quiet_until = None;
                // An explicit check lets the user reconsider a version previously skipped.
                settings.skipped = None;
                Some("Checking for a newer lumap.".into())
            }
        }
    }

    /// The live state of the one-click update controls.
    pub fn check_ui(&self, moment: Moment) -> CheckUi {
        let safe = !moment.show_mode && !moment.outputs_open;
        let checking = self.check_requested || self.check_in_flight;
        let enabled = safe && !checking && !self.worker.busy() && !self.closing;
        let detail = if !safe {
            "Close projector windows and leave show mode to check for updates.".to_string()
        } else if checking {
            "Checking GitHub releases…".to_string()
        } else if self.worker.busy() {
            "An update is downloading and being verified…".to_string()
        } else {
            self.check_feedback.clone().unwrap_or_else(|| {
                "Check GitHub releases for a newer version. Installing always asks first.".into()
            })
        };
        CheckUi {
            checking,
            enabled,
            detail,
        }
    }

    /// What the status bar shows.
    pub fn banner(settings: &UpdateSettings, moment: Moment, now_unix: u64) -> Banner {
        flow::banner(settings, moment, now_unix)
    }

    /// The moment, from what the editor knows and when it launched.
    pub fn moment(&self, show_mode: bool, outputs_open: bool, now: Instant) -> Moment {
        Moment {
            show_mode,
            outputs_open,
            since_launch: now.saturating_duration_since(self.launched),
        }
    }
}

impl Default for Updater {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use flow::Mode;
    use offer::{sha256_hex, LATEST_RELEASE_API};
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    #[derive(Default, Clone)]
    struct Fake {
        texts: Arc<Mutex<HashMap<String, String>>>,
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    }

    impl Fetcher for Fake {
        fn text(&self, url: &str) -> Result<String, String> {
            self.texts
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| format!("404 {url}"))
        }

        fn download(&self, url: &str, to: &Path) -> Result<(), String> {
            let body = self
                .files
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| format!("404 {url}"))?;
            std::fs::write(to, body).map_err(|e| e.to_string())
        }
    }

    const EXE: &str =
        "https://github.com/tochi-mba/lumap/releases/download/v0.4.0/lumap-Setup-0.4.0.exe";
    const SHA: &str =
        "https://github.com/tochi-mba/lumap/releases/download/v0.4.0/lumap-Setup-0.4.0.exe.sha256";

    fn fake_with_release() -> Fake {
        let fake = Fake::default();
        let body = b"the installer".to_vec();
        fake.texts.lock().unwrap().insert(
            LATEST_RELEASE_API.into(),
            format!(r#"{{"tag_name":"v0.4.0","assets":[{{"name":"lumap-Setup-0.4.0.exe","browser_download_url":"{EXE}","size":13}},{{"name":"lumap-Setup-0.4.0.exe.sha256","browser_download_url":"{SHA}"}}]}}"#),
        );
        fake.texts.lock().unwrap().insert(
            SHA.into(),
            format!("{}  lumap-Setup-0.4.0.exe", sha256_hex(&body)),
        );
        fake.files.lock().unwrap().insert(EXE.into(), body);
        fake
    }

    fn calm() -> Moment {
        Moment {
            show_mode: false,
            outputs_open: false,
            since_launch: flow::STARTUP_DELAY,
        }
    }

    /// Ticks until the worker is idle and returns every message in order.
    fn settle(u: &mut Updater, s: &mut UpdateSettings, moment: Moment, now: u64) -> Vec<String> {
        let mut out = Vec::new();
        for _ in 0..50 {
            if u.worker.busy() {
                if let Some(event) = u.worker.wait(Duration::from_secs(10)) {
                    out.extend(u.apply(s, event, now));
                }
            }
            match u.tick(s, moment, now) {
                Some(m) => out.push(m),
                None if !u.worker.busy() => return out,
                None => {}
            }
        }
        out
    }

    #[test]
    fn notify_mode_checks_offers_downloads_on_request_and_installs_on_request() {
        let dir = tempfile::tempdir().unwrap();
        let mut u = Updater::with(
            fake_with_release(),
            dir.path().join("updates"),
            Version::parse("0.3.0"),
            Instant::now(),
        );
        let mut s = UpdateSettings::default();
        assert_eq!(u.startup(&mut s), None);
        let now = 1_700_000_000;

        assert!(u
            .tick(
                &mut s,
                Moment {
                    show_mode: true,
                    ..calm()
                },
                now
            )
            .is_none());
        assert!(!u.worker.busy(), "nothing happens during a show");

        assert_eq!(
            settle(&mut u, &mut s, calm(), now),
            vec!["lumap 0.4.0 is available.".to_string()]
        );
        assert_eq!(s.last_check, Some(now));
        assert_eq!(
            Updater::banner(&s, calm(), now),
            Banner::Available("lumap 0.4.0 (1 MB)".into())
        );
        assert!(
            settle(&mut u, &mut s, calm(), now).is_empty(),
            "a day has not passed"
        );

        assert_eq!(
            u.request(&mut s, Request::Install, now),
            None,
            "nothing to install yet"
        );
        u.request(&mut s, Request::Download, now);
        assert_eq!(
            settle(&mut u, &mut s, calm(), now),
            vec!["lumap 0.4.0 is ready to install.".to_string()]
        );
        assert_eq!(
            Updater::banner(&s, calm(), now),
            Banner::Ready("lumap 0.4.0 is ready".into())
        );
        assert!(dir
            .path()
            .join("updates")
            .join("lumap-Setup-0.4.0.exe")
            .is_file());

        // The "installer" is not a program, so launching it fails and the failure is counted.
        let message = u.request(&mut s, Request::Install, now).unwrap();
        assert!(message.contains("failed"), "{message}");
        assert!(!u.closing);
        assert_eq!(s.pending.as_ref().unwrap().failures, 1);

        u.request(&mut s, Request::Later, now);
        assert_eq!(Updater::banner(&s, calm(), now), Banner::None);
        u.request(&mut s, Request::Skip, now);
        assert!(s.pending.is_none());
        assert!(!dir
            .path()
            .join("updates")
            .join("lumap-Setup-0.4.0.exe")
            .exists());
        assert_eq!(
            u.request(&mut s, Request::CheckNow, now),
            Some("Checking for a newer lumap.".into())
        );
        assert!(
            settle(&mut u, &mut s, calm(), now).is_empty(),
            "the skipped version is not offered again"
        );
        assert!(format!("{u:?}").contains("closing: false"));
    }

    #[test]
    fn download_mode_downloads_by_itself_and_a_missing_installer_is_fetched_again() {
        let dir = tempfile::tempdir().unwrap();
        let fake = fake_with_release();
        let mut u = Updater::with(
            fake.clone(),
            dir.path().join("updates"),
            Version::parse("0.3.0"),
            Instant::now(),
        );
        let mut s = UpdateSettings {
            mode: Mode::Download,
            ..UpdateSettings::default()
        };
        let now = 1_700_000_000;
        assert_eq!(
            settle(&mut u, &mut s, calm(), now),
            vec![
                "lumap 0.4.0 is available.".to_string(),
                "lumap 0.4.0 is ready to install.".to_string()
            ]
        );
        std::fs::remove_file(dir.path().join("updates").join("lumap-Setup-0.4.0.exe")).unwrap();
        assert_eq!(
            u.request(&mut s, Request::Install, now),
            Some("The installer went missing; downloading it again.".into())
        );
        assert_eq!(
            settle(&mut u, &mut s, calm(), now),
            vec!["lumap 0.4.0 is ready to install.".to_string()]
        );

        // The release is withdrawn: the next check drops the offer and the file.
        fake.texts.lock().unwrap().insert(
            LATEST_RELEASE_API.into(),
            r#"{"tag_name":"v0.3.0","assets":[]}"#.into(),
        );
        u.request(&mut s, Request::CheckNow, now);
        assert_eq!(
            settle(&mut u, &mut s, calm(), now),
            vec!["That update is no longer offered.".to_string()]
        );
        assert!(!dir
            .path()
            .join("updates")
            .join("lumap-Setup-0.4.0.exe")
            .exists());
    }

    #[test]
    fn failures_are_logged_not_shown_and_a_bad_download_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let mut u = Updater::with(
            fake.clone(),
            dir.path().to_path_buf(),
            Version::parse("0.3.0"),
            Instant::now(),
        );
        let mut s = UpdateSettings::default();
        assert!(
            settle(&mut u, &mut s, calm(), 5).is_empty(),
            "a failed check says nothing"
        );
        assert_eq!(s.last_check, None);

        let good = fake_with_release();
        fake.texts.lock().unwrap().insert(
            LATEST_RELEASE_API.into(),
            good.texts.lock().unwrap()[LATEST_RELEASE_API].clone(),
        );
        fake.texts.lock().unwrap().insert(SHA.into(), "0000000000000000000000000000000000000000000000000000000000000000  lumap-Setup-0.4.0.exe".into());
        fake.files.lock().unwrap().insert(EXE.into(), b"x".to_vec());
        u.request(&mut s, Request::CheckNow, 5);
        settle(&mut u, &mut s, calm(), 5);
        u.request(&mut s, Request::Download, 5);
        let messages = settle(&mut u, &mut s, calm(), 5);
        assert!(messages[0].contains("does not match"), "{messages:?}");
        assert_eq!(s.pending.as_ref().unwrap().failures, 1);
    }

    #[test]
    fn a_development_build_never_updates_but_says_so_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let mut u = Updater::with(
            fake_with_release(),
            dir.path().to_path_buf(),
            None,
            Instant::now(),
        );
        let mut s = UpdateSettings::default();
        assert!(settle(&mut u, &mut s, calm(), 5).is_empty());
        assert_eq!(u.running(), None);
        u.request(&mut s, Request::CheckNow, 5);
        assert_eq!(
            u.tick(&mut s, calm(), 5),
            Some("This is a development build; updates come from the repository.".into())
        );
    }

    #[test]
    fn startup_news_and_the_moment() {
        let dir = tempfile::tempdir().unwrap();
        let launched = Instant::now();
        let u = Updater::with(
            Fake::default(),
            dir.path().to_path_buf(),
            Version::parse("0.4.0"),
            launched,
        );
        let mut s = UpdateSettings {
            pending: Some(flow::Pending {
                version: "0.4.0".into(),
                stage: Stage::Installing,
                ..flow::Pending::default()
            }),
            ..UpdateSettings::default()
        };
        std::fs::write(dir.path().join("lumap-Setup-0.4.0.exe"), b"x").unwrap();
        assert_eq!(u.startup(&mut s), Some("Updated to lumap 0.4.0.".into()));
        assert!(!dir.path().join("lumap-Setup-0.4.0.exe").exists());

        s.pending = Some(flow::Pending {
            version: "0.5.0".into(),
            stage: Stage::Installing,
            ..flow::Pending::default()
        });
        assert_eq!(
            u.startup(&mut s),
            Some("The lumap 0.5.0 update did not finish; it is ready to try again.".into())
        );
        s.pending.as_mut().unwrap().stage = Stage::Installing;
        s.pending.as_mut().unwrap().failures = flow::MOST_FAILURES - 1;
        assert_eq!(
            u.startup(&mut s),
            Some("The lumap 0.5.0 update failed too often; it will be offered again later.".into())
        );

        let m = u.moment(true, false, launched + Duration::from_secs(3));
        assert_eq!(
            m,
            Moment {
                show_mode: true,
                outputs_open: false,
                since_launch: Duration::from_secs(3)
            }
        );
        assert!(!m.allows_update_work());
        assert!(now_unix() > 1_600_000_000);
        assert!(downloads_dir().ends_with("updates"));
    }
}
