// SPDX-License-Identifier: GPL-2.0-or-later
//! laptopkbd-settings: GTK4 / libadwaita window for ~/.config/laptopkbd/config.toml.
//! Every change is saved at once and laptopkbd re-reads it (SIGHUP).
#[allow(dead_code)]
#[path = "../../src/config.rs"]
mod config;

use adw::prelude::*;
use config::Config;
use gtk::{cairo, glib};
use std::cell::RefCell;
use std::rc::Rc;

type Rgb = (f64, f64, f64);

/// Colours for the skin previews (same palettes as laptopkbd's ui.rs).
struct Swatch {
    id: &'static str,
    name: &'static str,
    deck: Rgb,
    key: Rgb,
    txt: Rgb,
    pad: Rgb,
    acc: Rgb,
    rainbow: bool,
}

fn c(r: u8, g: u8, b: u8) -> Rgb {
    (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
}

fn swatches() -> Vec<Swatch> {
    vec![
        Swatch { id: "graphite", name: "Graphite", deck: c(28, 29, 33), key: c(9, 9, 10), txt: c(235, 235, 240), pad: c(34, 36, 41), acc: c(110, 170, 255), rainbow: false },
        Swatch { id: "silver", name: "Silver", deck: c(196, 199, 205), key: c(246, 246, 248), txt: c(28, 29, 34), pad: c(210, 213, 219), acc: c(30, 110, 230), rainbow: false },
        Swatch { id: "midnight", name: "Midnight", deck: c(15, 21, 40), key: c(7, 11, 25), txt: c(222, 232, 255), pad: c(22, 31, 57), acc: c(90, 205, 255), rainbow: false },
        Swatch { id: "retro", name: "Retro", deck: c(204, 197, 178), key: c(236, 231, 218), txt: c(58, 52, 46), pad: c(190, 182, 162), acc: c(196, 64, 50), rainbow: false },
        Swatch { id: "oled", name: "OLED", deck: c(0, 0, 0), key: c(0, 0, 0), txt: c(255, 255, 255), pad: c(0, 0, 0), acc: c(110, 170, 255), rainbow: false },
        Swatch { id: "neon", name: "Neon", deck: c(10, 10, 14), key: c(16, 15, 24), txt: c(245, 240, 255), pad: c(18, 16, 28), acc: c(255, 60, 200), rainbow: true },
    ]
}

fn rrect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
    cr.close_path();
}

fn hue(h: f64) -> Rgb {
    let h = h.rem_euclid(1.0) * 6.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    match h as i32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    }
}

/// A miniature deck: four rows of keys, the trackpad and two palm-rest cards.
fn preview(sw: &Swatch) -> gtk::DrawingArea {
    let (deck, key, txt, pad, acc, rainbow) = (sw.deck, sw.key, sw.txt, sw.pad, sw.acc, sw.rainbow);
    let da = gtk::DrawingArea::builder().content_width(176).content_height(110).build();
    da.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        rrect(cr, 0.0, 0.0, w, h, 10.0);
        cr.set_source_rgb(deck.0, deck.1, deck.2);
        let _ = cr.fill();
        let m = 8.0;
        let kb = h * 0.58;
        let cols = 12.0;
        let kw = (w - 2.0 * m) / cols;
        let kh = (kb - m) / 4.0;
        for r in 0..4 {
            for k in 0..12 {
                let x = m + k as f64 * kw;
                let y = m + r as f64 * kh;
                rrect(cr, x + 1.0, y + 1.0, kw - 2.0, kh - 2.0, 2.0);
                cr.set_source_rgb(key.0, key.1, key.2);
                let _ = cr.fill_preserve();
                let o = if rainbow { hue(x / w * 0.85 + 0.5) } else { txt };
                cr.set_source_rgba(o.0, o.1, o.2, if rainbow { 0.9 } else { 0.18 });
                cr.set_line_width(0.8);
                let _ = cr.stroke();
                cr.set_source_rgba(txt.0, txt.1, txt.2, 0.85);
                cr.rectangle(x + kw * 0.3, y + kh * 0.35, kw * 0.25, kh * 0.25);
                let _ = cr.fill();
            }
        }
        let py = kb + 4.0;
        let ph = h - py - 6.0;
        let pw = w * 0.42;
        rrect(cr, (w - pw) / 2.0, py, pw, ph, 4.0);
        cr.set_source_rgb(pad.0, pad.1, pad.2);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(txt.0, txt.1, txt.2, 0.2);
        let _ = cr.stroke();
        let cw = (w - pw) / 2.0 - m - 4.0;
        for x in [m, (w + pw) / 2.0 + 4.0] {
            rrect(cr, x, py, cw, ph, 3.0);
            cr.set_source_rgba(txt.0, txt.1, txt.2, 0.08);
            let _ = cr.fill();
            cr.set_source_rgb(acc.0, acc.1, acc.2);
            cr.rectangle(x + 4.0, py + ph - 8.0, cw * 0.6, 3.0);
            let _ = cr.fill();
        }
    });
    da
}

