//! Pruebas sin ventana: un guion mueve el ratón, teclea y guarda imágenes.
use crate::app::App;
use crate::db::Db;
use crate::gfx::Gpu;
use crate::jobs::Jobs;
use crate::ui::{KeyEv, Mods};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::keyboard::{Key, NamedKey};

struct S {
    app: App,
    w: f32,
    h: f32,
    dpi: f32,
}
impl S {
    fn step(&mut self) -> bool {
        self.app.frame(self.w, self.h, self.dpi, 1.0 / 60.0)
    }
    fn secs(&mut self, s: f32) {
        for _ in 0..(s * 60.0).max(1.0) as usize {
            self.step();
        }
    }
    /// Hasta que no quede trabajo ni animación.
    fn settle(&mut self, max: f32) {
        let t = Instant::now();
        let mut quiet = 0;
        while t.elapsed() < Duration::from_secs_f32(max) && quiet < 3 {
            let r = self.step();
            if !r && self.app.jobs.idle() {
                quiet += 1;
            } else {
                quiet = 0;
            }
            if !r {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
    fn mv(&mut self, x: f32, y: f32) {
        self.app.ui.input.pos = (x, y);
        self.app.ui.input.moved = true;
        self.step();
    }
    fn press(&mut self, n: u8) {
        let i = &mut self.app.ui.input;
        i.pressed = true;
        i.down = true;
        i.clicks = n;
        self.step();
    }
    fn release(&mut self) {
        let i = &mut self.app.ui.input;
        i.released = true;
        i.down = false;
        self.step();
    }
}
pub fn run(path: &str) {
    let src = std::fs::read_to_string(path).expect("guion");
    let mut s: Option<S> = None;
    let (mut w, mut h, mut dpi) = (1440.0f32, 900.0f32, 1.0f32);
    let mut log = String::new();
    for line in src.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let f: Vec<f32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if cmd == "size" {
            (w, h) = (f[0], f[1]);
        } else if cmd == "dpi" {
            dpi = f[0];
        }
        let st = s.get_or_insert_with(|| {
            let db = Db::open();
            let set = db.settings();
            let gpu = Gpu::new(None, (w * dpi) as u32, (h * dpi) as u32, set.high_power);
            let jobs = Jobs::new(Arc::new(|| {}));
            let mut app = App::new(gpu, db, set, jobs, w, h, dpi);
            app.headless = true;
            app.ui.input.pos = (-100.0, -100.0);
            S { app, w, h, dpi }
        });
        if matches!(cmd, "size" | "dpi") {
            st.w = w;
            st.h = h;
            st.dpi = dpi;
            st.app.gpu.resize((w * dpi) as u32, (h * dpi) as u32);
            continue;
        }
        match cmd {
            "wait" => st.secs(f[0]),
            "settle" => st.settle(f.first().copied().unwrap_or(20.0)),
            "sleep" => {
                let t = Instant::now();
                while t.elapsed() < Duration::from_secs_f32(f[0]) {
                    st.step();
                    std::thread::sleep(Duration::from_millis(8));
                }
            }
            "move" => st.mv(f[0], f[1]),
            "click" => {
                st.mv(f[0], f[1]);
                st.press(1);
                st.release();
            }
            "dbl" => {
                st.mv(f[0], f[1]);
                st.press(1);
                st.release();
                st.press(2);
                st.release();
            }
            "rclick" => {
                st.mv(f[0], f[1]);
                st.app.ui.input.rpressed = true;
                st.step();
            }
            "drag" | "hold" => {
                st.mv(f[0], f[1]);
                st.press(1);
                for k in 1..=12 {
                    let t = k as f32 / 12.0;
                    st.mv(f[0] + (f[2] - f[0]) * t, f[1] + (f[3] - f[1]) * t);
                }
                if cmd == "drag" {
                    st.release();
                }
            }
            "up" => st.release(),
            "wheel" => {
                st.app.ui.input.wheel = (f.get(1).copied().unwrap_or(0.0), f[0]);
                st.step();
            }
            "key" => {
                let mut m = Mods::default();
                let mut k = rest;
                while let Some((a, b)) = k.split_once('+').filter(|x| !x.1.is_empty()) {
                    match a {
                        "ctrl" => m.ctrl = true,
                        "shift" => m.shift = true,
                        _ => m.alt = true,
                    }
                    k = b;
                }
                let key = match k {
                    "Enter" => Key::Named(NamedKey::Enter),
                    "Escape" => Key::Named(NamedKey::Escape),
                    "Left" => Key::Named(NamedKey::ArrowLeft),
                    "Right" => Key::Named(NamedKey::ArrowRight),
                    "Up" => Key::Named(NamedKey::ArrowUp),
                    "Down" => Key::Named(NamedKey::ArrowDown),
                    "PageDown" => Key::Named(NamedKey::PageDown),
                    "Space" => Key::Named(NamedKey::Space),
                    "F11" => Key::Named(NamedKey::F11),
                    "Tab" => Key::Named(NamedKey::Tab),
                    "Home" => Key::Named(NamedKey::Home),
                    "End" => Key::Named(NamedKey::End),
                    c => Key::Character(c.into()),
                };
                st.app.ui.input.keys.push(KeyEv { key, mods: m });
                st.step();
            }
            "text" => {
                st.app.ui.input.text.push_str(rest);
                st.step();
            }
            "add" => st.app.add_paths(vec![PathBuf::from(rest)]),
            "open" => {
                let id = st.app.books.iter().find(|b| b.title.to_lowercase().contains(&rest.to_lowercase())).map(|b| b.id);
                match id {
                    Some(i) => st.app.open_book(i),
                    None => log.push_str(&format!("no está: {rest}\n")),
                }
            }
            "set" => {
                // set campo valor (JSON)
                let (k, v) = rest.split_once(' ').unwrap();
                let mut j = serde_json::to_value(&st.app.set).unwrap();
                j[k] = serde_json::from_str(v).unwrap();
                st.app.set = serde_json::from_value(j).unwrap();
            }
            "png" => {
                st.step();
                st.app.gpu.wait();
                let px = st.app.gpu.read_shot();
                image::save_buffer(rest, &px, (st.w * st.dpi) as u32, (st.h * st.dpi) as u32, image::ColorType::Rgba8).expect("png");
            }
            "bench" => {
                let n = f[0] as usize;
                let mut v = vec![];
                for k in 0..n {
                    if f.len() > 1 && k % 20 == 0 {
                        st.app.ui.input.wheel.1 = f[1];
                    }
                    let t = Instant::now();
                    st.step();
                    v.push(t.elapsed().as_secs_f32() * 1000.0);
                    st.app.gpu.wait();
                }
                v.sort_by(f32::total_cmp);
                let avg: f32 = v.iter().sum::<f32>() / n as f32;
                log.push_str(&format!("bench {}: media {:.2} ms, p99 {:.2}, peor {:.2}\n", rest, avg, v[n * 99 / 100], v[n - 1]));
            }
            "dump" => {
                for b in &st.app.books {
                    log.push_str(&format!("{} | {} | {} | {} | pág {} | {} | {} | {} | port {} asp {:.2} | {:.0}% | desc {}\n", b.id, b.title, b.authors, b.format, b.pages, b.year, b.genres, b.isbn, b.cover_ver, b.aspect, b.progress * 100.0, b.description.len()));
                }
            }
            _ => log.push_str(&format!("orden desconocida: {cmd}\n")),
        }
    }
    if let Some(mut s) = s {
        s.app.shutdown();
    }
    let _ = std::fs::write(format!("{path}.log"), log);
}
