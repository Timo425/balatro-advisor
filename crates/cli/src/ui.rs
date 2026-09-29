//! `balatro-advisor ui`: a local page that re-analyses the run whenever the game saves.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use anyhow::Result;
use balatro_advisor::advise::{self, Options};
use balatro_advisor::data::GameData;
use balatro_advisor::{gold, save};

const PAGE: &str = include_str!("ui.html");

#[derive(Default)]
struct Shared {
    version: u64,
    /// JSON body served at /api/analysis
    body: String,
    busy: bool,
}

pub fn run(save_dir: PathBuf, profile: u8, port: u16, open: bool) -> Result<()> {
    let shared = Arc::new(Mutex::new(Shared { body: r#"{"status":"starting"}"#.into(), ..Default::default() }));
    let save_path = save::save_path(&save_dir, profile);

    // Watcher: poll the save's mtime; re-analyse after it settles (the game writes several times).
    {
        let shared = shared.clone();
        std::thread::spawn(move || {
            let data = GameData::bundled();
            let mut last: Option<SystemTime> = None;
            loop {
                let m = std::fs::metadata(&save_path).and_then(|m| m.modified()).ok();
                if m != last {
                    std::thread::sleep(Duration::from_millis(300));
                    let settled = std::fs::metadata(&save_path).and_then(|m| m.modified()).ok();
                    if settled != m {
                        continue;
                    }
                    last = m;
                    shared.lock().unwrap().busy = true;
                    let body = match save::load(&save_path, data) {
                        Ok(r) => {
                            let g = gold::load(&save_dir, profile, data).ok();
                            let a = advise::analyze(&r, data, g.as_ref(), &Options::default());
                            serde_json::json!({ "status": "ok", "analysis": a, "gold": g }).to_string()
                        }
                        Err(e) => {
                            let g = gold::load(&save_dir, profile, data).ok();
                            serde_json::json!({ "status": "no_run", "message": e.to_string(), "gold": g }).to_string()
                        }
                    };
                    let mut s = shared.lock().unwrap();
                    s.body = body;
                    s.version += 1;
                    s.busy = false;
                }
                std::thread::sleep(Duration::from_millis(400));
            }
        });
    }

    let addr = format!("127.0.0.1:{port}");
    let server = tiny_http::Server::http(&addr).map_err(|e| anyhow::anyhow!("can't listen on {addr}: {e}"))?;
    let url = format!("http://{addr}/");
    println!("Balatro advisor running at {url} (Ctrl+C to stop). It updates whenever the game saves.");
    if open {
        open_window(&url);
    }
    for req in server.incoming_requests() {
        let path = req.url().split('?').next().unwrap_or("/").to_string();
        let resp = match path.as_str() {
            "/" => tiny_http::Response::from_string(PAGE)
                .with_header(header("Content-Type", "text/html; charset=utf-8")),
            "/api/analysis" => {
                let s = shared.lock().unwrap();
                let body = format!(r#"{{"version":{},"busy":{},"data":{}}}"#, s.version, s.busy, s.body);
                tiny_http::Response::from_string(body).with_header(header("Content-Type", "application/json"))
            }
            _ => tiny_http::Response::from_string("not found").with_status_code(404),
        };
        let _ = req.respond(resp);
    }
    Ok(())
}

/// A chromeless app window (Chrome/Chromium `--app`), else the default browser.
fn open_window(url: &str) {
    for browser in ["google-chrome", "chromium", "chromium-browser", "brave-browser"] {
        let ok = std::process::Command::new(browser)
            .arg(format!("--app={url}"))
            .arg("--window-size=1150,1000")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok();
        if ok {
            return;
        }
    }
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

fn header(k: &str, v: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("static header")
}
