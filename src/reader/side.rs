//! Panel lateral (índice, notas, marcadores y búsqueda) y ajustes de página.
use super::*;
use crate::icons::Icon;
use crate::theme::{MARKS, POSTITS};

pub const W: f32 = 360.0;
const SW: f32 = 340.0;

pub fn style_rect(app: &App) -> Rect {
    let h = (app.ui.h - bars::TOP - 28.0).min(680.0);
    rect(app.ui.w - SW - 18.0, bars::TOP + 12.0, SW, h)
}

impl Reader {
    pub(super) fn center_toc(&mut self, app: &App) {
        let cur = self.toc.iter().rposition(|t| t.loc <= self.loc).unwrap_or(0);
        let view = app.ui.h - 150.0;
        self.side_scroll.jump((cur as f32 * 44.0 - view * 0.4).max(0.0));
    }
    pub(super) fn side_frame(&mut self, app: &mut App, ik: &Ink) {
        let open = self.side.is_some();
        let mut t = self.side_t;
        app.ui.tween(&mut t, open, if open { 0.28 } else { 0.22 });
        self.side_t = t;
        if t <= 0.0 {
            return;
        }
        let e = if open { ease_out(t) } else { ease_io(t) };
        let th = app.ui.th;
        let was = app.ui.enabled;
        app.ui.enabled = was && open;
        let r = rect(-W * (1.0 - e), 0.0, W, app.ui.h);
        app.ui.d.shadow(r, 0.0, 50.0, Color::BLACK.a(if ik.dark { 0.55 } else { 0.25 } * e));
        app.ui.d.fill(r, 0.0, th.panel);
        app.ui.d.fill(rect(r.r() - 1.0, 0.0, 1.0, r.h), 0.0, th.line);
        let items = [(Some(Icon::Toc), "", "Índice"), (Some(Icon::Marker), "", "Notas"), (Some(Icon::Bookmark), "", "Marcadores"), (Some(Icon::Search), "", "Buscar")];
        let tabs = [Tab::Toc, Tab::Notes, Tab::Marks, Tab::Find];
        let sel = tabs.iter().position(|t| *t == self.side_tab).unwrap_or(0);
        if let Some(k) = app.ui.segmented(id("sd-tab"), rect(r.x + 18.0, 14.0, W - 80.0, 38.0), &items, sel) {
            self.open_side(app, tabs[k]);
        }
        if app.ui.icon_btn(id("sd-x"), rect(r.r() - 52.0, 14.0, 38.0, 38.0), Icon::Close, "Cerrar", false) {
            self.side = None;
        }
        app.ui.text(fonts().title, 24.0, r.x + 22.0, 84.0, ["Índice", "Notas", "Marcadores", "Buscar"][sel], th.text);
        let body = rect(r.x + 10.0, 112.0, W - 20.0, app.ui.h - 120.0);
        match self.side_tab {
            Tab::Toc => self.toc_list(app, body),
            Tab::Notes => self.note_list(app, body),
            Tab::Marks => self.mark_list(app, body),
            Tab::Find => self.find_list(app, body),
        }
        if app.ui.hover(r) && app.ui.input.pressed {
            app.ui.active = Some(id("sd-bg"));
        }
        app.ui.enabled = was;
    }
    fn empty(&self, app: &mut App, body: Rect, ic: Icon, s: &str) {
        let th = app.ui.th;
        app.ui.icon(ic, body.cx(), body.y + 110.0, 40.0, th.text3.a(0.6));
        for (i, l) in app.ui.t.wrap(fonts().ui, 13.5, s, body.w - 70.0, 4).iter().enumerate() {
            app.ui.text_c(fonts().ui, 13.5, body.cx(), body.y + 160.0 + i as f32 * 20.0, l, th.text3);
        }
    }
    fn toc_list(&mut self, app: &mut App, body: Rect) {
        let th = app.ui.th;
        if self.toc.is_empty() {
            self.empty(app, body, Icon::Toc, "Este libro no trae índice");
            return;
        }
        let rh = 44.0;
        let sy = app.ui.scroll(id("sd-sc"), &mut self.side_scroll, body, self.toc.len() as f32 * rh + 20.0);
        app.ui.clip(body, 0.0);
        let cur = self.toc.iter().rposition(|t| t.loc <= self.loc);
        let a = (sy / rh).floor().max(0.0) as usize;
        let b = ((sy + body.h) / rh).ceil() as usize + 1;
        let mut go = None;
        for i in a..b.min(self.toc.len()) {
            let t = &self.toc[i];
            let q = rect(body.x, body.y + i as f32 * rh - sy, body.w - 8.0, rh - 4.0);
            let bid = idx("toc", i);
            if app.ui.click(bid, q) {
                go = Some(t.loc);
            }
            let over = app.ui.over(q);
            if over {
                app.ui.cursor = CursorIcon::Pointer;
            }
            let on = cur == Some(i);
            let h = app.ui.anim(bid, over as i32 as f32, 18.0);
            app.ui.d.fill(q, 12.0, th.text.a(0.05 * h + 0.06 * on as i32 as f32));
            if on {
                app.ui.d.fill(rect(q.x + 2.0, q.cy() - 8.0, 3.0, 16.0), 1.5, th.accent);
            }
            let ind = 14.0 + (t.depth.min(4) as f32) * 16.0;
            let pg = match &self.content {
                Content::Flow(v) if !v.base.is_empty() => {
                    let c = v.doc.chapter_at(t.loc.max(0) as usize);
                    match v.flows.get(c).and_then(|f| f.as_ref()) {
                        Some(f) => (v.base[c] + f.page_of(t.loc.max(0) as u32) + 1).to_string(),
                        None => String::new(),
                    }
                }
                Content::Fixed(_) => (loc_page(t.loc) + 1).to_string(),
                _ => String::new(),
            };
            let col = if on { th.accent } else if t.depth == 0 { th.text } else { th.text2 };
            app.ui.text_fit(if t.depth == 0 { fonts().ui_m } else { fonts().ui }, 14.0, q.x + ind, q.cy(), &t.title.clone(), q.w - ind - 54.0, col);
            app.ui.text_r(fonts().ui, 12.5, q.r() - 12.0, q.cy(), &pg, th.text3);
        }
        app.ui.unclip();
        if let Some(l) = go {
            self.jump(l);
            app.ui.redraw = true;
            if app.ui.w < 1000.0 {
                self.side = None;
            }
        }
    }
    fn note_list(&mut self, app: &mut App, body: Rect) {
        let th = app.ui.th;
        let list: Vec<Note> = self.notes.iter().filter(|n| n.kind != Kind::Bookmark).cloned().collect();
        if list.is_empty() {
            self.empty(app, body, Icon::Marker, "Selecciona texto para resaltarlo o pulsa N para pegar un posit");
            return;
        }
        if app.ui.icon_btn(id("sd-exp"), rect(body.r() - 48.0, 66.0, 36.0, 36.0), Icon::Export, "Guardar las notas en un archivo", false) {
            self.export(app);
        }
        // alturas según el texto
        let mut rows = vec![];
        let mut total = 0.0;
        for n in &list {
            let t = n.text.replace('\n', " ");
            let lines = app.ui.t.wrap(fonts().ui, 13.5, &t, body.w - 58.0, 4);
            let extra = if n.note.is_empty() { vec![] } else { app.ui.t.wrap(fonts().ui, 13.0, &n.note, body.w - 76.0, 4) };
            let h = 46.0 + lines.len() as f32 * 19.5 + if extra.is_empty() { 0.0 } else { extra.len() as f32 * 18.5 + 14.0 };
            rows.push((total, h, lines, extra));
            total += h + 10.0;
        }
        let sy = app.ui.scroll(id("sd-sc"), &mut self.side_scroll, body, total + 20.0);
        app.ui.clip(body, 0.0);
        let mut go = None;
        let mut del = None;
        for (n, (y, h, lines, extra)) in list.iter().zip(&rows) {
            let q = rect(body.x + 6.0, body.y + y - sy, body.w - 20.0, *h);
            if q.b() < body.y || q.y > body.b() {
                continue;
            }
            let bid = idx("nl", n.id as usize);
            let trash = rect(q.r() - 36.0, q.y + 5.0, 30.0, 30.0);
            let over = app.ui.over(q);
            let hv = app.ui.anim(bid, over as i32 as f32, 18.0);
            let col = if n.kind == Kind::Postit { POSTITS[n.color as usize % 6] } else { MARKS[n.color as usize % 5] };
            app.ui.d.fill(q, 14.0, th.text.a(0.035 + 0.035 * hv));
            app.ui.d.fill(rect(q.x + 10.0, q.y + 14.0, 4.0, q.h - 28.0), 2.0, col);
            if hv > 0.02 {
                app.ui.d.push_alpha(hv);
                if app.ui.icon_btn_c(bid ^ 5, trash, Icon::Trash, "Quitar", false, th.text2, th.accent, false) {
                    del = Some(n.id);
                }
                app.ui.d.pop_alpha();
            }
            if app.ui.click(bid, q) {
                go = Some(n.a);
            }
            if over {
                app.ui.cursor = CursorIcon::Pointer;
            }
            let chap = self.toc.iter().rfind(|t| t.loc <= n.a).map(|t| t.title.clone()).unwrap_or_default();
            let kind = match n.kind {
                Kind::Postit => "Posit",
                Kind::Underline => "Subrayado",
                _ => "Resaltado",
            };
            let head = if chap.is_empty() { kind.to_string() } else { format!("{kind}  ·  {chap}") };
            app.ui.text_fit(fonts().ui, 12.0, q.x + 26.0, q.y + 20.0, &head, q.w - 70.0, th.text3);
            for (i, l) in lines.iter().enumerate() {
                app.ui.text(fonts().ui, 13.5, q.x + 26.0, q.y + 44.0 + i as f32 * 19.5, l, th.text);
            }
            let y2 = q.y + 44.0 + lines.len() as f32 * 19.5 + 2.0;
            for (i, l) in extra.iter().enumerate() {
                app.ui.text(fonts().ui, 13.0, q.x + 36.0, y2 + 4.0 + i as f32 * 18.5, l, th.accent);
            }
        }
        app.ui.unclip();
        if let Some(d) = del {
            self.drop_note(app, d);
        } else if let Some(l) = go {
            if !self.on_screen(l) {
                self.jump(l);
            }
            app.ui.redraw = true;
        }
    }
    fn mark_list(&mut self, app: &mut App, body: Rect) {
        let th = app.ui.th;
        let list: Vec<Note> = self.notes.iter().filter(|n| n.kind == Kind::Bookmark).cloned().collect();
        if list.is_empty() {
            self.empty(app, body, Icon::Bookmark, "Pulsa M para marcar la página en la que estás");
            return;
        }
        let rh = 84.0;
        let sy = app.ui.scroll(id("sd-sc"), &mut self.side_scroll, body, list.len() as f32 * rh + 10.0);
        app.ui.clip(body, 0.0);
        let mut go = None;
        let mut del = None;
        for (i, n) in list.iter().enumerate() {
            let q = rect(body.x + 6.0, body.y + i as f32 * rh - sy, body.w - 20.0, rh - 10.0);
            if q.b() < body.y || q.y > body.b() {
                continue;
            }
            let bid = idx("ml", n.id as usize);
            let over = app.ui.over(q);
            let hv = app.ui.anim(bid, over as i32 as f32, 18.0);
            app.ui.d.fill(q, 14.0, th.text.a(0.035 + 0.035 * hv));
            app.ui.icon(Icon::BookmarkFill, q.x + 24.0, q.y + 22.0, 16.0, th.accent);
            if hv > 0.02 {
                app.ui.d.push_alpha(hv);
                if app.ui.icon_btn_c(bid ^ 5, rect(q.r() - 36.0, q.y + 5.0, 30.0, 30.0), Icon::Trash, "Quitar", false, th.text2, th.accent, false) {
                    del = Some(n.id);
                }
                app.ui.d.pop_alpha();
            }
            if app.ui.click(bid, q) {
                go = Some(n.a);
            }
            if over {
                app.ui.cursor = CursorIcon::Pointer;
            }
            let chap = self.toc.iter().rfind(|t| t.loc <= n.a).map(|t| t.title.clone()).unwrap_or_default();
            let pct = format!("{} %", (self.content.progress(n.a) * 100.0).round() as i32);
            let head = if chap.is_empty() { pct } else { format!("{chap}  ·  {pct}") };
            app.ui.text_fit(fonts().ui_m, 13.5, q.x + 44.0, q.y + 22.0, &head, q.w - 88.0, th.text);
            let t = n.text.clone();
            app.ui.text_fit(fonts().ui, 13.0, q.x + 16.0, q.y + 52.0, &t, q.w - 32.0, th.text3);
        }
        app.ui.unclip();
        if let Some(d) = del {
            self.drop_note(app, d);
        } else if let Some(l) = go {
            self.jump(l);
            app.ui.redraw = true;
        }
    }
    fn find_list(&mut self, app: &mut App, body: Rect) {
        let th = app.ui.th;
        let f = rect(body.x + 8.0, body.y, body.w - 16.0, 42.0);
        let (ch, enter) = {
            let mut q = std::mem::take(&mut self.find.q);
            let r = app.ui.field(id("rd-find"), f, &mut q, Some(Icon::Search), "Palabra o frase");
            self.find.q = q;
            r
        };
        if ch {
            self.find.due = app.ui.time + 0.22;
        }
        if self.find.due > 0.0 {
            if app.ui.time >= self.find.due {
                self.find.due = 0.0;
                if self.find.ran != self.find.q.text.trim() {
                    self.run_find(app);
                }
            } else {
                app.ui.wake = app.ui.wake.min(self.find.due - app.ui.time + 0.01);
            }
        }
        if enter {
            if self.find.ran != self.find.q.text.trim() {
                self.run_find(app);
            } else if !self.find.hits.is_empty() {
                let n = self.find.hits.len();
                let d = if app.ui.input.mods.shift { n - 1 } else { 1 };
                let k = self.find.cur.map_or(0, |c| (c + d) % n);
                self.go_hit(app, k);
                self.side_scroll.to((k as f32 * 68.0 - 160.0).max(0.0));
            }
            app.ui.focus = Some(id("rd-find"));
        }
        let n = self.find.hits.len();
        let s = if self.find.ran.chars().count() < 2 {
            String::new()
        } else if n == 0 && self.find.running {
            "Buscando…".into()
        } else {
            match n {
                0 => "Sin resultados".into(),
                1 => "1 resultado".into(),
                n => format!("{n} resultados"),
            }
        };
        app.ui.text(fonts().ui, 12.5, body.x + 14.0, body.y + 62.0, &s, th.text3);
        let view = rect(body.x, body.y + 80.0, body.w, body.h - 80.0);
        let rh = 68.0;
        let sy = app.ui.scroll(id("sd-sc"), &mut self.side_scroll, view, n as f32 * rh + 10.0);
        app.ui.clip(view, 0.0);
        let a = (sy / rh).floor().max(0.0) as usize;
        let b = ((sy + view.h) / rh).ceil() as usize + 1;
        let mut go = None;
        for i in a..b.min(n) {
            let h = &self.find.hits[i];
            let q = rect(view.x + 6.0, view.y + i as f32 * rh - sy, view.w - 20.0, rh - 6.0);
            let bid = idx("fh", i);
            if app.ui.click(bid, q) {
                go = Some(i);
            }
            let over = app.ui.over(q);
            if over {
                app.ui.cursor = CursorIcon::Pointer;
            }
            let hv = app.ui.anim(bid, over as i32 as f32, 18.0);
            let on = self.find.cur == Some(i);
            app.ui.d.fill(q, 12.0, th.text.a(0.03 + 0.04 * hv + 0.05 * on as i32 as f32));
            let chap = self.toc.iter().rfind(|t| t.loc <= h.a).map(|t| t.title.clone()).unwrap_or_default();
            let chap = if self.content.is_flow() { chap } else { format!("Página {}", loc_page(h.a) + 1) };
            app.ui.text_fit(fonts().ui, 11.5, q.x + 14.0, q.y + 17.0, &chap, q.w - 28.0, th.text3);
            // el contexto se recorta por la izquierda para que se vea el hallazgo
            let mw = app.ui.t.measure(fonts().ui_m, 13.5, &h.mid);
            let mut pre = h.pre.clone();
            while app.ui.t.measure(fonts().ui, 13.5, &pre) > (q.w - 28.0 - mw).max(30.0) * 0.45 && pre.chars().count() > 2 {
                pre = format!("…{}", pre.trim_start_matches('…').chars().skip(2).collect::<String>());
            }
            let pw = app.ui.text(fonts().ui, 13.5, q.x + 14.0, q.y + 42.0, &pre, th.text2);
            let x = q.x + 14.0 + pw;
            let mid = app.ui.t.fit(fonts().ui_m, 13.5, &h.mid, q.r() - 14.0 - x);
            let m2 = app.ui.text(fonts().ui_m, 13.5, x, q.y + 42.0, &mid, th.accent);
            let left = q.r() - 14.0 - x - m2;
            if left > 20.0 {
                let post = h.post.clone();
                app.ui.text_fit(fonts().ui, 13.5, x + m2, q.y + 42.0, &post, left, th.text2);
            }
        }
        app.ui.unclip();
        if let Some(i) = go {
            self.go_hit(app, i);
        }
    }

