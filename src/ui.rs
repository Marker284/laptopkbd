// SPDX-License-Identifier: GPL-2.0-or-later
//! Laptop deck: layout, keys, trackpad and the palm-rest system monitor,
//! drawn with tiny-skia into one canvas; only what changed is redrawn.
use crate::stats::Stats;
use evdev::KeyCode as K;
use fontdue::{Font, FontSettings, Metrics};
use std::collections::HashMap;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect as SRect, Stroke, Transform};

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
}

pub struct Key {
    pub r: Rect,
    pub code: u16,
    pub label: &'static str,
    pub ru: &'static str,
    pub icon: Icon,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Look {
    Normal,
    Down,
    Latched,
}

// palette: dark aluminium deck
const DECK: (u8, u8, u8) = (28, 29, 33);
const KEY: (u8, u8, u8) = (9, 9, 10);
const KEY_DOWN: (u8, u8, u8) = (77, 82, 97);
const KEY_LATCH: (u8, u8, u8) = (51, 92, 158);
const TXT: (u8, u8, u8) = (235, 235, 240);
const TXT_DIM: (u8, u8, u8) = (150, 152, 160);
const RU_ON: (u8, u8, u8) = (140, 178, 242);
const RU_OFF: (u8, u8, u8) = (95, 115, 160);
const PAD: (u8, u8, u8) = (34, 36, 41);
const PAD_EDGE: (u8, u8, u8) = (56, 58, 64);
const CARD: (u8, u8, u8) = (22, 23, 27);
const ACC_CPU: (u8, u8, u8) = (110, 170, 255);
const ACC_GPU: (u8, u8, u8) = (190, 130, 255);
const ACC_RAM: (u8, u8, u8) = (90, 210, 170);
const ACC_DISK: (u8, u8, u8) = (240, 190, 90);
const GOOD: (u8, u8, u8) = (110, 215, 120);
const WARN: (u8, u8, u8) = (255, 160, 80);
const BAD: (u8, u8, u8) = (255, 95, 95);

fn paint(c: (u8, u8, u8), a: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(c.0, c.1, c.2, a));
    p.anti_alias = true;
    p
}

