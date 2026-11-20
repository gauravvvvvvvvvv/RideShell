//! RideShell App: the RideShell terminal window.
//!
//! A native window (tao) with a WebView2 UI (wry). Each tab is a pseudo
//! console running `rideshell open`, so every tab is the full RideShell
//! session (PowerShell + automatic cmd/bash routing + profiles + shims),
//! rendered by xterm.js with RideShell's own fonts and theme. All assets are
//! compiled in; nothing is loaded from the network.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
mod envpath;
mod exeicon;
mod explorer;
mod media;
mod pty;
mod state;
mod updater;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

use base64::Engine;
use rideshell_core::paths::Paths;
use serde::Deserialize;
use serde_json::json;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{ResizeDirection, Window, WindowBuilder};
use wry::http::{header::CONTENT_TYPE, Request, Response};
use wry::{WebContext, WebViewBuilder};

pub enum UserEvent {
    Ipc(String),
    Worked(serde_json::Value),
    /// Full app state, built off the UI thread.
    State(serde_json::Value),
    /// Commands on PATH (`all`, list), scanned off the UI thread.
    Tools(bool, serde_json::Value),
    /// Spotify's now playing (or null), sent only when it changes.
    Media(serde_json::Value),
    /// A folder listing for the Files panel, read off the UI thread.
    Dir(serde_json::Value),
    /// Result of creating an account (a message for the UI).
    Created(serde_json::Value),
    /// Update check result, or a wizard step (download, verify, install...).
    Update(serde_json::Value),
    Picked(Option<PathBuf>),
    Dropped(Vec<PathBuf>),
    PtyData(u32, Vec<u8>),
    PtyExit(u32, Option<u32>),
}

// ----- embedded assets -----------------------------------------------------------

fn asset(path: &str) -> Option<(&'static [u8], &'static str)> {
    Some(match path {
        "/" | "/index.html" => (include_bytes!("../assets/index.html"), "text/html; charset=utf-8"),
        "/app.css" => (include_bytes!("../assets/app.css"), "text/css; charset=utf-8"),
        "/app.js" => (include_bytes!("../assets/app.js"), "text/javascript; charset=utf-8"),
        "/usage.js" => (include_bytes!("../assets/usage.js"), "text/javascript; charset=utf-8"),
        "/update.js" => (include_bytes!("../assets/update.js"), "text/javascript; charset=utf-8"),
        "/settings.js" => (include_bytes!("../assets/settings.js"), "text/javascript; charset=utf-8"),
        "/icons.js" => (include_bytes!("../assets/icons.js"), "text/javascript; charset=utf-8"),
        "/icon.png" => (include_bytes!("../assets/icon-256.png"), "image/png"),
        "/vendor/xterm.js" => (include_bytes!("../assets/vendor/xterm.js"), "text/javascript; charset=utf-8"),
        "/vendor/xterm.css" => (include_bytes!("../assets/vendor/xterm.css"), "text/css; charset=utf-8"),
        "/vendor/addon-fit.js" => (include_bytes!("../assets/vendor/addon-fit.js"), "text/javascript; charset=utf-8"),
        "/vendor/addon-webgl.js" => {
            (include_bytes!("../assets/vendor/addon-webgl.js"), "text/javascript; charset=utf-8")
        }
        "/vendor/addon-web-links.js" => {
            (include_bytes!("../assets/vendor/addon-web-links.js"), "text/javascript; charset=utf-8")
        }
        "/fonts/Geist.woff2" => (include_bytes!("../assets/vendor/fonts/Geist-Variable.woff2"), "font/woff2"),
        "/fonts/GeistMono.woff2" => (include_bytes!("../assets/vendor/fonts/GeistMono-Variable.woff2"), "font/woff2"),
        _ => return None,
    })
}