const WIDGETS: [(&str, &str); 10] = [
    ("", "—"),
    ("cpu", "CPU"),
    ("gpu", "GPU"),
    ("ram", "RAM"),
    ("disk", "Disk"),
    ("battery", "Battery & power (2 slots)"),
    ("net", "Network"),
    ("clock", "Clock"),
    ("temps", "Temperatures"),
    ("workspaces", "niri workspaces (2 slots)"),
];

fn laptopkbd_pid() -> Option<i32> {
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let name = e.file_name();
        let pid = name.to_str()?;
        if !pid.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if std::fs::read_to_string(format!("/proc/{pid}/comm")).map_or(false, |c| c.trim() == "laptopkbd") {
            return pid.parse().ok();
        }
    }
    None
}

fn signal(sig: i32) -> bool {
    match laptopkbd_pid() {
        Some(p) => unsafe { libc::kill(p, sig) == 0 },
        None => false,
    }
}

struct State {
    cfg: Config,
    pending: Option<glib::SourceId>,
}

/// Save (debounced 250 ms) and tell laptopkbd.
fn save(st: &Rc<RefCell<State>>) {
    if let Some(id) = st.borrow_mut().pending.take() {
        id.remove();
    }
    let st2 = st.clone();
    let id = glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
        let mut s = st2.borrow_mut();
        s.pending = None;
        let p = config::path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        match toml::to_string_pretty(&s.cfg) {
            Ok(t) => {
                let _ = std::fs::write(&p, t);
                signal(libc::SIGHUP);
            }
            Err(e) => eprintln!("laptopkbd-settings: {e}"),
        }
    });
    st.borrow_mut().pending = Some(id);
}

fn switch_row(title: &str, sub: &str, on: bool, st: &Rc<RefCell<State>>, set: fn(&mut Config, bool)) -> adw::SwitchRow {
    let r = adw::SwitchRow::builder().title(title).subtitle(sub).active(on).build();
    let st = st.clone();
    r.connect_active_notify(move |r| {
        set(&mut st.borrow_mut().cfg, r.is_active());
        save(&st);
    });
    r
}

fn spin_row(title: &str, sub: &str, v: f64, lo: f64, hi: f64, step: f64, digits: u32,
            st: &Rc<RefCell<State>>, set: fn(&mut Config, f64)) -> adw::SpinRow {
    let adj = gtk::Adjustment::new(v, lo, hi, step, step * 5.0, 0.0);
    let r = adw::SpinRow::builder().title(title).subtitle(sub).adjustment(&adj).digits(digits).build();
    let st = st.clone();
    r.connect_value_notify(move |r| {
        set(&mut st.borrow_mut().cfg, r.value());
        save(&st);
    });
    r
}

