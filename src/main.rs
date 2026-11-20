// SPDX-License-Identifier: GPL-2.0-or-later
//! laptopkbd: while an external monitor is connected, the tablet panel becomes
//! the bottom half of a laptop: keyboard on top, trackpad between two palm rests
//! that show widgets. Runs all the time inside niri; idle (no surface, no grab,
//! no timers) while there is no external monitor.
//!
//! Signals: SIGHUP = re-read ~/.config/laptopkbd/config.toml,
//!          SIGUSR1 = toggle the preview (laptop mode without a monitor).
//!
//! Idea from hingeboard (github.com/brysonwf/hingeboard, GPL-2.0+).
mod board;
mod config;
mod niri;
mod stats;
mod touch;
mod ui;

use board::{Board, Fx};
use config::Config;
use niri_ipc::{Action, ConfiguredPosition, OutputAction, PositionToSet, Transform, WorkspaceReferenceArg};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            channel::{self, Event as ChEvent},
            generic::Generic,
            timer::{TimeoutAction, Timer},
            EventLoop, Interest, LoopHandle, Mode, PostAction, RegistrationToken,
        },
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{
        slot::{Buffer, SlotPool},
        Shm, ShmHandler,
    },
};
use stats::Stats;
use std::time::Duration;
use touch::{Kind, RawTouch, Touch};
use ui::{Look, Rect, Ui};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use wayland_protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter};

const GAIN: f32 = 1.2; // pointer px per buffer px at speed 1.0 (buffer = 1.5 x logical)

struct Buf {
    buf: Buffer,
    stale: Option<Rect>,
}

struct Surf {
    layer: LayerSurface,
    viewport: WpViewport,
    pool: Option<SlotPool>,
    bufs: Vec<Buf>,
    configured: bool,
    size: (u32, u32),
    frame_pending: bool,
    needs_present: bool,
    damage: Option<Rect>,
}

struct App {
    registry: RegistryState,
    outputs: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    viewporter: WpViewporter,
    qh: QueueHandle<App>,
    lh: LoopHandle<'static, App>,

    cfg: Config,
    external: Option<String>,
    manual_off: Option<String>, // Esc held: stay off until this monitor goes away
    active: bool,
    preview: bool,
    scale: f64,
    ru: bool,
    ws: Vec<niri::Ws>,
    hints: std::collections::HashMap<String, String>,
    hints_on: bool,

    surf: Option<Surf>,
    ui: Option<Ui>,
    board: Option<Board>,
    touch_tok: Option<RegistrationToken>,
    stats_tok: Option<RegistrationToken>,
    esc_tok: Option<RegistrationToken>,
    stats: Stats,
    touches: Vec<Touch>,
}

fn flag() -> std::path::PathBuf {
    let d = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    std::path::Path::new(&d).join("laptop-mode")
}

/// A user hook from the config, run detached through niri (inherits the session env).
fn hook(cmd: &str) {
    if !cmd.trim().is_empty() {
        niri::action(Action::Spawn { command: vec!["sh".into(), "-c".into(), cmd.to_owned()] });
    }
}

fn sh(cmd: &[&str]) {
    let _ = std::process::Command::new(cmd[0]).args(&cmd[1..]).status();
}

/// Keep the keyboard out of swap and first for the CPU, when it runs in its own scope.
fn protect() {
    if let Ok(cg) = std::fs::read_to_string("/proc/self/cgroup") {
        if let Some(unit) = cg.trim().rsplit('/').next() {
            if unit.ends_with(".scope") {
                sh(&["systemctl", "--user", "set-property", "--runtime", unit, "MemoryMin=64M",
                     "MemoryLow=64M", "CPUWeight=1000"]);
            }
        }
    }
}

