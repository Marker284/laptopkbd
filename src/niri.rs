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

/// Follow the keyboard layout through niri's event stream: no polling at all.
pub fn watch_layout(tx: Sender<Msg>) {
    std::thread::spawn(move || loop {
        if let Ok(mut s) = Socket::connect() {
            if let Ok(Reply::Ok(_)) = s.send(Request::EventStream) {
                let mut names: Vec<String> = Vec::new();
                let mut read = s.read_events();
                while let Ok(ev) = read() {
                    let idx = match ev {
                        Event::KeyboardLayoutsChanged { keyboard_layouts } => {
                            names = keyboard_layouts.names;
                            keyboard_layouts.current_idx
                        }
                        Event::KeyboardLayoutSwitched { idx } => idx,
                        _ => continue,
                    };
                    let ru = names
                        .get(idx as usize)
                        .map(|n| n.contains("Russian") || n.to_lowercase().starts_with("ru"))
                        .unwrap_or(false);
                    if tx.send(Msg::LayoutRu(ru)).is_err() {
                        return;
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_secs(5));
    });
}