fn serve(request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let path = request.uri().path().to_string();
    if path == "/exe-icon" {
        let target = request
            .uri()
            .query()
            .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("p=")))
            .map(exeicon::percent_decode)
            .unwrap_or_default();
        return match exeicon::png_for(&target) {
            Some(png) => Response::builder()
                .header(CONTENT_TYPE, "image/png")
                .header("Cache-Control", "max-age=86400")
                .body(Cow::Owned(png))
                .unwrap(),
            None => Response::builder().status(404).body(Cow::Borrowed(&b""[..])).unwrap(),
        };
    }
    match asset(&path) {
        Some((body, mime)) => Response::builder()
            .header(CONTENT_TYPE, mime)
            .header("Cache-Control", "no-store")
            .body(Cow::Borrowed(body))
            .unwrap(),
        None => Response::builder().status(404).body(Cow::Borrowed(&b"not found"[..])).unwrap(),
    }
}

// ----- messages from the UI -----------------------------------------------------------

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
enum Msg {
    State,
    Spawn {
        id: u32,
        profile: Option<String>,
        cwd: Option<String>,
        command: Option<String>,
        cols: u16,
        rows: u16,
    },
    Input {
        id: u32,
        data: String,
    },
    Resize {
        id: u32,
        cols: u16,
        rows: u16,
    },
    Kill {
        id: u32,
    },
    Win {
        op: String,
        dir: Option<String>,
    },
    Admin {
        profile: Option<String>,
        cwd: Option<String>,
    },
    CreateProfile {
        name: String,
        description: Option<String>,
    },
    SetDefault {
        name: String,
    },
    Open {
        what: String,
    },
    Paste {
        id: u32,
    },
    Tools {
        all: bool,
    },
    PickFolder {
        start: Option<String>,
    },
    Worked,
    ClassicMenu {
        on: bool,
    },
    RestartExplorer,
    ListDir {
        path: String,
    },
    SaveSettings {
        settings: serde_json::Value,
    },
    SaveConfig {
        config: serde_json::Value,
    },
    Shim {
        name: String,
        on: bool,
    },
    /// Spotify: "toggle", "next" or "previous".
    Media {
        op: String,
    },
    /// Look for a newer release (`manual`: also say when there is none).
    UpdateCheck {
        #[serde(default)]
        manual: bool,
    },
    /// Download, verify and install the latest release (update wizard).
    UpdateInstall,
    /// First-install wizard: run the installer this window was opened for.
    SetupInstall,
    /// Start the installed RideShell App and close this window.
    Relaunch,
}

struct Startup {
    profile: Option<String>,
    dir: Option<String>,
    wants_admin: bool,
    /// `--setup <rideshell.exe>`: show the install wizard for that installer.
    setup: Option<String>,
}

fn parse_args() -> Startup {
    let mut s = Startup { profile: None, dir: None, wants_admin: false, setup: None };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--admin" => s.wants_admin = true,
            "--profile" => s.profile = args.next(),
            "--dir" => s.dir = args.next(),
            "--setup" => s.setup = args.next(),
            _ => {}
        }
    }
    s
}

