//! `balatro-advisor ui`: a local page that re-analyses the run whenever the game saves.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;
use balatro_advisor::advise::{self, Options};
use balatro_advisor::data::GameData;
use balatro_advisor::progress::{self, Progress};
use balatro_advisor::{gold, save};

const PAGE: &str = include_str!("ui.html");

#[derive(Default)]
struct Shared {
    version: u64,
    /// JSON body served at /api/analysis
    body: String,
    busy: bool,
    /// The analysis running, and since when
    running: Option<(Progress, Instant)>,
    /// Shop options ticked "as if bought" (labels), and a counter that changes with them
    plan: Vec<String>,
    plan_version: u64,
}

pub fn run(save_dir: PathBuf, profile: u8, port: u16, open: bool) -> Result<()> {
    lower_priority();
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
            // Whether the game has written a state other than `fp` since `seen` (settled; a
            // reordered hand is the same state): what makes an analysis running stale
            let newer = |mut seen: Option<SystemTime>, fp: String| {
                move || {
                    let (p, m) = stamp();
                    if m == seen {
                        return false;
                    }
                    std::thread::sleep(Duration::from_millis(150));
                    if stamp().1 != m {
                        return false;
                    }
                    seen = m;
                    save::load(&p, data).map_or(true, |r| fingerprint(&r) != fp)
                }
            };
            let plan_changed_from = |v: u64| {
                let shared = shared.clone();
                move || shared.lock().unwrap().plan_version != v
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
                        let mut stale = (newer(m.as_ref().and_then(|x| x.1), last_fp.clone().unwrap_or_default()), plan_changed_from(plan_v));
                        let body = planned_body(r, a, g.as_ref(), data, &plan, &shared, move || stale.0() || stale.1());
                        let mut s = shared.lock().unwrap();
                        if let Some(body) = body {
                            s.body = body;
                            s.version += 1;
                        }
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
                    last = m.clone();
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
                        let fp = fingerprint(r);
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
                            let stale = newer(m.as_ref().and_then(|x| x.1), last_fp.clone().unwrap_or_default());
                            let analysed = stoppable(&shared, stale, |p| advise::analyze(&r, data, g.as_ref(), &Options { progress: Some(p), ..Default::default() }));
                            match analysed {
                                Ok(Some(a)) => {
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
                                    let mut stale = (newer(m.as_ref().and_then(|x| x.1), last_fp.clone().unwrap_or_default()), plan_changed_from(plan_v));
                                    let body = planned_body(&r, &a, g.as_ref(), data, &plan, &shared, move || stale.0() || stale.1());
                                    real = Some((r.clone(), a, g));
                                    // a plan stopped by a newer state or plan: the next turn of the
                                    // loop works that one out
                                    let Some(body) = body else { continue };
                                    body
                                }
                                // The game wrote a newer state: analyse that one (the page
                                // keeps the last analysis until then)
                                Ok(None) => {
                                    if let Some(k) = &keep {
                                        let _ = std::fs::remove_file(k);
                                    }
                                    last_fp = None;
                                    continue;
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
                // How far the analysis running has got: steps done of all, seconds so far
                let running = s.running.as_ref().map_or("null".to_string(), |(p, t)| {
                    format!(r#"{{"steps":{},"of":{},"secs":{}}}"#, p.steps_done(), progress::STEPS, t.elapsed().as_secs())
                });
                let body = format!(
                    r#"{{"boot":"{boot}","version":{},"busy":{},"running":{running},"game_seen_secs":{seen},"high_contrast":{high_contrast},"data":{}}}"#,
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
/// bought (the plan's notes say what was applied). None when that analysis went stale
/// (`stale`) and was stopped.
fn planned_body(
    r: &save::RunState,
    a: &advise::Analysis,
    g: Option<&gold::GoldReport>,
    data: &GameData,
    plan: &[String],
    shared: &Mutex<Shared>,
    stale: impl FnMut() -> bool + Send,
) -> Option<String> {
    if plan.is_empty() {
        return Some(serde_json::json!({ "status": "ok", "analysis": a, "gold": g }).to_string());
    }
    let (state, notes) = balatro_advisor::plan::apply(r, a, data, plan);
    let planned = stoppable(shared, stale, |p| advise::analyze(&state, data, g, &Options { progress: Some(p), ..Default::default() }));
    Some(match planned {
        Ok(Some(p)) => serde_json::json!({ "status": "ok", "analysis": p, "gold": g, "plan": { "items": plan, "notes": notes } }).to_string(),
        Ok(None) => return None,
        Err(_) => serde_json::json!({ "status": "ok", "analysis": a, "gold": g, "plan": { "items": plan, "notes": ["the planned state crashed the analysis; showing the real one"] } }).to_string(),
    })
}

/// Runs an analysis (`analyse`, given its `Progress`) while a watcher thread checks every
/// 400 ms whether it went stale (`stale`: the game wrote another state, the plan changed) and
/// then stops it. `Ok(None)` when it was stopped, `Err` when it crashed. The page shows how
/// far it got (`Shared::running`).
fn stoppable<T>(shared: &Mutex<Shared>, mut stale: impl FnMut() -> bool + Send, analyse: impl FnOnce(Progress) -> T) -> std::thread::Result<Option<T>> {
    let p = Progress::new();
    shared.lock().unwrap().running = Some((p.clone(), Instant::now()));
    let done = std::sync::atomic::AtomicBool::new(false);
    let out = std::thread::scope(|s| {
        s.spawn(|| {
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(400));
                if !done.load(std::sync::atomic::Ordering::Relaxed) && stale() {
                    p.stop();
                    return;
                }
            }
        });
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| analyse(p.clone())));
        done.store(true, std::sync::atomic::Ordering::Relaxed);
        out
    });
    shared.lock().unwrap().running = None;
    match out {
        Ok(a) => Ok(Some(a)),
        Err(e) if progress::is_stopped(&*e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// What the run is, as far as the advice goes: reordering your hand changes the file but not
/// this (unless a card is face down: then the order tells you something)
fn fingerprint(r: &save::RunState) -> String {
    let mut same = r.clone();
    if !same.hand.iter().any(|c| c.face_down) {
        same.hand.sort_by_key(|c| c.label());
    }
    same.snapshot.age_secs = None;
    same.snapshot.live = false;
    serde_json::to_string(&same).unwrap_or_default()
}

/// The analysis runs at a lower CPU priority (nice 10), so a long one doesn't slow the game or
/// anything else on the machine: it gets what they leave. Threads started after this inherit
/// it (Linux sets it per thread).
fn lower_priority() {
    #[cfg(unix)]
    // SAFETY: setpriority only reads its arguments
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
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
