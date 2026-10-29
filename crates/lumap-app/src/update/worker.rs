//! The network and file side of updating, on a background thread: ask GitHub for the latest
//! release, download the installer and its checksum, verify, keep the verified installer on
//! disk, and hand it to Windows when the user says so.
//!
//! Fetching goes through [`Fetcher`]. The default one runs `curl`, which Windows 10 1803+ (the
//! installer's floor), macOS and nearly every Linux ship, so lumap carries no HTTP stack; the
//! tests use a fake.

use super::flow::Pending;
use super::offer::{verify_sha256, Offer, Version, LATEST_RELEASE_API};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, Sender};

/// Something that fetches URLs.
pub trait Fetcher: Send + 'static {
    /// The body at `url` as text.
    fn text(&self, url: &str) -> Result<String, String>;
    /// Saves the body at `url` to `to`.
    fn download(&self, url: &str, to: &Path) -> Result<(), String>;
}

/// `curl` from the PATH, following redirects, failing on HTTP errors, with a time limit.
#[derive(Debug, Clone, Default)]
pub struct Curl;

impl Curl {
    fn command(url: &str, max_seconds: u32) -> Command {
        let mut c = Command::new("curl");
        c.args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            &max_seconds.to_string(),
            "--user-agent",
            concat!("lumap/", env!("CARGO_PKG_VERSION")),
            "--header",
            "Accept: application/vnd.github+json",
            url,
        ]);
        c
    }
}

