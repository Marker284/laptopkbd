// SPDX-License-Identifier: GPL-2.0-or-later
//! System monitor for the palm rests: CPU, GPU, RAM, disk, battery and power.
//! Only sysfs/procfs reads, every 2 s while the keyboard is shown.
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::time::Instant;

pub const HIST: usize = 48;

#[derive(Default)]
pub struct Stats {
    pub cpu: f32,
    pub cpu_hist: VecDeque<f32>,
    pub cpu_mhz: u32,
    pub gpu: f32,
    pub gpu_hist: VecDeque<f32>,
    pub gpu_mhz: u32,
    pub ram_used: f32, // GiB
    pub ram_total: f32,
    pub swap_used: f32,
    pub disk_used: f32, // GB
    pub disk_total: f32,
    pub bat: u32,
    pub bat_status: String,
    pub power_w: f32, // + charging, - discharging
    pub power_hist: VecDeque<f32>,
    pub net_rx: f32, // KiB/s
    pub net_tx: f32,
    pub net_hist: VecDeque<f32>,
    pub t_cpu: i32,
    pub t_gpu: i32,
    prev_cpu: Option<(u64, u64)>,
    prev_gpu: HashMap<u64, u64>,
    prev_gpu_t: Option<Instant>,
    prev_net: Option<(u64, u64, Instant)>,
    gpu_fds: Vec<String>,
    gpu_scan: u32,
}

fn read(p: &str) -> String {
    fs::read_to_string(p).unwrap_or_default()
}
fn num(p: &str) -> i64 {
    read(p).trim().parse().unwrap_or(0)
}
fn push(h: &mut VecDeque<f32>, v: f32) {
    if h.len() == HIST {
        h.pop_front();
    }
    h.push_back(v);
}

impl Stats {
    pub fn update(&mut self) {
        // CPU: /proc/stat aggregate
        if let Some(l) = read("/proc/stat").lines().next() {
            let v: Vec<u64> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
            if v.len() >= 5 {
                let idle = v[3] + v[4];
                let total: u64 = v.iter().take(8).sum();
                if let Some((pt, pi)) = self.prev_cpu {
                    let dt = total.saturating_sub(pt).max(1) as f32;
                    let di = idle.saturating_sub(pi) as f32;
                    self.cpu = (100.0 * (1.0 - di / dt)).clamp(0.0, 100.0);
                    push(&mut self.cpu_hist, self.cpu);
                }
                self.prev_cpu = Some((total, idle));
            }
        }
        let f0 = num("/sys/devices/system/cpu/cpufreq/policy6/scaling_cur_freq");
        self.cpu_mhz = (f0 / 1000) as u32;

        // GPU: busy time from the drm fdinfo of the processes that use it
        self.gpu_mhz = (num("/sys/class/devfreq/3d00000.gpu/cur_freq") / 1_000_000) as u32;
        if self.gpu_scan == 0 {
            self.scan_gpu_fds();
        }
        self.gpu_scan = (self.gpu_scan + 1) % 5;
        // per client: a client seen for the first time adds nothing (its whole
        // lifetime would read as a 100 % spike)
        let mut cur: HashMap<u64, u64> = HashMap::new();
        for p in &self.gpu_fds {
            let s = read(p);
            let mut client = 0u64;
            let mut eng = 0u64;
            for l in s.lines() {
                if let Some(v) = l.strip_prefix("drm-client-id:") {
                    client = v.trim().parse().unwrap_or(0);
                } else if let Some(v) = l.strip_prefix("drm-engine-gpu:") {
                    eng = v.trim().trim_end_matches("ns").trim().parse().unwrap_or(0);
                }
            }
            cur.insert(client, eng);
        }
        let now = Instant::now();
        if let Some(pt) = self.prev_gpu_t {
            let d: u64 = cur
                .iter()
                .filter_map(|(c, v)| self.prev_gpu.get(c).map(|p| v.saturating_sub(*p)))
                .sum();
            let dt = now.duration_since(pt).as_nanos().max(1) as f32;
            self.gpu = (100.0 * d as f32 / dt).clamp(0.0, 100.0);
            push(&mut self.gpu_hist, self.gpu);
        }
        self.prev_gpu = cur;
        self.prev_gpu_t = Some(now);

        // RAM
        let mi = read("/proc/meminfo");
        let kv = |k: &str| -> f32 {
            mi.lines()
                .find(|l| l.starts_with(k))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(0.0)
                / 1048576.0
        };
        self.ram_total = kv("MemTotal:");
        self.ram_used = self.ram_total - kv("MemAvailable:");
        self.swap_used = kv("SwapTotal:") - kv("SwapFree:");

        // disk /
        unsafe {
            let mut st: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c"/".as_ptr(), &mut st) == 0 {
                let bs = st.f_frsize as f64;
                self.disk_total = (st.f_blocks as f64 * bs / 1e9) as f32;
                self.disk_used = ((st.f_blocks - st.f_bfree) as f64 * bs / 1e9) as f32;
            }
        }