fn main() {
    let startup = parse_args();
    let elevated = rideshell_core::platform::is_elevated();

    // "RideShell (Administrator)" normally starts elevated already (its
    // shortcut asks Windows to). If not, ask now and let this copy exit.
    if startup.wants_admin && !elevated {
        if let Ok(exe) = std::env::current_exe() {
            let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
            let _ = rideshell_core::platform::elevate(&exe, "--admin", std::path::Path::new(&home));
        }
        return;
    }

    let paths = match Paths::discover() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rideshell-app: {e:#}");
            std::process::exit(1);
        }
    };
    let cli = pty::find_cli();

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let title = if elevated { "Administrator: RideShell" } else { "RideShell" };
    let builder = WindowBuilder::new()
        .with_title(title)
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(LogicalSize::new(760.0, 460.0))
        .with_visible(false);
    #[cfg(windows)]
    let builder = {
        use tao::platform::windows::{IconExtWindows, WindowBuilderExtWindows};
        let b = builder.with_undecorated_shadow(true);
        match tao::window::Icon::from_resource(1, None) {
            Ok(icon) => b.with_window_icon(Some(icon)).with_taskbar_icon(None),
            Err(_) => b,
        }
    };
    let window = builder.build(&event_loop).expect("cannot create the window");
    #[cfg(windows)]
    round_corners(&window);

    let mut web_context = WebContext::new(Some(paths.data_dir.join("webview")));
    let ipc_proxy = proxy.clone();
    let drop_proxy = proxy.clone();
    let url = if cfg!(windows) { "http://rs.localhost/index.html" } else { "rs://localhost/index.html" };
    let webview = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_custom_protocol("rs".into(), |_id, request| serve(request))
        .with_ipc_handler(move |req: Request<String>| {
            let _ = ipc_proxy.send_event(UserEvent::Ipc(req.into_body()));
        })
        .with_drag_drop_handler(move |event| {
            if let wry::DragDropEvent::Drop { paths, .. } = event {
                let _ = drop_proxy.send_event(UserEvent::Dropped(paths));
            }
            true
        })
        .with_background_color((10, 11, 18, 255))
        .with_devtools(cfg!(debug_assertions))
        .with_hotkeys_zoom(false)
        .with_url(url)
        .build(&window)
        .expect("cannot create the web view (is Microsoft Edge WebView2 installed?)");
    window.set_visible(true);

    let send = move |webview: &wry::WebView, value: serde_json::Value| {
        let _ = webview.evaluate_script(&format!("window.RS && RS.recv({value})"));
    };

    let mut sessions: HashMap<u32, pty::Session> = HashMap::new();
    let setup_exe = startup.setup.clone().map(PathBuf::from);
    let mut startup = Some(startup);
    let installing = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let refresher = Refresher::new(paths.clone(), cli.is_some(), proxy.clone());
    let media = media::start(proxy.clone());

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _ = &web_context;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                for s in sessions.values_mut() {
                    s.kill();
                }
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::PtyData(id, bytes)) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                let _ = webview.evaluate_script(&format!("RS.data({id},'{b64}')"));
            }
            Event::UserEvent(UserEvent::State(mut v)) => {
                if let Some(s) = startup.take() {
                    v["startup"] = json!({ "profile": s.profile, "dir": s.dir, "setup": s.setup.is_some() });
                }
                send(&webview, json!({ "t": "state", "state": v }));
            }
            Event::UserEvent(UserEvent::Update(v)) => send(&webview, v),
            Event::UserEvent(UserEvent::Dir(v)) => send(&webview, v),
            Event::UserEvent(UserEvent::Created(v)) => {
                let ok = v["t"] == "created";
                send(&webview, v);
                if ok {
                    refresher.request();
                }
            }
            Event::UserEvent(UserEvent::Media(now)) => {
                send(&webview, json!({ "t": "media", "now": now }));
            }
            Event::UserEvent(UserEvent::Tools(all, tools)) => {
                send(&webview, json!({ "t": "tools", "all": all, "tools": tools }));
            }
            Event::UserEvent(UserEvent::Worked(list)) => {
                send(&webview, json!({ "t": "worked", "folders": list }));
            }
            Event::UserEvent(UserEvent::Picked(Some(p))) => {
                send(&webview, json!({ "t": "picked", "path": p }));
            }
            Event::UserEvent(UserEvent::Picked(None)) => {}
            Event::UserEvent(UserEvent::Dropped(paths)) => {
                send(&webview, json!({ "t": "dropped",
                    "items": paths.iter().map(|p| json!({ "path": p, "dir": p.is_dir() })).collect::<Vec<_>>() }));
            }
            Event::UserEvent(UserEvent::PtyExit(id, code)) => {
                sessions.remove(&id);
                send(&webview, json!({ "t": "exit", "id": id, "code": code }));
            }
            Event::UserEvent(UserEvent::Ipc(raw)) => {
                let msg: Msg = match serde_json::from_str(&raw) {
                    Ok(m) => m,
                    Err(e) => {
                        send(&webview, json!({ "t": "toast", "kind": "error", "text": format!("bad message: {e}") }));
                        return;
                    }
                };
                match msg {
                    Msg::State => refresher.request(),
                    Msg::Spawn { id, profile, cwd, command, cols, rows } => {
                        let Some(cli) = &cli else {
                            send(&webview, json!({ "t": "spawnError", "id": id,
                                "text": "rideshell.exe was not found next to RideShell App or on PATH. Reinstall RideShell." }));
                            return;
                        };
                        let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| ".".into());
                        let dir = cwd.map(PathBuf::from).filter(|d| d.is_dir()).unwrap_or(home);
                        let spec = pty::Spec {
                            cli,
                            profile: profile.as_deref().filter(|p| !p.is_empty()),
                            cwd: &dir,
                            command: command.as_deref().filter(|c| !c.is_empty()),
                            cols,
                            rows,
                        };
                        match pty::spawn(id, spec, proxy.clone()) {
                            Ok(s) => {
                                sessions.insert(id, s);
                            }
                            Err(e) => send(&webview, json!({ "t": "spawnError", "id": id, "text": format!("{e:#}") })),
                        }
                    }
                    Msg::Input { id, data } => {
                        if let Some(s) = sessions.get_mut(&id) {
                            s.write(data.as_bytes());
                        }
                    }
                    Msg::Resize { id, cols, rows } => {
                        if let Some(s) = sessions.get(&id) {
                            s.resize(cols, rows);
                        }
                    }
                    Msg::Kill { id } => {
                        if let Some(mut s) = sessions.remove(&id) {
                            s.kill();
                        }
                    }
                    Msg::Win { op, dir } => window_op(&window, &op, dir.as_deref(), control_flow, &mut sessions),
                    Msg::Admin { profile, cwd } => {
                        let mut args = vec!["--admin".to_string()];
                        if let Some(p) = profile.filter(|p| !p.is_empty()) {
                            args.extend(["--profile".into(), p]);
                        }
                        if let Some(d) = cwd.filter(|d| !d.is_empty()) {
                            args.extend(["--dir".into(), d]);
                        }
                        let line =
                            args.iter().map(|a| rideshell_core::platform::quote_arg(a)).collect::<Vec<_>>().join(" ");
                        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
                        let result = std::env::current_exe()
                            .map_err(anyhow::Error::from)
                            .and_then(|exe| rideshell_core::platform::elevate(&exe, &line, std::path::Path::new(&home)));
                        if let Err(e) = result {
                            send(&webview, json!({ "t": "toast", "kind": "error", "text": format!("{e:#}") }));
                        }
                    }
                    Msg::CreateProfile { name, description } => {
                        let (p, paths2, cli2) = (proxy.clone(), paths.clone(), cli.clone());
                        // Opens the database and looks tools up on PATH: off the UI thread.
                        std::thread::spawn(move || {
                            let name = name.trim().to_string();
                            let desc = description.unwrap_or_default();
                            let v = match state::create_profile(&paths2, cli2.as_deref(), &name, desc.trim()) {
                                Ok(private) => json!({ "t": "created", "name": name, "privateHistory": private }),
                                Err(e) => json!({ "t": "toast", "kind": "error", "text": format!("{e:#}") }),
                            };
                            let _ = p.send_event(UserEvent::Created(v));
                        });
                    }
                    Msg::SetDefault { name } => {
                        if let Err(e) = state::set_default(&paths, &name) {
                            send(&webview, json!({ "t": "toast", "kind": "error", "text": format!("{e:#}") }));
                        }
                        refresher.request();
                    }
                    Msg::Open { what } => {
                        let target = match what.as_str() {
                            "config" => {
                                let _ = rideshell_core::config::Config::write_default_if_missing(&paths);
                                Some(paths.config_file().to_string_lossy().into_owned())
                            }
                            "data" => Some(paths.data_dir.to_string_lossy().into_owned()),
                            url if url.starts_with("https://") || url.starts_with("http://") => Some(url.to_string()),
                            _ => None,
                        };
                        if let Some(t) = target {
                            let _ = rideshell_core::platform::open_with_default(&t);
                        }
                    }
                    Msg::SaveSettings { settings } => {
                        let file = settings_file(&paths);
                        let result = paths
                            .ensure()
                            .and_then(|_| Ok(std::fs::write(&file, serde_json::to_string_pretty(&settings)?)?));
                        if let Err(e) = result {
                            send(&webview, json!({ "t": "toast", "kind": "error", "text": format!("cannot save settings: {e:#}") }));
                        }
                    }
                    Msg::SaveConfig { config } => {
                        let result = serde_json::from_value::<rideshell_core::config::Config>(config)
                            .map_err(anyhow::Error::from)
                            .and_then(|c| c.save(&paths));
                        match result {
                            Ok(()) => send(&webview, json!({ "t": "toast", "kind": "ok", "text": "Saved. New tabs use the new settings." })),
                            Err(e) => send(&webview, json!({ "t": "toast", "kind": "error", "text": format!("cannot save: {e:#}") })),
                        }
                    }
                    Msg::ClassicMenu { on } => {
                        match explorer::set_classic_menu(on) {
                            Ok(()) => send(&webview, json!({ "t": "toast", "kind": "ok", "text": if on {
                                "Classic right-click menu turned on. It applies after Explorer restarts (Restart Explorer, or sign out)."
                            } else {
                                "Windows 11 right-click menu restored. It applies after Explorer restarts."
                            } })),
                            Err(e) => send(&webview, json!({ "t": "toast", "kind": "error", "text": format!("{e:#}") })),
                        }
                        send(&webview, json!({ "t": "stateLite", "state": { "classicMenu": explorer::classic_menu() } }));
                    }
                    Msg::RestartExplorer => explorer::restart(),
                    Msg::UpdateCheck { manual } => {
                        let p = proxy.clone();
                        std::thread::spawn(move || {
                            let v = match updater::check() {
                                Ok(Some(info)) => json!({ "t": "update", "info": info }),
                                Ok(None) => json!({ "t": "update", "info": null, "manual": manual }),
                                Err(e) => json!({ "t": "update", "info": null, "manual": manual, "error": format!("{e:#}") }),
                            };
                            let _ = p.send_event(UserEvent::Update(v));
                        });
                    }
                    Msg::UpdateInstall | Msg::SetupInstall => {
                        use std::sync::atomic::Ordering::SeqCst;
                        let setup = matches!(msg, Msg::SetupInstall);
                        let installer = setup_exe.clone();
                        if (setup && installer.is_none()) || installing.swap(true, SeqCst) {
                            return;
                        }
                        let (p, busy) = (proxy.clone(), installing.clone());
                        std::thread::spawn(move || {
                            let report = |v: serde_json::Value| {
                                let _ = p.send_event(UserEvent::Update(v));
                            };
                            let result = match &installer {
                                Some(exe) if setup => updater::run_installer(exe, &report).map(|()| String::new()),
                                _ => updater::install_update(&report),
                            };
                            if let Err(e) = result {
                                report(json!({ "t": "wizard", "step": "error", "error": format!("{e:#}") }));
                            }
                            busy.store(false, SeqCst);
                        });
                    }
                    Msg::Relaunch => {
                        let target = updater::installed_app().or_else(|| std::env::current_exe().ok());
                        if let Some(app) = target {
                            let _ = std::process::Command::new(app).spawn();
                        }
                        for (_, mut s) in sessions.drain() {
                            s.kill();
                        }
                        *control_flow = ControlFlow::Exit;
                    }
                    Msg::Media { op } => {
                        let cmd = match op.as_str() {
                            "next" => media::Command::Next,
                            "previous" => media::Command::Previous,
                            _ => media::Command::Toggle,
                        };
                        let _ = media.send(cmd);
                    }
                    Msg::Worked => {
                        let p = proxy.clone();
                        let paths2 = paths.clone();
                        // Reading other tools' lists can take a moment: off the UI thread.
                        std::thread::spawn(move || {
                            let list = state::worked_folders(&paths2);
                            let _ = p.send_event(UserEvent::Worked(serde_json::to_value(list).unwrap_or_default()));
                        });
                    }
                    Msg::PickFolder { start } => {
                        // The dialog is modal; run it off the event loop so the UI keeps painting.
                        let p = proxy.clone();
                        std::thread::spawn(move || {
                            let mut dialog = rfd::FileDialog::new().set_title("Open a folder in RideShell");
                            if let Some(s) = start.filter(|s| std::path::Path::new(s).is_dir()) {
                                dialog = dialog.set_directory(s);
                            }
                            let _ = p.send_event(UserEvent::Picked(dialog.pick_folder()));
                        });
                    }
                    Msg::ListDir { path } => {
                        let p = proxy.clone();
                        // Network drives and big folders can be slow: off the UI thread.
                        std::thread::spawn(move || {
                            let v = match state::list_dir(std::path::Path::new(&path)) {
                                Ok((path, parent, entries)) => {
                                    json!({ "t": "dir", "path": path, "parent": parent, "entries": entries })
                                }
                                Err(e) => json!({ "t": "dir", "path": path, "error": format!("{e}") }),
                            };
                            let _ = p.send_event(UserEvent::Dir(v));
                        });
                    }
                    Msg::Tools { all } => {
                        let p = proxy.clone();
                        let shims = paths.shims_dir();
                        // Scanning every PATH folder takes a moment: off the UI thread.
                        std::thread::spawn(move || {
                            envpath::refresh();
                            let tools = rideshell_core::tools::scan(Some(&shims), all);
                            let _ = p.send_event(UserEvent::Tools(all, serde_json::to_value(tools).unwrap_or_default()));
                        });
                    }
                    Msg::Shim { name, on } => {
                        let dir = paths.shims_dir();
                        let result = if on {
                            match &cli {
                                Some(cli) => rideshell_core::shims::create(&dir, &name, cli).map(|_| ()),
                                None => Err(anyhow::anyhow!("rideshell.exe was not found")),
                            }
                        } else {
                            rideshell_core::shims::remove(&dir, &name).map(|_| ())
                        };
                        match result {
                            Ok(()) => send(&webview, json!({ "t": "toast", "kind": "ok",
                                "text": if on { format!("{name} now runs with your RideShell account in every terminal") }
                                        else { format!("{name} runs directly again") } })),
                            Err(e) => send(&webview, json!({ "t": "toast", "kind": "error", "text": format!("{e:#}") })),
                        }
                        let tools = rideshell_core::tools::scan(Some(&dir), false);
                        send(&webview, json!({ "t": "tools", "all": false, "tools": tools }));
                    }
                    Msg::Paste { id } => {
                        if let Some(text) = clipboard::read_text() {
                            send(&webview, json!({ "t": "paste", "id": id, "text": text }));
                        }
                    }
                }
            }
            _ => {}
        }
    });
}

