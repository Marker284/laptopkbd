// SPDX-License-Identifier: GPL-2.0-or-later
//! Keys, latching modifiers and the trackpad; output through uinput, so niri
//! sees a real keyboard and mouse (its binds work).
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode as K, RelativeAxisCode as R};
use niri_ipc::Action;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

pub enum Fx {
    Key(usize),
    Pad,
    Niri(Action),
    EscHold,
    EscRelease,
}

struct Pt {
    x: f32,
    y: f32,
}

pub struct Board {
    kbd: VirtualDevice,
    mouse: VirtualDevice,
    pub down: HashMap<i32, usize>, // touch id -> key index
    codes: Vec<u16>,
    pub latched: HashSet<u16>,
    pub locked: HashSet<u16>,
    mod_used: HashSet<u16>,
    last_mod_tap: HashMap<u16, Instant>,
    pad: HashMap<i32, Pt>,
    pad_t0: Instant,
    pad_max: usize,
    pad_moved: f32,
    pad_sum: (f32, f32),
    pub drag: bool,
    last_tap: Option<Instant>,
    acc: (f32, f32),
    wacc: (f32, f32),
    pub gain: f32, // pointer px per buffer px
}

fn is_mod(c: u16) -> bool {
    [K::KEY_LEFTSHIFT, K::KEY_RIGHTSHIFT, K::KEY_LEFTCTRL, K::KEY_RIGHTCTRL, K::KEY_LEFTALT,
     K::KEY_RIGHTALT, K::KEY_LEFTMETA]
        .iter()
        .any(|k| k.0 == c)
}

impl Board {
    pub fn new(codes: Vec<u16>, gain: f32) -> std::io::Result<Self> {
        let mut keys = AttributeSet::<K>::new();
        for c in &codes {
            keys.insert(K(*c));
        }
        let kbd = VirtualDevice::builder()?.name("laptopkbd keyboard").with_keys(&keys)?.build()?;
        let mut btn = AttributeSet::<K>::new();
        for b in [K::BTN_LEFT, K::BTN_RIGHT, K::BTN_MIDDLE] {
            btn.insert(b);
        }
        let mut rel = AttributeSet::<R>::new();
        for a in [R::REL_X, R::REL_Y, R::REL_WHEEL, R::REL_HWHEEL, R::REL_WHEEL_HI_RES, R::REL_HWHEEL_HI_RES] {
            rel.insert(a);
        }
        let mouse = VirtualDevice::builder()?
            .name("laptopkbd mouse")
            .with_keys(&btn)?
            .with_relative_axes(&rel)?
            .build()?;
        Ok(Self {
            kbd,
            mouse,
            down: HashMap::new(),
            codes,
            latched: HashSet::new(),
            locked: HashSet::new(),
            mod_used: HashSet::new(),
            last_mod_tap: HashMap::new(),
            pad: HashMap::new(),
            pad_t0: Instant::now(),
            pad_max: 0,
            pad_moved: 0.0,
            pad_sum: (0.0, 0.0),
            drag: false,
            last_tap: None,
            acc: (0.0, 0.0),
            wacc: (0.0, 0.0),
            gain,
        })
    }

    fn key(&mut self, code: u16, v: i32) {
        let _ = self.kbd.emit(&[InputEvent::new(EventType::KEY.0, code, v)]);
    }
    fn btn(&mut self, b: K, v: i32) {
        let _ = self.mouse.emit(&[InputEvent::new(EventType::KEY.0, b.0, v)]);
    }
    fn click(&mut self, b: K) {
        self.btn(b, 1);
        self.btn(b, 0);
    }

    pub fn release_all(&mut self) {
        let codes = self.codes.clone();
        let ev: Vec<InputEvent> = codes.iter().map(|c| InputEvent::new(EventType::KEY.0, *c, 0)).collect();
        let _ = self.kbd.emit(&ev);
        for b in [K::BTN_LEFT, K::BTN_RIGHT, K::BTN_MIDDLE] {
            self.btn(b, 0);
        }
        self.down.clear();
        self.latched.clear();
        self.locked.clear();
        self.pad.clear();
        self.drag = false;
    }

    // ---- keys ----------------------------------------------------------
    pub fn key_down(&mut self, id: i32, idx: usize, code: u16, fx: &mut Vec<Fx>) {
        self.down.insert(id, idx);
        fx.push(Fx::Key(idx));
        if code == K::KEY_ESC.0 {
            fx.push(Fx::EscHold);
        }
        if is_mod(code) {
            if self.locked.contains(&code) {
                return; // unlocks on release
            }
            if !self.latched.contains(&code) {
                self.key(code, 1);
            }
            self.mod_used.remove(&code);
            return;
        }
        let held: Vec<u16> = self.down.values().map(|i| self.codes[*i]).filter(|c| is_mod(*c)).collect();
        self.mod_used.extend(held);
        self.key(code, 1);
    }