/// SIGHUP / SIGUSR1 arrive as channel messages (blocked in every thread, sigwait in one).
fn signals(tx: channel::Sender<niri::Msg>) {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGHUP);
        libc::sigaddset(&mut set, libc::SIGUSR1);
        libc::sigaddset(&mut set, libc::SIGUSR2);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        std::thread::spawn(move || loop {
            let mut sig = 0;
            if libc::sigwait(&set, &mut sig) == 0 {
                let m = match sig {
                    libc::SIGUSR1 => niri::Msg::TogglePreview,
                    libc::SIGUSR2 => niri::Msg::DemoHints,
                    _ => niri::Msg::Reload,
                };
                if tx.send(m).is_err() {
                    return;
                }
            }
        });
    }
}

impl App {
    // ---- mode ----------------------------------------------------------
    fn on_outputs(&mut self, external: Option<String>, scale: f64) {
        self.scale = scale;
        if self.manual_off.is_some() && self.manual_off != external {
            self.manual_off = None;
        }
        self.external = external.clone();
        if self.preview {
            return;
        }
        match external {
            Some(e) if !self.active && self.manual_off.is_none() && self.cfg.auto_laptop_mode => self.enter(&e),
            None if self.active => self.leave(),
            _ => {}
        }
    }

    fn pause_tablet_mode(&self) {
        let _ = std::fs::write(flag(), b"");
        hook(&self.cfg.device.on_enter);
        let t = match self.cfg.device.transform.as_str() {
            "90" => Transform::_90,
            "180" => Transform::_180,
            "270" => Transform::_270,
            _ => Transform::Normal,
        };
        niri::output(&self.cfg.device.output, OutputAction::Transform { transform: t });
    }

    fn enter(&mut self, ext: &str) {
        self.pause_tablet_mode();
        for w in niri::workspaces() {
            if w.output.as_deref() == Some(self.cfg.device.output.as_str()) && w.active_window_id.is_some() {
                niri::action(Action::MoveWorkspaceToMonitor {
                    output: ext.to_owned(),
                    reference: Some(WorkspaceReferenceArg::Id(w.id)),
                });
            }
        }
        // the panel far away from the monitor: the pointer can not wander onto it
        niri::output(&self.cfg.device.output, OutputAction::Position {
            position: PositionToSet::Specific(ConfiguredPosition { x: -10000, y: 0 }),
        });
        niri::action(Action::FocusMonitor { output: ext.to_owned() });
        self.active = true;
        self.show();
    }

    fn enter_preview(&mut self) {
        self.pause_tablet_mode();
        self.preview = true;
        self.active = true;
        self.show();
    }

    fn leave(&mut self) {
        self.hide();
        self.active = false;
        if !self.preview {
            niri::output(&self.cfg.device.output, OutputAction::Position { position: PositionToSet::Automatic });
        }
        self.preview = false;
        let _ = std::fs::remove_file(flag());
        hook(&self.cfg.device.on_leave);
    }

    fn reload(&mut self) {
        let old = std::mem::replace(&mut self.cfg, Config::load());
        if let Some(b) = self.board.as_mut() {
            b.configure(&self.cfg.trackpad, GAIN);
        }
        if old != self.cfg {
            if let Some((w, h)) = self.surf.as_ref().filter(|s| s.configured).map(|s| s.size) {
                self.rebuild_ui(w, h);
            }
        }
        if !self.cfg.auto_laptop_mode && self.active && !self.preview {
            self.leave();
        } else if self.cfg.auto_laptop_mode && !self.active {
            if let Some(e) = self.external.clone() {
                if self.manual_off.is_none() {
                    self.enter(&e);
                }
            }
        }
    }

    fn show(&mut self) {
        if self.surf.is_some() {
            return;
        }
        let Some(out) = self.outputs.outputs().find(|o| {
            self.outputs.info(o).and_then(|i| i.name).as_deref() == Some(self.cfg.device.output.as_str())
        }) else {
            eprintln!("laptopkbd: no wl_output {}", self.cfg.device.output);
            return;
        };
        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(&self.qh, surface, Layer::Overlay, Some("laptopkbd"), Some(&out));
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        let viewport = self.viewporter.get_viewport(layer.wl_surface(), &self.qh, ());
        layer.commit();
        self.surf = Some(Surf {
            layer,
            viewport,
            pool: None,
            bufs: Vec::new(),
            configured: false,
            size: (0, 0),
            frame_pending: false,
            needs_present: false,
            damage: None,
        });
    }

