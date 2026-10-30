//! Pure text parsing: the camera lists ffmpeg prints on each platform, the `/dev/video*` nodes
//! of Linux, and the stream size in ffmpeg's banner. Nothing here runs a process, so every
//! parser is tested on every platform against recorded output.

use super::CameraInfo;

/// Drops ffmpeg's log context (`[dshow @ 0000020a4a3b0f00] `, `[in#0 @ 0x...] `,
/// `[AVFoundation indev @ 0x7f...] `) from the start of a line.
fn strip_context(line: &str) -> &str {
    let line = line.trim_end_matches(['\r', '\n']);
    if let Some(rest) = line.strip_prefix('[') {
        if let Some((context, after)) = rest.split_once("] ") {
            if context.contains(" @ ") {
                return after;
            }
        }
    }
    line
}

/// Adds a camera unless one with the same id is already listed.
fn push_unique(out: &mut Vec<CameraInfo>, name: &str, id: &str) {
    if !out.iter().any(|c| c.id == id) {
        out.push(CameraInfo::new(name, id));
    }
}

/// Cameras in the stderr of `ffmpeg -list_devices true -f dshow -i dummy`.
///
/// Reads both layouts ffmpeg has used: the current one, where each device line ends in its
/// media types (`"Integrated Camera" (video)`), and the older one, where devices sit under
/// "DirectShow video devices" and "DirectShow audio devices" headings. Audio devices, devices
/// with no video, and the "Alternative name" lines are skipped. On DirectShow the name is also
/// the id ffmpeg opens (`-i video=<name>`).
pub fn parse_dshow(stderr: &str) -> Vec<CameraInfo> {
    let mut out = Vec::new();
    let mut video_section = false;
    for raw in stderr.lines() {
        let line = strip_context(raw).trim();
        if line.contains("DirectShow video devices") {
            video_section = true;
        } else if line.contains("DirectShow audio devices") {
            video_section = false;
        } else if let Some(quoted) = line.strip_prefix('"') {
            let Some(end) = quoted.rfind('"') else {
                continue;
            };
            let (name, kinds) = (&quoted[..end], quoted[end + 1..].trim());
            let video = if kinds.is_empty() {
                video_section
            } else {
                kinds
                    .trim_start_matches('(')
                    .trim_end_matches(')')
                    .split(',')
                    .any(|k| k.trim() == "video")
            };
            if video && !name.is_empty() {
                push_unique(&mut out, name, name);
            }
        }
    }
    out
}

/// Cameras in the stderr of `ffmpeg -f avfoundation -list_devices true -i ""`.
///
/// Only the "AVFoundation video devices" section counts, and the screens it also lists
/// ("Capture screen 0") are left out because they are not cameras. The id is the device's
/// index, which ffmpeg opens as `-i <index>:none`.
pub fn parse_avfoundation(stderr: &str) -> Vec<CameraInfo> {
    let mut out = Vec::new();
    let mut video_section = false;
    for raw in stderr.lines() {
        let line = strip_context(raw).trim();
        if line.contains("AVFoundation video devices") {
            video_section = true;
        } else if line.contains("AVFoundation audio devices") {
            video_section = false;
        } else if video_section {
            let entry = line
                .strip_prefix('[')
                .and_then(|rest| rest.split_once("] "))
                .filter(|(index, name)| {
                    !index.is_empty()
                        && index.bytes().all(|b| b.is_ascii_digit())
                        && !name.trim().is_empty()
                        && !name.starts_with("Capture screen")
                });
            if let Some((index, name)) = entry {
                push_unique(&mut out, name.trim(), index);
            }
        }
    }
    out
}

/// The number in `.../videoN`, if the path names a Video4Linux node.
fn video_node_number(path: &str) -> Option<u64> {
    let name = path.rsplit(['/', '\\']).next()?;
    let digits = name.strip_prefix("video")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Cameras from a list of device paths (the entries of `/dev`): every `videoN` node, sorted by
/// its number so `video10` comes after `video2`. Other paths are ignored. The path is both the
/// name and the id.
pub fn parse_v4l2(paths: &[String]) -> Vec<CameraInfo> {
    let mut nodes: Vec<(u64, &String)> = paths
        .iter()
        .filter_map(|p| video_node_number(p).map(|n| (n, p)))
        .collect();
    nodes.sort();
    let mut out = Vec::new();
    for (_, path) in nodes {
        push_unique(&mut out, path, path);
    }
    out
}

/// A `WIDTHxHEIGHT` token with both sides positive and written without leading zeros, so the
/// hex codec tags in the same line (`0x41424752`) never match.
fn parse_size(token: &str) -> Option<(u32, u32)> {
    let (w, h) = token.split_once('x')?;
    let number = |s: &str| {
        (!s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u32>().ok())
            .flatten()
    };
    Some((number(w)?, number(h)?))
}

/// The frame size in one `Stream #0:0: Video: ...` line of ffmpeg's banner.
pub(super) fn video_size(line: &str) -> Option<(u32, u32)> {
    let line = line.trim();
    if !line.starts_with("Stream #") {
        return None;
    }
    let (_, video) = line.split_once("Video:")?;
    video
        .split([' ', ',', '(', ')', '[', ']'])
        .find_map(parse_size)
}

/// Follows ffmpeg's banner line by line and reports the size of the stream it writes: the first
/// video stream after the `Output #0` heading, which is what arrives on stdout.
#[derive(Debug, Default)]
pub(super) struct Banner {
    output: bool,
}

impl Banner {
    /// Feeds one stderr line; returns the output size once it appears.
    pub(super) fn feed(&mut self, line: &str) -> Option<(u32, u32)> {
        let line = strip_context(line).trim();
        if line.starts_with("Output #") {
            self.output = true;
        } else if line.starts_with("Input #") {
            self.output = false;
        } else if self.output {
            return video_size(line);
        }
        None
    }
}
