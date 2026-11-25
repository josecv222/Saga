//! Barras del lector: aparecen al acercarse a los bordes y se van solas.
use super::*;
use crate::icons::Icon;

pub const TOP: f32 = 60.0;

pub fn bottom_rect(app: &App) -> Rect {
    let w = (app.ui.w - 80.0).min(860.0);
    rect(app.ui.w / 2.0 - w / 2.0, app.ui.h - 80.0, w, 60.0)
}
fn left_text(min: f32) -> String {
    let m = min.ceil() as i64;
    match m {
        0 | 1 => "1 min".into(),
        m if m < 60 => format!("{m} min"),
        m => format!("{} h {:02} min", m / 60, m % 60),
    }
}

impl Reader {
    pub(super) fn bars_logic(&mut self, app: &mut App) {
        let t = app.ui.time;
        let p = app.ui.input.pos;
        let recent = t - self.last_move < 2.2 && self.press.is_none();
        let edge = p.1 < 84.0 || p.1 > app.ui.h - 104.0;
        let inside = p.1 >= 0.0 && p.1 <= app.ui.h && p.0 >= 0.0 && p.0 <= app.ui.w;
        let busy = self.style_open || self.side.is_some() || app.ui.any_menu() || self.slider_drag.is_some() || app.confirm.is_some();
        // leyendo hacia abajo no hace falta que salgan
        let show = !app.set.auto_hide || self.pinned || busy || (recent && edge && inside && self.last_move > 0.0) || t - self.start_t < 2.4;
        let mut b = self.bar;
        app.ui.tween(&mut b, show, if show { 0.18 } else { 0.26 });
        self.bar = b;
        if recent {
            app.ui.wake = app.ui.wake.min(2.25 - (t - self.last_move));
        }
        // el cursor también estorba al leer
        if self.bar <= 0.0 && t - self.last_move > 3.0 && app.ui.input.pos.1 > 0.0 {
            app.ui.hide_cursor = true;
        }
    }
    pub fn open_side(&mut self, app: &mut App, t: Tab) {
        if self.side != Some(t) {
            self.side_scroll.jump(0.0);
        }
        self.side = Some(t);
        self.side_tab = t;
        self.style_open = false;
        if t == Tab::Find {
            app.ui.focus = Some(id("rd-find"));
            self.find.q.anchor = 0;
            self.find.q.caret = self.find.q.text.len();
        }
        if t == Tab::Toc {
            self.center_toc(app);
        }
        app.ui.redraw = true;
    }
    pub fn toggle_side(&mut self, app: &mut App, t: Tab) {
        if self.side == Some(t) {
            self.side = None;
            app.ui.redraw = true;
        } else {
            self.open_side(app, t);
        }
    }
    pub fn chapter_name(&self) -> String {
        self.toc.iter().rfind(|t| t.loc <= self.loc).map(|t| t.title.clone()).unwrap_or_default()
    }
    pub(super) fn bars(&mut self, app: &mut App, ik: &Ink) {
        let e = smooth(self.bar);
        if e <= 0.0 {
            return;
        }
        let th = app.ui.th;
        let was = app.ui.enabled;
        app.ui.enabled = was && self.bar > 0.4;
        let w = app.ui.w;
        let full = app.fullscreen;
        app.ui.d.push_alpha(e);
        // ---- arriba
        app.ui.d.push_xf(1.0, 0.0, 0.0, 0.0, -14.0 * (1.0 - e));
        let top = rect(0.0, 0.0, w, TOP);
        app.ui.d.shadow(top.off(0.0, -4.0), 0.0, 24.0, Color::BLACK.a(if ik.dark { 0.45 } else { 0.18 }));
        app.ui.d.fill(top, 0.0, th.panel.a(0.975));
        app.ui.d.fill(rect(0.0, TOP - 1.0, w, 1.0), 0.0, th.line);
        let cy = TOP / 2.0;
        let mut x = 12.0;
        let b = |x: f32| rect(x, cy - 19.0, 38.0, 38.0);
        if app.ui.icon_btn(id("rb-back"), b(x), Icon::Left, "Volver a la biblioteca (Esc)", false) {
            self.close(app);
        }
        x += 44.0;
        if app.ui.icon_btn(id("rb-toc"), b(x), Icon::Toc, "Índice y notas (T)", self.side.is_some()) {
            let t = if self.side_tab == Tab::Find { Tab::Toc } else { self.side_tab };
            if self.side.is_some() {
                self.side = None;
            } else {
                self.open_side(app, t);
            }
        }
        x += 42.0;
        if !self.history.is_empty() {
            if app.ui.icon_btn(id("rb-hist"), b(x), Icon::Refresh, "Volver a donde estaba", false) {
                self.go_back(app);
            }
            x += 42.0;
        }
        // derecha
        let pad = if full { 12.0 } else { 150.0 };
        let mut rx = w - pad - 38.0;
        let mode = self.mode(app);
        if app.ui.icon_btn(id("rb-full"), b(rx), Icon::Expand, "Pantalla completa (F)", full) {
            app.win.push(crate::app::WinReq::Fullscreen(!full));
        }
        rx -= 42.0;
        if app.ui.icon_btn(id("rb-style"), b(rx), Icon::Type, if self.style_open { "" } else { "Aspecto y colores" }, self.style_open) {
            self.style_open = !self.style_open;
            self.side = None;
        }
        if w > 900.0 {
            rx -= 4.0 * 40.0 + 14.0;
            let items = [(Some(Icon::PageTwo), "", "Dos páginas (1)"), (Some(Icon::PageOne), "", "Una página (2)"), (Some(Icon::ScrollV), "", "Continuo vertical (3)"), (Some(Icon::ScrollH), "", "Continuo horizontal (4)")];
            let modes = [ReadMode::Double, ReadMode::Single, ReadMode::ScrollV, ReadMode::ScrollH];
            let sel = modes.iter().position(|m| *m == mode).unwrap_or(0);
            if let Some(k) = app.ui.segmented(id("rb-mode"), rect(rx, cy - 18.0, 4.0 * 40.0, 36.0), &items, sel) {
                self.set_mode(app, modes[k]);
            }
            rx -= 8.0;
        }
        rx -= 42.0;
        let marked = self.page_mark().is_some();
        if app.ui.icon_btn(id("rb-mark"), b(rx), if marked { Icon::BookmarkFill } else { Icon::Bookmark }, "Marcar la página (M)", marked) {
            self.toggle_mark(app);
        }
        rx -= 40.0;
        if app.ui.icon_btn(id("rb-note"), b(rx), Icon::Note, "Pegar un posit (N)", self.placing) {
            self.placing = !self.placing;
            self.pinned = false;
        }
        rx -= 40.0;
        if app.ui.icon_btn(id("rb-tts"), b(rx), Icon::Speak, "Leer en voz alta", self.tts.is_some()) {
            if self.tts.is_some() {
                self.tts_stop();
            } else {
                let l = self.loc;
                self.tts_from(app, l);
            }
        }
        rx -= 40.0;
        if app.ui.icon_btn(id("rb-find"), b(rx), Icon::Search, "Buscar (Ctrl+F)", self.side == Some(Tab::Find)) {
            self.toggle_side(app, Tab::Find);
        }
        // título
        let x = x + 8.0;
        let avail = (rx - x - 20.0).max(0.0);
        let chap = self.chapter_name();
        if avail > 60.0 {
            let title = self.title.clone();
            if chap.is_empty() || chap == title {
                app.ui.text_fit(fonts().ui_m, 14.5, x, cy, &title, avail, th.text);
            } else {
                app.ui.text_fit(fonts().ui_m, 14.0, x, cy - 9.0, &title, avail, th.text);
                app.ui.text_fit(fonts().ui, 12.0, x, cy + 10.0, &chap, avail, th.text3);
            }
        }
        app.ui.d.pop_xf();
        // ---- abajo
        app.ui.d.push_xf(1.0, 0.0, 0.0, 0.0, 16.0 * (1.0 - e));
        let r = bottom_rect(app);
        app.ui.d.shadow(r.off(0.0, 8.0), 24.0, 34.0, Color::BLACK.a(if ik.dark { 0.5 } else { 0.24 }));
        app.ui.d.boxed(r, 24.0, th.panel.a(0.975), 1.0, th.line);
        let prog = self.content.progress(self.loc).clamp(0.0, 1.0);
        let line = rect(r.x + 26.0, r.y + 10.0, r.w - 52.0, 18.0);
        // marcas de capítulo
        let n = self.toc.len();
        if n > 1 && n < 140 {
            for t in self.toc.iter().filter(|t| t.depth == 0) {
                let p = self.content.progress(t.loc);
                if p > 0.004 {
                    app.ui.d.fill(rect(line.x + line.w * p - 0.75, line.cy() - 6.0, 1.5, 12.0), 0.7, th.text.a(0.22));
                }
            }
        }
        for m in self.notes.iter().filter(|m| m.kind == Kind::Bookmark) {
            let p = self.content.progress(m.a);
            app.ui.d.fill(rect(line.x + line.w * p - 2.0, line.y - 2.0, 4.0, 4.0), 2.0, th.accent);
        }
        let shown = self.slider_drag.unwrap_or(prog);
        if let Some(v) = app.ui.slider(id("rb-prog"), line, shown) {
            if self.slider_drag.is_none() {
                self.history.push(self.loc);
            }
            self.slider_drag = Some(v);
        }
        if let Some(v) = self.slider_drag {
            let l = self.content.loc_at(v);
            let name = self.toc.iter().rfind(|t| t.loc <= l).map(|t| t.title.clone()).unwrap_or_default();
            let num = self.page_label(l);
            let s = if name.is_empty() { num } else { format!("{name}  ·  {num}") };
            let s = app.ui.t.fit(fonts().ui_m, 13.0, &s, 420.0);
            let tw = app.ui.t.measure(fonts().ui_m, 13.0, &s) + 32.0;
            let cx = (line.x + line.w * v).clamp(r.x + tw / 2.0, r.r() - tw / 2.0);
            let q = rect(cx - tw / 2.0, r.y - 46.0, tw, 36.0);
            app.ui.d.shadow(q.off(0.0, 6.0), 18.0, 22.0, Color::BLACK.a(0.35));
            app.ui.d.boxed(q, 18.0, th.raised, 1.0, th.line);
            app.ui.text_c(fonts().ui_m, 13.0, q.cx(), q.cy(), &s, th.text);
            if app.ui.active != Some(id("rb-prog")) || !app.ui.input.down {
                self.slider_drag = None;
                self.turning = None;
                self.seek(l);
                self.touch();
            }
            app.ui.redraw = true;
        }
        let ty = r.b() - 17.0;
        let label = self.page_label(self.loc);
        app.ui.text(fonts().ui, 12.5, line.x, ty, &label, th.text2);
        let pct = format!("{} %", (prog * 100.0).round() as i32);
        app.ui.text_c(fonts().ui_m, 12.5, r.cx(), ty, &pct, th.text);
        let left = self.time_left();
        app.ui.text_r(fonts().ui, 12.5, line.r(), ty, &left, th.text2);
        app.ui.d.pop_xf();
        app.ui.d.pop_alpha();
        app.ui.enabled = was;
        if app.ui.enabled && self.bar > 0.4 && app.ui.active.is_none() && app.ui.input.pressed && (r.has(app.ui.input.pos) || (top.has(app.ui.input.pos) && app.ui.input.pos.1 > crate::app::STRIP)) {
            app.ui.active = Some(id("rb-bg"));
        }
    }
    fn page_label(&mut self, l: i64) -> String {
        match &mut self.content {
            Content::Flow(v) => {
                if v.base.is_empty() {
                    return String::new();
                }
                let a = v.addr_of(l);
                format!("Página {} de {}", v.base[a.c] + a.p + 1, v.total)
            }
            Content::Fixed(v) => format!("Página {} de {}", loc_page(l) + 1, v.sizes.len()),
        }
    }
    fn time_left(&mut self) -> String {
        // a ritmo tranquilo
        match &self.content {
            Content::Flow(v) => {
                let next = self.toc.iter().find(|t| t.loc > self.loc).map_or(v.doc.len as i64, |t| t.loc);
                let ch = (next - self.loc).max(0) as f32 / 1300.0;
                let all = (v.doc.len as i64 - self.loc).max(0) as f32 / 1300.0;
                if app_small(all, ch) {
                    format!("Quedan {}", left_text(all))
                } else {
                    format!("{} de capítulo  ·  {} de libro", left_text(ch), left_text(all))
                }
            }
            Content::Fixed(v) => {
                let n = v.sizes.len() as i64 - 1 - loc_page(self.loc) as i64;
                match n {
                    0 => "Última página".into(),
                    1 => "Queda 1 página".into(),
                    n => format!("Quedan {n} páginas"),
                }
            }
        }
    }
    /// Control de la voz, flotando sobre la página.
    pub(super) fn tts_bar(&mut self, app: &mut App) {
        let on = self.tts.is_some();
        let a = app.ui.anim(id("tts-bar"), on as i32 as f32, 14.0);
        if a <= 0.0 {
            return;
        }
        let th = app.ui.th;
        let paused = self.tts.as_ref().is_some_and(|t| t.paused);
        let w = 4.0 * 42.0 + 20.0;
        let y = app.ui.h - 78.0 - 74.0 * smooth(self.bar);
        let r = rect(app.ui.w / 2.0 - w / 2.0, y + 10.0 * (1.0 - a), w, 52.0);
        app.ui.d.push_alpha(a);
        app.ui.d.shadow(r.off(0.0, 8.0), 26.0, 30.0, Color::BLACK.a(0.4));
        app.ui.d.boxed(r, 26.0, th.raised, 1.0, th.line);
        let mut x = r.x + 12.0;
        for (k, ic, tip) in [(0, Icon::Left, "Frase anterior"), (1, if paused { Icon::Play } else { Icon::Pause }, if paused { "Seguir" } else { "Pausa" }), (2, Icon::Right, "Frase siguiente"), (3, Icon::Stop, "Parar")] {
            if app.ui.icon_btn_c(idx("tts-b", k), rect(x, r.cy() - 19.0, 38.0, 38.0), ic, tip, false, th.text, th.accent, false) && on {
                match k {
                    0 => self.tts_skip(app, -1),
                    1 => self.tts_pause(app),
                    2 => self.tts_skip(app, 1),
                    _ => self.tts_stop(),
                }
            }
            x += 42.0;
        }
        if app.ui.hover(r) && app.ui.input.pressed {
            app.ui.active = Some(id("tts-bg"));
        }
        app.ui.d.pop_alpha();
    }
}
fn app_small(all: f32, ch: f32) -> bool {
    (all - ch).abs() < 1.0
}