    fn hide(&mut self) {
        if let Some(t) = self.touch_tok.take() {
            self.lh.remove(t); // drops RawTouch: ends the grab, closes the fd
        }
        for t in [self.stats_tok.take(), self.esc_tok.take()].into_iter().flatten() {
            self.lh.remove(t);
        }
        if let Some(mut b) = self.board.take() {
            b.release_all();
        }
        if let Some(s) = self.surf.take() {
            s.viewport.destroy();
            drop(s.bufs);
            drop(s.layer);
        }
        self.ui = None;
        self.hints_on = false;
    }

    fn rebuild_ui(&mut self, lw: u32, lh: u32) {
        let s = self.scale as f32;
        let (pw, ph) = ((lw as f32 * s).round() as u32, (lh as f32 * s).round() as u32);
        let mut ui = Ui::new(pw, ph, s, &self.cfg);
        ui.hints = self.hints.clone();
        ui.draw_all(self.ru, &self.stats, &self.ws, self.external.as_deref());
        let codes: Vec<u16> = ui.keys.iter().map(|k| k.code).collect();
        self.ui = Some(ui);
        if let Some(b) = self.board.as_mut() {
            b.set_keys(codes);
        }
        self.mark(Rect::new(0.0, 0.0, pw as f32, ph as f32));
        self.present();
    }

