// SPDX-License-Identifier: GPL-2.0-or-later
//! The touchscreen, grabbed: niri never sees these touches, so the pointer stays
//! on the monitor. Raw panel coordinates (the panel's native orientation) are
//! mapped to the overlay's buffer for the output transform, e.g. for 270:
//! X = y, Y = xmax - x.
use evdev::raw_stream::RawDevice;
use evdev::{EventType, InputEvent};
use std::collections::HashMap;

const ABS_MT_SLOT: u16 = 0x2f;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TRACKING_ID: u16 = 0x39;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Begin,
    Move,
    End,
}

#[derive(Clone, Copy, Debug)]
pub struct Touch {
    pub kind: Kind,
    pub id: i32,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy)]
struct Slot {
    id: i32,
    x: i32,
    y: i32,
    state: u8, // 0 new, 1 moving, 2 lifted
}

pub struct RawTouch {
    pub dev: RawDevice,
    xmax: f32,
    ymax: f32,
    slot: usize,
    slots: HashMap<usize, Slot>,
    rot: u16,
}

impl RawTouch {
    pub fn open(name: &str, rot: u16) -> std::io::Result<Self> {
        for (_path, mut dev) in evdev::raw_stream::enumerate() {
            if dev.name().map_or(false, |n| n.contains(name)) {
                let abs = dev.get_abs_state()?;
                let xmax = abs[ABS_MT_POSITION_X as usize].maximum as f32 + 1.0;
                let ymax = abs[ABS_MT_POSITION_Y as usize].maximum as f32 + 1.0;
                dev.grab()?;
                let fd = std::os::fd::AsRawFd::as_raw_fd(&dev);
                unsafe {
                    let fl = libc::fcntl(fd, libc::F_GETFL);
                    libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
                }
                return Ok(Self { dev, xmax, ymax, slot: 0, slots: HashMap::new(), rot });
            }
        }
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "touchscreen not found"))
    }

    pub fn release(&mut self) {
        let _ = self.dev.ungrab();
    }

    /// Read what is pending; touches are mapped into a `w` x `h` buffer.
    pub fn read(&mut self, w: f32, h: f32, out: &mut Vec<Touch>) {
        let evs: Vec<InputEvent> = match self.dev.fetch_events() {
            Ok(it) => it.collect(),
            Err(_) => return,
        };
        for ev in evs {
            let (t, c, v) = (ev.event_type(), ev.code(), ev.value());
            if t == EventType::ABSOLUTE {
                match c {
                    ABS_MT_SLOT => self.slot = v.max(0) as usize,
                    ABS_MT_TRACKING_ID => {
                        if v < 0 {
                            if let Some(s) = self.slots.get_mut(&self.slot) {
                                s.state = 2;
                            }
                        } else {
                            let old = self.slots.get(&self.slot).copied();
                            if let Some(o) = old {
                                // slot reused within one frame: end the old contact first
                                if o.id != v && o.state != 0 {
                                    out.push(Touch { kind: Kind::End, id: o.id, x: 0.0, y: 0.0 });
                                }
                            }
                            let (x, y) = old.map_or((0, 0), |o| (o.x, o.y));
                            self.slots.insert(self.slot, Slot { id: v, x, y, state: 0 });
                        }
                    }
                    ABS_MT_POSITION_X => {
                        if let Some(s) = self.slots.get_mut(&self.slot) {
                            s.x = v;
                        }
                    }
                    ABS_MT_POSITION_Y => {
                        if let Some(s) = self.slots.get_mut(&self.slot) {
                            s.y = v;
                        }
                    }
                    _ => {}
                }
            } else if t == EventType::SYNCHRONIZATION && c == 0 {
                self.flush(w, h, out);
            }
        }
    }

    fn flush(&mut self, w: f32, h: f32, out: &mut Vec<Touch>) {
        let mut gone = Vec::new();
        for (k, s) in self.slots.iter_mut() {
            let (rx, ry) = (s.x as f32, s.y as f32);
            let (x, y) = match self.rot {
                90 => ((self.ymax - 1.0 - ry) * w / self.ymax, rx * h / self.xmax),
                180 => ((self.xmax - 1.0 - rx) * w / self.xmax, (self.ymax - 1.0 - ry) * h / self.ymax),
                270 => (ry * w / self.ymax, (self.xmax - 1.0 - rx) * h / self.xmax),
                _ => (rx * w / self.xmax, ry * h / self.ymax),
            };
            let kind = match s.state {
                0 => {
                    s.state = 1;
                    Kind::Begin
                }
                1 => Kind::Move,
                _ => {
                    gone.push(*k);
                    Kind::End
                }
            };
            out.push(Touch { kind, id: s.id, x, y });
        }
        for k in gone {
            self.slots.remove(&k);
        }
    }
}

impl std::os::fd::AsFd for RawTouch {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.dev.as_fd()
    }
}

impl Drop for RawTouch {
    fn drop(&mut self) {
        self.release();
    }
}