fn appearance(st: &Rc<RefCell<State>>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder().title("Look").icon_name("applications-graphics-symbolic").build();
    let skins = adw::PreferencesGroup::builder().title("Skin").description("Colours of the deck, keys and widgets").build();
    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(3)
        .min_children_per_line(2)
        .column_spacing(12)
        .row_spacing(12)
        .homogeneous(true)
        .build();
    let cur = st.borrow().cfg.skin.clone();
    let mut first: Option<gtk::ToggleButton> = None;
    for sw in swatches() {
        let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
        b.append(&preview(&sw));
        b.append(&gtk::Label::new(Some(sw.name)));
        let t = gtk::ToggleButton::builder().child(&b).css_classes(["flat"]).build();
        if let Some(f) = &first {
            t.set_group(Some(f));
        } else {
            first = Some(t.clone());
        }
        t.set_active(sw.id == cur);
        let st = st.clone();
        let id = sw.id;
        t.connect_toggled(move |t| {
            if t.is_active() {
                st.borrow_mut().cfg.skin = id.to_owned();
                save(&st);
            }
        });
        flow.append(&t);
    }
    skins.add(&flow);
    page.add(&skins);

    let g = adw::PreferencesGroup::builder().title("Keyboard").build();
    let cfg = st.borrow().cfg.clone();
    g.add(&switch_row("Russian legends", "Second legend on the letter keys", cfg.ru_legends, st, |c, v| c.ru_legends = v));
    g.add(&spin_row("Keyboard height", "Share of the screen, %", (cfg.keyboard_height * 100.0) as f64, 45.0, 72.0, 1.0, 0, st,
                    |c, v| c.keyboard_height = (v / 100.0) as f32));
    let frow = adw::ComboRow::builder()
        .title("Function row")
        .subtitle("Media: mute, volume, track, play/pause, brightness on F1–F8")
        .model(&gtk::StringList::new(&["F1 – F12", "Media keys"]))
        .selected(if cfg.frow == "media" { 1 } else { 0 })
        .build();
    {
        let st = st.clone();
        frow.connect_selected_notify(move |r| {
            st.borrow_mut().cfg.frow = if r.selected() == 1 { "media" } else { "fkeys" }.into();
            save(&st);
        });
    }
    g.add(&frow);
    let lang = adw::ComboRow::builder()
        .title("Widget language")
        .model(&gtk::StringList::new(&["Automatic", "English", "Русский"]))
        .selected(match cfg.language.as_str() { "en" => 1, "ru" => 2, _ => 0 })
        .build();
    {
        let st = st.clone();
        lang.connect_selected_notify(move |r| {
            st.borrow_mut().cfg.language = ["auto", "en", "ru"][r.selected().min(2) as usize].into();
            save(&st);
        });
    }
    g.add(&lang);
    g.add(&switch_row("Tux hints", "Hold or tap Tux: the keys show what Tux + key does in niri", cfg.tux_hints, st,
                      |c, v| c.tux_hints = v));
    page.add(&g);
    page
}

fn widgets(st: &Rc<RefCell<State>>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder().title("Widgets").icon_name("view-grid-symbolic").build();
    let model = gtk::StringList::new(&WIDGETS.iter().map(|w| w.1).collect::<Vec<_>>());
    for (side, title) in [(0usize, "Left palm rest"), (1, "Right palm rest")] {
        let g = adw::PreferencesGroup::builder()
            .title(title)
            .description("Three slots, top to bottom. Battery and workspaces take two.")
            .build();
        for slot in 0..3 {
            let cur = {
                let c = &st.borrow().cfg.widgets;
                let v = if side == 0 { &c.left } else { &c.right };
                v.get(slot).cloned().unwrap_or_default()
            };
            let idx = WIDGETS.iter().position(|w| w.0 == cur).unwrap_or(0) as u32;
            let row = adw::ComboRow::builder().title(format!("Slot {}", slot + 1)).model(&model).selected(idx).build();
            let st = st.clone();
            row.connect_selected_notify(move |r| {
                let id = WIDGETS.get(r.selected() as usize).map(|w| w.0).unwrap_or("");
                let mut s = st.borrow_mut();
                let v = if side == 0 { &mut s.cfg.widgets.left } else { &mut s.cfg.widgets.right };
                while v.len() <= slot {
                    v.push(String::new());
                }
                v[slot] = id.to_owned();
                while v.last().map_or(false, |x| x.is_empty()) {
                    v.pop();
                }
                drop(s);
                save(&st);
            });
            g.add(&row);
        }
        page.add(&g);
    }
    page
}

