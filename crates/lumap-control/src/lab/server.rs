//! The lab's HTTP server: `127.0.0.1` only, every request checked for a local `Host`, no
//! `Origin` and the session token, and a few worker threads that hand requests to the app and
//! wait for its answers.

use super::routes::{dispatch, Dispatch, Refusal};
use super::{LabRequest, Pending, Reply};
use serde_json::json;
use std::collections::hash_map::RandomState;
use std::ffi::OsString;
use std::hash::{BuildHasher, Hasher};
use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The port `lumap --lab` listens on unless `--lab-port` says otherwise.
pub const DEFAULT_PORT: u16 = 7414;

/// Requests the server works on at once; a scan holds one worker for as long as it runs.
const WORKERS: usize = 4;

/// How often a worker waiting for the app checks whether the lab is shutting down.
const SLICE: Duration = Duration::from_millis(50);

/// Starts a named thread.
pub(crate) type Spawn = fn(String, Box<dyn FnOnce() + Send>) -> std::io::Result<JoinHandle<()>>;

/// Server settings. Tests shorten the timeouts and swap the thread spawner.
#[derive(Clone, Copy)]
pub(crate) struct Config {
    /// How long a client waits for the app.
    pub(crate) reply: Duration,
    /// How long a client waits for a scan, a recording or a re-align.
    pub(crate) long_reply: Duration,
    /// Largest request body, in bytes.
    pub(crate) max_body: usize,
    /// Starts the worker threads.
    pub(crate) spawn: Spawn,
}

impl Config {
    /// What `lumap --lab` uses.
    pub(crate) const DEFAULT: Self = Self {
        reply: Duration::from_secs(30),
        long_reply: Duration::from_secs(15 * 60),
        max_body: 8 << 20,
        spawn: os_thread,
    };

    /// How long a client waits for the app's answer to `request`.
    pub(crate) fn timeout(&self, request: &LabRequest) -> Duration {
        match request {
            LabRequest::Scan { .. } | LabRequest::Record { .. } | LabRequest::Realign => {
                self.long_reply
            }
            _ => self.reply,
        }
    }
}

pub(crate) fn os_thread(
    name: String,
    work: Box<dyn FnOnce() + Send>,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new().name(name).spawn(work)
}

/// What the server threads share with the [`LabServer`].
#[derive(Default)]
struct Shared {
    /// Set when the server is dropped.
    stop: AtomicBool,
    /// Called after a request is queued for the app.
    waker: Mutex<Option<Box<dyn Fn() + Send>>>,
    /// When the app last took requests.
    drained: Mutex<Option<Instant>>,
}

impl Shared {
    fn wake(&self) {
        if let Ok(waker) = self.waker.lock() {
            if let Some(wake) = waker.as_ref() {
                wake();
            }
        }
    }
}

/// The lab's HTTP server. Dropping it stops the threads; clients still waiting for the app get a
/// `503`.
pub struct LabServer {
    addr: SocketAddr,
    token: String,
    rx: Receiver<Pending>,
    shared: Arc<Shared>,
    server: Arc<tiny_http::Server>,
    workers: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for LabServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the token: this ends up in logs.
        f.debug_struct("LabServer")
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

impl LabServer {
    /// Binds `127.0.0.1:port` (`0` picks a free port), makes a fresh token and starts serving.
    pub fn bind(port: u16) -> Result<Self, String> {
        Self::bind_with(port, Config::DEFAULT)
    }

    pub(crate) fn bind_with(port: u16, config: Config) -> Result<Self, String> {
        let wanted = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let server = Arc::new(
            tiny_http::Server::http(wanted)
                .map_err(|e| format!("cannot start the lab on {wanted}: {e}"))?,
        );
        let addr = server.server_addr().to_ip().unwrap_or(wanted);
        let token = new_token();
        let (queue, rx) = channel();
        let shared = Arc::new(Shared::default());
        let worker = Worker {
            server: Arc::clone(&server),
            token: token.as_str().into(),
            port: addr.port(),
            config,
            queue,
            shared: Arc::clone(&shared),
        };
        let started = (0..WORKERS)
            .map(|i| {
                let worker = worker.clone();
                (config.spawn)(format!("lumap-lab-{i}"), Box::new(move || worker.serve()))
            })
            .collect::<std::io::Result<Vec<_>>>();
        let workers = match started {
            Ok(workers) => workers,
            Err(e) => {
                // Workers that did start leave on these.
                for _ in 0..WORKERS {
                    server.unblock();
                }
                return Err(format!("cannot start the lab's threads: {e}"));
            }
        };
        log::info!("lab listening on http://{addr}");
        Ok(Self {
            addr,
            token,
            rx,
            shared,
            server,
            workers,
        })
    }

