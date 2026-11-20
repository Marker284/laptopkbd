// SPDX-License-Identifier: GPL-2.0-or-later
//! Laptop deck: layout, keys, trackpad and the palm-rest widgets, drawn with
//! tiny-skia into one canvas; only what changed is redrawn.
use crate::config::Config;
use crate::niri::Ws;
use crate::stats::{Stats, HIST};
use evdev::KeyCode as K;
use fontdue::{Font, FontSettings, Metrics};
use std::collections::{HashMap, VecDeque};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect as SRect, Stroke, Transform};

type Rgb = (u8, u8, u8);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32, m: f32) -> bool {
        px >= self.x - m && px < self.x + self.w + m && py >= self.y - m && py < self.y + self.h + m
    }
    pub fn union(a: Option<Rect>, b: Rect) -> Rect {
        match a {
            None => b,
            Some(a) => {
                let x0 = a.x.min(b.x);
                let y0 = a.y.min(b.y);
                let x1 = (a.x + a.w).max(b.x + b.w);
                let y1 = (a.y + a.h).max(b.y + b.h);
                Rect::new(x0, y0, x1 - x0, y1 - y0)
            }
        }
    }
    pub fn grow(&self, m: f32) -> Rect {
        Rect::new(self.x - m, self.y - m, self.w + 2.0 * m, self.h + 2.0 * m)
    }
}

// ---- skins -------------------------------------------------------------
pub struct Palette {
    deck: Rgb,
    hinge: Rgb,
    key: Rgb,
    key_down: Rgb,
    key_latch: Rgb,
    outline: (Rgb, u8),
    shadow: u8,
    txt: Rgb,
    dim: Rgb,
    ru_on: Rgb,
    ru_off: Rgb,
    pad: Rgb,
    pad_edge: Rgb,
    card: Rgb,
    card_edge: (Rgb, u8),
    track: Rgb,
    hint: Rgb,
    acc: [Rgb; 5], // cpu gpu ram disk net
    good: Rgb,
    warn: Rgb,
    bad: Rgb,
    rainbow: bool,
}

#[allow(dead_code)]
pub const SKINS: [&str; 6] = ["graphite", "silver", "midnight", "retro", "oled", "neon"];

pub fn palette(name: &str) -> Palette {
    let acc = [(110, 170, 255), (190, 130, 255), (90, 210, 170), (240, 190, 90), (120, 220, 240)];
    let base = Palette {
        deck: (28, 29, 33),
        hinge: (44, 45, 50),
        key: (9, 9, 10),
        key_down: (77, 82, 97),
        key_latch: (51, 92, 158),
        outline: ((255, 255, 255), 13),
        shadow: 115,
        txt: (235, 235, 240),
        dim: (150, 152, 160),
        ru_on: (140, 178, 242),
        ru_off: (95, 115, 160),
        pad: (34, 36, 41),
        pad_edge: (56, 58, 64),
        card: (22, 23, 27),
        card_edge: ((255, 255, 255), 10),
        track: (45, 47, 53),
        hint: (140, 200, 255),
        acc,
        good: (110, 215, 120),
        warn: (255, 160, 80),
        bad: (255, 95, 95),
        rainbow: false,
    };
    match name {
        "silver" => Palette {
            deck: (196, 199, 205),
            hinge: (170, 173, 180),
            key: (246, 246, 248),
            key_down: (178, 190, 214),
            key_latch: (110, 155, 235),
            outline: ((0, 0, 0), 22),
            shadow: 60,
            txt: (28, 29, 34),
            dim: (105, 108, 118),
            ru_on: (30, 90, 210),
            ru_off: (110, 130, 180),
            pad: (210, 213, 219),
            pad_edge: (172, 175, 183),
            card: (222, 224, 229),
            card_edge: ((0, 0, 0), 18),
            track: (190, 193, 200),
            hint: (20, 100, 220),
            acc: [(30, 110, 230), (140, 70, 220), (20, 160, 120), (210, 140, 20), (20, 150, 190)],
            good: (30, 150, 60),
            warn: (215, 110, 20),
            bad: (210, 40, 40),
            ..base
        },
        "midnight" => Palette {
            deck: (15, 21, 40),
            hinge: (30, 40, 70),
            key: (7, 11, 25),
            key_down: (40, 62, 115),
            key_latch: (0, 130, 200),
            outline: ((120, 170, 255), 22),
            txt: (222, 232, 255),
            dim: (120, 136, 172),
            ru_on: (90, 205, 255),
            ru_off: (70, 112, 165),
            pad: (22, 31, 57),
            pad_edge: (52, 66, 108),
            card: (11, 17, 34),
            card_edge: ((120, 170, 255), 18),
            track: (32, 42, 72),
            hint: (90, 220, 255),
            ..base
        },
        "retro" => Palette {
            deck: (204, 197, 178),
            hinge: (180, 172, 152),
            key: (236, 231, 218),
            key_down: (196, 184, 160),
            key_latch: (196, 64, 50),
            outline: ((60, 50, 30), 40),
            shadow: 70,
            txt: (58, 52, 46),
            dim: (125, 114, 98),
            ru_on: (170, 60, 40),
            ru_off: (150, 115, 95),
            pad: (190, 182, 162),
            pad_edge: (160, 151, 132),
            card: (214, 207, 188),
            card_edge: ((60, 50, 30), 30),
            track: (182, 173, 152),
            hint: (190, 50, 40),
            acc: [(60, 100, 160), (130, 80, 150), (60, 130, 90), (190, 120, 40), (40, 120, 140)],
            good: (60, 130, 60),
            warn: (200, 110, 30),
            bad: (190, 40, 40),
            ..base
        },
        "oled" => Palette {
            deck: (0, 0, 0),
            hinge: (20, 20, 20),
            key: (0, 0, 0),
            key_down: (45, 45, 48),
            key_latch: (0, 80, 190),
            outline: ((255, 255, 255), 60),
            shadow: 0,
            pad: (0, 0, 0),
            pad_edge: (80, 80, 84),
            card: (0, 0, 0),
            card_edge: ((255, 255, 255), 45),
            track: (35, 35, 38),
            ..base
        },
        "neon" => Palette {
            deck: (10, 10, 14),
            hinge: (30, 20, 50),
            key: (16, 15, 24),
            key_down: (70, 40, 110),
            key_latch: (220, 40, 160),
            outline: ((255, 255, 255), 0),
            txt: (245, 240, 255),
            dim: (150, 140, 175),
            ru_on: (255, 120, 220),
            ru_off: (170, 100, 170),
            pad: (18, 16, 28),
            pad_edge: (120, 60, 200),
            card: (14, 13, 22),
            card_edge: ((180, 100, 255), 40),
            hint: (255, 120, 220),
            acc: [(0, 220, 255), (255, 60, 200), (120, 255, 120), (255, 200, 0), (180, 120, 255)],
            rainbow: true,
            ..base
        },
        _ => base,
    }
}