impl Fetcher for Curl {
    fn text(&self, url: &str) -> Result<String, String> {
        let out = Self::command(url, 30)
            .output()
            .map_err(|e| format!("cannot run curl: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        String::from_utf8(out.stdout).map_err(|e| e.to_string())
    }

    fn download(&self, url: &str, to: &Path) -> Result<(), String> {
        let out = Self::command(url, 1800)
            .arg("--output")
            .arg(to)
            .output()
            .map_err(|e| format!("cannot run curl: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
    }
}

/// What the worker reports back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A check finished: the offer, or none, or why it failed.
    Checked(Result<Option<Offer>, String>),
    /// A download finished: where the verified installer is, or why it failed.
    Downloaded(Result<PathBuf, String>),
}

/// Asks about the latest release.
pub fn check(fetcher: &dyn Fetcher, current: Version) -> Result<Option<Offer>, String> {
    let json = fetcher.text(LATEST_RELEASE_API)?;
    Ok(Offer::from_release_json(&json, current))
}

/// Where a pending update's installer lives.
pub fn installer_path(dir: &Path, pending: &Pending) -> PathBuf {
    dir.join(&pending.installer_name)
}

/// Downloads the installer and its checksum into `dir`, verifies them, and leaves only the
/// verified installer behind. A verified installer already there is reused.
pub fn download(fetcher: &dyn Fetcher, pending: &Pending, dir: &Path) -> Result<PathBuf, String> {
    if !pending
        .installer_url
        .starts_with(super::offer::DOWNLOAD_PREFIX)
        || !pending
            .checksum_url
            .starts_with(super::offer::DOWNLOAD_PREFIX)
    {
        return Err("the installer is not hosted by the lumap repository".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let checksum = fetcher.text(&pending.checksum_url)?;
    let target = installer_path(dir, pending);
    if let Ok(bytes) = std::fs::read(&target) {
        if verify_sha256(&bytes, &checksum, &pending.installer_name) {
            return Ok(target);
        }
        let _ = std::fs::remove_file(&target);
    }
    let part = target.with_extension("exe.part");
    let result = fetcher
        .download(&pending.installer_url, &part)
        .and_then(|()| {
            let bytes = std::fs::read(&part).map_err(|e| e.to_string())?;
            if !verify_sha256(&bytes, &checksum, &pending.installer_name) {
                return Err("the downloaded installer does not match its checksum".into());
            }
            std::fs::rename(&part, &target).map_err(|e| e.to_string())?;
            Ok(target.clone())
        });
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// Deletes every downloaded installer; called when an offer is withdrawn, skipped or installed.
pub fn discard_downloads(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("lumap-Setup-") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// The arguments the installer is run with: silent with a progress bar, close the running
/// lumap, and reopen it afterwards.
pub const INSTALL_ARGS: [&str; 5] = [
    "/SILENT",
    "/SUPPRESSMSGBOXES",
    "/NORESTART",
    "/CLOSEAPPLICATIONS",
    "/relaunch=1",
];

/// Starts the verified installer and returns at once; the caller closes the app.
pub fn launch_installer(installer: &Path) -> Result<(), String> {
    Command::new(installer)
        .args(INSTALL_ARGS)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start {}: {e}", installer.display()))
}

/// A background thread doing one job at a time, reporting through a channel.
pub struct Worker {
    tx: Sender<Event>,
    rx: Receiver<Event>,
    fetcher: std::sync::Arc<dyn Fetcher + Sync>,
    busy: bool,
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worker")
            .field("busy", &self.busy)
            .finish_non_exhaustive()
    }
}

impl Worker {
    /// A worker that fetches with `fetcher`.
    pub fn new(fetcher: impl Fetcher + Sync) -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            fetcher: std::sync::Arc::new(fetcher),
            busy: false,
        }
    }

    /// Whether a job is running.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Starts a check unless a job is running.
    pub fn start_check(&mut self, current: Version) {
        if self.busy {
            return;
        }
        self.busy = true;
        let (tx, fetcher) = (self.tx.clone(), std::sync::Arc::clone(&self.fetcher));
        self.spawn(move || {
            let _ = tx.send(Event::Checked(check(fetcher.as_ref(), current)));
        });
    }

    /// Starts a download unless a job is running.
    pub fn start_download(&mut self, pending: Pending, dir: PathBuf) {
        if self.busy {
            return;
        }
        self.busy = true;
        let (tx, fetcher) = (self.tx.clone(), std::sync::Arc::clone(&self.fetcher));
        self.spawn(move || {
            let _ = tx.send(Event::Downloaded(download(
                fetcher.as_ref(),
                &pending,
                &dir,
            )));
        });
    }

    fn spawn(&self, job: impl FnOnce() + Send + 'static) {
        if std::thread::Builder::new()
            .name("lumap-update".into())
            .spawn(job)
            .is_err()
        {
            let _ = self
                .tx
                .send(Event::Checked(Err("cannot start the update thread".into())));
        }
    }

    /// The next finished job, if any. Never blocks.
    pub fn poll(&mut self) -> Option<Event> {
        let event = self.rx.try_recv().ok()?;
        self.busy = false;
        Some(event)
    }

    /// Waits up to `timeout` for the next finished job.
    #[cfg(test)]
    pub fn wait(&mut self, timeout: std::time::Duration) -> Option<Event> {
        let event = self.rx.recv_timeout(timeout).ok()?;
        self.busy = false;
        Some(event)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::update::offer::sha256_hex;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Duration;

    #[derive(Default)]
    struct Fake {
        texts: Mutex<HashMap<String, Result<String, String>>>,
        files: Mutex<HashMap<String, Vec<u8>>>,
        downloads: Mutex<u32>,
    }

    impl Fetcher for Fake {
        fn text(&self, url: &str) -> Result<String, String> {
            self.texts
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .unwrap_or_else(|| Err(format!("404 {url}")))
        }

        fn download(&self, url: &str, to: &Path) -> Result<(), String> {
            *self.downloads.lock().unwrap() += 1;
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

    fn release_json() -> String {
        format!(
            r#"{{"tag_name":"v0.4.0","assets":[{{"name":"lumap-Setup-0.4.0.exe","browser_download_url":"{EXE}","size":9}},{{"name":"lumap-Setup-0.4.0.exe.sha256","browser_download_url":"{SHA}"}}]}}"#
        )
    }

    fn pending() -> Pending {
        Pending {
            version: "0.4.0".into(),
            installer_name: "lumap-Setup-0.4.0.exe".into(),
            installer_url: EXE.into(),
            checksum_url: SHA.into(),
            ..Pending::default()
        }
    }

    #[test]
    fn a_check_parses_the_release_or_reports_the_failure() {
        let fake = Fake::default();
        assert_eq!(
            check(&fake, Version(0, 3, 0)),
            Err(format!("404 {LATEST_RELEASE_API}"))
        );
        fake.texts
            .lock()
            .unwrap()
            .insert(LATEST_RELEASE_API.into(), Ok(release_json()));
        let offer = check(&fake, Version(0, 3, 0)).unwrap().unwrap();
        assert_eq!(offer.version, Version(0, 4, 0));
        assert_eq!(check(&fake, Version(0, 4, 0)).unwrap(), None);
    }

    #[test]
    fn a_download_is_verified_reused_and_cleaned_up() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let body = b"installer".to_vec();
        fake.files.lock().unwrap().insert(EXE.into(), body.clone());
        assert!(
            download(&fake, &pending(), dir.path())
                .unwrap_err()
                .contains("404"),
            "no checksum yet"
        );
        fake.texts.lock().unwrap().insert(
            SHA.into(),
            Ok(format!("{}  lumap-Setup-0.4.0.exe\n", sha256_hex(&body))),
        );

        let path = download(&fake, &pending(), dir.path()).unwrap();
        assert_eq!(path, dir.path().join("lumap-Setup-0.4.0.exe"));
        assert_eq!(std::fs::read(&path).unwrap(), body);
        assert!(!dir.path().join("lumap-Setup-0.4.0.exe.part").exists());
        assert_eq!(*fake.downloads.lock().unwrap(), 1);

        assert_eq!(download(&fake, &pending(), dir.path()).unwrap(), path);
        assert_eq!(
            *fake.downloads.lock().unwrap(),
            1,
            "a verified installer is reused"
        );

        std::fs::write(&path, b"tampered").unwrap();
        download(&fake, &pending(), dir.path()).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            body,
            "a tampered file is replaced"
        );
        assert_eq!(*fake.downloads.lock().unwrap(), 2);

        fake.files
            .lock()
            .unwrap()
            .insert(EXE.into(), b"wrong".to_vec());
        std::fs::remove_file(&path).unwrap();
        let err = download(&fake, &pending(), dir.path()).unwrap_err();
        assert!(err.contains("does not match"), "{err}");
        assert!(!path.exists());
        assert!(!dir.path().join("lumap-Setup-0.4.0.exe.part").exists());

        let foreign = Pending {
            installer_url: "https://evil.example/x.exe".into(),
            ..pending()
        };
        assert!(download(&fake, &foreign, dir.path())
            .unwrap_err()
            .contains("not hosted"));

        std::fs::write(dir.path().join("lumap-Setup-0.3.0.exe"), b"old").unwrap();
        std::fs::write(dir.path().join("keep.txt"), b"x").unwrap();
        discard_downloads(dir.path());
        assert!(!dir.path().join("lumap-Setup-0.3.0.exe").exists());
        assert!(dir.path().join("keep.txt").exists());
        discard_downloads(&dir.path().join("missing"));
    }

    #[test]
    fn the_worker_runs_one_job_at_a_time_and_reports_back() {
        let fake = Fake::default();
        fake.texts
            .lock()
            .unwrap()
            .insert(LATEST_RELEASE_API.into(), Ok(release_json()));
        let mut w = Worker::new(fake);
        assert!(!w.busy());
        assert!(w.poll().is_none());
        w.start_check(Version(0, 3, 0));
        assert!(w.busy());
        w.start_check(Version(0, 3, 0));
        let event = w.wait(Duration::from_secs(10)).unwrap();
        assert!(matches!(event, Event::Checked(Ok(Some(_)))), "{event:?}");
        assert!(!w.busy());
        assert!(
            w.wait(Duration::from_millis(10)).is_none(),
            "only one job ran"
        );

        let dir = tempfile::tempdir().unwrap();
        w.start_download(pending(), dir.path().to_path_buf());
        let event = w.wait(Duration::from_secs(10)).unwrap();
        assert!(matches!(event, Event::Downloaded(Err(_))), "{event:?}");
        assert!(format!("{w:?}").contains("busy: false"));
    }

    #[test]
    fn curl_reports_a_bad_url_as_an_error() {
        // A URL curl cannot resolve, so this passes offline too. Skipped when curl is absent.
        if Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        assert!(Curl.text("http://invalid.invalid/x").is_err());
        let dir = tempfile::tempdir().unwrap();
        assert!(Curl
            .download("http://invalid.invalid/x", &dir.path().join("x"))
            .is_err());
        assert!(launch_installer(&dir.path().join("missing-Setup.exe")).is_err());
    }
}