    fn configured(&mut self, lw: u32, lh: u32) {
        let s = self.scale as f32;
        let (pw, ph) = ((lw as f32 * s).round() as u32, (lh as f32 * s).round() as u32);
        let Some(surf) = self.surf.as_mut() else { return };
        surf.viewport.set_destination(lw as i32, lh as i32);
        let mut pool = match SlotPool::new((pw * ph * 4 * 2) as usize, &self.shm) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("laptopkbd: shm pool: {e}");
                return;
            }
        };
        surf.bufs.clear();
        for _ in 0..2 {
            let (buf, _) = pool
                .create_buffer(pw as i32, ph as i32, pw as i32 * 4, wl_shm::Format::Xrgb8888)
                .expect("buffer");
            surf.bufs.push(Buf { buf, stale: None });
        }
        surf.pool = Some(pool);
        surf.configured = true;
        surf.size = (lw, lh);

        if self.ui.is_none() {
            self.stats.update();
            self.hints = niri::tux_binds();
        }
        if self.board.is_none() {
            let keys: Vec<u16> = Ui::new(1, 1, s, &self.cfg).keys.iter().map(|k| k.code).collect();
            match Board::new(ui::all_codes(), keys, GAIN) {
                Ok(mut b) => {
                    b.configure(&self.cfg.trackpad, GAIN);
                    self.board = Some(b);
                }
                Err(e) => eprintln!("laptopkbd: uinput: {e}"),
            }
        }
        self.rebuild_ui(lw, lh);

        if self.touch_tok.is_none() {
            let rot = self.cfg.device.transform.parse().unwrap_or(0);
            match RawTouch::open(&self.cfg.device.touchscreen, rot) {
                Ok(rt) => {
                    let src = Generic::new(rt, Interest::READ, Mode::Level);
                    self.touch_tok = self
                        .lh
                        .insert_source(src, |_, rt, app: &mut App| {
                            let (w, h) = app.ui.as_ref().map_or((1.0, 1.0), |u| (u.w, u.h));
                            let mut v = std::mem::take(&mut app.touches);
                            // SAFETY: the device stays owned by the source
                            unsafe { rt.get_mut() }.read(w, h, &mut v);
                            for t in v.drain(..) {
                                app.on_touch(t);
                            }
                            app.touches = v;
                            app.present();
                            Ok(PostAction::Continue)
                        })
                        .ok();
                }
                Err(e) => eprintln!("laptopkbd: touchscreen: {e}"),
            }
        }
        if self.stats_tok.is_none() {
            self.stats_tok = self
                .lh
                .insert_source(Timer::from_duration(Duration::from_secs(2)), |_, _, app: &mut App| {
                    app.stats.update();
                    if let Some(ui) = app.ui.as_mut() {
                        let rs = ui.draw_panels(&app.stats, &app.ws, app.external.as_deref());
                        for r in rs {
                            app.mark(r);
                        }
                        app.present();
                    }
                    TimeoutAction::ToDuration(Duration::from_secs(2))
                })
                .ok();
        }
        self.present();
    }

    // ---- input ---------------------------------------------------------
    fn look(&self, i: usize) -> Look {
        let (Some(ui), Some(b)) = (self.ui.as_ref(), self.board.as_ref()) else { return Look::Normal };
        let code = ui.keys[i].code;
        if b.down.values().any(|j| *j == i) {
            Look::Down
        } else if b.latched.contains(&code) || b.locked.contains(&code) {
            Look::Latched
        } else {
            Look::Normal
        }
    }

    fn redraw_key(&mut self, i: usize) {
        let look = self.look(i);
        let ru = self.ru;
        if let Some(ui) = self.ui.as_mut() {
            let r = ui.draw_key(i, look, ru);
            self.mark(r);
        }
    }

    fn redraw_all_keys(&mut self) {
        let n = self.ui.as_ref().map_or(0, |u| u.keys.len());
        for i in 0..n {
            self.redraw_key(i);
        }
    }

    fn on_touch(&mut self, t: Touch) {
        let mut fx = Vec::new();
        let (Some(ui), Some(b)) = (self.ui.as_ref(), self.board.as_mut()) else { return };
        match t.kind {
            Kind::Begin => {
                if let Some(i) = ui.hit_key(t.x, t.y) {
                    b.key_down(t.id, i, ui.keys[i].code, &mut fx);
                } else if ui.pad.contains(t.x, t.y, 12.0 * ui.s) {
                    b.pad_begin(t.id, t.x, t.y, &mut fx);
                } else if let Some((_, id)) = ui.ws_hits.iter().find(|(r, _)| r.contains(t.x, t.y, 4.0 * ui.s)) {
                    fx.push(Fx::Niri(Action::FocusWorkspace { reference: WorkspaceReferenceArg::Id(*id) }));
                }
            }
            Kind::Move => b.pad_move(t.id, t.x, t.y),
            Kind::End => {
                if b.down.contains_key(&t.id) {
                    let keys: Vec<u16> = ui.keys.iter().map(|k| k.code).collect();
                    b.key_up(t.id, &mut fx, |c| {
                        keys.iter().enumerate().filter(|(_, k)| **k == c).map(|(i, _)| i).collect()
                    });
                } else if b.in_pad(t.id) {
                    b.pad_end(t.id, ui.s, &mut fx);
                }
            }
        }
        for f in fx {
            match f {
                Fx::Key(i) => self.redraw_key(i),
                Fx::Pad => {
                    let drag = self.board.as_ref().map_or(false, |b| b.drag);
                    if let Some(ui) = self.ui.as_mut() {
                        let r = ui.draw_pad(drag);
                        self.mark(r);
                    }
                }
                Fx::Niri(a) => niri::action(a),
                Fx::EscHold => {
                    if let Some(t) = self.esc_tok.take() {
                        self.lh.remove(t);
                    }
                    self.esc_tok = self
                        .lh
                        .insert_source(Timer::from_duration(Duration::from_secs(2)), |_, _, app: &mut App| {
                            app.esc_tok = None;
                            let esc = evdev::KeyCode::KEY_ESC.0;
                            let held = match (app.ui.as_ref(), app.board.as_ref()) {
                                (Some(ui), Some(b)) => b.down.values().any(|i| ui.keys[*i].code == esc),
                                _ => false,
                            };
                            if held {
                                app.manual_off = app.external.clone();
                                app.leave();
                            }
                            TimeoutAction::Drop
                        })
                        .ok();
                }
                Fx::EscRelease => {
                    if let Some(t) = self.esc_tok.take() {
                        self.lh.remove(t);
                    }
                }
            }
        }
        // Tux held or latched: keys show the niri binds
        let tux = self.cfg.tux_hints && self.board.as_ref().map_or(false, |b| b.tux_active());
        if tux != self.hints_on {
            self.hints_on = tux;
            if let Some(ui) = self.ui.as_mut() {
                ui.hints_on = tux;
            }
            self.redraw_all_keys();
        }
    }

    fn set_ru(&mut self, ru: bool) {
        if self.ru == ru {
            return;
        }
        self.ru = ru;
        let idx: Vec<usize> = self.ui.as_ref().map_or(Vec::new(), |u| {
            u.keys
                .iter()
                .enumerate()
                .filter(|(_, k)| !k.ru.is_empty() || k.code == evdev::KeyCode::KEY_CAPSLOCK.0)
                .map(|(i, _)| i)
                .collect()
        });
        for i in idx {
            self.redraw_key(i);
        }
        self.present();
    }

    fn set_workspaces(&mut self, ws: Vec<niri::Ws>) {
        self.ws = ws;
        if let Some(ui) = self.ui.as_mut() {
            let rs = ui.draw_workspaces(&self.stats, &self.ws, self.external.as_deref());
            for r in rs {
                self.mark(r);
            }
            self.present();
        }
    }

    // ---- presenting ----------------------------------------------------
    fn mark(&mut self, r: Rect) {
        if let Some(s) = self.surf.as_mut() {
            for b in s.bufs.iter_mut() {
                b.stale = Some(Rect::union(b.stale, r));
            }
            s.damage = Some(Rect::union(s.damage, r));
        }
    }

    fn present(&mut self) {
        let (Some(s), Some(ui)) = (self.surf.as_mut(), self.ui.as_ref()) else { return };
        if !s.configured || s.damage.is_none() {
            return;
        }
        if s.frame_pending {
            s.needs_present = true;
            return;
        }
        let Some(pool) = s.pool.as_mut() else { return };
        let (w, h) = (ui.canvas.width() as i32, ui.canvas.height() as i32);
        let src = ui.canvas.data();
        let mut used = None;
        for (bi, b) in s.bufs.iter_mut().enumerate() {
            let Some(dst) = b.buf.canvas(pool) else { continue };
            if let Some(r) = b.stale.take() {
                let x0 = (r.x.floor() as i32).clamp(0, w);
                let x1 = ((r.x + r.w).ceil() as i32).clamp(0, w);
                let y0 = (r.y.floor() as i32).clamp(0, h);
                let y1 = ((r.y + r.h).ceil() as i32).clamp(0, h);
                for y in y0..y1 {
                    let o = ((y * w + x0) * 4) as usize;
                    let e = ((y * w + x1) * 4) as usize;
                    // RGBA (tiny-skia) -> XRGB8888 little endian (B, G, R, X)
                    for (d, sp) in dst[o..e].chunks_exact_mut(4).zip(src[o..e].chunks_exact(4)) {
                        d[0] = sp[2];
                        d[1] = sp[1];
                        d[2] = sp[0];
                        d[3] = 255;
                    }
                }
            }
            used = Some(bi);
            break;
        }
        let Some(bi) = used else {
            s.needs_present = true;
            return;
        };
        let d = s.damage.take().unwrap();
        let surf = s.layer.wl_surface();
        surf.damage_buffer(d.x.floor() as i32, d.y.floor() as i32, d.w.ceil() as i32 + 2, d.h.ceil() as i32 + 2);
        surf.frame(&self.qh, FrameCallbackData(surf.clone()));
        if s.bufs[bi].buf.attach_to(surf).is_ok() {
            s.layer.commit();
            s.frame_pending = true;
            s.needs_present = false;
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        if let Some(s) = self.surf.as_mut() {
            s.frame_pending = false;
            if s.needs_present {
                self.present();
            }
        }
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        // the panel's wl_output may arrive after niri reported the monitor
        if self.active && self.surf.is_none() {
            self.show();
        }
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.hide();
        if self.active {
            self.show();
        }
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, c: LayerSurfaceConfigure, _: u32) {
        let (w, h) = c.new_size;
        if w == 0 || h == 0 {
            return;
        }
        let same = self.surf.as_ref().map_or(false, |s| s.configured && s.size == (w, h));
        if !same {
            self.configured(w, h);
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState];
}

delegate_registry!(App);
smithay_client_toolkit::delegate_dispatch2!(App);
wayland_client::delegate_noop!(App: WpViewporter);
wayland_client::delegate_noop!(App: WpViewport);

fn main() {
    if std::env::args().any(|a| a == "--version") {
        println!("laptopkbd {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let (tx, rx) = channel::channel::<niri::Msg>();
    signals(tx.clone()); // before any other thread starts: they inherit the blocked mask
    protect();
    let conn = Connection::connect_to_env().expect("wayland");
    let (globals, queue) = registry_queue_init(&conn).expect("registry");
    let qh = queue.handle();
    let mut ev: EventLoop<'static, App> = EventLoop::try_new().expect("event loop");
    let lh = ev.handle();
    WaylandSource::new(conn.clone(), queue).insert(lh.clone()).expect("wayland source");

    let viewporter: WpViewporter = globals.bind(&qh, 1..=1, ()).expect("wp_viewporter");
    let mut app = App {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        compositor: CompositorState::bind(&globals, &qh).expect("wl_compositor"),
        layer_shell: LayerShell::bind(&globals, &qh).expect("layer shell"),
        shm: Shm::bind(&globals, &qh).expect("wl_shm"),
        viewporter,
        qh: qh.clone(),
        lh: lh.clone(),
        cfg: Config::load(),
        external: None,
        manual_off: None,
        active: false,
        preview: false,
        scale: 1.5,
        ru: false,
        ws: Vec::new(),
        hints: Default::default(),
        hints_on: false,
        surf: None,
        ui: None,
        board: None,
        touch_tok: None,
        stats_tok: None,
        esc_tok: None,
        stats: Stats::default(),
        touches: Vec::new(),
    };
    let _ = std::fs::remove_file(flag());

    lh.insert_source(rx, |e, _, app: &mut App| {
        if let ChEvent::Msg(m) = e {
            match m {
                niri::Msg::Outputs { external, panel_scale } => app.on_outputs(external, panel_scale),
                niri::Msg::LayoutRu(ru) => app.set_ru(ru),
                niri::Msg::Workspaces(ws) => app.set_workspaces(ws),
                niri::Msg::ConfigReloaded => {
                    app.hints = niri::tux_binds();
                    if let Some(ui) = app.ui.as_mut() {
                        ui.hints = app.hints.clone();
                    }
                }
                niri::Msg::Reload => app.reload(),
                niri::Msg::DemoHints => {
                    app.hints_on = !app.hints_on;
                    let on = app.hints_on;
                    if let Some(ui) = app.ui.as_mut() {
                        ui.hints_on = on;
                    }
                    app.redraw_all_keys();
                    app.present();
                }
                niri::Msg::TogglePreview => {
                    if app.preview {
                        app.leave();
                    } else if !app.active {
                        app.enter_preview();
                    }
                }
            }
        }
    })
    .expect("channel");
    niri::watch_outputs(tx.clone(), app.cfg.device.output.clone());
    niri::watch_events(tx);

    ev.dispatch(Duration::from_millis(200), &mut app).ok();
    loop {
        if ev.dispatch(None, &mut app).is_err() {
            break;
        }
    }
    app.hide();
}