    /// Returns the indexes of latched modifier keys released by this key, for redraw.
    pub fn key_up(&mut self, id: i32, fx: &mut Vec<Fx>, key_index_of: impl Fn(u16) -> Vec<usize>) {
        let Some(idx) = self.down.remove(&id) else { return };
        let code = self.codes[idx];
        fx.push(Fx::Key(idx));
        if code == K::KEY_ESC.0 {
            fx.push(Fx::EscRelease);
        }
        if is_mod(code) {
            let now = Instant::now();
            let recent = self
                .last_mod_tap
                .get(&code)
                .map_or(false, |t| now.duration_since(*t).as_secs_f32() < 0.35);
            if self.locked.remove(&code) {
                self.key(code, 0);
            } else if self.mod_used.contains(&code) {
                self.key(code, 0); // chord: held while another key was pressed
            } else if self.latched.contains(&code) {
                self.latched.remove(&code);
                if recent {
                    self.locked.insert(code); // double tap = lock
                } else {
                    self.key(code, 0); // tap again = off
                }
            } else {
                self.latched.insert(code); // one-shot: stays pressed for the next key
            }
            self.last_mod_tap.insert(code, now);
            return;
        }
        self.key(code, 0);
        if !self.latched.is_empty() {
            let l: Vec<u16> = self.latched.drain().collect();
            for m in l {
                self.key(m, 0);
                for i in key_index_of(m) {
                    fx.push(Fx::Key(i));
                }
            }
        }
    }

    // ---- trackpad -------------------------------------------------------
    pub fn in_pad(&self, id: i32) -> bool {
        self.pad.contains_key(&id)
    }

    pub fn pad_begin(&mut self, id: i32, x: f32, y: f32, fx: &mut Vec<Fx>) {
        let now = Instant::now();
        if self.pad.is_empty() {
            self.pad_t0 = now;
            self.pad_max = 0;
            self.pad_moved = 0.0;
            self.pad_sum = (0.0, 0.0);
            if self.last_tap.map_or(false, |t| now.duration_since(t).as_secs_f32() < 0.30) {
                self.drag = true; // tap, then touch again: drag
                self.btn(K::BTN_LEFT, 1);
                fx.push(Fx::Pad);
            }
        }
        self.pad.insert(id, Pt { x, y });
        self.pad_max = self.pad_max.max(self.pad.len());
    }

    pub fn pad_move(&mut self, id: i32, x: f32, y: f32) {
        let n = self.pad.len();
        let Some(p) = self.pad.get_mut(&id) else { return };
        let (dx, dy) = (x - p.x, y - p.y);
        p.x = x;
        p.y = y;
        self.pad_moved += dx.hypot(dy) / n as f32;
        if n == 1 && self.pad_max == 1 {
            let speed = dx.hypot(dy);
            let g = self.gain * (1.0 + speed.min(45.0) / 90.0); // mild; libinput adds its own
            self.acc.0 += dx * g;
            self.acc.1 += dy * g;
            let (ix, iy) = (self.acc.0.trunc() as i32, self.acc.1.trunc() as i32);
            if ix != 0 || iy != 0 {
                self.acc.0 -= ix as f32;
                self.acc.1 -= iy as f32;
                let mut ev = Vec::with_capacity(2);
                if ix != 0 {
                    ev.push(InputEvent::new(EventType::RELATIVE.0, R::REL_X.0, ix));
                }
                if iy != 0 {
                    ev.push(InputEvent::new(EventType::RELATIVE.0, R::REL_Y.0, iy));
                }
                let _ = self.mouse.emit(&ev);
            }
        } else if n == 2 {
            // natural scrolling, as on the laptop
            self.wacc.0 += dx / 2.0 * 4.0 / self.gain.max(0.1);
            self.wacc.1 += dy / 2.0 * 4.0 / self.gain.max(0.1);
            let (wx, wy) = (self.wacc.0.trunc() as i32, self.wacc.1.trunc() as i32);
            if wx != 0 || wy != 0 {
                self.wacc.0 -= wx as f32;
                self.wacc.1 -= wy as f32;
                let mut ev = Vec::with_capacity(2);
                if wy != 0 {
                    ev.push(InputEvent::new(EventType::RELATIVE.0, R::REL_WHEEL_HI_RES.0, wy));
                }
                if wx != 0 {
                    ev.push(InputEvent::new(EventType::RELATIVE.0, R::REL_HWHEEL_HI_RES.0, -wx));
                }
                let _ = self.mouse.emit(&ev);
            }
        } else if n >= 3 {
            self.pad_sum.0 += dx / n as f32;
            self.pad_sum.1 += dy / n as f32;
        }
    }

    pub fn pad_end(&mut self, id: i32, scale: f32, fx: &mut Vec<Fx>) {
        if self.pad.remove(&id).is_none() || !self.pad.is_empty() {
            return;
        }
        let now = Instant::now();
        let dur = now.duration_since(self.pad_t0).as_secs_f32();
        let moved = self.pad_moved / scale; // logical px
        if self.drag {
            self.drag = false;
            self.btn(K::BTN_LEFT, 0);
            self.last_tap = None;
            fx.push(Fx::Pad);
            if dur < 0.18 && moved < 10.0 {
                self.click(K::BTN_LEFT); // it was a double tap
            }
            return;
        }
        if self.pad_max >= 3 && moved > 40.0 {
            let (sx, sy) = self.pad_sum;
            let a = if sx.abs() > sy.abs() {
                if sx < 0.0 { Action::FocusColumnRight {} } else { Action::FocusColumnLeft {} }
            } else if self.pad_max >= 4 && sy < 0.0 {
                Action::ToggleOverview {}
            } else if sy < 0.0 {
                Action::FocusWorkspaceDown {}
            } else {
                Action::FocusWorkspaceUp {}
            };
            fx.push(Fx::Niri(a));
            return;
        }
        if dur < 0.22 && moved < 12.0 {
            let b = match self.pad_max {
                1 => K::BTN_LEFT,
                2 => K::BTN_RIGHT,
                _ => K::BTN_MIDDLE,
            };
            self.click(b);
            self.last_tap = if b == K::BTN_LEFT { Some(now) } else { None };
        }
    }
}
