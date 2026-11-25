//! Ajustes y resumen de lectura.
use super::sheet::{civil, dur, tz, MONTHS};
use crate::app::App;
use crate::book3d::fill_uv;
use crate::covers::Size;
use crate::db::*;
use crate::gfx::*;
use crate::icons::Icon;
use crate::text::fonts;
use crate::theme::THEMES;
use crate::ui::*;
use winit::keyboard::NamedKey;

pub struct Stats {
    /// Segundos por día: los últimos 371 días.
    days: Vec<i64>,
    today: i64,
    total: i64,
    streak: i64,
    best: i64,
    done: usize,
    done_year: usize,
    reading: usize,
    notes: usize,
    top: Vec<(i64, i64)>,
    genres: Vec<(String, usize)>,
}
enum What {
    Settings,
    Stats(Box<Stats>),
}
pub struct Panel {
    what: What,
    t: f32,
    closing: bool,
    scroll: Scroll,
}
impl Panel {
    pub fn settings() -> Panel {
        Panel { what: What::Settings, t: 0.0, closing: false, scroll: Scroll::default() }
    }
    pub fn stats(app: &App) -> Panel {
        let ss = app.db.sessions();
        let off = tz();
        let today = (now() + off).div_euclid(86400);
        let mut days = vec![0i64; 371];
        let mut all: std::collections::BTreeMap<i64, i64> = Default::default();
        let mut per: std::collections::HashMap<i64, i64> = Default::default();
        for s in &ss {
            let d = (s.start + off).div_euclid(86400);
            *all.entry(d).or_default() += s.secs;
            *per.entry(s.book).or_default() += s.secs;
            let ago = today - d;
            if (0..371).contains(&ago) {
                days[370 - ago as usize] += s.secs;
            }
        }
        let ok = |d: i64| all.get(&d).is_some_and(|s| *s >= 60);
        let mut streak = 0;
        let mut d = if ok(today) { today } else { today - 1 };
        while ok(d) {
            streak += 1;
            d -= 1;
        }
        let mut best = 0;
        let mut run = 0;
        let mut prev = i64::MIN;
        for (d, s) in &all {
            if *s < 60 {
                continue;
            }
            run = if *d == prev + 1 { run + 1 } else { 1 };
            prev = *d;
            best = best.max(run);
        }
        let year = civil(today).0;
        let mut top: Vec<(i64, i64)> = per.into_iter().filter(|p| app.index.contains_key(&p.0) && p.1 > 30).collect();
        top.sort_by_key(|x| -x.1);
        top.truncate(5);
        let mut g: std::collections::HashMap<String, usize> = Default::default();
        for b in &app.books {
            for x in b.genre_list() {
                *g.entry(x.to_string()).or_default() += 1;
            }
        }
        let mut genres: Vec<_> = g.into_iter().collect();
        genres.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        genres.truncate(6);
        let s = Stats {
            today: *all.get(&today).unwrap_or(&0),
            total: ss.iter().map(|s| s.secs).sum(),
            days,
            streak,
            best,
            done: app.books.iter().filter(|b| b.status == Status::Done).count(),
            done_year: app.books.iter().filter(|b| b.finished > 0 && civil((b.finished + off).div_euclid(86400)).0 == year).count(),
            reading: app.books.iter().filter(|b| b.status == Status::Reading).count(),
            notes: app.db.note_counts().values().sum(),
            top,
            genres,
        };
        Panel { what: What::Stats(Box::new(s)), t: 0.0, closing: false, scroll: Scroll::default() }
    }
    pub fn frame(&mut self, app: &mut App) -> bool {
        let open = !self.closing;
        let t = app.ui.tween(&mut self.t, open, if open { 0.26 } else { 0.18 });
        if self.closing && t <= 0.0 {
            return false;
        }
        let th = app.ui.th;
        let e = ease_out(t);
        let full = app.ui.full();
        let was = app.ui.enabled;
        app.ui.enabled = was && open;
        app.ui.d.push_alpha(e);
        app.ui.d.fill(full, 0.0, Color::BLACK.a(if th.dark { 0.5 } else { 0.3 }));
        let stats = matches!(self.what, What::Stats(_));
        let w = (full.w - 80.0).min(if stats { 980.0 } else { 720.0 });
        let h = (full.h - 90.0).min(if stats { 700.0 } else { 720.0 });
        let r = rect(full.cx() - w / 2.0, full.cy() - h / 2.0 + 8.0, w, h);
        app.ui.d.push_xf(0.965 + 0.035 * e, r.cx(), r.cy(), 0.0, (1.0 - e) * 14.0);
        app.ui.card(r, 26.0);
        app.ui.text(fonts().title, 30.0, r.x + 36.0, r.y + 48.0, if stats { "Tu lectura" } else { "Ajustes" }, th.text);
        if app.ui.icon_btn(id("pn-x"), rect(r.r() - 64.0, r.y + 26.0, 40.0, 40.0), Icon::Close, "Cerrar", false) {
            self.closing = true;
        }
        let body = rect(r.x + 36.0, r.y + 88.0, r.w - 72.0, r.h - 104.0);
        match &self.what {
            What::Settings => settings(app, &mut self.scroll, body),
            What::Stats(s) => draw_stats(app, s, &mut self.scroll, body),
        }
        app.ui.d.pop_xf();
        app.ui.d.pop_alpha();
        if open && app.ui.enabled && (app.ui.key(NamedKey::Escape) || (app.ui.input.pressed && !r.has(app.ui.input.pos) && app.ui.input.pos.1 > 40.0 && !app.ui.any_menu())) {
            self.closing = true;
        }
        app.ui.enabled = was;
        true
    }
}
fn row(app: &mut App, r: Rect, y: &mut f32, title: &str, sub: &str) -> f32 {
    let th = app.ui.th;
    let cy = *y + 28.0;
    if sub.is_empty() {
        app.ui.text(fonts().ui, 15.0, r.x, cy, title, th.text);
    } else {
        app.ui.text(fonts().ui, 15.0, r.x, cy - 10.0, title, th.text);
        app.ui.text_fit(fonts().ui, 12.5, r.x, cy + 11.0, sub, r.w - 200.0, th.text3);
    }
    *y += 60.0;
    cy
}
fn settings(app: &mut App, sc: &mut Scroll, r: Rect) {
    let th = app.ui.th;
    let content = 880.0;
    let sy = app.ui.scroll(id("sets"), sc, rect(r.x, r.y, r.w + 22.0, r.h), content);
    app.ui.clip(rect(r.x - 10.0, r.y, r.w + 32.0, r.h), 0.0);
    let mut y = r.y - sy;
    app.ui.label(r.x, y + 8.0, "Ambiente", th.text3);
    y += 26.0;
    let n = THEMES.len();
    let cols = 3;
    let cw = (r.w - 24.0) / cols as f32;
    for (i, t) in THEMES.iter().enumerate() {
        let c = rect(r.x + (i % cols) as f32 * (cw + 12.0), y + (i / cols) as f32 * 94.0, cw, 82.0);
        let bid = idx("thm", i);
        if app.ui.click(bid, c) {
            app.set.theme = i;
        }
        let ov = app.ui.over(c);
        if ov {
            app.ui.cursor = winit::window::CursorIcon::Pointer;
        }
        let h = app.ui.anim(bid, ov as i32 as f32, 14.0);
        let on = app.ui.anim(bid ^ 9, (app.set.theme == i) as i32 as f32, 14.0);
        app.ui.d.boxed(c, 16.0, t.bg, 1.0 + on, th.text.a(0.10 + 0.12 * h).mix(th.accent, on));
        app.ui.d.fill(rect(c.x + 14.0, c.y + 16.0, 24.0, 34.0), 3.0, t.accent);
        app.ui.d.fill(rect(c.x + 44.0, c.y + 18.0, c.w * 0.36, 7.0), 3.5, t.text.a(0.85));
        app.ui.d.fill(rect(c.x + 44.0, c.y + 32.0, c.w * 0.24, 6.0), 3.0, t.text.a(0.4));
        app.ui.text(fonts().ui_m, 13.0, c.x + 14.0, c.b() - 16.0, t.name, t.text);
        if on > 0.01 {
            app.ui.icon(Icon::Check, c.r() - 20.0, c.y + 20.0, 16.0, t.accent.fade(on));
        }
    }
    y += n.div_ceil(cols) as f32 * 94.0 + 16.0;
    app.ui.label(r.x, y + 8.0, "Biblioteca", th.text3);
    y += 22.0;
    let cy = row(app, r, &mut y, "Tamaño de los libros", "Para la cuadrícula y el estante");
    let v = (app.set.grid_size - 110.0) / 150.0;
    if let Some(nv) = app.ui.slider(id("gsz"), rect(r.r() - 180.0, cy - 10.0, 170.0, 20.0), v) {
        app.set.grid_size = 110.0 + nv * 150.0;
    }
    let cy = row(app, r, &mut y, "Buscar datos al añadir", "Autor, sinopsis, año y portada, solo si faltan. Se envía el título y el autor.");
    if app.ui.toggle(id("t-auto"), r.r() - 54.0, cy, app.set.auto_meta) {
        app.set.auto_meta = !app.set.auto_meta;
    }
    let cy = row(app, r, &mut y, "Completar los que faltan", "Busca ahora los datos de los libros incompletos");
    if app.ui.button(id("t-all"), rect(r.r() - 130.0, cy - 19.0, 124.0, 38.0), Btn::Soft, None, "Completar") {
        let ids: Vec<i64> = app.books.iter().filter(|b| b.description.is_empty() || b.cover_ver == 0 || b.authors.is_empty()).map(|b| b.id).collect();
        app.ui.toast(format!("Buscando datos de {} libros…", ids.len()));
        for i in ids {
            app.auto_meta(i);
        }
    }
    let cy = row(app, r, &mut y, "Meta diaria", "Minutos de lectura al día");
    let s = format!("{} min", app.set.goal_min);
    app.ui.text_r(fonts().ui_m, 14.0, r.r() - 196.0, cy, &s, th.text2);
    if let Some(nv) = app.ui.slider(id("goal"), rect(r.r() - 180.0, cy - 10.0, 170.0, 20.0), (app.set.goal_min - 5) as f32 / 115.0) {
        app.set.goal_min = ((5.0 + nv * 115.0) / 5.0).round() as i32 * 5;
    }
    y += 12.0;
    app.ui.label(r.x, y + 8.0, "Aplicación", th.text3);
    y += 22.0;
    let cy = row(app, r, &mut y, "Animaciones reducidas", "Cambios más directos, sin vuelo");
    if app.ui.toggle(id("t-calm"), r.r() - 54.0, cy, app.set.calm) {
        app.set.calm = !app.set.calm;
    }
    let sub = format!("Ahora dibuja: {}. Cambia al reiniciar.", app.gpu.adapter);
    let cy = row(app, r, &mut y, "Usar la gráfica potente", &sub);
    if app.ui.toggle(id("t-gpu"), r.r() - 54.0, cy, app.set.high_power) {
        app.set.high_power = !app.set.high_power;
    }
    let cy = row(app, r, &mut y, "Datos de la biblioteca", "Portadas, notas y progreso");
    if app.ui.button(id("t-dir"), rect(r.r() - 130.0, cy - 19.0, 124.0, 38.0), Btn::Soft, None, "Abrir carpeta") {
        let _ = std::process::Command::new("explorer.exe").arg(data_dir()).spawn();
    }
    y += 10.0;
    app.ui.text(fonts().ui, 12.5, r.x, y + 12.0, &format!("Saga {}  ·  {} libros", env!("CARGO_PKG_VERSION"), app.books.len()), th.text3);
    app.ui.unclip();
}
fn draw_stats(app: &mut App, s: &Stats, sc: &mut Scroll, r: Rect) {
    let th = app.ui.th;
    let content = 600.0;
    let sy = app.ui.scroll(id("stss"), sc, rect(r.x, r.y, r.w + 22.0, r.h), content);
    app.ui.clip(rect(r.x - 10.0, r.y, r.w + 32.0, r.h), 0.0);
    let y = r.y - sy;
    // hoy
    let goal = app.set.goal_min.max(1) as i64 * 60;
    let ring = rect(r.x + 6.0, y + 4.0, 128.0, 128.0);
    let p = app.ui.anim_from(id("ring"), 0.0, (s.today as f32 / goal as f32).min(1.0), 5.0);
    app.ui.d.arc(ring, 9.0, p, th.accent, th.text.a(0.10));
    app.ui.text_c(fonts().title, 34.0, ring.cx(), ring.cy() - 8.0, &(s.today / 60).to_string(), th.text);
    app.ui.text_c(fonts().ui, 12.0, ring.cx(), ring.cy() + 20.0, &format!("de {} min hoy", app.set.goal_min), th.text3);
    // cifras
    let vals = [
        (if s.streak == 1 { "1 día".into() } else { format!("{} días", s.streak) }, "Racha actual", Icon::Sun),
        (if s.best == 1 { "1 día".to_string() } else { format!("{} días", s.best) }, "Mejor racha", Icon::Star),
        (if s.total < 60 { "0 min".into() } else { dur(s.total) }, "Tiempo total", Icon::Clock),
        (s.done.to_string(), "Terminados", Icon::Check),
        (s.done_year.to_string(), "Este año", Icon::Calendar),
        (s.reading.to_string(), "Leyendo", Icon::Book),
        (app.books.len().to_string(), "En la biblioteca", Icon::Shelf),
        (s.notes.to_string(), "Notas y marcas", Icon::Note),
    ];
    let x0 = r.x + 170.0;
    let cw = (r.w - 170.0) / 4.0;
    for (i, (v, k, ic)) in vals.iter().enumerate() {
        let c = rect(x0 + (i % 4) as f32 * cw, y + (i / 4) as f32 * 72.0, cw - 10.0, 62.0);
        app.ui.d.fill(c, 16.0, th.text.a(0.035));
        app.ui.icon(*ic, c.x + 24.0, c.cy(), 19.0, th.accent);
        app.ui.text_fit(fonts().ui_m, 17.0, c.x + 46.0, c.cy() - 9.0, v, c.w - 54.0, th.text);
        app.ui.text_fit(fonts().ui, 12.0, c.x + 46.0, c.cy() + 13.0, k, c.w - 54.0, th.text3);
    }
    // calendario
    let hy = y + 176.0;
    app.ui.label(r.x, hy, "El último año", th.text3);
    let today = (now() + tz()).div_euclid(86400);
    // 1970-01-01 fue jueves; semana desde el lunes
    let dow = (today + 3).rem_euclid(7);
    let weeks = 53;
    let cell = ((r.w - 4.0) / weeks as f32).floor();
    let size = cell - 3.0;
    let mut tip = None;
    let mut last_m = 0;
    for wk in 0..weeks {
        for d in 0..7 {
            let ago = (weeks - 1 - wk) * 7 + (dow as i32 - d);
            if ago < 0 || ago > 370 {
                continue;
            }
            let secs = s.days[370 - ago as usize];
            let day = today - ago as i64;
            let (yy, m, dd) = civil(day);
            let c = rect(r.x + wk as f32 * cell, hy + 34.0 + d as f32 * cell, size, size);
            if d == 0 && m != last_m {
                if last_m != 0 || wk > 0 {
                    app.ui.text(fonts().ui, 10.5, c.x, hy + 24.0, &MONTHS[m as usize - 1][..3], th.text3);
                }
                last_m = m;
            }
            let k = (secs as f32 / goal as f32).clamp(0.0, 1.0);
            let col = if secs < 30 { th.text.a(0.05) } else { th.accent.a(0.25 + 0.75 * k) };
            app.ui.d.fill(c, 3.5, col);
            if app.ui.over(c) {
                tip = Some((c, format!("{dd} de {} de {yy}: {}", MONTHS[m as usize - 1], if secs < 30 { "sin lectura".into() } else { dur(secs) })));
            }
        }
    }
    if let Some((c, t)) = tip {
        app.ui.tooltip(c, &t);
    }
    let by = hy + 34.0 + 7.0 * cell + 30.0;
    let half = r.w / 2.0 - 16.0;
    app.ui.label(r.x, by, "Más leídos", th.text3);
    if s.top.is_empty() {
        app.ui.text(fonts().ui, 13.5, r.x, by + 34.0, "Aquí aparecerán tus lecturas", th.text3);
    }
    for (i, (id, secs)) in s.top.iter().enumerate() {
        if let Some(k) = app.index.get(id).copied() {
            let l = app.look(k, Size::Thumb);
            let c = rect(r.x, by + 18.0 + i as f32 * 46.0, 26.0, 38.0);
            if let Some((t, a)) = l.tex {
                app.ui.d.image(t, c, 3.0, fill_uv(l.img_aspect, c.w / c.h), Color::WHITE.a(a));
            }
            let title = app.books[k].title.clone();
            app.ui.text_fit(fonts().ui, 14.0, c.r() + 14.0, c.cy(), &title, half - 140.0, th.text);
            app.ui.text_r(fonts().ui, 13.0, r.x + half, c.cy(), &dur(*secs), th.text2);
        }
    }
    let gx = r.x + half + 32.0;
    app.ui.label(gx, by, "Géneros", th.text3);
    let max = s.genres.first().map_or(1, |g| g.1).max(1);
    for (i, (g, n)) in s.genres.iter().enumerate() {
        let cy = by + 34.0 + i as f32 * 34.0;
        app.ui.text_fit(fonts().ui, 13.5, gx, cy, g, 140.0, th.text);
        let w = (half - 190.0).max(40.0);
        let p = app.ui.anim_from(idx("gbar", i), 0.0, *n as f32 / max as f32, 6.0);
        app.ui.d.fill(rect(gx + 150.0, cy - 4.0, w, 8.0), 4.0, th.text.a(0.06));
        app.ui.d.fill(rect(gx + 150.0, cy - 4.0, (w * p).max(8.0), 8.0), 4.0, th.accent.a(0.85));
        app.ui.text_r(fonts().ui, 13.0, gx + half, cy, &n.to_string(), th.text2);
    }
    app.ui.unclip();
}
