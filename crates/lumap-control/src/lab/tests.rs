//! The server: security, the HTTP round trip with a test thread playing the app, timeouts,
#![allow(clippy::unwrap_used, clippy::duration_suboptimal_units)]
//! shutdown, limits, the token and the session file.

use super::server::{
    authorize, new_token, os_thread, read_body, session_dir_from, wait, Config, Spawn,
};
use super::{session_dir, LabRequest, LabServer, Pending, Reply};
use serde_json::json;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really";

/// Generous timeouts and a small body limit.
fn patient() -> Config {
    Config {
        reply: Duration::from_secs(20),
        long_reply: Duration::from_secs(20),
        max_body: 1024,
        spawn: os_thread,
    }
}

/// A lab whose waker signals the returned receiver, so a test waits for each request.
fn lab(config: Config) -> (LabServer, Receiver<()>) {
    let lab = LabServer::bind_with(0, config).unwrap();
    let (tx, woke) = channel();
    lab.set_waker(move || {
        let _ = tx.send(());
    });
    (lab, woke)
}

/// The next request, once the waker says one is there.
fn next(lab: &LabServer, woke: &Receiver<()>) -> Pending {
    woke.recv_timeout(Duration::from_secs(20)).unwrap();
    let mut pending = lab.drain();
    assert_eq!(pending.len(), 1);
    pending.remove(0)
}

/// One HTTP answer.
struct Answer {
    status: u16,
    content_type: String,
    body: Vec<u8>,
}

impl Answer {
    fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap()
    }

    fn error(&self) -> String {
        self.json()["error"].as_str().unwrap().to_owned()
    }
}

/// A raw HTTP client, cheap to send to another thread.
#[derive(Clone)]
struct Client {
    addr: SocketAddr,
    token: String,
}

impl Client {
    fn of(lab: &LabServer) -> Self {
        Self {
            addr: lab.addr(),
            token: lab.token().to_owned(),
        }
    }

    /// Good `Host` and token header lines.
    fn auth(&self) -> String {
        format!(
            "Host: 127.0.0.1:{}\r\nX-Lumap-Lab: {}\r\n",
            self.addr.port(),
            self.token
        )
    }

    fn call(&self, method: &str, path: &str, headers: &str, body: &str) -> Answer {
        let mut stream = TcpStream::connect(self.addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&raw[..split]).to_string();
        Answer {
            status: head.split_whitespace().nth(1).unwrap().parse().unwrap(),
            content_type: head
                .lines()
                .find_map(|line| line.strip_prefix("Content-Type: "))
                .unwrap_or_default()
                .to_owned(),
            body: raw[split + 4..].to_vec(),
        }
    }

    fn get(&self, path: &str) -> Answer {
        self.call("GET", path, &self.auth(), "")
    }

    fn post(&self, path: &str, body: &str) -> Answer {
        self.call("POST", path, &self.auth(), body)
    }

    /// Runs a request on its own thread while the test plays the app.
    fn spawn(&self, call: impl FnOnce(&Self) -> Answer + Send + 'static) -> JoinHandle<Answer> {
        let client = self.clone();
        std::thread::spawn(move || call(&client))
    }
}