    // ------------------------------------------------------------ aspecto
    pub(super) fn style_frame(&mut self, app: &mut App) {
        let open = self.style_open;
        let mut t = self.style_t;
        app.ui.tween(&mut t, open, if open { 0.22 } else { 0.16 });
        self.style_t = t;
        if t <= 0.0 {
            return;
        }
        let e = ease_out(t);
        let th = app.ui.th;
        let was = app.ui.enabled;
        app.ui.enabled = was && open;
        let r = style_rect(app);
        app.ui.d.push_alpha(e);
        app.ui.d.push_xf(0.96 + 0.04 * e, r.r() - 40.0, r.y, 0.0, -8.0 * (1.0 - e));
        app.ui.card(r, 22.0);
        let flow = self.content.is_flow();
        let mode = self.mode(app);
        let paged = matches!(mode, ReadMode::Single | ReadMode::Double);
        let content = 168.0 + 92.0 + if flow { 44.0 + 4.0 * 44.0 + 88.0 + 46.0 } else { 88.0 } + if paged { 78.0 } else if mode == ReadMode::ScrollV { 88.0 } else { 0.0 } + 3.0 * 44.0 + 26.0 + 96.0;
        let body = r.inset(6.0);
        let sy = app.ui.scroll(id("st-sc"), &mut self.style_scroll, body, content);
        app.ui.clip(body, 18.0);
        let x = r.x + 22.0;
        let w = r.w - 44.0;
        let mut y = r.y + 20.0 - sy;
        // papeles
        let cw = (w - 30.0) / 4.0;
        for (i, p) in PAPERS.iter().enumerate() {
            let q = rect(x + (i % 4) as f32 * (cw + 10.0), y + (i / 4) as f32 * 72.0, cw, 46.0);
            let bid = idx("paper", i);
            if app.ui.click(bid, rect(q.x, q.y, q.w, 64.0)) {
                app.set.paper = i;
            }
            let over = app.ui.over(q);
            if over {
                app.ui.cursor = CursorIcon::Pointer;
            }
            let h = app.ui.anim(bid, over as i32 as f32, 16.0);
            let on = app.ui.anim(bid ^ 1, (app.set.paper == i) as i32 as f32, 16.0);
            app.ui.d.boxed(q, 12.0, p.bg, 1.0 + on, th.text.a(0.16 + 0.15 * h).mix(th.accent, on));
            app.ui.text_c(fonts().title, 20.0, q.cx(), q.cy(), "Aa", p.ink);
            app.ui.text_c(fonts().ui, 11.0, q.cx(), q.b() + 11.0, p.name, th.text3.mix(th.text, on));
        }
        y += 150.0;
        let row = |app: &mut App, y: &mut f32, name: &str| -> Rect {
            let cy = *y + 22.0;
            app.ui.text(fonts().ui, 13.5, x, cy, name, th.text2);
            *y += 44.0;
            rect(x + 126.0, cy - 10.0, w - 130.0, 20.0)
        };
        let q = row(app, &mut y, "Brillo");
        if let Some(v) = app.ui.slider(id("st-br"), q, (app.set.brightness - 0.4) / 0.6) {
            app.set.brightness = 0.4 + 0.6 * v;
        }
        let q = row(app, &mut y, "Calidez");
        if let Some(v) = app.ui.slider(id("st-wa"), q, app.set.warmth) {
            app.set.warmth = v;
        }
        let toggle = |app: &mut App, y: &mut f32, key: &str, name: &str, on: bool| -> bool {
            let cy = *y + 22.0;
            app.ui.text(fonts().ui, 13.5, x, cy, name, th.text2);
            *y += 44.0;
            app.ui.toggle(id(key), x + w - 44.0, cy, on)
        };
        if flow {
            let fams = &fonts().families;
            let q = row(app, &mut y, "Letra");
            let b = rect(q.x, q.cy() - 18.0, q.w, 36.0);
            if app.ui.button(id("st-fam"), b, Btn::Soft, None, fams[app.set.family.min(fams.len() - 1)].name) {
                let items = fams.iter().enumerate().map(|(i, f)| MenuItem::check(f.name, i == app.set.family)).collect();
                app.ui.open_menu(id("st-fam"), b, items);
            }
            if let Some(k) = app.ui.picked(id("st-fam")) {
                app.set.family = k;
            }
            let q = row(app, &mut y, "Tamaño");
            let v = app.set.size;
            if app.ui.icon_btn(id("st-sm"), rect(q.x, q.cy() - 17.0, 34.0, 34.0), Icon::Min, "Más pequeña", false) {
                app.set.size = (v - 1.0).max(11.0);
            }
            app.ui.text_c(fonts().ui_m, 14.0, q.cx(), q.cy(), &format!("{:.0}", app.set.size), th.text);
            if app.ui.icon_btn(id("st-sp"), rect(q.r() - 34.0, q.cy() - 17.0, 34.0, 34.0), Icon::Plus, "Más grande", false) {
                app.set.size = (v + 1.0).min(44.0);
            }
            let q = row(app, &mut y, "Interlineado");
            if let Some(v) = app.ui.slider(id("st-ld"), q, (app.set.leading - 1.15) / 0.95) {
                app.set.leading = ((1.15 + 0.95 * v) * 20.0).round() / 20.0;
            }
            let q = row(app, &mut y, "Márgenes");
            if let Some(v) = app.ui.slider(id("st-mg"), q, app.set.margin) {
                app.set.margin = (v * 20.0).round() / 20.0;
            }
            let q = row(app, &mut y, "Entre párrafos");
            if let Some(v) = app.ui.slider(id("st-pa"), q, app.set.para / 1.2) {
                app.set.para = (v * 24.0).round() / 20.0;
            }
            if toggle(app, &mut y, "st-ju", "Justificar", app.set.justify) {
                app.set.justify = !app.set.justify;
            }
            if toggle(app, &mut y, "st-hy", "Partir palabras", app.set.hyphen) {
                app.set.hyphen = !app.set.hyphen;
            }
        } else {
            let comic = matches!(&self.content, Content::Fixed(v) if v.doc.is_comic());
            let on = if comic { app.set.recolor_comics } else { app.set.recolor };
            if toggle(app, &mut y, "st-rc", "Teñir con el papel", on) {
                if comic {
                    app.set.recolor_comics = !on;
                } else {
                    app.set.recolor = !on;
                }
            }
            let q = row(app, &mut y, "Tamaño");
            let z = self.zoom_to;
            if app.ui.icon_btn(id("st-zm"), rect(q.x, q.cy() - 17.0, 34.0, 34.0), Icon::ZoomOut, "Alejar", false) {
                self.zoom_by(app, -1);
            }
            app.ui.text_c(fonts().ui_m, 14.0, q.cx(), q.cy(), &format!("{:.0} %", z * 100.0), th.text);
            if app.ui.icon_btn(id("st-zp"), rect(q.r() - 34.0, q.cy() - 17.0, 34.0, 34.0), Icon::Zoom, "Acercar", false) {
                self.zoom_by(app, 1);
            }
        }
        if paged {
            app.ui.text(fonts().ui, 13.5, x, y + 18.0, "Paso de página", th.text2);
            let ts = [Turn::Curl, Turn::Slide, Turn::Fade, Turn::None];
            let sel = ts.iter().position(|t| *t == app.set.turn).unwrap_or(0);
            if let Some(k) = app.ui.segmented(id("st-tn"), rect(x, y + 34.0, w, 36.0), &[(None, "Hoja", ""), (None, "Deslizar", ""), (None, "Fundido", ""), (None, "Nada", "")], sel) {
                app.set.turn = ts[k];
            }
            y += 78.0;
        } else if mode == ReadMode::ScrollV {
            if toggle(app, &mut y, "st-au", "Avance automático (A)", self.auto) {
                self.auto = !self.auto;
            }
            let q = row(app, &mut y, "Velocidad");
            if let Some(v) = app.ui.slider(id("st-as"), q, app.set.auto_scroll) {
                app.set.auto_scroll = v;
            }
        }
        if toggle(app, &mut y, "st-ft", "Título y número de página", app.set.footer) {
            app.set.footer = !app.set.footer;
        }
        if toggle(app, &mut y, "st-ah", "Ocultar las barras solas", app.set.auto_hide) {
            app.set.auto_hide = !app.set.auto_hide;
        }
        if toggle(app, &mut y, "st-ru", "Regla de lectura", app.set.ruler) {
            app.set.ruler = !app.set.ruler;
        }
        app.ui.d.fill(rect(x, y + 10.0, w, 1.0), 0.0, th.line);
        y += 18.0;
        let q = row(app, &mut y, "Voz");
        let b = rect(q.x, q.cy() - 18.0, q.w, 36.0);
        let name = if app.set.tts_voice.is_empty() { "Según el idioma".to_string() } else { app.set.tts_voice.replace("Microsoft ", "") };
        if app.ui.button(id("st-vo"), b, Btn::Soft, None, &name) {
            let mut items = vec![MenuItem::check("Según el idioma", app.set.tts_voice.is_empty())];
            for (n, l) in tts::voices() {
                items.push(MenuItem::check(format!("{} ({l})", n.replace("Microsoft ", "")), n == app.set.tts_voice));
            }
            app.ui.open_menu(id("st-vo"), b, items);
        }
        if let Some(k) = app.ui.picked(id("st-vo")) {
            app.set.tts_voice = if k == 0 { String::new() } else { tts::voices().get(k - 1).map(|v| v.0.clone()).unwrap_or_default() };
        }
        let q = row(app, &mut y, "Ritmo de la voz");
        if let Some(v) = app.ui.slider(id("st-vr"), q, (app.set.tts_rate - 0.5) / 2.0) {
            app.set.tts_rate = 0.5 + 2.0 * v;
        }
        app.ui.unclip();
        if app.ui.hover(r) && app.ui.input.pressed {
            app.ui.active = Some(id("st-bg"));
        }
        app.ui.d.pop_xf();
        app.ui.d.pop_alpha();
        app.ui.enabled = was;
    }
    /// Franja que deja a la vista solo unas líneas.
    pub(super) fn ruler(&mut self, app: &mut App, ik: &Ink) {
        let on = app.set.ruler && !self.style_open && self.bar < 0.5;
        let a = app.ui.anim(id("ruler"), on as i32 as f32, 10.0);
        if a <= 0.0 {
            return;
        }
        let (w, h) = (app.ui.w, app.ui.h);
        let band = app.set.size * app.set.leading * 3.2;
        let y = app.ui.anim(id("ruler-y"), app.ui.input.pos.1, 22.0);
        let (t, b) = (y - band / 2.0, y + band / 2.0);
        let c = desk(ik).a(0.72 * a);
        let f = 26.0;
        app.ui.d.fill(rect(0.0, 0.0, w, (t - f).max(0.0)), 0.0, c);
        app.ui.d.grad(rect(0.0, t - f, w, f), 0.0, c, c.a(0.0), false);
        app.ui.d.grad(rect(0.0, b, w, f), 0.0, c.a(0.0), c, false);
        app.ui.d.fill(rect(0.0, b + f, w, (h - b - f).max(0.0)), 0.0, c);
    }
}