        // battery
        let b = "/sys/class/power_supply/sm5714-battery";
        self.bat = num(&format!("{b}/capacity")).clamp(0, 100) as u32;
        self.bat_status = read(&format!("{b}/status")).trim().to_owned();
        let pw = num(&format!("{b}/power_now")) as f32 / 1e6;
        self.power_w = if pw != 0.0 {
            pw
        } else {
            num(&format!("{b}/voltage_now")) as f32 / 1e6 * num(&format!("{b}/current_now")) as f32 / 1e6
        };
        push(&mut self.power_hist, self.power_w);

        // network: every interface except lo / usb gadget / tailscale
        let (mut rx, mut tx) = (0u64, 0u64);
        for l in read("/proc/net/dev").lines().skip(2) {
            let Some((name, rest)) = l.split_once(':') else { continue };
            let n = name.trim();
            if n == "lo" || n.starts_with("usb") || n.starts_with("tailscale") || n.starts_with("docker") {
                continue;
            }
            let v: Vec<u64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if v.len() > 8 {
                rx += v[0];
                tx += v[8];
            }
        }
        let now = Instant::now();
        if let Some((prx, ptx, pt)) = self.prev_net {
            let dt = now.duration_since(pt).as_secs_f32().max(0.1);
            self.net_rx = rx.saturating_sub(prx) as f32 / 1024.0 / dt;
            self.net_tx = tx.saturating_sub(ptx) as f32 / 1024.0 / dt;
            push(&mut self.net_hist, self.net_rx + self.net_tx);
        }
        self.prev_net = Some((rx, tx, now));

        // temperatures: hottest cpu zone, hottest gpu zone
        self.t_cpu = 0;
        self.t_gpu = 0;
        if let Ok(rd) = fs::read_dir("/sys/class/thermal") {
            for e in rd.flatten() {
                let p = e.path();
                let ty = read(&format!("{}/type", p.display()));
                let t = (num(&format!("{}/temp", p.display())) / 1000) as i32;
                if ty.starts_with("cpu") {
                    self.t_cpu = self.t_cpu.max(t);
                } else if ty.starts_with("gpu") {
                    self.t_gpu = self.t_gpu.max(t);
                }
            }
        }
    }

    fn scan_gpu_fds(&mut self) {
        self.gpu_fds.clear();
        let Ok(procs) = fs::read_dir("/proc") else { return };
        for p in procs.flatten() {
            let name = p.file_name();
            let Some(pid) = name.to_str() else { continue };
            if !pid.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let Ok(fds) = fs::read_dir(format!("/proc/{pid}/fd")) else { continue };
            for fd in fds.flatten() {
                if let Ok(t) = fs::read_link(fd.path()) {
                    if t.to_str().map_or(false, |s| s.starts_with("/dev/dri/renderD")) {
                        self.gpu_fds
                            .push(format!("/proc/{pid}/fdinfo/{}", fd.file_name().to_string_lossy()));
                    }
                }
            }
        }
    }
}
