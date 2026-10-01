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
    /// Shop options ticked "as if bought" (labels), and a counter that changes with them
    plan: Vec<String>,
    plan_version: u64,
}

pub fn run(save_dir: PathBuf, profile: u8, port: u16, open: bool) -> Result<()> {
    let shared = Arc::new(Mutex::new(Shared { body: r#"{"status":"starting"}"#.into(), ..Default::default() }));

    let beat_dir = save_dir.clone();
    // Watcher: poll the save's mtime; re-analyse after it settles (the game writes several times).
    {
        let shared = shared.clone();
        std::thread::spawn(move || {
            let data = GameData::bundled();
            // Calibration log: predictions at blind start, and how each blind ended
            let mut tracker = balatro_advisor::calibration::default_dir().map(balatro_advisor::calibration::Tracker::new);
            let mut last: Option<(PathBuf, Option<SystemTime>)> = None;
            let mut last_fp: Option<String> = None;
            let mut last_plan = 0u64;
            // The real state's analysis, reused when only the plan changes
            let mut real: Option<(save::RunState, advise::Analysis, Option<gold::GoldReport>)> = None;
            let stamp = || {
                let p = save::run_path(&save_dir, profile);
                let m = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
                (p, m)
            };
            loop {
                let m = Some(stamp());
                let (plan, plan_v) = {
                    let s = shared.lock().unwrap();
                    (s.plan.clone(), s.plan_version)
                };
                if m == last && plan_v != last_plan {
                    last_plan = plan_v;
                    if let Some((r, a, g)) = &real {
                        shared.lock().unwrap().busy = true;
                        let body = planned_body(r, a, g.as_ref(), data, &plan);
                        let mut s = shared.lock().unwrap();
                        s.body = body;
                        s.version += 1;
                        s.busy = false;
                    }
                    continue;
                }
                if m != last {
                    std::thread::sleep(Duration::from_millis(150));
                    let settled = Some(stamp());
                    if settled != m {
                        continue;
                    }
                    last = m;
                    let plan_changed = plan_v != last_plan;
                    last_plan = plan_v;
                    shared.lock().unwrap().busy = true;
                    let save_path = save::run_path(&save_dir, profile);
                    // Keep a copy of the state being analysed until it finishes: if the
                    // analysis ever hangs or panics, `last-analysed.jkr` holds the state that did it.
                    let keep = balatro_advisor::calibration::default_dir().map(|d| d.join("last-analysed.jkr"));
                    if let Some(k) = &keep {
                        let _ = std::fs::create_dir_all(k.parent().unwrap_or(k));
                        let _ = std::fs::copy(&save_path, k);
                    }
                    // Reordering your hand changes the file but not the run: skip those.
                    let loaded = save::load(&save_path, data);
                    if let Ok(r) = &loaded {
                        let mut same = r.clone();
                        // (unless a card is face down: then the order tells you something)
                        if !same.hand.iter().any(|c| c.face_down) {
                            same.hand.sort_by_key(|c| c.label());
                        }
                        same.snapshot.age_secs = None;
                        same.snapshot.live = false;
                        let fp = serde_json::to_string(&same).unwrap_or_default();
                        if last_fp.as_deref() == Some(fp.as_str()) && !plan_changed {
                            shared.lock().unwrap().busy = false;
                            std::thread::sleep(Duration::from_millis(400));
                            continue;
                        }
                        last_fp = Some(fp);
                    }
                    let body = match loaded {
                        Ok(r) => {
                            let g = gold::load(&save_dir, profile, data).ok();
                            let analysed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                advise::analyze(&r, data, g.as_ref(), &Options::default())
                            }));
                            match analysed {
                                Ok(a) => {
                                    if let Some(k) = &keep {
                                        let _ = std::fs::remove_file(k);
                                    }
                                    if let Some(t) = tracker.as_mut() {
                                        let p = a.blinds.iter().find(|b| b.state == "Current").and_then(|b| b.p_win);
                                        t.observe(Some(&r), p);
                                    }
                                    // Ticked options bought for real, or no longer on offer, drop out
                                    let plan: Vec<String> = plan.into_iter().filter(|l| a.options.iter().any(|o| &o.label == l)).collect();
                                    {
                                        let mut s = shared.lock().unwrap();
                                        if s.plan_version == plan_v {
                                            s.plan = plan.clone();
                                        }
                                    }
                                    let body = planned_body(&r, &a, g.as_ref(), data, &plan);
                                    real = Some((r.clone(), a, g));
                                    body
                                }
                                Err(_) => serde_json::json!({
                                    "status": "error",
                                    "message": format!("the analysis crashed on this state (a copy is kept as {})", keep.as_ref().map_or(String::new(), |k| k.display().to_string())),
                                })
                                .to_string(),
                            }
                        }
                        Err(e) => {
                            if let (Some(t), balatro_advisor::Error::NoRun(_)) = (tracker.as_mut(), &e) {
                                t.observe(None, None);
                            }
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

    // Changes on every start, so an open page can tell it's talking to a new build and reload
    let boot = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let addr = format!("127.0.0.1:{port}");
    let server = tiny_http::Server::http(&addr).map_err(|e| anyhow::anyhow!("can't listen on {addr}: {e}"))?;
    let url = format!("http://{addr}/");
    println!("Balatro advisor running at {url} (Ctrl+C to stop). It updates whenever the game saves.");
    if open {
        open_window(&url);
    }
    for mut req in server.incoming_requests() {
        let path = req.url().split('?').next().unwrap_or("/").to_string();
        let resp = match path.as_str() {
            // The ticked "as if bought" options: a JSON list of option labels
            "/api/plan" => {
                let mut body = String::new();
                let _ = std::io::Read::read_to_string(req.as_reader(), &mut body);
                match serde_json::from_str::<Vec<String>>(&body) {
                    Ok(plan) => {
                        let mut s = shared.lock().unwrap();
                        s.plan = plan;
                        s.plan_version += 1;
                        s.busy = true;
                        tiny_http::Response::from_string("ok")
                    }
                    Err(e) => tiny_http::Response::from_string(format!("bad plan: {e}")).with_status_code(400),
                }
            }
            "/" => tiny_http::Response::from_string(PAGE)
                .with_header(header("Content-Type", "text/html; charset=utf-8")),
            "/api/analysis" => {
                let s = shared.lock().unwrap();
                // Heartbeat from the advisor-live mod: tells a running game from a closed or crashed one.
                let seen = std::fs::metadata(beat_dir.join(profile.to_string()).join("live.beat"))
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| SystemTime::now().duration_since(t).ok())
                    .map_or("null".to_string(), |d| d.as_secs().to_string());
                // The game's "high contrast cards" setting picks its suit palette.
                let high_contrast = balatro_advisor::jkr::read(&beat_dir.join("settings.jkr"))
                    .ok()
                    .is_some_and(|v| v.get("colourblind_option").truthy());
                let body = format!(
                    r#"{{"boot":"{boot}","version":{},"busy":{},"game_seen_secs":{seen},"high_contrast":{high_contrast},"data":{}}}"#,
                    s.version, s.busy, s.body
                );
                tiny_http::Response::from_string(body).with_header(header("Content-Type", "application/json"))
            }
            _ => tiny_http::Response::from_string("not found").with_status_code(404),
        };
        let _ = req.respond(resp);
    }
    Ok(())
}

/// The page's JSON for a state: the analysis as it is, or with the ticked options already
/// bought (the plan's notes say what was applied).
fn planned_body(r: &save::RunState, a: &advise::Analysis, g: Option<&gold::GoldReport>, data: &GameData, plan: &[String]) -> String {
    if plan.is_empty() {
        return serde_json::json!({ "status": "ok", "analysis": a, "gold": g }).to_string();
    }
    let (state, notes) = balatro_advisor::plan::apply(r, a, data, plan);
    let planned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| advise::analyze(&state, data, g, &Options::default())));
    match planned {
        Ok(p) => serde_json::json!({ "status": "ok", "analysis": p, "gold": g, "plan": { "items": plan, "notes": notes } }).to_string(),
        Err(_) => serde_json::json!({ "status": "ok", "analysis": a, "gold": g, "plan": { "items": plan, "notes": ["the planned state crashed the analysis; showing the real one"] } }).to_string(),
    }
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
