//! The local API other front ends drive Spotlite through.
//!
//! The running app is the server: it owns the Spotify session, playback,
//! the queue and the library, and a client — the Swift app in `apple/` —
//! asks it for what to show and tells it what the person did.
//!
//! The transport is a Unix socket in the app's state folder that only the
//! user can open. Each side writes one JSON object per line:
//!
//! - a request: `{"id": 1, "method": "playlist.get", "params": {"id": "…"}}`
//! - its reply: `{"id": 1, "result": …}` or `{"id": 1, "error": "…"}`
//! - an event the server pushes unasked: `{"event": "player", "data": …}`
//!
//! The listener and the connections run on their own threads and never
//! touch app state: requests queue up for the app's next pass, which
//! answers them (`App::serve_api`) and hands each reply back to the
//! connection's writer.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::backend::Waker;

/// A request waiting for the app's next pass.
pub struct Request {
    pub client: u64,
    pub id: Value,
    pub method: String,
    pub params: Value,
}

#[derive(serde::Deserialize)]
struct Incoming {
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Default)]
struct Shared {
    requests: Vec<Request>,
    /// Each connection's outgoing lines.
    clients: Vec<(u64, Sender<String>)>,
}

/// The listening socket, and what its connections asked for.
pub struct Server {
    shared: Arc<Mutex<Shared>>,
    path: PathBuf,
}

static NEXT_CLIENT: AtomicU64 = AtomicU64::new(1);

/// Set once the front end that started this process has gone.
static PARENT_GONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

unsafe extern "C" {
    fn getppid() -> i32;
}

/// Watches the process that started this one, the front end that keeps
/// this app as its engine, and wakes the app once it has gone, so the
/// engine never plays on after the front end quits or crashes.
pub fn exit_with_parent(waker: Waker) {
    // SAFETY: getppid has no preconditions and cannot fail.
    let parent = unsafe { getppid() };
    let _ = std::thread::Builder::new()
        .name("parent-watch".into())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                // An orphan is handed to launchd, so its parent changes.
                // SAFETY: as above.
                if unsafe { getppid() } != parent {
                    log::info!("The front end that started Spotlite has gone; quitting");
                    PARENT_GONE.store(true, std::sync::atomic::Ordering::Relaxed);
                    waker.wake();
                    return;
                }
            }
        });
}

/// Whether the front end that started this process has gone.
pub fn parent_gone() -> bool {
    PARENT_GONE.load(std::sync::atomic::Ordering::Relaxed)
}

impl Server {
    /// Listens at `path`, replacing a socket a previous run left behind.
    pub fn start(path: &Path, waker: Waker) -> std::io::Result<Server> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // A live server answers; a dead one's socket is only a file.
        if UnixStream::connect(path).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "another Spotlite is serving the API",
            ));
        }
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path)?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        let shared = Arc::new(Mutex::new(Shared::default()));
        let accept_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("api-listener".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    connect(stream, &accept_shared, waker.clone());
                }
            })?;
        log::info!("API listening at {}", path.display());
        Ok(Server {
            shared,
            path: path.to_owned(),
        })
    }

    /// The requests that arrived since the last call.
    pub fn take_requests(&self) -> Vec<Request> {
        self.lock()
            .map_or_else(Vec::new, |mut shared| std::mem::take(&mut shared.requests))
    }

    pub fn has_clients(&self) -> bool {
        self.lock().is_some_and(|shared| !shared.clients.is_empty())
    }

    /// Answers a request.
    pub fn reply(&self, request: &Request, result: Result<Value, String>) {
        let line = match result {
            Ok(result) => serde_json::json!({ "id": request.id, "result": result }),
            Err(error) => serde_json::json!({ "id": request.id, "error": error }),
        };
        self.send(Some(request.client), &line);
    }

    /// Pushes an event to every connection.
    pub fn broadcast(&self, event: &str, data: Value) {
        self.send(None, &serde_json::json!({ "event": event, "data": data }));
    }

    fn send(&self, to: Option<u64>, line: &Value) {
        let Ok(text) = serde_json::to_string(line) else {
            return;
        };
        if let Some(mut shared) = self.lock() {
            // A closed connection's writer is gone; forget it.
            shared.clients.retain(|(client, sender)| {
                to.is_some_and(|to| to != *client) || sender.send(text.clone()).is_ok()
            });
        }
    }

    fn lock(&self) -> Option<std::sync::MutexGuard<'_, Shared>> {
        self.shared.lock().ok()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Reads one connection's requests on a thread of its own, and writes its
/// replies and events on another.
fn connect(stream: UnixStream, shared: &Arc<Mutex<Shared>>, waker: Waker) {
    let client = NEXT_CLIENT.fetch_add(1, Ordering::Relaxed);
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let (sender, lines) = channel::<String>();
    if let Ok(mut shared) = shared.lock() {
        shared.clients.push((client, sender));
    }
    let _ = std::thread::Builder::new()
        .name(format!("api-writer-{client}"))
        .spawn(move || {
            for line in lines {
                if writer
                    .write_all(line.as_bytes())
                    .and_then(|()| writer.write_all(b"\n"))
                    .is_err()
                {
                    break;
                }
            }
        });
    let shared = Arc::clone(shared);
    let reader_waker = waker.clone();
    let _ = std::thread::Builder::new()
        .name(format!("api-reader-{client}"))
        .spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else {
                    break;
                };
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(incoming) = serde_json::from_str::<Incoming>(&line) else {
                    log::warn!("API: ignored a line that is not a request");
                    continue;
                };
                if let Ok(mut shared) = shared.lock() {
                    shared.requests.push(Request {
                        client,
                        id: incoming.id,
                        method: incoming.method,
                        params: incoming.params,
                    });
                }
                reader_waker.wake();
            }
            if let Ok(mut shared) = shared.lock() {
                shared.clients.retain(|(each, _)| *each != client);
            }
        });
    waker.wake();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(server: &Server, stream: &mut UnixStream) -> Value {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let request = loop {
            if let Some(request) = server.take_requests().pop() {
                break request;
            }
            assert!(std::time::Instant::now() < deadline, "no request arrived");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(request.method, "player.state");
        server.reply(&request, Ok(serde_json::json!({ "playing": true })));
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[test]
    fn a_client_gets_its_reply_and_the_events() {
        let dir = std::env::temp_dir().join(format!("spotlite-api-{}", std::process::id()));
        let path = dir.join("api.sock");
        let server = Server::start(&path, Waker::default()).unwrap();
        let mut stream = UnixStream::connect(&path).unwrap();
        stream
            .write_all(b"{\"id\":7,\"method\":\"player.state\"}\n")
            .unwrap();
        let reply = round_trip(&server, &mut stream);
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["result"]["playing"], true);

        server.broadcast("player", serde_json::json!({ "title": "Rosewood" }));
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        let event: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(event["event"], "player");
        assert_eq!(event["data"]["title"], "Rosewood");

        // A second server finds the first one serving.
        assert!(Server::start(&path, Waker::default()).is_err());
        drop(server);
        let _ = std::fs::remove_dir_all(dir);
    }
}
