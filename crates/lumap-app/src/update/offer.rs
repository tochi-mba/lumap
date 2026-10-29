//! What the latest release offers, and whether a downloaded installer is the one it published.
//!
//! Pure: the GitHub release JSON comes in as text and decisions come out. The worker does the
//! network and the files.

use serde::Deserialize;
use std::fmt::{self, Write as _};
use std::time::{Duration, SystemTime};

/// Where releases are asked about: this repository's latest, never a pre-release.
pub const LATEST_RELEASE_API: &str = "https://api.github.com/repos/tochi-mba/lumap/releases/latest";
/// Only installers published by this repository are ever run.
pub const DOWNLOAD_PREFIX: &str = "https://github.com/tochi-mba/lumap/releases/download/";
/// The releases page, for the "see what changed" link when the JSON lacks one.
pub const RELEASES_PAGE: &str = "https://github.com/tochi-mba/lumap/releases/latest";

/// A released version: exactly `major.minor.patch`. Anything else is a developer's own build,
/// which never updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// Parses `x.y.z` (a leading `v` is allowed). `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().trim_start_matches('v').split('.');
        let mut next = || parts.next()?.parse::<u32>().ok();
        let v = Self(next()?, next()?, next()?);
        parts.next().is_none().then_some(v)
    }

    /// The version this binary was built as, when it is a release version.
    pub fn running() -> Option<Self> {
        Self::parse(env!("CARGO_PKG_VERSION"))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// A newer version and where its installer and checksum are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The newer version.
    pub version: Version,
    /// `lumap-Setup-<version>.exe`.
    pub installer_name: String,
    /// Where the installer is.
    pub installer_url: String,
    /// Where its `.sha256` is.
    pub checksum_url: String,
    /// The release page.
    pub page_url: String,
    /// Installer size in bytes as the release states it (0 when it does not say).
    pub size: u64,
    /// Release notes, as plain text.
    pub notes: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

impl Offer {
    /// The offer in the API's description of the latest release, when it is newer than
    /// `current` and carries the installer with its checksum, both hosted by this repository.
    /// `None` otherwise, and for anything that is not such a description.
    pub fn from_release_json(json: &str, current: Version) -> Option<Self> {
        let release: Release = serde_json::from_str(json).ok()?;
        let version = Version::parse(&release.tag_name)?;
        if version <= current || release.prerelease {
            return None;
        }
        let installer_name = format!("lumap-Setup-{version}.exe");
        let checksum_name = format!("{installer_name}.sha256");
        let hosted = |a: &&Asset| a.browser_download_url.starts_with(DOWNLOAD_PREFIX);
        let installer = release
            .assets
            .iter()
            .filter(hosted)
            .find(|a| a.name == installer_name)?;
        let checksum = release
            .assets
            .iter()
            .filter(hosted)
            .find(|a| a.name == checksum_name)?;
        Some(Self {
            version,
            installer_url: installer.browser_download_url.clone(),
            checksum_url: checksum.browser_download_url.clone(),
            page_url: release
                .html_url
                .filter(|u| u.starts_with("https://github.com/"))
                .unwrap_or_else(|| RELEASES_PAGE.to_string()),
            size: installer.size,
            notes: plain_notes(release.body.as_deref().unwrap_or_default()),
            installer_name,
        })
    }
}

/// Release notes in Markdown as plain text: headings lose their marks, list items become
/// bullets, emphasis and link syntax go, generated credits and the "Full Changelog" line are
/// dropped.
pub fn plain_notes(markdown: &str) -> String {
    let mut lines = Vec::new();
    for raw in markdown.lines() {
        let mut line = raw.trim().to_string();
        if line.is_empty() || line.starts_with("**Full Changelog**") || line.starts_with("<!--") {
            continue;
        }
        line = line.trim_start_matches('#').trim().to_string();
        if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            line = format!("{} {}", '\u{2022}', rest.trim());
        }
        if let Some(i) = line.find(" by @") {
            if line[i..].contains(" in https://") {
                line.truncate(i);
            }
        }
        line = strip_links(&line).replace("**", "").replace('`', "");
        lines.push(line);
    }
    lines.join("\n")
}

/// `[text](https://...)` becomes `text`.
fn strip_links(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](") else {
            break;
        };
        let Some(end) = rest[open + close..].find(')') else {
            break;
        };
        let text = &rest[open + 1..open + close];
        out.push_str(&rest[..open]);
        out.push_str(text);
        rest = &rest[open + close + end + 1..];
    }
    out.push_str(rest);
    out
}