fn hue(h: f32) -> Rgb {
    let h = h.rem_euclid(1.0) * 6.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as i32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

// ---- keys ----------------------------------------------------------------
#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    None,
    Backspace,
    Tux,
    Left,
    Right,
    Up,
    Down,
    Caps,
    Mute,
    VolDown,
    VolUp,
    Prev,
    Play,
    Next,
    BriDown,
    BriUp,
}

pub struct Key {
    pub r: Rect,
    pub code: u16,
    pub label: &'static str,
    pub ru: &'static str,
    pub icon: Icon,
    /// niri key name for the Tux hints ("Q", "Return", "BracketLeft" ...)
    pub nkey: &'static str,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Look {
    Normal,
    Down,
    Latched,
}

fn paint(c: Rgb, a: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(c.0, c.1, c.2, a));
    p.anti_alias = true;
    p
}

fn rrect(r: Rect, rad: f32) -> Option<tiny_skia::Path> {
    let rad = rad.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
    let k = 0.4477;
    let mut pb = PathBuilder::new();
    pb.move_to(x0 + rad, y0);
    pb.line_to(x1 - rad, y0);
    pb.cubic_to(x1 - rad * k, y0, x1, y0 + rad * k, x1, y0 + rad);
    pb.line_to(x1, y1 - rad);
    pb.cubic_to(x1, y1 - rad * k, x1 - rad * k, y1, x1 - rad, y1);
    pb.line_to(x0 + rad, y1);
    pb.cubic_to(x0 + rad * k, y1, x0, y1 - rad * k, x0, y1 - rad);
    pb.line_to(x0, y0 + rad);
    pb.cubic_to(x0, y0 + rad * k, x0 + rad * k, y0, x0 + rad, y0);
    pb.close();
    pb.finish()
}

const RUS: [(&str, &str); 33] = [
    ("`", "Ё"), ("q", "Й"), ("w", "Ц"), ("e", "У"), ("r", "К"), ("t", "Е"), ("y", "Н"), ("u", "Г"),
    ("i", "Ш"), ("o", "Щ"), ("p", "З"), ("[", "Х"), ("]", "Ъ"), ("a", "Ф"), ("s", "Ы"), ("d", "В"),
    ("f", "А"), ("g", "П"), ("h", "Р"), ("j", "О"), ("k", "Л"), ("l", "Д"), (";", "Ж"), ("'", "Э"),
    ("z", "Я"), ("x", "Ч"), ("c", "С"), ("v", "М"), ("b", "И"), ("n", "Т"), ("m", "Ь"), (",", "Б"),
    (".", "Ю"),
];
fn ru(l: &str) -> &'static str {
    RUS.iter().find(|(a, _)| *a == l).map(|(_, b)| *b).unwrap_or("")
}

// (label, code, width units, icon, niri key name)
type Spec = (&'static str, u16, f32, Icon, &'static str);

fn rows(media: bool) -> Vec<(f32, Vec<Spec>)> {
    let c = |k: K| k.0;
    let n = Icon::None;
    let mut f: Vec<Spec> = vec![("esc", c(K::KEY_ESC), 1.25, n, "Escape")];
    let fk = [
        (K::KEY_F1, "F1", K::KEY_MUTE, Icon::Mute),
        (K::KEY_F2, "F2", K::KEY_VOLUMEDOWN, Icon::VolDown),
        (K::KEY_F3, "F3", K::KEY_VOLUMEUP, Icon::VolUp),
        (K::KEY_F4, "F4", K::KEY_PREVIOUSSONG, Icon::Prev),
        (K::KEY_F5, "F5", K::KEY_PLAYPAUSE, Icon::Play),
        (K::KEY_F6, "F6", K::KEY_NEXTSONG, Icon::Next),
        (K::KEY_F7, "F7", K::KEY_BRIGHTNESSDOWN, Icon::BriDown),
        (K::KEY_F8, "F8", K::KEY_BRIGHTNESSUP, Icon::BriUp),
        (K::KEY_F9, "F9", K::KEY_F9, n),
        (K::KEY_F10, "F10", K::KEY_F10, n),
        (K::KEY_F11, "F11", K::KEY_F11, n),
        (K::KEY_F12, "F12", K::KEY_F12, n),
    ];
    for (fk, l, mk, mi) in fk {
        if media && mi != n {
            f.push((l, c(mk), 1.0, mi, ""));
        } else {
            f.push((l, c(fk), 1.0, n, l));
        }
    }
    f.push(("prt", c(K::KEY_SYSRQ), 1.0, n, "Print"));
    f.push(("del", c(K::KEY_DELETE), 0.75, n, "Delete"));

    let mut r1: Vec<Spec> = vec![("`", c(K::KEY_GRAVE), 1.0, n, "Grave")];
    for (l, k, nk) in [("1", K::KEY_1, "1"), ("2", K::KEY_2, "2"), ("3", K::KEY_3, "3"), ("4", K::KEY_4, "4"),
                       ("5", K::KEY_5, "5"), ("6", K::KEY_6, "6"), ("7", K::KEY_7, "7"), ("8", K::KEY_8, "8"),
                       ("9", K::KEY_9, "9"), ("0", K::KEY_0, "0"), ("-", K::KEY_MINUS, "Minus"),
                       ("=", K::KEY_EQUAL, "Equal")] {
        r1.push((l, c(k), 1.0, n, nk));
    }
    r1.push(("", c(K::KEY_BACKSPACE), 2.0, Icon::Backspace, "BackSpace"));

    let mut r2: Vec<Spec> = vec![("tab", c(K::KEY_TAB), 1.5, n, "Tab")];
    for (l, k, nk) in [("q", K::KEY_Q, "Q"), ("w", K::KEY_W, "W"), ("e", K::KEY_E, "E"), ("r", K::KEY_R, "R"),
                       ("t", K::KEY_T, "T"), ("y", K::KEY_Y, "Y"), ("u", K::KEY_U, "U"), ("i", K::KEY_I, "I"),
                       ("o", K::KEY_O, "O"), ("p", K::KEY_P, "P"), ("[", K::KEY_LEFTBRACE, "BracketLeft"),
                       ("]", K::KEY_RIGHTBRACE, "BracketRight")] {
        r2.push((l, c(k), 1.0, n, nk));
    }
    r2.push(("\\", c(K::KEY_BACKSLASH), 1.5, n, "Backslash"));

    let mut r3: Vec<Spec> = vec![("", c(K::KEY_CAPSLOCK), 1.75, Icon::Caps, "")];
    for (l, k, nk) in [("a", K::KEY_A, "A"), ("s", K::KEY_S, "S"), ("d", K::KEY_D, "D"), ("f", K::KEY_F, "F"),
                       ("g", K::KEY_G, "G"), ("h", K::KEY_H, "H"), ("j", K::KEY_J, "J"), ("k", K::KEY_K, "K"),
                       ("l", K::KEY_L, "L"), (";", K::KEY_SEMICOLON, "Semicolon"),
                       ("'", K::KEY_APOSTROPHE, "Apostrophe")] {
        r3.push((l, c(k), 1.0, n, nk));
    }
    r3.push(("enter", c(K::KEY_ENTER), 2.25, n, "Return"));

    let mut r4: Vec<Spec> = vec![("shift", c(K::KEY_LEFTSHIFT), 2.25, n, "")];
    for (l, k, nk) in [("z", K::KEY_Z, "Z"), ("x", K::KEY_X, "X"), ("c", K::KEY_C, "C"), ("v", K::KEY_V, "V"),
                       ("b", K::KEY_B, "B"), ("n", K::KEY_N, "N"), ("m", K::KEY_M, "M"),
                       (",", K::KEY_COMMA, "Comma"), (".", K::KEY_DOT, "Period"), ("/", K::KEY_SLASH, "Slash")] {
        r4.push((l, c(k), 1.0, n, nk));
    }
    r4.push(("shift", c(K::KEY_RIGHTSHIFT), 2.75, n, ""));

    let r5: Vec<Spec> = vec![
        ("ctrl", c(K::KEY_LEFTCTRL), 1.25, n, ""),
        ("", c(K::KEY_LEFTMETA), 1.25, Icon::Tux, ""),
        ("alt", c(K::KEY_LEFTALT), 1.25, n, ""),
        ("", c(K::KEY_SPACE), 5.75, n, "Space"),
        ("alt", c(K::KEY_RIGHTALT), 1.0, n, ""),
        ("ctrl", c(K::KEY_RIGHTCTRL), 1.0, n, ""),
        ("ARROWS", 0, 3.0, n, ""),
    ];
    vec![(0.6, f), (1.0, r1), (1.0, r2), (1.0, r3), (1.0, r4), (1.0, r5)]
}