#[test]
fn the_app_answers_with_json_png_and_errors() {
    let (lab, woke) = lab(patient());
    let client = Client::of(&lab);
    assert!(lab.drain().is_empty());

    let state = client.spawn(|c| c.get("/lab/state"));
    let pending = next(&lab, &woke);
    assert_eq!(pending.request, LabRequest::State);
    pending.reply(Reply::Json(json!({"step": 2})));
    let answer = state.join().unwrap();
    assert_eq!(
        (answer.status, answer.content_type.as_str()),
        (200, "application/json")
    );
    assert_eq!(answer.json(), json!({"step": 2}));

    let snap = client.spawn(|c| c.get("/lab/camera.png?frames=5"));
    let pending = next(&lab, &woke);
    assert_eq!(
        pending.request,
        LabRequest::Camera {
            settle_ms: 300,
            frames: 5
        }
    );
    pending.reply(Reply::Png(PNG.to_vec()));
    let answer = snap.join().unwrap();
    assert_eq!(
        (answer.status, answer.content_type.as_str()),
        (200, "image/png")
    );
    assert_eq!(answer.body, PNG);

    let act = client.spawn(|c| c.post("/lab/act", r#"[{"do":"go","cue":"c9"}]"#));
    let pending = next(&lab, &woke);
    assert!(matches!(&pending.request, LabRequest::Act(actions) if actions.len() == 1));
    pending.reply(Reply::Error(409, "no cue c9".into()));
    let answer = act.join().unwrap();
    assert_eq!(answer.status, 409);
    assert_eq!(answer.json(), json!({"error": "no cue c9"}));
}

#[test]
fn ping_needs_no_app() {
    let (lab, _woke) = lab(patient());
    let client = Client::of(&lab);
    let ping = client.get("/lab/ping");
    assert_eq!(ping.status, 200);
    let body = ping.json();
    assert_eq!(body["ok"], true);
    assert_eq!(body["lumap"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["pid"], std::process::id());
    assert!(body["since_drain_ms"].is_null(), "the app never drained");
    assert!(lab.drain().is_empty());
    assert!(client.get("/lab/ping").json()["since_drain_ms"].is_u64());
}

#[test]
fn strangers_and_web_pages_are_turned_away() {
    let (lab, _woke) = lab(patient());
    let client = Client::of(&lab);
    let port = lab.addr().port();
    let host = format!("Host: 127.0.0.1:{port}\r\n");
    let token = format!("X-Lumap-Lab: {}\r\n", lab.token());
    let status = |headers: String| client.call("GET", "/lab/ping", &headers, "").status;
    assert_eq!(status(host.clone()), 401, "no token");
    assert_eq!(status(format!("{host}X-Lumap-Lab: 0123\r\n")), 401);
    assert_eq!(
        status(format!("{host}{token}Origin: http://evil.example\r\n")),
        403
    );
    assert_eq!(status(format!("Host: evil.example:{port}\r\n{token}")), 403);
    assert_eq!(status(format!("Host: localhost:{port}\r\n{token}")), 200);
    assert_eq!(status(format!("{host}{token}")), 200);
    let answer = client.call("GET", "/lab/state", &host, "");
    assert!(answer.error().contains("X-Lumap-Lab"));
    assert!(lab.drain().is_empty(), "nothing reached the app");
    assert_eq!(lab.addr().ip(), Ipv4Addr::LOCALHOST);
}

#[test]
fn routing_errors_and_limits_come_back_as_json() {
    let (lab, _woke) = lab(patient());
    let client = Client::of(&lab);
    assert_eq!(client.get("/lab/nope").status, 404);
    assert_eq!(client.get("/lab/act").status, 405);
    assert_eq!(client.post("/lab/ping", "").status, 405);
    let bad = client.post("/lab/act", "[{");
    assert_eq!(bad.status, 400);
    assert!(bad.error().contains("EOF"), "{}", bad.error());
    let big = client.post("/lab/act", &"x".repeat(2000));
    assert_eq!(
        (big.status, big.content_type.as_str()),
        (413, "application/json")
    );
    assert!(lab.drain().is_empty());
}

#[test]
fn an_app_that_never_answers_times_out() {
    let (lab, woke) = lab(Config {
        reply: Duration::from_millis(100),
        ..patient()
    });
    let started = Instant::now();
    let answer = Client::of(&lab).get("/lab/state");
    assert_eq!(answer.status, 504);
    assert!(started.elapsed() >= Duration::from_millis(100));
    assert!(answer.error().contains("did not answer"));
    // Answering late does nothing.
    next(&lab, &woke).reply(Reply::Json(json!({})));
}

#[test]
fn a_dropped_request_is_a_500() {
    let (lab, woke) = lab(patient());
    let ui = Client::of(&lab).spawn(|c| c.get("/lab/ui.png"));
    drop(next(&lab, &woke));
    let answer = ui.join().unwrap();
    assert_eq!(answer.status, 500);
    assert!(answer.error().contains("without answering"));
}

#[test]
fn dropping_the_lab_answers_waiting_clients() {
    let (lab, woke) = lab(patient());
    let scan = Client::of(&lab).spawn(|c| c.post("/lab/scan", ""));
    woke.recv_timeout(Duration::from_secs(20)).unwrap();
    drop(lab);
    assert_eq!(scan.join().unwrap().status, 503);
}

#[test]
fn bind_reports_a_taken_port_and_missing_threads() {
    let first = LabServer::bind(0).unwrap();
    let taken = LabServer::bind(first.addr().port()).unwrap_err();
    assert!(taken.contains("cannot start the lab on"), "{taken}");
    let refuse: Spawn = |_, _| Err(std::io::Error::other("no threads today"));
    let err = LabServer::bind_with(
        0,
        Config {
            spawn: refuse,
            ..patient()
        },
    )
    .unwrap_err();
    assert!(err.contains("no threads today"), "{err}");
}

#[test]
fn tokens_are_128_random_bits_in_hex_and_stay_out_of_logs() {
    let (a, b) = (new_token(), new_token());
    assert_eq!(a.len(), 32);
    assert!(a
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_ne!(a, b);
    let lab = LabServer::bind(0).unwrap();
    assert_eq!(lab.token().len(), 32);
    let shown = format!("{lab:?}");
    assert!(
        shown.contains("LabServer") && !shown.contains(lab.token()),
        "{shown}"
    );
    assert_eq!(lab.url(), format!("http://127.0.0.1:{}", lab.addr().port()));
}

#[test]
fn the_session_file_says_where_and_how() {
    let lab = LabServer::bind(0).unwrap();
    let dir = std::env::temp_dir().join(format!("lumap-lab-test-{}", new_token()));
    let path = lab.write_session(&dir.join("lab")).unwrap();
    assert_eq!(path, dir.join("lab").join("session.json"));
    let text = std::fs::read_to_string(&path).unwrap();
    let session: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        session,
        json!({
            "url": lab.url(),
            "port": lab.addr().port(),
            "token": lab.token(),
            "pid": std::process::id(),
        })
    );
    assert!(lab.write_session(&path).is_err(), "a file is in the way");
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(
        lab.write_session(&dir.join("lab")).is_err(),
        "a folder is in the way"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_session_folder_matches_the_client() {
    fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }
    let lab_in = |base: PathBuf| Some(base.join("lumap").join("lab"));
    assert_eq!(
        session_dir_from(&env(&[("LOCALAPPDATA", "C:/Local"), ("HOME", "/h")])),
        lab_in(PathBuf::from("C:/Local"))
    );
    assert_eq!(
        session_dir_from(&env(&[("XDG_DATA_HOME", "/data"), ("HOME", "/h")])),
        lab_in(PathBuf::from("/data"))
    );
    assert_eq!(
        session_dir_from(&env(&[
            ("LOCALAPPDATA", ""),
            ("XDG_DATA_HOME", ""),
            ("HOME", "/h")
        ])),
        lab_in(PathBuf::from("/h").join(".local").join("share"))
    );
    assert_eq!(session_dir_from(&env(&[])), None);
    let _ = session_dir();
}

#[test]
fn authorize_wants_a_local_host_no_origin_and_one_right_token() {
    let check = |headers: &[(&str, &str)]| {
        let owned: Vec<(String, String)> = headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        authorize(&owned, "abc", 7414).map_err(|(status, _)| status)
    };
    let host = ("Host", "127.0.0.1:7414");
    let token = ("X-Lumap-Lab", "abc");
    assert_eq!(check(&[host, token]), Ok(()));
    assert_eq!(
        check(&[("host", "LOCALHOST:7414"), ("x-lumap-lab", " abc ")]),
        Ok(())
    );
    assert_eq!(check(&[("Host", "127.0.0.1:7415"), token]), Err(403));
    assert_eq!(check(&[("Host", "evil.example"), token]), Err(403));
    assert_eq!(check(&[token]), Err(403), "no Host");
    assert_eq!(check(&[host, ("Host", "evil"), token]), Err(403), "two");
    assert_eq!(check(&[host, ("origin", "null"), token]), Err(403));
    assert_eq!(check(&[host]), Err(401));
    assert_eq!(check(&[host, ("X-Lumap-Lab", "abd")]), Err(401));
    assert_eq!(check(&[host, ("X-Lumap-Lab", "abcd")]), Err(401));
    assert_eq!(check(&[host, token, token]), Err(401), "two tokens");
}

struct Broken;

impl Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("cable cut"))
    }
}

#[test]
fn bodies_are_limited() {
    assert_eq!(
        read_body(&mut &b"hello"[..], Some(5), 5),
        Ok(b"hello".to_vec())
    );
    assert_eq!(
        read_body(&mut &b"hello!"[..], Some(6), 5).unwrap_err().0,
        413
    );
    assert_eq!(
        read_body(&mut &b"hello!"[..], None, 5).unwrap_err().0,
        413,
        "a body without a length is counted"
    );
    let broken = read_body(&mut Broken, None, 5).unwrap_err();
    assert_eq!(broken.0, 400);
    assert!(broken.1.contains("cable cut"));
}

#[test]
fn waiting_for_the_app() {
    let stop = AtomicBool::new(false);
    let (pending, answer) = Pending::new(LabRequest::State);
    pending.reply(Reply::Json(json!(1)));
    assert_eq!(
        wait(&answer, Duration::from_secs(5), &stop),
        Reply::Json(json!(1))
    );
    let (_pending, answer) = Pending::new(LabRequest::State);
    assert_eq!(
        wait(&answer, Duration::from_millis(120), &stop).status(),
        504
    );
    let (pending, answer) = Pending::new(LabRequest::State);
    drop(pending);
    assert_eq!(wait(&answer, Duration::from_secs(5), &stop).status(), 500);
    stop.store(true, Ordering::SeqCst);
    let (_pending, answer) = Pending::new(LabRequest::State);
    assert_eq!(wait(&answer, Duration::from_secs(5), &stop).status(), 503);
}

#[test]
fn timeouts_and_statuses() {
    let config = Config::DEFAULT;
    assert_eq!(config.timeout(&LabRequest::State), Duration::from_secs(30));
    for long in [
        LabRequest::Scan { projector: None },
        LabRequest::Record { seconds: 5.0 },
        LabRequest::Realign,
    ] {
        assert_eq!(config.timeout(&long), Duration::from_secs(900));
    }
    assert_eq!(config.max_body, 8 * 1024 * 1024);
    assert_eq!(Reply::Json(json!(null)).status(), 200);
    assert_eq!(Reply::Png(Vec::new()).status(), 200);
    assert_eq!(Reply::Error(404, String::new()).status(), 404);
    assert_eq!(Reply::Error(599, String::new()).status(), 599);
    assert_eq!(Reply::Error(42, String::new()).status(), 500);
    assert_eq!(Reply::Error(600, String::new()).status(), 500);
}
