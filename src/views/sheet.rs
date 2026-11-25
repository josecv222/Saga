//! Ficha: el libro en grande, que se puede girar, y todo lo que se sabe de él.
use super::editor::Editor;
use super::home::num;
use crate::app::{App, STRIP};
use crate::book3d::{self, Pose};
use crate::covers::Size;
use crate::db::*;
use crate::gfx::*;
use crate::icons::Icon;
use crate::meta::lang_name;
use crate::text::fonts;
use crate::ui::*;
use winit::keyboard::NamedKey;
use winit::window::CursorIcon;

pub struct Sheet {
    pub id: i64,
    pub t: f32,
    pub closing: bool,
    yaw: Spring,
    pitch: Spring,
    grab: Option<(f32, f32, f32, f32)>,
    scroll: Scroll,
    notes: Vec<Note>,
    notes_of: i64,
    more: bool,
    secs: i64,
}
impl Sheet {
    pub fn new(app: &App, id: i64) -> Sheet {
        Sheet { id, t: 0.0, closing: false, yaw: Spring::at(0.46), pitch: Spring::at(0.0), grab: None, scroll: Scroll::default(), notes: app.db.notes(id), notes_of: id, more: false, secs: app.book(id).map_or(0, |b| b.secs) }
    }
    pub fn pose(&self, app: &App) -> (Pose, (f32, f32)) {
        let (w, h) = (app.ui.w, app.ui.h);
        let bh = (h * 0.60).min(w * 0.42).clamp(240.0, 760.0);
        let left = (w * 0.40).max(330.0);
        let cx = (left * 0.54).max(bh * 0.40 + 40.0);
        let cy = STRIP + (h - STRIP) * 0.46;
        (Pose { cx, cy, h: bh, yaw: self.yaw.x, pitch: self.pitch.x, z: 0.0, open: 0.0 }, (cx, cy))
    }
    /// Devuelve false cuando termina de cerrarse.
    pub fn frame(&mut self, app: &mut App) -> bool {
        let open = !self.closing;
        let t = app.ui.tween(&mut self.t, open, if open { 0.46 } else { 0.34 });
        if self.closing && t <= 0.0 {
            return false;
        }
        let Some(bi) = app.index.get(&self.id).copied() else { return false };
        if self.notes_of != self.id || app.books[bi].secs != self.secs {
            self.notes = app.db.notes(self.id);
            self.secs = app.books[bi].secs;
        }
        let th = app.ui.th;
        let e = if open { ease_out(t) } else { ease_io(t) };
        let full = app.ui.full();
        let was = app.ui.enabled;
        app.ui.enabled = was && open;
        // velo
        app.ui.d.push_alpha(e);
        app.ui.d.fill(full, 0.0, th.bg.a(if th.dark { 0.985 } else { 0.97 }));
        let time = app.ui.time;
        if let Some((tex, a)) = app.covers.get(&app.jobs, &app.books[bi], Size::Back, time) {
            let k = full.w / full.h;
            let ta = 1.6;
            let uv = if k > ta { let z = ta / k; [0.0, (1.0 - z) / 2.0, 1.0, (1.0 + z) / 2.0] } else { let z = k / ta; [(1.0 - z) / 2.0, 0.0, (1.0 + z) / 2.0, 1.0] };
            app.ui.d.backdrop(tex, full, uv, th.bg, 0.85, th.glow * 0.8, a * 0.9);
        }
        app.ui.d.pop_alpha();

        // el libro viaja desde su sitio
        let (to, vpt) = self.pose(app);
        let (from, vpf) = app.home_pose(self.id);
        let mut p = from.lerp(&to, e);
        let vp = (lerp(vpf.0, vpt.0, e), lerp(vpf.1, vpt.1, e));
        p.cy -= (e * (1.0 - e)) * 40.0;
        let mut l = app.look(bi, Size::Full);
        l.spine_text = true;
        let zone = rect(to.cx - to.h * 0.46, to.cy - to.h * 0.55, to.h * 0.92, to.h * 1.1);
        if app.ui.hover(zone) && app.ui.input.pressed {
            app.ui.active = Some(id("spin"));
            self.grab = Some((app.ui.input.pos.0, app.ui.input.pos.1, self.yaw.x, self.pitch.x));
        }
        if app.ui.over(zone) && self.grab.is_none() {
            app.ui.cursor = CursorIcon::Grab;
        }
        let mut click_book = false;
        if let Some((gx, gy, y0, p0)) = self.grab {
            let (dx, dy) = (app.ui.input.pos.0 - gx, app.ui.input.pos.1 - gy);
            self.yaw = Spring::at(y0 + dx * 0.0085);
            self.pitch = Spring::at((p0 + dy * 0.004).clamp(-0.45, 0.45));
            app.ui.cursor = CursorIcon::Grabbing;
            app.ui.redraw = true;
            if !app.ui.input.down {
                self.grab = None;
                click_book = dx.abs() + dy.abs() < 5.0;
            }
        } else {
            // vuelve despacio a su postura, por el lado más corto
            let tau = std::f32::consts::TAU;
            let turns = ((self.yaw.x - 0.46) / tau).round() * tau;
            let dt = app.ui.dt;
            let sway = if app.ui.over(zone) { (app.ui.input.pos.0 - to.cx) / to.h * 0.22 } else { 0.0 };
            let a = self.yaw.step(0.46 + turns + sway, 4.2, dt, 0.0008);
            let b = self.pitch.step(0.0, 4.2, dt, 0.0008);
            if a || b {
                app.ui.redraw = true;
            }
        }
        if app.opening.is_none() {
            let fl = p.cy + p.h / 2.0 + lerp(3.0, p.h * 0.035, e);
            app.ui.d.push_alpha(if th.dark { 0.9 } else { 0.4 });
            book3d::floor_shadow(&mut app.ui, &p, &l, vp, fl, 1.0);
            app.ui.d.pop_alpha();
            book3d::draw(&mut app.ui, &p, &l, vp);
        }
        if click_book && app.ui.enabled && open {
            app.open_book(self.id);
        }

        // panel de datos
        let left = (full.w * 0.40).max(330.0);
        let px = left + 12.0 + (1.0 - e) * 60.0;
        let panel = rect(px, STRIP + 8.0, full.w - left - 12.0 - 36.0, full.h - STRIP - 32.0);
        app.ui.d.push_alpha(smooth((t - 0.15) / 0.85));
        self.body(app, bi, panel);
        // cerrar
        if app.ui.icon_btn(id("sh-x"), rect(24.0, STRIP + 10.0, 42.0, 42.0), Icon::Left, "Volver", false) {
            self.closing = true;
        }
        app.ui.d.pop_alpha();
        // por si se pulsa fuera
        if app.ui.enabled && open {
            if app.ui.key(NamedKey::Escape) || app.ui.input.back {
                self.closing = true;
            }
            if app.ui.key(NamedKey::Enter) {
                app.open_book(self.id);
            }
            let step = if app.ui.key(NamedKey::ArrowRight) { 1 } else if app.ui.key(NamedKey::ArrowLeft) { -1 } else { 0 };
            if step != 0 {
                self.go(app, step);
            }
        }
        app.ui.enabled = was;
        app.ui.redraw |= self.closing;
        true
    }
    fn go(&mut self, app: &mut App, step: i32) {
        let ids: Vec<i64> = app.home.list.iter().map(|i| app.books[*i].id).collect();
        if let Some(p) = ids.iter().position(|i| *i == self.id) {
            let n = p as i32 + step;
            if n >= 0 && (n as usize) < ids.len() {
                let nid = ids[n as usize];
                let y = self.yaw.x + step as f32 * 0.9;
                *self = Sheet::new(app, nid);
                self.t = 1.0;
                // entra girando un poco
                self.yaw = Spring::at(y.clamp(-0.6, 1.5));
                app.home.focus = Some(nid);
                app.home.dirty = true;
            }
        }
    }
    fn body(&mut self, app: &mut App, bi: usize, r: Rect) {
        let th = app.ui.th;
        let b = app.books[bi].clone();
        let w = (r.w - 24.0).min(820.0);
        let x = r.x;
        // alto previo para el desplazamiento
        let title_lines = app.ui.t.wrap(fonts().title, 42.0, &b.title, w, 3);
        let desc = if b.description.is_empty() { vec![] } else { app.ui.t.wrap(fonts().ui, 15.0, &b.description, w.min(720.0), if self.more { 0 } else { 6 }) };
        let total_desc = if self.more { 99 } else { app.ui.t.wrap(fonts().ui, 15.0, &b.description, w.min(720.0), 0).len() };
        let marks: Vec<&Note> = self.notes.iter().filter(|n| n.kind != Kind::Bookmark).collect();
        let shown = marks.len().min(8);
        let info = info_rows(&b);
        let h = 40.0 + title_lines.len() as f32 * 48.0 + 30.0 + if b.subtitle.is_empty() { 0.0 } else { 28.0 } + 92.0 + 92.0 + if desc.is_empty() { 0.0 } else { desc.len() as f32 * 24.0 + 60.0 } + (info.len().div_ceil(3)) as f32 * 62.0 + 60.0 + if shown > 0 { 50.0 + shown as f32 * 66.0 } else { 0.0 } + 90.0;
        let view = rect(r.x - 10.0, r.y, r.w + 20.0, r.h);
        // centrado si sobra sitio
        let sy = app.ui.scroll(id("shs"), &mut self.scroll, view, h);
        let mut y = r.y - sy + ((r.h - h) * 0.40).max(0.0) + 16.0;
        app.ui.clip(view, 0.0);
        if !b.series.is_empty() {
            let s = if b.series_ix > 0.0 { format!("{} · {}", b.series, num(b.series_ix)) } else { b.series.clone() };
            app.ui.label(x, y + 10.0, &s, th.accent);
        }
        y += 48.0;
        for l in &title_lines {
            app.ui.text(fonts().title, 42.0, x - 1.0, y, l, th.text);
            y += 48.0;
        }
        if !b.subtitle.is_empty() {
            let s = app.ui.t.fit(fonts().title_i, 20.0, &b.subtitle, w);
            app.ui.text(fonts().title_i, 20.0, x, y - 8.0, &s, th.text2);
            y += 28.0;
        }
        app.ui.text_fit(fonts().ui, 17.0, x, y, &b.author_line(), w, th.text2);
        y += 36.0;
        // géneros
        let mut gx = x;
        for (k, g) in b.genre_list().iter().take(5).enumerate() {
            let (cw, c) = app.ui.chip(idx("shg", k), gx, y, g, false);
            if c {
                app.home.facet = vec![super::home::Facet::Genre(g.to_string())];
                app.home.dirty = true;
                self.closing = true;
            }
            gx += cw + 8.0;
        }
        if !b.genre_list().is_empty() {
            y += 46.0;
        } else {
            y += 8.0;
        }
        // botones
        let label = if b.progress >= 0.995 { "Leer de nuevo" } else if b.progress > 0.002 { "Continuar" } else { "Leer" };
        let bw = (app.ui.btn_w(label, true) + 34.0).max(168.0);
        if app.ui.button(id("sh-read"), rect(x, y, bw, 52.0), Btn::Primary, Some(Icon::Book), label) {
            app.open_book(self.id);
        }
        let mut bx = x + bw + 12.0;
        let nid = self.id;
        if app.ui.icon_btn(id("sh-fav"), rect(bx, y + 2.0, 48.0, 48.0), if b.favorite { Icon::HeartFill } else { Icon::Heart }, if b.favorite { "Quitar de favoritos" } else { "Favorito" }, b.favorite) {
            app.edit_book(nid, |b| b.favorite = !b.favorite);
        }
        bx += 60.0;
        if app.ui.icon_btn(id("sh-edit"), rect(bx, y + 2.0, 48.0, 48.0), Icon::Pencil, "Editar información", false) {
            app.editor = Editor::new(app, nid, false);
        }
        bx += 60.0;
        if app.ui.icon_btn(id("sh-wand"), rect(bx, y + 2.0, 48.0, 48.0), Icon::Wand, "Buscar datos y portada", false) {
            app.editor = Editor::new(app, nid, true);
        }
        bx += 60.0;
        let rs = rect(bx, y + 2.0, 148.0, 48.0);
        if app.ui.button(id("sh-st"), rs, Btn::Soft, Some(Icon::Down), b.status.name()) {
            let items = Status::ALL.iter().map(|(s, n)| MenuItem::check(*n, *s == b.status)).collect();
            app.ui.open_menu(id("sh-stm"), rs, items);
        }
        if let Some(i) = app.ui.picked(id("sh-stm")) {
            app.set_status(nid, Status::ALL[i].0);
        }
        bx += 160.0;
        if bx + 48.0 < r.r() {
            let rc = rect(bx, y + 2.0, 48.0, 48.0);
            if app.ui.icon_btn(id("sh-coll"), rc, Icon::Layers, "Colecciones", app.colls.iter().any(|c| c.books.contains(&nid))) {
                let mut items = vec![MenuItem::head("Colecciones")];
                for c in &app.colls {
                    items.push(MenuItem::check(c.name.clone(), c.books.contains(&nid)));
                }
                if app.colls.is_empty() {
                    items.push(MenuItem::plain("Crea la primera desde el filtro"));
                }
                app.ui.open_menu(id("sh-collm"), rc, items);
            }
            if let Some(i) = app.ui.picked(id("sh-collm")) {
                if let Some(c) = app.colls.get_mut(i - 1) {
                    let on = !c.books.contains(&nid);
                    if on {
                        c.books.push(nid);
                    } else {
                        c.books.retain(|b| *b != nid);
                    }
                    let cid = c.id;
                    app.db.set_in_collection(cid, nid, on);
                    app.home.dirty = true;
                }
            }
            bx += 60.0;
        }
        if bx + 48.0 < r.r() && app.ui.icon_btn(id("sh-del"), rect(bx, y + 2.0, 48.0, 48.0), Icon::Trash, "Quitar de la biblioteca", false) {
            app.ask_remove(nid);
        }
        y += 78.0;
        // progreso y nota
        if let Some(v) = app.ui.stars(id("sh-stars"), x, y + 8.0, 22.0, b.rating) {
            app.edit_book(nid, |b| b.rating = v);
        }
        let px = x + 178.0;
        let pw = (w - 190.0).min(420.0);
        app.ui.d.fill(rect(px, y + 6.0, pw, 5.0), 2.5, th.text.a(0.13));
        let pv = app.ui.anim(idx("shp", nid as usize), b.progress, 8.0);
        if pv > 0.001 {
            app.ui.d.fill(rect(px, y + 6.0, (pw * pv).max(5.0), 5.0), 2.5, th.accent);
        }
        let mut s = if b.progress < 0.002 { "Sin empezar".to_string() } else if b.progress >= 0.995 { "Terminado".into() } else { format!("{} %", (b.progress * 100.0).round().max(1.0)) };
        if b.secs > 60 {
            s.push_str(&format!("   ·   {} de lectura", dur(b.secs)));
        }
        if b.opened > 0 {
            s.push_str(&format!("   ·   última vez {}", ago(b.opened)));
        }
        app.ui.text_fit(fonts().ui, 13.0, px, y + 28.0, &s, pw, th.text3);
        y += 66.0;
        // sinopsis
        if !desc.is_empty() {
            for l in &desc {
                app.ui.text(fonts().ui, 15.0, x, y + 10.0, l, th.text2);
                y += 24.0;
            }
            if total_desc > 6 && !self.more {
                let rr = rect(x - 14.0, y + 4.0, 110.0, 34.0);
                if app.ui.button(id("sh-more"), rr, Btn::Ghost, None, "Leer más") {
                    self.more = true;
                }
                y += 30.0;
            }
            y += 30.0;
        }
        // datos
        app.ui.d.fill(rect(x, y, w, 1.0), 0.0, th.line);
        y += 24.0;
        let cw = w / 3.0;
        for (i, (k, v)) in info.iter().enumerate() {
            let cx = x + (i % 3) as f32 * cw;
            let cy = y + (i / 3) as f32 * 62.0;
            app.ui.label(cx, cy + 8.0, k, th.text3);
            app.ui.text_fit(fonts().ui, 15.0, cx, cy + 30.0, v, cw - 18.0, th.text);
            if app.ui.over(rect(cx, cy, cw - 18.0, 46.0)) && v.len() > 24 {
                app.ui.tooltip(rect(cx, cy, cw - 18.0, 42.0), v);
            }
        }
        y += info.len().div_ceil(3) as f32 * 62.0 + 8.0;
        // notas
        if shown > 0 {
            app.ui.d.fill(rect(x, y, w, 1.0), 0.0, th.line);
            y += 30.0;
            app.ui.text(fonts().title, 21.0, x, y, "Notas y subrayados", th.text);
            app.ui.text_r(fonts().ui, 13.0, x + w, y, &marks.len().to_string(), th.text3);
            y += 26.0;
            for (k, n) in marks.iter().take(shown).enumerate() {
                let rr = rect(x, y, w, 58.0);
                let c = if n.kind == Kind::Postit { crate::theme::POSTITS[n.color as usize % 6] } else { crate::theme::MARKS[n.color as usize % 5] };
                let nb = idx("shn", k);
                let hit = app.ui.click(nb, rr);
                let ov = app.ui.over(rr);
                let hv = app.ui.anim(nb, ov as i32 as f32, 16.0);
                app.ui.d.fill(rr, 12.0, th.text.a(0.025 + 0.045 * hv));
                app.ui.d.fill(rect(rr.x + 12.0, rr.y + 12.0, 4.0, 34.0), 2.0, c);
                let t1 = n.text.replace('\n', " ");
                let t2 = n.note.replace('\n', " ");
                if t2.is_empty() || t1.is_empty() {
                    let ls = app.ui.t.wrap(fonts().ui, 13.5, if t1.is_empty() { &t2 } else { &t1 }, w - 44.0, 2);
                    let oy = if ls.len() > 1 { -10.0 } else { 0.0 };
                    for (i, l) in ls.iter().enumerate() {
                        app.ui.text(fonts().ui, 13.5, rr.x + 30.0, rr.cy() + oy + i as f32 * 20.0, l, th.text2);
                    }
                } else {
                    app.ui.text_fit(fonts().ui, 13.5, rr.x + 30.0, rr.cy() - 10.0, &t1, w - 44.0, th.text2);
                    app.ui.text_fit(fonts().title_i, 14.5, rr.x + 30.0, rr.cy() + 11.0, &t2, w - 44.0, th.text);
                }
                if ov {
                    app.ui.cursor = CursorIcon::Pointer;
                }
                if hit {
                    let loc = n.a;
                    app.open_book_at(self.id, loc);
                }
                y += 66.0;
            }
        }
        app.ui.unclip();
    }
}
fn info_rows(b: &Book) -> Vec<(&'static str, String)> {
    let mut v: Vec<(&'static str, String)> = vec![];
    let mut put = |k, s: String| {
        if !s.is_empty() && s != "0" {
            v.push((k, s));
        }
    };
    put("Año", b.year.to_string());
    put("Páginas", b.pages.to_string());
    put("Editorial", b.publisher.clone());
    put("Idioma", lang_name(&b.language));
    put("Formato", b.format.to_uppercase());
    put("ISBN", b.isbn.clone());
    put("Añadido", date(b.added));
    if b.finished > 0 {
        put("Terminado", date(b.finished));
    }
    if let Ok(m) = std::fs::metadata(&b.path) {
        put("Tamaño", size(m.len()));
    } else {
        put("Archivo", "No encontrado".into());
    }
    v
}
pub fn size(n: u64) -> String {
    if n > 1 << 20 {
        format!("{:.1} MB", n as f64 / 1048576.0).replace('.', ",")
    } else {
        format!("{} KB", n / 1024)
    }
}
pub fn dur(s: i64) -> String {
    let (h, m) = (s / 3600, s % 3600 / 60);
    if h > 0 {
        if m > 0 { format!("{h} h {m} min") } else { format!("{h} h") }
    } else {
        format!("{} min", m.max(1))
    }
}
/// Días desde 1970 a (año, mes, día).
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}
/// Desfase local respecto a UTC, en segundos.
pub fn tz() -> i64 {
    use std::sync::OnceLock;
    static T: OnceLock<i64> = OnceLock::new();
    *T.get_or_init(|| {
        use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        let mut i = TIME_ZONE_INFORMATION::default();
        let r = unsafe { GetTimeZoneInformation(&mut i) };
        let extra = match r {
            2 => i.DaylightBias,
            1 => i.StandardBias,
            _ => 0,
        };
        -((i.Bias + extra) as i64) * 60
    })
}
pub const MONTHS: [&str; 12] = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
pub fn date(t: i64) -> String {
    if t <= 0 {
        return String::new();
    }
    let (y, m, d) = civil((t + tz()).div_euclid(86400));
    format!("{d} {} {y}", &MONTHS[m as usize - 1][..3])
}
pub fn ago(t: i64) -> String {
    let d = (now() + tz()).div_euclid(86400) - (t + tz()).div_euclid(86400);
    match d {
        0 => {
            let s = now() - t;
            if s < 3600 { format!("hace {} min", (s / 60).max(1)) } else { "hoy".into() }
        }
        1 => "ayer".into(),
        2..=30 => format!("hace {d} días"),
        _ => date(t),
    }
}