fn rrect(r: Rect, rad: f32) -> Option<tiny_skia::Path> {
    let rad = rad.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
    let k = 0.4477; // cubic approximation of a quarter circle (1 - 0.5523)
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

type Spec = (&'static str, u16, f32, Icon);

fn rows() -> Vec<(f32, Vec<Spec>)> {
    let c = |k: K| k.0;
    let n = Icon::None;
    let mut f: Vec<Spec> = vec![("esc", c(K::KEY_ESC), 1.25, n)];
    let fk = [K::KEY_F1, K::KEY_F2, K::KEY_F3, K::KEY_F4, K::KEY_F5, K::KEY_F6, K::KEY_F7, K::KEY_F8,
              K::KEY_F9, K::KEY_F10, K::KEY_F11, K::KEY_F12];
    let fl = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"];
    for (k, l) in fk.iter().zip(fl) {
        f.push((l, c(*k), 1.0, n));
    }
    f.push(("prt", c(K::KEY_SYSRQ), 1.0, n));
    f.push(("del", c(K::KEY_DELETE), 0.75, n));

    let mut r1: Vec<Spec> = vec![("`", c(K::KEY_GRAVE), 1.0, n)];
    let nums = [("1", K::KEY_1), ("2", K::KEY_2), ("3", K::KEY_3), ("4", K::KEY_4), ("5", K::KEY_5),
                ("6", K::KEY_6), ("7", K::KEY_7), ("8", K::KEY_8), ("9", K::KEY_9), ("0", K::KEY_0),
                ("-", K::KEY_MINUS), ("=", K::KEY_EQUAL)];
    for (l, k) in nums {
        r1.push((l, c(k), 1.0, n));
    }
    r1.push(("", c(K::KEY_BACKSPACE), 2.0, Icon::Backspace));

    let mut r2: Vec<Spec> = vec![("tab", c(K::KEY_TAB), 1.5, n)];
    for (l, k) in [("q", K::KEY_Q), ("w", K::KEY_W), ("e", K::KEY_E), ("r", K::KEY_R), ("t", K::KEY_T),
                   ("y", K::KEY_Y), ("u", K::KEY_U), ("i", K::KEY_I), ("o", K::KEY_O), ("p", K::KEY_P),
                   ("[", K::KEY_LEFTBRACE), ("]", K::KEY_RIGHTBRACE)] {
        r2.push((l, c(k), 1.0, n));
    }
    r2.push(("\\", c(K::KEY_BACKSLASH), 1.5, n));

    let mut r3: Vec<Spec> = vec![("", c(K::KEY_CAPSLOCK), 1.75, Icon::Caps)];
    for (l, k) in [("a", K::KEY_A), ("s", K::KEY_S), ("d", K::KEY_D), ("f", K::KEY_F), ("g", K::KEY_G),
                   ("h", K::KEY_H), ("j", K::KEY_J), ("k", K::KEY_K), ("l", K::KEY_L),
                   (";", K::KEY_SEMICOLON), ("'", K::KEY_APOSTROPHE)] {
        r3.push((l, c(k), 1.0, n));
    }
    r3.push(("enter", c(K::KEY_ENTER), 2.25, n));

    let mut r4: Vec<Spec> = vec![("shift", c(K::KEY_LEFTSHIFT), 2.25, n)];
    for (l, k) in [("z", K::KEY_Z), ("x", K::KEY_X), ("c", K::KEY_C), ("v", K::KEY_V), ("b", K::KEY_B),
                   ("n", K::KEY_N), ("m", K::KEY_M), (",", K::KEY_COMMA), (".", K::KEY_DOT),
                   ("/", K::KEY_SLASH)] {
        r4.push((l, c(k), 1.0, n));
    }
    r4.push(("shift", c(K::KEY_RIGHTSHIFT), 2.75, n));

    let r5: Vec<Spec> = vec![
        ("ctrl", c(K::KEY_LEFTCTRL), 1.25, n),
        ("", c(K::KEY_LEFTMETA), 1.25, Icon::Tux),
        ("alt", c(K::KEY_LEFTALT), 1.25, n),
        ("", c(K::KEY_SPACE), 5.75, n),
        ("alt", c(K::KEY_RIGHTALT), 1.0, n),
        ("ctrl", c(K::KEY_RIGHTCTRL), 1.0, n),
        ("ARROWS", 0, 3.0, n),
    ];
    vec![(0.6, f), (1.0, r1), (1.0, r2), (1.0, r3), (1.0, r4), (1.0, r5)]
}

fn load_font() -> Font {
    for p in [
        "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf",
        "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
    ] {
        if let Ok(b) = std::fs::read(p) {
            if let Ok(f) = Font::from_bytes(b, FontSettings::default()) {
                return f;
            }
        }
    }
    panic!("no usable font");
}

impl Ui {
    pub fn new(w: u32, h: u32, s: f32) -> Self {
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
        };
        ui.layout();
        ui
    }

    fn layout(&mut self) {
        let (w, h) = (self.w, self.h);
        let kb_h = h * 0.60;
        let mx = w * 0.035;
        let top = kb_h * 0.06;
        let u = (w - 2.0 * mx) / 15.0;
        let rs = rows();
        let rh = (kb_h - top - kb_h * 0.03) / rs.iter().map(|r| r.0).sum::<f32>();
        self.gap = (u * 0.07).max(3.0);
        self.unit = u;
        let mut y = top;
        for (hf, row) in rs {
            let kh = rh * hf;
            let mut x = mx;
            for (label, code, wu, icon) in row {
                let kw = wu * u;
                if label == "ARROWS" {
                    let (hw, hh) = (kw / 3.0, kh / 2.0);
                    let mk = |r, code: K, icon| Key { r, code: code.0, label: "", ru: "", icon };
                    self.keys.push(mk(Rect::new(x, y + hh, hw, hh), K::KEY_LEFT, Icon::Left));
                    self.keys.push(mk(Rect::new(x + hw, y, hw, hh), K::KEY_UP, Icon::Up));
                    self.keys.push(mk(Rect::new(x + hw, y + hh, hw, hh), K::KEY_DOWN, Icon::Down));
                    self.keys.push(mk(Rect::new(x + 2.0 * hw, y + hh, hw, hh), K::KEY_RIGHT, Icon::Right));
                } else {
                    self.keys.push(Key { r: Rect::new(x, y, kw, kh), code, label, ru: ru(label), icon });
                }
                x += kw;
            }
            y += kh;
        }
        self.kb_h = kb_h;
        let pw = w * 0.42;
        let rest = h - kb_h;
        let ph = rest * 0.86;
        self.pad = Rect::new((w - pw) / 2.0, kb_h + (rest - ph) / 2.0 - rest * 0.02, pw, ph);
        let side = (w - pw) / 2.0;
        let m = side * 0.07;
        self.left = Rect::new(mx, self.pad.y, side - mx - m, ph);
        self.right = Rect::new(self.pad.x + pw + m, self.pad.y, side - mx - m, ph);
    }

    pub fn hit_key(&self, x: f32, y: f32) -> Option<usize> {
        if y >= self.kb_h {
            return None;
        }
        let g = self.gap / 2.0;
        self.keys.iter().position(|k| k.r.contains(x, y, g))
    }

    // ---- primitives ----------------------------------------------------
    fn fill(&mut self, r: Rect, rad: f32, c: (u8, u8, u8), a: u8) {
        if let Some(p) = rrect(r, rad) {
            self.canvas.fill_path(&p, &paint(c, a), FillRule::Winding, Transform::identity(), None);
        }
    }
    fn stroke(&mut self, r: Rect, rad: f32, c: (u8, u8, u8), a: u8, wdt: f32) {
        if let Some(p) = rrect(r, rad) {
            let st = Stroke { width: wdt, ..Stroke::default() };
            self.canvas.stroke_path(&p, &paint(c, a), &st, Transform::identity(), None);
        }
    }
    fn clear(&mut self, r: Rect) {
        if let Some(sr) = SRect::from_xywh(r.x, r.y, r.w, r.h) {
            self.canvas.fill_rect(sr, &paint(DECK, 255), Transform::identity(), None);
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
    pub fn text(&mut self, s: &str, x: f32, by: f32, px: f32, c: (u8, u8, u8)) {
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

    fn text_center(&mut self, s: &str, r: Rect, px: f32, c: (u8, u8, u8)) {
        let tw = self.text_w(s, px);
        self.text(s, r.x + (r.w - tw) / 2.0, r.y + r.h / 2.0 + px * 0.36, px, c);
    }

    // ---- whole frame ---------------------------------------------------
    pub fn draw_all(&mut self, ru: bool, st: &Stats) {
        self.canvas.fill(Color::from_rgba8(DECK.0, DECK.1, DECK.2, 255));
        // hinge edge
        let w = self.w;
        self.fill(Rect::new(0.0, 0.0, w, 2.0 * self.s), 0.0, (44, 45, 50), 255);
        for i in 0..self.keys.len() {
            self.draw_key(i, Look::Normal, ru);
        }
        self.draw_pad(false);
        self.draw_panels(st);
    }

    pub fn draw_key(&mut self, i: usize, look: Look, ru_on: bool) -> Rect {
        let (r, code, label, ru, icon) = {
            let k = &self.keys[i];
            (k.r, k.code, k.label, k.ru, k.icon)
        };
        let g = self.gap / 2.0;
        let area = r.grow(1.0);
        self.clear(Rect::new(area.x, area.y, area.w, area.h + 3.0 * self.s));
        let k = Rect::new(r.x + g, r.y + g, r.w - 2.0 * g, r.h - 2.0 * g);
        let rad = k.w.min(k.h) * 0.12;
        let col = match look {
            Look::Normal => KEY,
            Look::Down => KEY_DOWN,
            Look::Latched => KEY_LATCH,
        };
        self.fill(Rect::new(k.x, k.y + 2.0 * self.s, k.w, k.h), rad, (0, 0, 0), 115);
        self.fill(k, rad, col, 255);
        self.stroke(k.grow(-0.5), rad, (255, 255, 255), 13, 1.0);
        match icon {
            Icon::None => {}
            Icon::Caps => {
                let px = k.h * 0.27;
                let s = if ru_on { "⇪ RU" } else { "⇪ EN" };
                let s = if self.font.has_glyph('⇪') { s } else { &s[4..] };
                self.text(s, k.x + k.w * 0.12, k.y + k.h / 2.0 + px * 0.36, px, TXT);
                return area;
            }
            _ => {
                self.icon(icon, k);
                return area;
            }
        }
        if !ru.is_empty() {
            let px = k.h * 0.34;
            let main = if ru_on { TXT_DIM } else { TXT };
            let up = label.to_uppercase();
            self.text(&up, k.x + k.w * 0.14, k.y + k.h * 0.44, px, main);
            let px2 = k.h * 0.27;
            self.text(ru, k.x + k.w * 0.52, k.y + k.h * 0.84, px2, if ru_on { RU_ON } else { RU_OFF });
        } else if label.chars().count() > 1 && !label.starts_with('F') {
            let px = k.h * 0.24;
            if k.w > self.unit * 1.2 {
                self.text(label, k.x + k.w * 0.1, k.y + k.h / 2.0 + px * 0.36, px, TXT);
            } else {
                self.text_center(label, k, px, TXT);
            }
        } else {
            let px = if label.starts_with('F') { k.h * 0.30 } else { k.h * 0.36 };
            self.text_center(label, k, px, TXT);
        }
        let _ = code;
        area
    }

    fn icon(&mut self, icon: Icon, k: Rect) {
        let (cx, cy) = (k.x + k.w / 2.0, k.y + k.h / 2.0);
        let s = k.w.min(k.h);
        let white = paint(TXT, 255);
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
                let pt = |dx: f32, dy: f32| (cx + dx * a.cos() - dy * a.sin(), cy + dx * a.sin() + dy * a.cos());
                let mut pb = PathBuilder::new();
                let (x0, y0) = pt(r, 0.0);
                pb.move_to(x0, y0);
                let (x1, y1) = pt(-r * 0.6, -r * 0.85);
                pb.line_to(x1, y1);
                let (x2, y2) = pt(-r * 0.6, r * 0.85);
                pb.line_to(x2, y2);
                pb.close();
                if let Some(p) = pb.finish() {
                    self.canvas.fill_path(&p, &white, FillRule::Winding, Transform::identity(), None);
                }
            }
            Icon::Backspace => {
                let (w, h) = (s * 0.62, s * 0.34);
                let x0 = cx - w / 2.0;
                let mut pb = PathBuilder::new();
                pb.move_to(x0, cy);
                pb.line_to(x0 + h * 0.6, cy - h / 2.0);
                pb.line_to(x0 + w, cy - h / 2.0);
                pb.line_to(x0 + w, cy + h / 2.0);
                pb.line_to(x0 + h * 0.6, cy + h / 2.0);
                pb.close();
                let m = h * 0.22;
                let xc = x0 + w * 0.6;
                pb.move_to(xc - m, cy - m);
                pb.line_to(xc + m, cy + m);
                pb.move_to(xc + m, cy - m);
                pb.line_to(xc - m, cy + m);
                if let Some(p) = pb.finish() {
                    let st = Stroke { width: (s * 0.035).max(1.5), ..Stroke::default() };
                    self.canvas.stroke_path(&p, &white, &st, Transform::identity(), None);
                }
            }
            Icon::Tux => self.tux(cx, cy + s * 0.02, s * 0.42),
            _ => {}
        }
    }

    fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: (u8, u8, u8)) {
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
        self.fill(Rect::new(p.x - 1.0, p.y - 1.0, p.w + 2.0, p.h + 3.0), rad, (0, 0, 0), 90);
        self.fill(p, rad, PAD, 255);
        self.stroke(p.grow(-0.75), rad, PAD_EDGE, 255, 1.5 * self.s);
        if drag {
            self.fill(p, rad, (77, 128, 230), 30);
        }
        area
    }

    // ---- palm-rest monitor ---------------------------------------------
    pub fn draw_panels(&mut self, st: &Stats) -> [Rect; 2] {
        let (l, r) = (self.left, self.right);
        self.clear(l.grow(2.0));
        self.clear(r.grow(2.0));
        let gap = l.h * 0.05;
        let ch = (l.h - 2.0 * gap) / 3.0;
        let card = |i: usize, b: Rect| Rect::new(b.x, b.y + i as f32 * (ch + gap), b.w, ch);

        // left: CPU, GPU, RAM
        let c0 = card(0, l);
        self.card(c0);
        self.metric(c0, "CPU", &format!("{:.0}%", st.cpu), &format!("{} МГц  {}°C", st.cpu_mhz, st.t_cpu), ACC_CPU);
        self.spark(c0, &st.cpu_hist, 100.0, ACC_CPU);

        let c1 = card(1, l);
        self.card(c1);
        self.metric(c1, "GPU", &format!("{:.0}%", st.gpu), &format!("{} МГц  {}°C", st.gpu_mhz, st.t_gpu), ACC_GPU);
        self.spark(c1, &st.gpu_hist, 100.0, ACC_GPU);

        let c2 = card(2, l);
        self.card(c2);
        let sub = format!("из {:.1} ГБ  swap {:.1}", st.ram_total, st.swap_used);
        self.metric(c2, "ОЗУ", &format!("{:.1} ГБ", st.ram_used), &sub, ACC_RAM);
        self.bar(c2, st.ram_used / st.ram_total.max(0.1), ACC_RAM);

        // right: battery (two cards tall), disk
        let b = Rect::new(r.x, r.y, r.w, 2.0 * ch + gap);
        self.card(b);
        let charging = st.power_w > 0.05 || st.bat_status == "Charging";
        let col = if charging {
            GOOD
        } else if st.bat <= 15 {
            BAD
        } else if st.bat <= 30 {
            WARN
        } else {
            TXT
        };
        let pad = b.w * 0.07;
        let lp = b.h * 0.10;
        self.text("БАТАРЕЯ", b.x + pad, b.y + pad + lp * 0.8, lp * 0.75, TXT_DIM);
        let big = b.h * 0.30;
        let pct = format!("{}%", st.bat);
        self.text(&pct, b.x + pad, b.y + pad + lp + big * 0.95, big, col);
        // battery glyph
        let gw = b.w * 0.30;
        let gh = big * 0.62;
        let gx = b.x + b.w - pad - gw;
        let gy = b.y + pad + lp + big * 0.95 - gh;
        self.stroke(Rect::new(gx, gy, gw, gh), gh * 0.18, TXT_DIM, 255, 1.6 * self.s);
        self.fill(Rect::new(gx + gw, gy + gh * 0.3, gw * 0.06, gh * 0.4), 2.0, TXT_DIM, 255);
        let inner = Rect::new(gx + 3.0 * self.s, gy + 3.0 * self.s, (gw - 6.0 * self.s) * st.bat as f32 / 100.0, gh - 6.0 * self.s);
        self.fill(inner, gh * 0.12, col, 255);
        // power
        let wpx = b.h * 0.14;
        let sign = if st.power_w > 0.0 { "+" } else { "−" };
        let pw = format!("{sign}{:.1} Вт", st.power_w.abs());
        let state = if charging { "заряд" } else { "разряд" };
        let by = b.y + b.h * 0.74;
        self.text(&pw, b.x + pad, by, wpx, if charging { GOOD } else { WARN });
        let tw = self.text_w(state, wpx * 0.6);
        self.text(state, b.x + b.w - pad - tw, by, wpx * 0.6, TXT_DIM);
        // power history: bars around zero
        let hb = Rect::new(b.x + pad, b.y + b.h * 0.80, b.w - 2.0 * pad, b.h * 0.14);
        let maxp = st.power_hist.iter().fold(5.0f32, |m, v| m.max(v.abs()));
        let n = crate::stats::HIST as f32;
        let bw = hb.w / n;
        let mid = hb.y + hb.h / 2.0;
        self.fill(Rect::new(hb.x, mid - 0.5, hb.w, 1.0), 0.0, (60, 62, 68), 255);
        let off = n - st.power_hist.len() as f32;
        let hist: Vec<f32> = st.power_hist.iter().copied().collect();
        for (i, v) in hist.iter().enumerate() {
            let hh = (v.abs() / maxp) * hb.h / 2.0;
            let x = hb.x + (off + i as f32) * bw;
            let rr = if *v >= 0.0 { Rect::new(x, mid - hh, bw * 0.7, hh) } else { Rect::new(x, mid, bw * 0.7, hh) };
            self.fill(rr, 0.0, if *v >= 0.0 { GOOD } else { WARN }, 200);
        }

        let d = card(2, r);
        self.card(d);
        let sub = format!("из {:.0} ГБ", st.disk_total);
        self.metric(d, "ДИСК", &format!("{:.0} ГБ", st.disk_used), &sub, ACC_DISK);
        self.bar(d, st.disk_used / st.disk_total.max(1.0), ACC_DISK);
        [l.grow(2.0), r.grow(2.0)]
    }

    fn card(&mut self, c: Rect) {
        self.fill(c, c.h.min(c.w) * 0.10, CARD, 255);
        self.stroke(c.grow(-0.5), c.h.min(c.w) * 0.10, (255, 255, 255), 10, 1.0);
    }

    fn metric(&mut self, c: Rect, name: &str, val: &str, sub: &str, acc: (u8, u8, u8)) {
        let pad = c.w * 0.07;
        let lp = c.h * 0.17;
        self.fill(Rect::new(c.x + pad, c.y + pad * 0.9, lp * 0.32, lp * 0.32), lp * 0.16, acc, 255);
        self.text(name, c.x + pad + lp * 0.55, c.y + pad * 0.9 + lp * 0.36, lp * 0.75, TXT_DIM);
        let vp = c.h * 0.30;
        let vw = self.text_w(val, vp);
        self.text(val, c.x + c.w - pad - vw, c.y + pad * 0.6 + vp * 0.9, vp, TXT);
        self.text(sub, c.x + pad, c.y + c.h * 0.58, lp * 0.62, TXT_DIM);
    }

    fn spark(&mut self, c: Rect, h: &std::collections::VecDeque<f32>, max: f32, acc: (u8, u8, u8)) {
        let pad = c.w * 0.07;
        let g = Rect::new(c.x + pad, c.y + c.h * 0.66, c.w - 2.0 * pad, c.h * 0.26);
        if h.len() < 2 {
            return;
        }
        let n = crate::stats::HIST as f32;
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

    fn bar(&mut self, c: Rect, frac: f32, acc: (u8, u8, u8)) {
        let pad = c.w * 0.07;
        let g = Rect::new(c.x + pad, c.y + c.h * 0.72, c.w - 2.0 * pad, c.h * 0.12);
        self.fill(g, g.h / 2.0, (45, 47, 53), 255);
        self.fill(Rect::new(g.x, g.y, g.w * frac.clamp(0.0, 1.0), g.h), g.h / 2.0, acc, 255);
    }
}