/// Every key code the keyboard can send (both F-row modes).
pub fn all_codes() -> Vec<u16> {
    let mut v: Vec<u16> = rows(false)
        .into_iter()
        .chain(rows(true))
        .flat_map(|(_, r)| r.into_iter().map(|s| s.1))
        .filter(|c| *c != 0)
        .collect();
    v.extend([K::KEY_LEFT.0, K::KEY_RIGHT.0, K::KEY_UP.0, K::KEY_DOWN.0]);
    v.sort();
    v.dedup();
    v
}

fn load_font() -> Font {
    for p in [
        "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf",
        "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ] {
        if let Ok(b) = std::fs::read(p) {
            if let Ok(f) = Font::from_bytes(b, FontSettings::default()) {
                return f;
            }
        }
    }
    panic!("no usable font");
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Widget {
    Cpu,
    Gpu,
    Ram,
    Disk,
    Battery,
    Net,
    Clock,
    Temps,
    Workspaces,
}
impl Widget {
    pub fn parse(s: &str) -> Option<Widget> {
        Some(match s {
            "cpu" => Widget::Cpu,
            "gpu" => Widget::Gpu,
            "ram" => Widget::Ram,
            "disk" => Widget::Disk,
            "battery" => Widget::Battery,
            "net" => Widget::Net,
            "clock" => Widget::Clock,
            "temps" => Widget::Temps,
            "workspaces" => Widget::Workspaces,
            _ => return None,
        })
    }
    fn slots(self) -> usize {
        match self {
            Widget::Battery | Widget::Workspaces => 2,
            _ => 1,
        }
    }
}

pub struct Ui {
    pub w: f32,
    pub h: f32,
    pub s: f32,
    pub canvas: Pixmap,
    font: Font,
    glyphs: HashMap<(char, u32), (Metrics, Vec<u8>)>,
    pub keys: Vec<Key>,
    pub pad: Rect,
    pub kb_h: f32,
    pub left: Rect,
    pub right: Rect,
    gap: f32,
    unit: f32,
    pal: Palette,
    ru_legends: bool,
    left_w: Vec<(Widget, Rect)>,
    right_w: Vec<(Widget, Rect)>,
    /// Workspace tiles on the palm rest: tap = focus that workspace.
    pub ws_hits: Vec<(Rect, u64)>,
    pub hints: HashMap<String, String>,
    pub hints_on: bool,
    ru_ui: bool,
}

impl Ui {
    fn t(&self, en: &'static str, ru: &'static str) -> &'static str {
        if self.ru_ui { ru } else { en }
    }

    pub fn new(w: u32, h: u32, s: f32, cfg: &Config) -> Self {
        let mut ui = Ui {
            w: w as f32,
            h: h as f32,
            s,
            canvas: Pixmap::new(w, h).expect("canvas"),
            font: load_font(),
            glyphs: HashMap::new(),
            keys: Vec::new(),
            pad: Rect::default(),
            kb_h: 0.0,
            left: Rect::default(),
            right: Rect::default(),
            gap: 0.0,
            unit: 0.0,
            pal: palette(&cfg.skin),
            ru_legends: cfg.ru_legends,
            left_w: Vec::new(),
            right_w: Vec::new(),
            ws_hits: Vec::new(),
            hints: HashMap::new(),
            hints_on: false,
            ru_ui: cfg.russian(),
        };
        ui.layout(cfg);
        ui
    }

    fn layout(&mut self, cfg: &Config) {
        let (w, h) = (self.w, self.h);
        let kb_h = h * cfg.keyboard_height;
        let mx = w * 0.035;
        let top = kb_h * 0.06;
        let u = (w - 2.0 * mx) / 15.0;
        let rs = rows(cfg.frow == "media");
        let rh = (kb_h - top - kb_h * 0.03) / rs.iter().map(|r| r.0).sum::<f32>();
        self.gap = (u * 0.07).max(3.0);
        self.unit = u;
        let mut y = top;
        for (hf, row) in rs {
            let kh = rh * hf;
            let mut x = mx;
            for (label, code, wu, icon, nkey) in row {
                let kw = wu * u;
                if label == "ARROWS" {
                    let (hw, hh) = (kw / 3.0, kh / 2.0);
                    let mk = |r, code: K, icon, nkey| Key { r, code: code.0, label: "", ru: "", icon, nkey };
                    self.keys.push(mk(Rect::new(x, y + hh, hw, hh), K::KEY_LEFT, Icon::Left, "Left"));
                    self.keys.push(mk(Rect::new(x + hw, y, hw, hh), K::KEY_UP, Icon::Up, "Up"));
                    self.keys.push(mk(Rect::new(x + hw, y + hh, hw, hh), K::KEY_DOWN, Icon::Down, "Down"));
                    self.keys.push(mk(Rect::new(x + 2.0 * hw, y + hh, hw, hh), K::KEY_RIGHT, Icon::Right, "Right"));
                } else {
                    let r = if self.ru_legends { ru(label) } else { "" };
                    self.keys.push(Key { r: Rect::new(x, y, kw, kh), code, label, ru: r, icon, nkey });
                }
                x += kw;
            }
            y += kh;
        }
        self.kb_h = kb_h;
        let pw = w * cfg.trackpad.width;
        let rest = h - kb_h;
        let ph = rest * 0.86;
        self.pad = Rect::new((w - pw) / 2.0, kb_h + (rest - ph) / 2.0 - rest * 0.02, pw, ph);
        let side = (w - pw) / 2.0;
        let m = side * 0.07;
        self.left = Rect::new(mx, self.pad.y, side - mx - m, ph);
        self.right = Rect::new(self.pad.x + pw + m, self.pad.y, side - mx - m, ph);
        self.left_w = Self::place(&cfg.widgets.left, self.left);
        self.right_w = Self::place(&cfg.widgets.right, self.right);
    }

    fn place(names: &[String], area: Rect) -> Vec<(Widget, Rect)> {
        let gap = area.h * 0.05;
        let slot = (area.h - 2.0 * gap) / 3.0;
        let mut out = Vec::new();
        let mut used = 0usize;
        for n in names {
            let Some(wd) = Widget::parse(n) else { continue };
            let k = wd.slots();
            if used + k > 3 {
                continue;
            }
            let y = area.y + used as f32 * (slot + gap);
            let hgt = k as f32 * slot + (k as f32 - 1.0) * gap;
            out.push((wd, Rect::new(area.x, y, area.w, hgt)));
            used += k;
        }
        out
    }

    pub fn hit_key(&self, x: f32, y: f32) -> Option<usize> {
        if y >= self.kb_h {
            return None;
        }
        let g = self.gap / 2.0;
        self.keys.iter().position(|k| k.r.contains(x, y, g))
    }

    // ---- primitives ----------------------------------------------------
    fn fill(&mut self, r: Rect, rad: f32, c: Rgb, a: u8) {
        if let Some(p) = rrect(r, rad) {
            self.canvas.fill_path(&p, &paint(c, a), FillRule::Winding, Transform::identity(), None);
        }
    }
    fn stroke(&mut self, r: Rect, rad: f32, c: Rgb, a: u8, wdt: f32) {
        if a == 0 {
            return;
        }
        if let Some(p) = rrect(r, rad) {
            let st = Stroke { width: wdt, ..Stroke::default() };
            self.canvas.stroke_path(&p, &paint(c, a), &st, Transform::identity(), None);
        }
    }
    fn clear(&mut self, r: Rect) {
        if let Some(sr) = SRect::from_xywh(r.x, r.y, r.w, r.h) {
            let d = self.pal.deck;
            self.canvas.fill_rect(sr, &paint(d, 255), Transform::identity(), None);
        }
    }

    fn glyph(&mut self, ch: char, px: f32) -> &(Metrics, Vec<u8>) {
        let key = (ch, (px * 4.0) as u32);
        let font = &self.font;
        self.glyphs.entry(key).or_insert_with(|| font.rasterize(ch, px))
    }

    pub fn text_w(&mut self, s: &str, px: f32) -> f32 {
        s.chars().map(|c| self.glyph(c, px).0.advance_width).sum()
    }

    /// Draw text with its baseline at `by`.
    pub fn text(&mut self, s: &str, x: f32, by: f32, px: f32, c: Rgb) {
        let mut pen = x;
        let (cw, ch) = (self.canvas.width() as i32, self.canvas.height() as i32);
        for chr in s.chars() {
            let (m, bmp) = {
                let g = self.glyph(chr, px);
                (g.0, g.1.clone())
            };
            let gx = (pen + m.xmin as f32).round() as i32;
            let gy = (by - m.ymin as f32 - m.height as f32).round() as i32;
            let data = self.canvas.data_mut();
            for row in 0..m.height as i32 {
                let yy = gy + row;
                if yy < 0 || yy >= ch {
                    continue;
                }
                for col in 0..m.width as i32 {
                    let xx = gx + col;
                    if xx < 0 || xx >= cw {
                        continue;
                    }
                    let a = bmp[(row * m.width as i32 + col) as usize] as u32;
                    if a == 0 {
                        continue;
                    }
                    let i = ((yy * cw + xx) * 4) as usize;
                    let inv = 255 - a;
                    data[i] = ((c.0 as u32 * a + data[i] as u32 * inv) / 255) as u8;
                    data[i + 1] = ((c.1 as u32 * a + data[i + 1] as u32 * inv) / 255) as u8;
                    data[i + 2] = ((c.2 as u32 * a + data[i + 2] as u32 * inv) / 255) as u8;
                    data[i + 3] = 255;
                }
            }
            pen += m.advance_width;
        }
    }

    /// Text shrunk to fit `maxw`.
    fn text_fit(&mut self, s: &str, x: f32, by: f32, px: f32, maxw: f32, c: Rgb) {
        let w = self.text_w(s, px);
        let px = if w > maxw && w > 0.0 { px * maxw / w } else { px };
        self.text(s, x, by, px, c);
    }

    fn text_center(&mut self, s: &str, r: Rect, px: f32, c: Rgb) {
        let tw = self.text_w(s, px);
        let px2 = if tw > r.w * 0.9 { px * r.w * 0.9 / tw } else { px };
        let tw = self.text_w(s, px2);
        self.text(s, r.x + (r.w - tw) / 2.0, r.y + r.h / 2.0 + px2 * 0.36, px2, c);
    }

    // ---- whole frame ---------------------------------------------------
    pub fn draw_all(&mut self, ru: bool, st: &Stats, ws: &[Ws], ext: Option<&str>) {
        let d = self.pal.deck;
        self.canvas.fill(Color::from_rgba8(d.0, d.1, d.2, 255));
        let (w, hinge) = (self.w, self.pal.hinge);
        self.fill(Rect::new(0.0, 0.0, w, 2.0 * self.s), 0.0, hinge, 255);
        for i in 0..self.keys.len() {
            self.draw_key(i, Look::Normal, ru);
        }
        self.draw_pad(false);
        self.draw_panels(st, ws, ext);
    }

    pub fn draw_key(&mut self, i: usize, look: Look, ru_on: bool) -> Rect {
        let (r, label, ru, icon, nkey) = {
            let k = &self.keys[i];
            (k.r, k.label, k.ru, k.icon, k.nkey)
        };
        let g = self.gap / 2.0;
        let area = r.grow(1.0);
        self.clear(Rect::new(area.x, area.y, area.w, area.h + 3.0 * self.s));
        let k = Rect::new(r.x + g, r.y + g, r.w - 2.0 * g, r.h - 2.0 * g);
        let rad = k.w.min(k.h) * 0.12;
        let col = match look {
            Look::Normal => self.pal.key,
            Look::Down => self.pal.key_down,
            Look::Latched => self.pal.key_latch,
        };
        let sh = self.pal.shadow;
        if sh > 0 {
            self.fill(Rect::new(k.x, k.y + 2.0 * self.s, k.w, k.h), rad, (0, 0, 0), sh);
        }
        self.fill(k, rad, col, 255);
        if self.pal.rainbow {
            let c = hue((k.x + k.w / 2.0) / self.w * 0.85 + 0.5);
            self.stroke(k.grow(-0.75), rad, c, 200, 1.6 * self.s);
        } else {
            let (oc, oa) = self.pal.outline;
            self.stroke(k.grow(-0.5), rad, oc, oa, 1.0);
        }
        let (txt, dim) = (self.pal.txt, self.pal.dim);

        // Tux held: the key shows what Tux+key does
        if self.hints_on && icon != Icon::Tux {
            let hint = self.hints.get(nkey).cloned();
            let small = k.h * 0.22;
            let name = if !label.is_empty() { label.to_uppercase() } else { nkey.to_owned() };
            self.text(&name, k.x + k.w * 0.1, k.y + small * 1.2, small, dim);
            if let Some(hn) = hint {
                let hc = self.pal.hint;
                let px = k.h * 0.24;
                self.text_fit(&hn, k.x + k.w * 0.08, k.y + k.h * 0.78, px, k.w * 0.86, hc);
            }
            return area;
        }

        match icon {
            Icon::None => {}
            Icon::Caps => {
                let px = k.h * 0.27;
                let s = if ru_on { "⇪ RU" } else { "⇪ EN" };
                let s = if self.font.has_glyph('⇪') { s } else { &s[4..] };
                self.text(s, k.x + k.w * 0.12, k.y + k.h / 2.0 + px * 0.36, px, txt);
                return area;
            }
            _ => {
                self.icon(icon, k);
                if matches!(icon, Icon::Mute | Icon::VolDown | Icon::VolUp | Icon::Prev | Icon::Play
                    | Icon::Next | Icon::BriDown | Icon::BriUp) {
                    let px = k.h * 0.22;
                    self.text(label, k.x + k.w * 0.08, k.y + px * 1.1, px, dim);
                }
                return area;
            }
        }
        if !ru.is_empty() {
            let px = k.h * 0.34;
            let main = if ru_on { dim } else { txt };
            let up = label.to_uppercase();
            self.text(&up, k.x + k.w * 0.14, k.y + k.h * 0.44, px, main);
            let px2 = k.h * 0.27;
            let rc = if ru_on { self.pal.ru_on } else { self.pal.ru_off };
            self.text(ru, k.x + k.w * 0.52, k.y + k.h * 0.84, px2, rc);
        } else if label.chars().count() > 1 && !label.starts_with('F') {
            let px = k.h * 0.24;
            if k.w > self.unit * 1.2 {
                self.text(label, k.x + k.w * 0.1, k.y + k.h / 2.0 + px * 0.36, px, txt);
            } else {
                self.text_center(label, k, px, txt);
            }
        } else {
            let px = if label.starts_with('F') { k.h * 0.30 } else { k.h * 0.36 };
            let up = if label.len() == 1 { label.to_uppercase() } else { label.to_owned() };
            self.text_center(&up, k, px, txt);
        }
        area
    }

    fn poly(&mut self, pts: &[(f32, f32)], c: Rgb) {
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].0, pts[0].1);
        for p in &pts[1..] {
            pb.line_to(p.0, p.1);
        }
        pb.close();
        if let Some(p) = pb.finish() {
            self.canvas.fill_path(&p, &paint(c, 255), FillRule::Winding, Transform::identity(), None);
        }
    }

    fn line(&mut self, pts: &[(f32, f32)], c: Rgb, w: f32) {
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].0, pts[0].1);
        for p in &pts[1..] {
            pb.line_to(p.0, p.1);
        }
        if let Some(p) = pb.finish() {
            let st = Stroke { width: w, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() };
            self.canvas.stroke_path(&p, &paint(c, 255), &st, Transform::identity(), None);
        }
    }

    fn speaker(&mut self, cx: f32, cy: f32, s: f32, c: Rgb) {
        let u = s * 0.14;
        self.poly(&[(cx - 2.0 * u, cy - u), (cx - u, cy - u), (cx + 0.6 * u, cy - 2.2 * u),
                    (cx + 0.6 * u, cy + 2.2 * u), (cx - u, cy + u), (cx - 2.0 * u, cy + u)], c);
    }

    fn sun(&mut self, cx: f32, cy: f32, r: f32, rays: f32, c: Rgb) {
        self.ellipse(cx, cy, r, r, c);
        for i in 0..8 {
            let a = i as f32 * std::f32::consts::PI / 4.0;
            let (x0, y0) = (cx + a.cos() * r * 1.5, cy + a.sin() * r * 1.5);
            let (x1, y1) = (cx + a.cos() * r * (1.5 + rays), cy + a.sin() * r * (1.5 + rays));
            self.line(&[(x0, y0), (x1, y1)], c, (r * 0.35).max(1.2));
        }
    }

    fn icon(&mut self, icon: Icon, k: Rect) {
        let (cx, cy) = (k.x + k.w / 2.0, k.y + k.h / 2.0 + k.h * 0.06);
        let s = k.w.min(k.h);
        let c = self.pal.txt;
        let lw = (s * 0.05).max(1.5);
        match icon {
            Icon::Left | Icon::Right | Icon::Up | Icon::Down => {
                let r = s * 0.22;
                let ang: f32 = match icon {
                    Icon::Right => 0.0,
                    Icon::Down => 90.0,
                    Icon::Left => 180.0,
                    _ => 270.0,
                };
                let a = ang.to_radians();
                let cy = k.y + k.h / 2.0;
                let pt = |dx: f32, dy: f32| (cx + dx * a.cos() - dy * a.sin(), cy + dx * a.sin() + dy * a.cos());
                self.poly(&[pt(r, 0.0), pt(-r * 0.6, -r * 0.85), pt(-r * 0.6, r * 0.85)], c);
            }
            Icon::Backspace => {
                let cy = k.y + k.h / 2.0;
                let (w, h) = (s * 0.62, s * 0.34);
                let x0 = cx - w / 2.0;
                self.line(&[(x0, cy), (x0 + h * 0.6, cy - h / 2.0), (x0 + w, cy - h / 2.0), (x0 + w, cy + h / 2.0),
                            (x0 + h * 0.6, cy + h / 2.0), (x0, cy)], c, (s * 0.035).max(1.5));
                let m = h * 0.22;
                let xc = x0 + w * 0.6;
                self.line(&[(xc - m, cy - m), (xc + m, cy + m)], c, (s * 0.035).max(1.5));
                self.line(&[(xc + m, cy - m), (xc - m, cy + m)], c, (s * 0.035).max(1.5));
            }
            Icon::Tux => self.tux(cx, k.y + k.h / 2.0 + s * 0.02, s * 0.42),
            Icon::Mute | Icon::VolDown | Icon::VolUp => {
                let x = cx - s * 0.12;
                self.speaker(x, cy, s, c);
                let u = s * 0.14;
                let bx = x + 1.6 * u;
                match icon {
                    Icon::Mute => {
                        self.line(&[(bx, cy - u), (bx + 1.6 * u, cy + u)], c, lw);
                        self.line(&[(bx + 1.6 * u, cy - u), (bx, cy + u)], c, lw);
                    }
                    Icon::VolDown => self.line(&[(bx, cy), (bx + 1.6 * u, cy)], c, lw),
                    _ => {
                        self.line(&[(bx, cy), (bx + 1.6 * u, cy)], c, lw);
                        self.line(&[(bx + 0.8 * u, cy - 0.8 * u), (bx + 0.8 * u, cy + 0.8 * u)], c, lw);
                    }
                }
            }
            Icon::Prev | Icon::Next => {
                let u = s * 0.11;
                let d = if icon == Icon::Next { 1.0 } else { -1.0 };
                for o in [-1.0f32, 0.6] {
                    let x = cx + o * u * d;
                    self.poly(&[(x - d * u, cy - u), (x + d * 0.8 * u, cy), (x - d * u, cy + u)], c);
                }
                let bx = cx + d * 1.6 * u;
                self.line(&[(bx, cy - u), (bx, cy + u)], c, lw);
            }
            Icon::Play => {
                let u = s * 0.12;
                self.poly(&[(cx - 1.4 * u, cy - u), (cx + 0.2 * u, cy), (cx - 1.4 * u, cy + u)], c);
                self.line(&[(cx + 0.8 * u, cy - u), (cx + 0.8 * u, cy + u)], c, lw);
                self.line(&[(cx + 1.5 * u, cy - u), (cx + 1.5 * u, cy + u)], c, lw);
            }
            Icon::BriDown => self.sun(cx, cy, s * 0.07, 0.6, c),
            Icon::BriUp => self.sun(cx, cy, s * 0.09, 1.0, c),
            _ => {}
        }
    }

    fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Rgb) {
        if let Some(r) = SRect::from_xywh(cx - rx, cy - ry, 2.0 * rx, 2.0 * ry) {
            if let Some(p) = PathBuilder::from_oval(r) {
                self.canvas.fill_path(&p, &paint(c, 255), FillRule::Winding, Transform::identity(), None);
            }
        }
    }

    fn tux(&mut self, cx: f32, cy: f32, u: f32) {
        let orange = (250, 178, 26);
        let black = (5, 5, 5);
        let rim = (210, 210, 217);
        let e = |ui: &mut Ui, x: f32, y: f32, rx: f32, ry: f32, c| ui.ellipse(cx + x * u, cy + y * u, rx * u, ry * u, c);
        e(self, -0.22, 0.50, 0.20, 0.09, orange);
        e(self, 0.22, 0.50, 0.20, 0.09, orange);
        e(self, 0.0, 0.12, 0.40, 0.44, rim);
        e(self, 0.0, 0.12, 0.36, 0.40, black);
        e(self, 0.0, 0.20, 0.25, 0.31, (247, 247, 242));
        e(self, 0.0, -0.38, 0.27, 0.25, rim);
        e(self, 0.0, -0.38, 0.23, 0.21, black);
        e(self, -0.09, -0.43, 0.065, 0.085, (255, 255, 255));
        e(self, 0.09, -0.43, 0.065, 0.085, (255, 255, 255));
        e(self, -0.08, -0.42, 0.03, 0.04, (0, 0, 0));
        e(self, 0.08, -0.42, 0.03, 0.04, (0, 0, 0));
        e(self, 0.0, -0.29, 0.11, 0.055, orange);
    }

    pub fn draw_pad(&mut self, drag: bool) -> Rect {
        let p = self.pad;
        let area = p.grow(4.0 * self.s);
        self.clear(area);
        let rad = p.h * 0.05;
        let sh = self.pal.shadow;
        if sh > 0 {
            self.fill(Rect::new(p.x - 1.0, p.y - 1.0, p.w + 2.0, p.h + 3.0), rad, (0, 0, 0), sh * 3 / 4);
        }
        let (pc, pe) = (self.pal.pad, self.pal.pad_edge);
        self.fill(p, rad, pc, 255);
        self.stroke(p.grow(-0.75), rad, pe, 255, 1.5 * self.s);
        if drag {
            self.fill(p, rad, (77, 128, 230), 30);
        }
        area
    }

    // ---- palm-rest widgets ---------------------------------------------
    pub fn draw_panels(&mut self, st: &Stats, ws: &[Ws], ext: Option<&str>) -> [Rect; 2] {
        let (l, r) = (self.left, self.right);
        self.clear(l.grow(2.0));
        self.clear(r.grow(2.0));
        self.ws_hits.clear();
        let all: Vec<(Widget, Rect)> = self.left_w.iter().chain(self.right_w.iter()).copied().collect();
        for (wd, rc) in all {
            self.widget(wd, rc, st, ws, ext);
        }
        [l.grow(2.0), r.grow(2.0)]
    }

    /// Only the workspace widgets (on workspace events).
    pub fn draw_workspaces(&mut self, st: &Stats, ws: &[Ws], ext: Option<&str>) -> Vec<Rect> {
        let all: Vec<(Widget, Rect)> = self
            .left_w
            .iter()
            .chain(self.right_w.iter())
            .copied()
            .filter(|(w, _)| *w == Widget::Workspaces)
            .collect();
        if !all.is_empty() {
            self.ws_hits.clear();
        }
        let mut out = Vec::new();
        for (wd, rc) in all {
            self.clear(rc.grow(2.0));
            self.widget(wd, rc, st, ws, ext);
            out.push(rc.grow(2.0));
        }
        out
    }

    fn widget(&mut self, wd: Widget, c: Rect, st: &Stats, ws: &[Ws], ext: Option<&str>) {
        let a = self.pal.acc;
        self.card(c);
        match wd {
            Widget::Cpu => {
                let sub = format!("{} {}  {}°C", st.cpu_mhz, self.t("MHz", "МГц"), st.t_cpu);
                self.metric(c, "CPU", &format!("{:.0}%", st.cpu), &sub, a[0]);
                self.spark(c, &st.cpu_hist, 100.0, a[0]);
            }
            Widget::Gpu => {
                let sub = format!("{} {}  {}°C", st.gpu_mhz, self.t("MHz", "МГц"), st.t_gpu);
                self.metric(c, "GPU", &format!("{:.0}%", st.gpu), &sub, a[1]);
                self.spark(c, &st.gpu_hist, 100.0, a[1]);
            }
            Widget::Ram => {
                let gb = self.t("GB", "ГБ");
                let sub = format!("{} {:.1} {gb}  swap {:.1}", self.t("of", "из"), st.ram_total, st.swap_used);
                self.metric(c, self.t("RAM", "ОЗУ"), &format!("{:.1} {gb}", st.ram_used), &sub, a[2]);
                self.bar(c, st.ram_used / st.ram_total.max(0.1), a[2]);
            }
            Widget::Disk => {
                let gb = self.t("GB", "ГБ");
                let sub = format!("{} {:.0} {gb}", self.t("of", "из"), st.disk_total);
                self.metric(c, self.t("DISK", "ДИСК"), &format!("{:.0} {gb}", st.disk_used), &sub, a[3]);
                self.bar(c, st.disk_used / st.disk_total.max(1.0), a[3]);
            }
            Widget::Net => {
                let (mb, kb) = (self.t("MB/s", "МБ/с"), self.t("KB/s", "КБ/с"));
                let f = |v: f32| if v >= 1024.0 { format!("{:.1} {mb}", v / 1024.0) } else { format!("{:.0} {kb}", v) };
                let sub = format!("↓ {}   ↑ {}", f(st.net_rx), f(st.net_tx));
                self.metric(c, self.t("NETWORK", "СЕТЬ"), &f(st.net_rx + st.net_tx), &sub, a[4]);
                let mx = st.net_hist.iter().fold(64.0f32, |m, v| m.max(*v));
                self.spark(c, &st.net_hist, mx, a[4]);
            }
            Widget::Temps => {
                let sub = self.t("CPU · GPU", "CPU · GPU");
                let v = format!("{}° {}°", st.t_cpu, st.t_gpu);
                let hot = st.t_cpu.max(st.t_gpu);
                let col = if hot >= 70 { self.pal.bad } else if hot >= 55 { self.pal.warn } else { self.pal.good };
                self.metric(c, self.t("TEMP", "ТЕМП"), &v, sub, col);
                self.bar(c, (hot as f32 - 25.0) / 60.0, col);
            }
            Widget::Clock => self.clock(c),
            Widget::Battery => self.battery(c, st),
            Widget::Workspaces => self.workspaces(c, ws, ext),
        }
    }

    fn card(&mut self, c: Rect) {
        let rad = c.h.min(c.w) * 0.10;
        let (cc, (ec, ea)) = (self.pal.card, self.pal.card_edge);
        self.fill(c, rad, cc, 255);
        self.stroke(c.grow(-0.5), rad, ec, ea, 1.0);
    }

    fn metric(&mut self, c: Rect, name: &str, val: &str, sub: &str, acc: Rgb) {
        let pad = c.w * 0.07;
        let lp = c.h * 0.17;
        let dim = self.pal.dim;
        let txt = self.pal.txt;
        self.fill(Rect::new(c.x + pad, c.y + pad * 0.9, lp * 0.32, lp * 0.32), lp * 0.16, acc, 255);
        self.text(name, c.x + pad + lp * 0.55, c.y + pad * 0.9 + lp * 0.36, lp * 0.75, dim);
        let vp = c.h * 0.30;
        let vw = self.text_w(val, vp);
        self.text(val, c.x + c.w - pad - vw, c.y + pad * 0.6 + vp * 0.9, vp, txt);
        self.text_fit(sub, c.x + pad, c.y + c.h * 0.58, lp * 0.62, c.w - 2.0 * pad, dim);
    }

    fn spark(&mut self, c: Rect, h: &VecDeque<f32>, max: f32, acc: Rgb) {
        let pad = c.w * 0.07;
        let g = Rect::new(c.x + pad, c.y + c.h * 0.66, c.w - 2.0 * pad, c.h * 0.26);
        if h.len() < 2 {
            return;
        }
        let n = HIST as f32;
        let dx = g.w / (n - 1.0);
        let off = n - h.len() as f32;
        let mut line = PathBuilder::new();
        let mut area = PathBuilder::new();
        area.move_to(g.x + off * dx, g.y + g.h);
        for (i, v) in h.iter().enumerate() {
            let x = g.x + (off + i as f32) * dx;
            let y = g.y + g.h - (v / max).clamp(0.0, 1.0) * g.h;
            if i == 0 {
                line.move_to(x, y);
            } else {
                line.line_to(x, y);
            }
            area.line_to(x, y);
        }
        area.line_to(g.x + g.w, g.y + g.h);
        area.close();
        if let Some(p) = area.finish() {
            self.canvas.fill_path(&p, &paint(acc, 40), FillRule::Winding, Transform::identity(), None);
        }
        if let Some(p) = line.finish() {
            let st = Stroke { width: 1.6 * self.s, ..Stroke::default() };
            self.canvas.stroke_path(&p, &paint(acc, 255), &st, Transform::identity(), None);
        }
    }

    fn bar(&mut self, c: Rect, frac: f32, acc: Rgb) {
        let pad = c.w * 0.07;
        let g = Rect::new(c.x + pad, c.y + c.h * 0.72, c.w - 2.0 * pad, c.h * 0.12);
        let t = self.pal.track;
        self.fill(g, g.h / 2.0, t, 255);
        self.fill(Rect::new(g.x, g.y, g.w * frac.clamp(0.0, 1.0), g.h), g.h / 2.0, acc, 255);
    }

    fn clock(&mut self, c: Rect) {
        let (mut tm, now) = unsafe { (std::mem::zeroed::<libc::tm>(), libc::time(std::ptr::null_mut())) };
        unsafe { libc::localtime_r(&now, &mut tm) };
        let days = if self.ru_ui { ["вс", "пн", "вт", "ср", "чт", "пт", "сб"] } else { ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"] };
        let months = if self.ru_ui {
            ["янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"]
        } else {
            ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
        };
        let t = format!("{:02}:{:02}", tm.tm_hour, tm.tm_min);
        let (wd, mo) = (days[tm.tm_wday.clamp(0, 6) as usize], months[tm.tm_mon.clamp(0, 11) as usize]);
        let d = if self.ru_ui { format!("{wd}, {} {mo}", tm.tm_mday) } else { format!("{wd}, {mo} {}", tm.tm_mday) };
        let pad = c.w * 0.07;
        let (txt, dim) = (self.pal.txt, self.pal.dim);
        let big = c.h * 0.48;
        self.text(&t, c.x + pad, c.y + c.h * 0.58, big, txt);
        self.text(&d, c.x + pad, c.y + c.h * 0.86, c.h * 0.16, dim);
    }

    fn battery(&mut self, b: Rect, st: &Stats) {
        let (good, warn, bad, txt, dim) = (self.pal.good, self.pal.warn, self.pal.bad, self.pal.txt, self.pal.dim);
        let charging = st.power_w > 0.05 || st.bat_status == "Charging";
        let col = if charging { good } else if st.bat <= 15 { bad } else if st.bat <= 30 { warn } else { txt };
        let pad = b.w * 0.07;
        let lp = b.h * 0.10;
        let lbl = self.t("BATTERY", "БАТАРЕЯ");
        self.text(lbl, b.x + pad, b.y + pad + lp * 0.8, lp * 0.75, dim);
        let big = b.h * 0.30;
        let pct = format!("{}%", st.bat);
        self.text(&pct, b.x + pad, b.y + pad + lp + big * 0.95, big, col);
        let gw = b.w * 0.30;
        let gh = big * 0.62;
        let gx = b.x + b.w - pad - gw;
        let gy = b.y + pad + lp + big * 0.95 - gh;
        self.stroke(Rect::new(gx, gy, gw, gh), gh * 0.18, dim, 255, 1.6 * self.s);
        self.fill(Rect::new(gx + gw, gy + gh * 0.3, gw * 0.06, gh * 0.4), 2.0, dim, 255);
        let inner = Rect::new(gx + 3.0 * self.s, gy + 3.0 * self.s, (gw - 6.0 * self.s) * st.bat as f32 / 100.0, gh - 6.0 * self.s);
        self.fill(inner, gh * 0.12, col, 255);
        let wpx = b.h * 0.14;
        let sign = if st.power_w > 0.0 { "+" } else { "−" };
        let pw = format!("{sign}{:.1} {}", st.power_w.abs(), self.t("W", "Вт"));
        let state = if charging { self.t("charging", "заряд") } else { self.t("discharging", "разряд") };
        let by = b.y + b.h * 0.74;
        self.text(&pw, b.x + pad, by, wpx, if charging { good } else { warn });
        let tw = self.text_w(state, wpx * 0.6);
        self.text(state, b.x + b.w - pad - tw, by, wpx * 0.6, dim);
        let hb = Rect::new(b.x + pad, b.y + b.h * 0.80, b.w - 2.0 * pad, b.h * 0.14);
        let maxp = st.power_hist.iter().fold(5.0f32, |m, v| m.max(v.abs()));
        let n = HIST as f32;
        let bw = hb.w / n;
        let mid = hb.y + hb.h / 2.0;
        let t = self.pal.track;
        self.fill(Rect::new(hb.x, mid - 0.5, hb.w, 1.0), 0.0, t, 255);
        let off = n - st.power_hist.len() as f32;
        let hist: Vec<f32> = st.power_hist.iter().copied().collect();
        for (i, v) in hist.iter().enumerate() {
            let hh = (v.abs() / maxp) * hb.h / 2.0;
            let x = hb.x + (off + i as f32) * bw;
            let rr = if *v >= 0.0 { Rect::new(x, mid - hh, bw * 0.7, hh) } else { Rect::new(x, mid, bw * 0.7, hh) };
            self.fill(rr, 0.0, if *v >= 0.0 { good } else { warn }, 200);
        }
    }

    fn workspaces(&mut self, c: Rect, ws: &[Ws], ext: Option<&str>) {
        let pad = c.w * 0.07;
        let lp = c.h * 0.085;
        let (dim, txt, acc, track) = (self.pal.dim, self.pal.txt, self.pal.acc[0], self.pal.track);
        let lbl = self.t("WORKSPACES", "РАБОЧИЕ СТОЛЫ");
        self.text(lbl, c.x + pad, c.y + pad + lp * 0.8, lp * 0.75, dim);
        let list: Vec<&Ws> = ws.iter().filter(|w| ext.map_or(true, |e| w.output == e)).take(8).collect();
        if list.is_empty() {
            return;
        }
        let cols = 4usize;
        let rows = list.len().div_ceil(cols).max(1);
        let area = Rect::new(c.x + pad, c.y + pad + lp * 1.6, c.w - 2.0 * pad, c.h - 2.0 * pad - lp * 1.6);
        let g = area.w * 0.04;
        let tw = (area.w - g * (cols as f32 - 1.0)) / cols as f32;
        let th = ((area.h - g * (rows as f32 - 1.0)) / rows as f32).min(tw * 0.75);
        for (i, w) in list.iter().enumerate() {
            let (cx, cy) = (i % cols, i / cols);
            let r = Rect::new(area.x + cx as f32 * (tw + g), area.y + cy as f32 * (th + g), tw, th);
            let rad = th * 0.16;
            if w.active {
                self.fill(r, rad, acc, 255);
            } else {
                self.fill(r, rad, track, 255);
            }
            let num = format!("{}", w.idx);
            let px = th * 0.42;
            let col = if w.active { (255, 255, 255) } else { txt };
            self.text(&num, r.x + r.w * 0.12, r.y + px * 1.05, px, col);
            // a dot per window (up to 5)
            let n = w.windows.min(5);
            let dr = th * 0.07;
            for k in 0..n {
                let dx = r.x + r.w * 0.14 + k as f32 * dr * 2.8;
                self.ellipse(dx + dr, r.y + r.h * 0.78, dr, dr, col);
            }
            self.ws_hits.push((r, w.id));
        }
    }
}