/// Build-time options. Official builds may set an anonymous usage counter
/// (`RIDESHELL_TELEMETRY_URL`) and a one-line message slot (`RIDESHELL_PROMO_URL`).
/// Builds without them (the default, and any fork) have neither. Users can
/// always turn the counter off; `DO_NOT_TRACK` / `RIDESHELL_NO_TELEMETRY` also do.
/// The full state (profiles, tools on PATH, conversation counts...) without
/// the one-time startup request. Touches the disk and registry: never call it
/// on the UI thread.
fn full_state(paths: &Paths, cli_found: bool) -> serde_json::Value {
    // Tools installed since the app started (winget, setups) show up live.
    envpath::refresh();
    let mut v = serde_json::to_value(state::load(paths, cli_found)).unwrap_or_default();
    v["config"] = rideshell_core::config::Config::load(paths)
        .ok()
        .and_then(|c| serde_json::to_value(c).ok())
        .unwrap_or(serde_json::Value::Null);
    v["paths"] = json!({ "config": paths.config_file(), "data": paths.data_dir });
    v["build"] = build_info();
    v["classicMenu"] = json!(explorer::classic_menu());
    v["settings"] = std::fs::read_to_string(settings_file(paths))
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or(serde_json::Value::Null);
    v
}

/// Builds the state on a background thread so the terminals never wait for
/// it. Requests that arrive while one is being built are merged into a
/// single follow-up build, so refreshes can never pile up.
struct Refresher {
    paths: Paths,
    cli_found: bool,
    proxy: tao::event_loop::EventLoopProxy<UserEvent>,
    /// 0 idle, 1 building, 2 building and another build requested.
    busy: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

impl Refresher {
    fn new(paths: Paths, cli_found: bool, proxy: tao::event_loop::EventLoopProxy<UserEvent>) -> Self {
        Self { paths, cli_found, proxy, busy: Default::default() }
    }