/// Whether a check is due: never checked, a day since the last one, or the clock was put back
/// (a last check in the future could mean no check for weeks).
pub fn check_due(last_check: Option<SystemTime>, now: SystemTime, every: Duration) -> bool {
    match last_check {
        None => true,
        Some(last) => match now.duration_since(last) {
            Ok(since) => since >= every,
            Err(_) => now + Duration::from_hours(1) < last,
        },
    }
}

/// Whether `installer` matches the checksum file's hash for `installer_name` (`hash  name`,
/// `hash *name`, or the hash alone), ignoring case.
pub fn verify_sha256(installer: &[u8], checksum_text: &str, installer_name: &str) -> bool {
    let expected = checksum_text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let mut parts = l.splitn(2, ' ');
            let hash = parts.next()?;
            let name = parts.next().map(|n| n.trim().trim_start_matches('*'));
            (name.is_none() || name == Some(installer_name)).then_some(hash)
        })
        .find(|h| h.len() == 64);
    let Some(expected) = expected else {
        return false;
    };
    sha256_hex(installer).eq_ignore_ascii_case(expected)
}

/// SHA-256 of `data` as lowercase hex. A small, dependency-free implementation: the installer
/// is hashed once per update, so speed does not matter and a crate would be more surface than
/// it is worth.
#[allow(clippy::too_many_lines)]
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(v);
        }
    }
    h.iter().fold(String::with_capacity(64), |mut s, v| {
        let _ = write!(s, "{v:08x}");
        s
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool, assets: &[(&str, &str)]) -> String {
        let assets: Vec<String> = assets
            .iter()
            .map(|(n, u)| {
                format!(r#"{{"name":"{n}","browser_download_url":"{u}","size":1234567}}"#)
            })
            .collect();
        format!(
            r###"{{"tag_name":"{tag}","prerelease":{prerelease},"html_url":"https://github.com/tochi-mba/lumap/releases/tag/{tag}","body":"## What\n- Faster [blends](https://x.y/z) by @me in https://github.com/tochi-mba/lumap/pull/9\n**Full Changelog**: x","assets":[{}]}}"###,
            assets.join(",")
        )
    }

    const EXE: &str =
        "https://github.com/tochi-mba/lumap/releases/download/v0.4.0/lumap-Setup-0.4.0.exe";
    const SHA: &str =
        "https://github.com/tochi-mba/lumap/releases/download/v0.4.0/lumap-Setup-0.4.0.exe.sha256";

    #[test]
    fn versions_parse_only_as_three_numbers() {
        assert_eq!(Version::parse("v1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(Version::parse(" 0.10.0 "), Some(Version(0, 10, 0)));
        for bad in ["1.2", "1.2.3.4", "1.2.x", "", "0.3.0-build.5"] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
        assert!(Version(0, 4, 0) > Version(0, 3, 9));
        assert_eq!(Version(1, 2, 3).to_string(), "1.2.3");
        assert!(Version::running().is_some());
    }

    #[test]
    fn an_offer_needs_a_newer_release_with_a_hosted_installer_and_checksum() {
        let json = release(
            "v0.4.0",
            false,
            &[
                ("lumap-Setup-0.4.0.exe", EXE),
                ("lumap-Setup-0.4.0.exe.sha256", SHA),
                ("lumap-0.4.0-windows-x64.zip", EXE),
            ],
        );
        let offer = Offer::from_release_json(&json, Version(0, 3, 0)).unwrap();
        assert_eq!(offer.version, Version(0, 4, 0));
        assert_eq!(offer.installer_name, "lumap-Setup-0.4.0.exe");
        assert_eq!(offer.installer_url, EXE);
        assert_eq!(offer.checksum_url, SHA);
        assert_eq!(offer.size, 1_234_567);
        assert_eq!(
            offer.page_url,
            "https://github.com/tochi-mba/lumap/releases/tag/v0.4.0"
        );
        assert_eq!(offer.notes, "What\n\u{2022} Faster blends");

        assert!(
            Offer::from_release_json(&json, Version(0, 4, 0)).is_none(),
            "same version"
        );
        assert!(
            Offer::from_release_json(&json, Version(1, 0, 0)).is_none(),
            "older release"
        );
        let pre = release(
            "v0.4.0",
            true,
            &[
                ("lumap-Setup-0.4.0.exe", EXE),
                ("lumap-Setup-0.4.0.exe.sha256", SHA),
            ],
        );
        assert!(
            Offer::from_release_json(&pre, Version(0, 3, 0)).is_none(),
            "pre-release"
        );
        let no_sum = release("v0.4.0", false, &[("lumap-Setup-0.4.0.exe", EXE)]);
        assert!(
            Offer::from_release_json(&no_sum, Version(0, 3, 0)).is_none(),
            "no checksum"
        );
        let foreign = release(
            "v0.4.0",
            false,
            &[
                (
                    "lumap-Setup-0.4.0.exe",
                    "https://evil.example/lumap-Setup-0.4.0.exe",
                ),
                ("lumap-Setup-0.4.0.exe.sha256", SHA),
            ],
        );
        assert!(
            Offer::from_release_json(&foreign, Version(0, 3, 0)).is_none(),
            "foreign host"
        );
        let odd_tag = release(
            "build-12",
            false,
            &[
                ("lumap-Setup-0.4.0.exe", EXE),
                ("lumap-Setup-0.4.0.exe.sha256", SHA),
            ],
        );
        assert!(
            Offer::from_release_json(&odd_tag, Version(0, 3, 0)).is_none(),
            "tag is not a version"
        );
        assert!(Offer::from_release_json("not json", Version(0, 3, 0)).is_none());
        assert!(Offer::from_release_json("[]", Version(0, 3, 0)).is_none());
        let no_page = r#"{"tag_name":"v0.4.0","assets":[{"name":"lumap-Setup-0.4.0.exe","browser_download_url":"EXE"},{"name":"lumap-Setup-0.4.0.exe.sha256","browser_download_url":"SHA"}]}"#
            .replace("EXE", EXE)
            .replace("SHA", SHA);
        let offer = Offer::from_release_json(&no_page, Version(0, 3, 0)).unwrap();
        assert_eq!(offer.page_url, RELEASES_PAGE);
        assert_eq!(offer.size, 0);
        assert_eq!(offer.notes, "");
    }

    #[test]
    fn notes_become_plain_text() {
        let md = "# lumap 0.4.0\r\n\r\n* **Bezier** meshes in `show` mode by @x in https://github.com/t/l/pull/1\n- See [the manual](https://a.b/c) and [this](https://d.e)\n<!-- hidden -->\n**Full Changelog**: https://x";
        assert_eq!(
            plain_notes(md),
            "lumap 0.4.0\n\u{2022} Bezier meshes in show mode\n\u{2022} See the manual and this"
        );
        assert_eq!(strip_links("[broken](no-close"), "[broken](no-close");
        assert_eq!(strip_links("plain"), "plain");
    }

    #[test]
    fn checks_are_due_daily_and_when_the_clock_went_back() {
        let day = Duration::from_hours(24);
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        assert!(check_due(None, now, day));
        assert!(check_due(Some(now - day), now, day));
        assert!(!check_due(
            Some(now - day + Duration::from_secs(1)),
            now,
            day
        ));
        assert!(
            !check_due(Some(now + Duration::from_mins(10)), now, day),
            "a little ahead is clock skew"
        );
        assert!(
            check_due(Some(now + Duration::from_hours(2)), now, day),
            "far ahead means the clock was put back"
        );
    }

    #[test]
    fn sha256_matches_the_reference_vectors_and_checksum_files() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = vec![b'a'; 1000];
        assert_eq!(
            sha256_hex(&long),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
        let hash = sha256_hex(b"installer bytes");
        assert!(verify_sha256(
            b"installer bytes",
            &format!("{hash}  lumap-Setup-0.4.0.exe\n"),
            "lumap-Setup-0.4.0.exe"
        ));
        assert!(verify_sha256(
            b"installer bytes",
            &format!("{} *lumap-Setup-0.4.0.exe", hash.to_uppercase()),
            "lumap-Setup-0.4.0.exe"
        ));
        assert!(verify_sha256(
            b"installer bytes",
            &hash,
            "lumap-Setup-0.4.0.exe"
        ));
        assert!(
            !verify_sha256(
                b"installer bytes",
                &format!("{hash}  other.exe"),
                "lumap-Setup-0.4.0.exe"
            ),
            "names a different file"
        );
        assert!(!verify_sha256(
            b"other bytes",
            &hash,
            "lumap-Setup-0.4.0.exe"
        ));
        assert!(!verify_sha256(
            b"installer bytes",
            "short  lumap-Setup-0.4.0.exe",
            "lumap-Setup-0.4.0.exe"
        ));
        assert!(!verify_sha256(
            b"installer bytes",
            "",
            "lumap-Setup-0.4.0.exe"
        ));
    }
}