fn trackpad(st: &Rc<RefCell<State>>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder().title("Trackpad").icon_name("input-touchpad-symbolic").build();
    let g = adw::PreferencesGroup::builder().title("Trackpad").build();
    let t = st.borrow().cfg.trackpad.clone();
    g.add(&spin_row("Pointer speed", "1.0 = default", t.speed as f64, 0.2, 4.0, 0.1, 1, st, |c, v| c.trackpad.speed = v as f32));
    g.add(&spin_row("Width", "Share of the screen width, %", (t.width * 100.0) as f64, 25.0, 60.0, 1.0, 0, st,
                    |c, v| c.trackpad.width = (v / 100.0) as f32));
    g.add(&switch_row("Natural scrolling", "Content follows the fingers", t.natural_scroll, st, |c, v| c.trackpad.natural_scroll = v));
    g.add(&switch_row("Tap to click", "One finger left, two right, three middle", t.tap_to_click, st, |c, v| c.trackpad.tap_to_click = v));
    g.add(&switch_row("Three- and four-finger gestures", "Swipe: columns and workspaces, four up: overview", t.gestures, st,
                      |c, v| c.trackpad.gestures = v));
    page.add(&g);
    page
}

fn behaviour(st: &Rc<RefCell<State>>, toast: &adw::ToastOverlay) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder().title("Behaviour").icon_name("preferences-system-symbolic").build();
    let g = adw::PreferencesGroup::builder().title("Laptop mode").build();
    let cfg = st.borrow().cfg.clone();
    g.add(&switch_row("Start with a monitor", "Connecting a monitor turns the tablet into the keyboard", cfg.auto_laptop_mode, st,
                      |c, v| c.auto_laptop_mode = v));
    let prev = adw::ActionRow::builder()
        .title("Preview on the tablet")
        .subtitle("Show the keyboard now, without a monitor. Hold Esc for 2 s to close it.")
        .build();
    let b = gtk::Button::builder().label("Show").valign(gtk::Align::Center).css_classes(["suggested-action"]).build();
    {
        let toast = toast.clone();
        b.connect_clicked(move |_| {
            let msg = if signal(libc::SIGUSR1) { "Preview toggled" } else { "laptopkbd is not running" };
            toast.add_toast(adw::Toast::new(msg));
        });
    }
    prev.add_suffix(&b);
    g.add(&prev);
    page.add(&g);

    let about = adw::PreferencesGroup::builder().title("About").build();
    let path = config::path();
    about.add(&adw::ActionRow::builder().title("Config file").subtitle(path.display().to_string()).build());
    let running = if laptopkbd_pid().is_some() { "running" } else { "not running" };
    about.add(&adw::ActionRow::builder().title("laptopkbd").subtitle(running).build());
    page.add(&about);
    page
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id("dev.gts7xllite.LaptopKbdSettings").build();
    app.connect_activate(|app| {
        let st = Rc::new(RefCell::new(State { cfg: Config::load(), pending: None }));
        let toast = adw::ToastOverlay::new();
        let stack = adw::ViewStack::new();
        for (p, name) in [
            (appearance(&st), "look"),
            (widgets(&st), "widgets"),
            (trackpad(&st), "trackpad"),
            (behaviour(&st, &toast), "behaviour"),
        ] {
            let t = p.title();
            let icon = p.icon_name();
            let sp = stack.add_titled(&p, Some(name), &t);
            if let Some(i) = icon {
                sp.set_icon_name(Some(&i));
            }
        }
        let switcher = adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).build();
        let header = adw::HeaderBar::builder().title_widget(&switcher).build();
        let tv = adw::ToolbarView::new();
        tv.add_top_bar(&header);
        toast.set_child(Some(&stack));
        tv.set_content(Some(&toast));
        let win = adw::ApplicationWindow::builder()
            .application(app)
            .title("Laptop keyboard")
            .default_width(720)
            .default_height(760)
            .content(&tv)
            .build();
        win.present();
    });
    app.run()
}