    fn request(&self) {
        use std::sync::atomic::Ordering::SeqCst;
        // Idle -> start a build; building -> ask it to go once more.
        loop {
            match self.busy.compare_exchange(0, 1, SeqCst, SeqCst) {
                Ok(_) => break,
                Err(2) => return,
                Err(_) => {
                    if self.busy.compare_exchange(1, 2, SeqCst, SeqCst).is_ok() {
                        return;
                    }
                }
            }
        }
        let (paths, cli_found, proxy, busy) =
            (self.paths.clone(), self.cli_found, self.proxy.clone(), self.busy.clone());
        std::thread::spawn(move || loop {
            let v = full_state(&paths, cli_found);
            let _ = proxy.send_event(UserEvent::State(v));
            // 2 -> 1: build once more; 1 -> 0: done.
            if busy.fetch_sub(1, SeqCst) == 1 {
                break;
            }
        });
    }
}

fn build_info() -> serde_json::Value {
    let off = |k: &str| std::env::var(k).is_ok_and(|v| !v.is_empty() && v != "0");
    json!({
        "telemetryUrl": option_env!("RIDESHELL_TELEMETRY_URL").filter(|u| u.starts_with("https://")),
        "promoUrl": option_env!("RIDESHELL_PROMO_URL").filter(|u| u.starts_with("https://")),
        "telemetryBlockedByEnv": off("DO_NOT_TRACK") || off("RIDESHELL_NO_TELEMETRY"),
        "os": match std::env::consts::OS { "macos" => "macos", "linux" => "linux", _ => "windows" },
    })
}

/// The app's look and sidebar (theme, fonts, pinned tools): a JSON file next
/// to config.toml, so it survives reinstalls and can be edited by hand.
fn settings_file(paths: &Paths) -> PathBuf {
    paths.config_dir.join("app.json")
}

fn window_op(
    window: &Window,
    op: &str,
    dir: Option<&str>,
    control_flow: &mut ControlFlow,
    sessions: &mut HashMap<u32, pty::Session>,
) {
    match op {
        "minimize" => window.set_minimized(true),
        "maximize" => window.set_maximized(!window.is_maximized()),
        "close" => {
            for s in sessions.values_mut() {
                s.kill();
            }
            *control_flow = ControlFlow::Exit;
        }
        "drag" => {
            let _ = window.drag_window();
        }
        "resize" => {
            let d = match dir.unwrap_or("") {
                "n" => ResizeDirection::North,
                "s" => ResizeDirection::South,
                "e" => ResizeDirection::East,
                "w" => ResizeDirection::West,
                "ne" => ResizeDirection::NorthEast,
                "nw" => ResizeDirection::NorthWest,
                "se" => ResizeDirection::SouthEast,
                "sw" => ResizeDirection::SouthWest,
                _ => return,
            };
            let _ = window.drag_resize_window(d);
        }
        "focus" => window.set_focus(),
        _ => {}
    }
}

/// Windows 11: rounded window corners for the undecorated window.
#[cfg(windows)]
fn round_corners(window: &Window) {
    use tao::platform::windows::WindowExtWindows;
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
    let hwnd = window.hwnd() as windows_sys::Win32::Foundation::HWND;
    let pref = DWMWCP_ROUND;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &pref as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&pref) as u32,
        );
    }
}
