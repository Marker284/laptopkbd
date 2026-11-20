// SPDX-License-Identifier: GPL-2.0-or-later
//! niri IPC: requests straight over $NIRI_SOCKET (no `niri msg` processes),
//! an outputs watcher and a keyboard-layout watcher (event stream).
use niri_ipc::socket::Socket;
use niri_ipc::{Action, Event, Output, OutputAction, Reply, Request, Response};
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::collections::HashMap;
use std::time::Duration;

pub enum Msg {
    /// Name of the first enabled output other than the tablet panel, and the panel's scale.
    Outputs { external: Option<String>, panel_scale: f64 },
    /// The active xkb layout is Russian.
    LayoutRu(bool),
    /// Workspaces (sorted by output, then index) for the mini-map.
    Workspaces(Vec<Ws>),
    /// niri reloaded its config: re-read the binds for the Tux hints.
    ConfigReloaded,
    /// SIGHUP: config changed; SIGUSR1: toggle preview.
    Reload,
    TogglePreview,
    /// SIGUSR2: show the Tux hints without Tux (screenshots).
    DemoHints,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ws {
    pub id: u64,
    pub idx: u8,
    pub output: String,
    pub active: bool,
    pub focused: bool,
    pub windows: u32,
}

pub fn request(req: Request) -> Option<Response> {
    let mut s = Socket::connect().ok()?;
    match s.send(req) {
        Ok(Reply::Ok(r)) => Some(r),
        _ => None,
    }
}

pub fn action(a: Action) {
    let _ = request(Request::Action(a));
}

pub fn output(name: &str, action: OutputAction) {
    let _ = request(Request::Output { output: name.to_owned(), action });
}

pub fn outputs() -> Option<HashMap<String, Output>> {
    match request(Request::Outputs)? {
        Response::Outputs(o) => Some(o),
        _ => None,
    }
}

pub fn workspaces() -> Vec<niri_ipc::Workspace> {
    match request(Request::Workspaces) {
        Some(Response::Workspaces(w)) => w,
        _ => Vec::new(),
    }
}

fn summarize(o: &HashMap<String, Output>, panel: &str) -> (Option<String>, f64) {
    let mut ext: Vec<&String> = o
        .iter()
        .filter(|(n, v)| n.as_str() != panel && v.current_mode.is_some() && v.logical.is_some())
        .map(|(n, _)| n)
        .collect();
    ext.sort();
    let scale = o
        .get(panel)
        .and_then(|v| v.logical.as_ref())
        .map(|l| l.scale)
        .unwrap_or(1.5);
    (ext.first().map(|s| (*s).clone()), scale)
}

/// Poll the output list every 2 s (niri's event stream has no output events);
/// one request over an open socket, a few hundred microseconds.
pub fn watch_outputs(tx: Sender<Msg>, panel: String) {
    std::thread::spawn(move || {
        let mut last: Option<(Option<String>, i64)> = None;
        loop {
            if let Some(o) = outputs() {
                let (external, panel_scale) = summarize(&o, &panel);
                let key = (external.clone(), (panel_scale * 1000.0) as i64);
                if last.as_ref() != Some(&key) {
                    last = Some(key);
                    if tx.send(Msg::Outputs { external, panel_scale }).is_err() {
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// One event stream: keyboard layout, workspaces + window counts, config reloads.
pub fn watch_events(tx: Sender<Msg>) {
    std::thread::spawn(move || loop {
        if let Ok(mut s) = Socket::connect() {
            if let Ok(Reply::Ok(_)) = s.send(Request::EventStream) {
                let mut names: Vec<String> = Vec::new();
                let mut wss: Vec<niri_ipc::Workspace> = Vec::new();
                let mut wins: HashMap<u64, Option<u64>> = HashMap::new();
                let mut last: Vec<Ws> = Vec::new();
                let mut read = s.read_events();
                while let Ok(ev) = read() {
                    let mut ws_dirty = false;
                    match ev {
                        Event::KeyboardLayoutsChanged { keyboard_layouts } => {
                            names = keyboard_layouts.names;
                            send_layout(&tx, &names, keyboard_layouts.current_idx);
                        }
                        Event::KeyboardLayoutSwitched { idx } => send_layout(&tx, &names, idx),
                        Event::WorkspacesChanged { workspaces } => {
                            wss = workspaces;
                            ws_dirty = true;
                        }
                        Event::WorkspaceActivated { id, focused } => {
                            let out = wss.iter().find(|w| w.id == id).and_then(|w| w.output.clone());
                            for w in wss.iter_mut() {
                                if w.output == out {
                                    w.is_active = w.id == id;
                                }
                                if focused {
                                    w.is_focused = w.id == id;
                                }
                            }
                            ws_dirty = true;
                        }
                        Event::WindowsChanged { windows } => {
                            wins = windows.into_iter().map(|w| (w.id, w.workspace_id)).collect();
                            ws_dirty = true;
                        }
                        Event::WindowOpenedOrChanged { window } => {
                            wins.insert(window.id, window.workspace_id);
                            ws_dirty = true;
                        }
                        Event::WindowClosed { id } => {
                            wins.remove(&id);
                            ws_dirty = true;
                        }
                        Event::ConfigLoaded { .. } => {
                            if tx.send(Msg::ConfigReloaded).is_err() {
                                return;
                            }
                        }
                        _ => {}
                    }
                    if ws_dirty {
                        let mut v: Vec<Ws> = wss
                            .iter()
                            .map(|w| Ws {
                                id: w.id,
                                idx: w.idx,
                                output: w.output.clone().unwrap_or_default(),
                                active: w.is_active,
                                focused: w.is_focused,
                                windows: wins.values().filter(|x| **x == Some(w.id)).count() as u32,
                            })
                            .collect();
                        v.sort_by(|a, b| a.output.cmp(&b.output).then(a.idx.cmp(&b.idx)));
                        if v != last {
                            last = v.clone();
                            if tx.send(Msg::Workspaces(v)).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_secs(5));
    });
}

fn send_layout(tx: &Sender<Msg>, names: &[String], idx: u8) {
    let ru = names
        .get(idx as usize)
        .map(|n| n.contains("Russian") || n.to_lowercase().starts_with("ru"))
        .unwrap_or(false);
    let _ = tx.send(Msg::LayoutRu(ru));
}

/// `Mod+<Key>` binds of the niri config (includes followed): key name -> short label.
pub fn tux_binds() -> HashMap<String, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let cfg = std::env::var("NIRI_CONFIG").unwrap_or(format!("{home}/.config/niri/config.kdl"));
    let mut out = HashMap::new();
    let mut files = vec![std::path::PathBuf::from(cfg)];
    let mut seen = 0;
    while let Some(f) = files.pop() {
        seen += 1;
        if seen > 16 {
            break;
        }
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let dir = std::fs::canonicalize(&f).ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
        for line in text.lines() {
            let t = line.trim();
            if let Some(inc) = t.strip_prefix("include ") {
                let inc = inc.trim().trim_matches('"');
                if let Some(d) = &dir {
                    files.push(d.join(inc));
                }
                continue;
            }
            let Some(rest) = t.strip_prefix("Mod+") else { continue };
            let key: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            if key.is_empty() || rest[key.len()..].starts_with('+') {
                continue; // Mod+Shift+..., Mod+Ctrl+...: only plain Mod+Key
            }
            let title = t.split("hotkey-overlay-title=\"").nth(1).and_then(|r| r.split('"').next());
            let body = t.split('{').nth(1).map(|b| b.trim_end_matches('}').trim().trim_end_matches(';'));
            let label = match (title, body) {
                (Some(ti), _) => ti
                    .split(|c| c == '(' || c == '—' || c == '-')
                    .next()
                    .unwrap_or(ti)
                    .trim()
                    .to_owned(),
                (None, Some(b)) => short_action(b),
                _ => continue,
            };
            out.entry(key).or_insert(label);
        }
    }
    out
}

fn short_action(b: &str) -> String {
    let w: Vec<&str> = b.split_whitespace().collect();
    let a = w.first().copied().unwrap_or("");
    let arg = w.get(1).map(|x| x.trim_matches('"')).unwrap_or("");
    match a {
        "spawn" | "spawn-sh" => arg.rsplit('/').next().unwrap_or(arg).to_owned(),
        "focus-workspace" => format!("ws {arg}"),
        "close-window" => "close".into(),
        "toggle-overview" => "overview".into(),
        "maximize-column" => "maximize".into(),
        "maximize-window-to-edges" => "fill".into(),
        "fullscreen-window" => "fullscreen".into(),
        "switch-preset-column-width" => "width".into(),
        "set-column-width" => format!("w {arg}"),
        "focus-column-left" => "← col".into(),
        "focus-column-right" => "col →".into(),
        "focus-window-up" => "↑ win".into(),
        "focus-window-down" => "↓ win".into(),
        "focus-workspace-up" => "↑ ws".into(),
        "focus-workspace-down" => "↓ ws".into(),
        "focus-column-first" => "first".into(),
        "focus-column-last" => "last".into(),
        "toggle-column-tabbed-display" => "tabs".into(),
        "consume-or-expel-window-left" => "← stack".into(),
        "consume-or-expel-window-right" => "stack →".into(),
        "expel-window-from-column" => "unstack".into(),
        "toggle-window-floating" => "float".into(),
        "screenshot" | "screenshot-screen" | "screenshot-window" => "shot".into(),
        _ => a.replace('-', " ").chars().take(10).collect(),
    }
}