    /// The bound address, always on `127.0.0.1`.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// The token clients send in the `X-Lumap-Lab` header: 128 random bits as 32 hex digits.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Calls `wake` from a server thread whenever a request starts waiting for the app, so an
    /// idle editor repaints and drains it (in egui: `move || ctx.request_repaint()`).
    pub fn set_waker(&self, wake: impl Fn() + Send + 'static) {
        if let Ok(mut slot) = self.shared.waker.lock() {
            *slot = Some(Box::new(wake));
        }
    }

    /// Requests waiting for the app; call once per frame. Answer each with [`Pending::reply`],
    /// now or in a later frame.
    pub fn drain(&self) -> Vec<Pending> {
        if let Ok(mut at) = self.shared.drained.lock() {
            *at = Some(Instant::now());
        }
        self.rx.try_iter().collect()
    }

    /// Writes `session.json` (`url`, `port`, `token` and `pid`) into `dir`, creating the
    /// folder, and returns the file's path.
    pub fn write_session(&self, dir: &Path) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("session.json");
        let session = json!({
            "url": self.url(),
            "port": self.addr.port(),
            "token": self.token,
            "pid": std::process::id(),
        });
        std::fs::write(&path, format!("{session:#}\n"))?;
        Ok(path)
    }
}

impl Drop for LabServer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        for _ in &self.workers {
            self.server.unblock();
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// The folder `lumap --lab` writes `session.json` to, and where `scripts/lab/lab.py` looks for
/// it: `%LOCALAPPDATA%\lumap\lab` where `LOCALAPPDATA` is set (Windows), else
/// `$XDG_DATA_HOME/lumap/lab`, else `~/.local/share/lumap/lab`. `None` without any of them.
pub fn session_dir() -> Option<PathBuf> {
    session_dir_from(&|name| std::env::var_os(name))
}

/// [`session_dir`] over any environment.
pub(crate) fn session_dir_from(var: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let get = |name: &str| {
        var(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let base = get("LOCALAPPDATA")
        .or_else(|| get("XDG_DATA_HOME"))
        .or_else(|| get("HOME").map(|home| home.join(".local").join("share")))?;
    Some(base.join("lumap").join("lab"))
}

/// One server thread's view of the lab.
#[derive(Clone)]
struct Worker {
    server: Arc<tiny_http::Server>,
    token: Arc<str>,
    port: u16,
    config: Config,
    queue: Sender<Pending>,
    shared: Arc<Shared>,
}

impl Worker {
    /// Serves until the server is unblocked for shutdown.
    fn serve(self) {
        while let Ok(request) = self.server.recv() {
            self.handle(request);
        }
    }

    fn handle(&self, mut request: tiny_http::Request) {
        let reply = self
            .answer(&mut request)
            .unwrap_or_else(|(status, message)| Reply::Error(status, message));
        log::debug!(
            "lab: {} {} -> {}",
            request.method(),
            request.url(),
            reply.status()
        );
        respond(request, reply);
    }

    fn answer(&self, request: &mut tiny_http::Request) -> Result<Reply, Refusal> {
        let headers: Vec<(String, String)> = request
            .headers()
            .iter()
            .map(|h| (h.field.as_str().to_string(), h.value.to_string()))
            .collect();
        authorize(&headers, &self.token, self.port)?;
        let declared = request.body_length();
        let body = read_body(request.as_reader(), declared, self.config.max_body)?;
        Ok(
            match dispatch(request.method().as_str(), request.url(), &body)? {
                Dispatch::Ping => Reply::Json(self.ping()),
                Dispatch::App(lab_request) => self.ask(lab_request),
            },
        )
    }

    /// Alive, which lumap, and how long since the app last took requests (`null` before its
    /// first frame; a number that keeps growing means the app is stuck).
    fn ping(&self) -> serde_json::Value {
        let drained = self.shared.drained.lock().ok().and_then(|at| *at);
        json!({
            "ok": true,
            "lumap": env!("CARGO_PKG_VERSION"),
            "pid": std::process::id(),
            "since_drain_ms": drained.map(|at| at.elapsed().as_millis() as u64),
        })
    }

    fn ask(&self, request: LabRequest) -> Reply {
        let timeout = self.config.timeout(&request);
        let (pending, answer) = Pending::new(request);
        // This only fails once the server is gone, and then `wait` sees the closed channel.
        let _ = self.queue.send(pending);
        self.shared.wake();
        wait(&answer, timeout, &self.shared.stop)
    }
}

/// Waits for the app's answer: `504` after `timeout`, `500` when the app dropped the request,
/// `503` when the lab shuts down first.
pub(crate) fn wait(answer: &Receiver<Reply>, timeout: Duration, stop: &AtomicBool) -> Reply {
    let deadline = Instant::now() + timeout;
    loop {
        if stop.load(Ordering::SeqCst) {
            return Reply::Error(503, "the lab is shutting down".into());
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Reply::Error(
                504,
                format!("lumap did not answer within {timeout:?}; GET /lab/ping shows whether it is still taking requests"),
            );
        }
        match answer.recv_timeout(left.min(SLICE)) {
            Ok(reply) => return reply,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Reply::Error(500, "lumap dropped the request without answering it".into())
            }
        }
    }
}

fn respond(request: tiny_http::Request, reply: Reply) {
    let status = reply.status();
    let (mime, body) = match reply {
        Reply::Json(value) => ("application/json", value.to_string().into_bytes()),
        Reply::Png(png) => ("image/png", png),
        Reply::Error(_, message) => (
            "application/json",
            json!({ "error": message }).to_string().into_bytes(),
        ),
    };
    let mut response = tiny_http::Response::from_data(body).with_status_code(status);
    for header in [format!("Content-Type: {mime}"), "Cache-Control: no-store".into()] {
        if let Ok(header) = header.parse::<tiny_http::Header>() {
            response.add_header(header);
        }
    }
    // A client that hung up has nothing left to be told.
    let _ = request.respond(response);
}

/// Checks a request's headers: a local `Host` (DNS rebinding), no `Origin` (browsers send one,
/// so web pages are refused) and the session token. Header names match in any case.
pub(crate) fn authorize(headers: &[(String, String)], token: &str, port: u16) -> Result<(), Refusal> {
    let local = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !one(headers, "Host").is_some_and(|host| local.iter().any(|l| l.eq_ignore_ascii_case(host))) {
        return Err((
            403,
            format!("the Host header must be {} or {}", local[0], local[1]),
        ));
    }
    if headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("Origin")) {
        return Err((
            403,
            "requests from web pages (with an Origin header) are refused".into(),
        ));
    }
    if !one(headers, "X-Lumap-Lab").is_some_and(|given| same(given.as_bytes(), token.as_bytes())) {
        return Err((
            401,
            "missing or wrong X-Lumap-Lab token; it is in the lab's session.json".into(),
        ));
    }
    Ok(())
}

/// The value of a header that is there exactly once.
fn one<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let mut values = headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim());
    match (values.next(), values.next()) {
        (Some(value), None) => Some(value),
        _ => None,
    }
}

/// Compares in a time that does not depend on where the inputs differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// Reads a request body of at most `max` bytes: `413` beyond that, `400` when reading fails.
pub(crate) fn read_body(
    reader: &mut dyn Read,
    declared: Option<usize>,
    max: usize,
) -> Result<Vec<u8>, Refusal> {
    let too_big = || (413, format!("request bodies are limited to {max} bytes"));
    if declared.is_some_and(|n| n > max) {
        return Err(too_big());
    }
    let mut body = Vec::new();
    match reader.take(max as u64 + 1).read_to_end(&mut body) {
        Ok(_) if body.len() > max => Err(too_big()),
        Ok(_) => Ok(body),
        Err(e) => Err((400, format!("cannot read the request body: {e}"))),
    }
}

/// A random 128-bit token as 32 hex digits: two `SipHash` hashes, each keyed by a fresh
/// [`RandomState`] (which the OS seeds), over the time, the process id and a counter.
pub(crate) fn new_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let half = |which: u8| {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(nanos);
        hasher.write_u32(std::process::id());
        hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        hasher.write_u8(which);
        hasher.finish()
    };
    format!("{:016x}{:016x}", half(0), half(1))
}
