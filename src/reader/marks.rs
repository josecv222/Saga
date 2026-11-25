//! Subrayados, posits y marcadores.
use super::content::*;
use super::*;
use crate::icons::Icon;
use crate::theme::{MARKS, POSTITS};

const NOTE_W: f32 = 216.0;
const NOTE_H: f32 = 176.0;
const INK: Color = Color::hex(0x3B3220);

/// Lo que hace falta para poner un posit en su sitio.
fn frame_of(p: &Placed, cfg_w: f32, scroll: bool, w: f32) -> Rect {
    match p {
        Placed::Flow { o, sheet, .. } if scroll => {
            let cw = (cfg_w + 480.0).min(w);
            rect(o.0 + cfg_w / 2.0 - cw / 2.0, sheet.y, cw, sheet.h)
        }
        Placed::Flow { sheet, .. } => *sheet,
        Placed::Fixed { r, .. } => *r,
    }
}

impl Reader {
    fn cfg_w(&self) -> f32 {
        match &self.content {
            Content::Flow(v) => v.cfg.w,
            _ => 0.0,
        }
    }
    /// Esquina del posit en pantalla si su página está a la vista.
    fn note_pos(&self, w: f32, n: &Note, p: &Placed) -> Option<(f32, f32)> {
        let scroll = self.mode_seen == Some(ReadMode::ScrollV);
        let fr = frame_of(p, self.cfg_w(), scroll, w);
        match p {
            Placed::Flow { flow, range, o, .. } => {
                let li = flow.line_of(n.a.max(0) as u32);
                let (first, last) = (flow.lines.first()?, flow.lines.last()?);
                if n.a < first.start as i64 || n.a > last.end as i64 {
                    return None;
                }
                if !scroll && !range.contains(&li) {
                    return None;
                }
                Some((fr.x + n.x * fr.w, o.1 + flow.lines[li].y + n.y))
            }
            Placed::Fixed { page, .. } => (loc_page(n.a) == *page).then(|| (fr.x + n.x * fr.w, fr.y + n.y * fr.h)),
        }
    }
    fn note_rect(&self, app: &App, n: &Note, p: &Placed) -> Option<Rect> {
        let (x, y) = self.note_pos(app.ui.w, n, p)?;
        let s = p.sheet();
        let scroll = self.mode_seen == Some(ReadMode::ScrollV);
        if n.w < 0.0 {
            let x = if scroll { x } else { x.clamp(s.x - 6.0, s.r() - 28.0) };
            return Some(rect(x, y, 34.0, 34.0));
        }
        let (w, h) = (n.w.max(120.0), n.h.max(90.0));
        // nunca se sale de la pantalla
        let x = x.clamp(8.0, (app.ui.w - w - 8.0).max(8.0));
        let y = if scroll { y } else { y.clamp(s.y + 4.0, (s.b() - h - 4.0).max(s.y + 4.0)) };
        Some(rect(x, y, w, h))
    }
    /// Posits pegados en una página que se dibuja fuera de pantalla.
    pub(super) fn page_notes(&mut self, app: &mut App, a: Option<Addr>, sheet: Rect, text: Rect, _ik: &Ink) {
        let Some(a) = a else { return };
        let p = match &mut self.content {
            Content::Flow(v) => {
                let flow = v.flow(a.c);
                let Some(pg) = flow.pages.get(a.p).copied() else { return };
                Placed::Flow { flow, range: pg.a as usize..pg.b as usize, o: (text.x, text.y - pg.y0), sheet }
            }
            Content::Fixed(v) => {
                let (pw, ph) = v.sizes[a.p];
                let k = (sheet.w / pw).min(sheet.h / ph);
                let _ = k;
                Placed::Fixed { page: a.p, r: sheet, sheet }
            }
        };
        for n in self.notes.iter().filter(|n| n.kind == Kind::Postit) {
            if let Some(r) = self.note_rect(app, n, &p) {
                paper_note(app, n, r, 0.0, true);
            }
        }
    }
    pub(super) fn postits(&mut self, app: &mut App, _ik: &Ink, over_ui: bool) {
        let was = app.ui.enabled;
        app.ui.enabled = was && !over_ui;
        // el foco se fue: se guarda
        if let Some((nid, _)) = &self.edit_note {
            if app.ui.focus != Some(idx("note-ed", *nid as usize)) {
                self.end_edit(app);
            }
        }
        let placed = self.placed.clone();
        let mut act: Option<(i64, u8)> = None;
        let p = app.ui.input.pos;
        let ids: Vec<i64> = self.notes.iter().filter(|n| n.kind == Kind::Postit).map(|n| n.id).collect();
        for nid in ids {
            let Some(n) = self.notes.iter().find(|n| n.id == nid).cloned() else { continue };
            let base = placed.iter().find_map(|pl| self.note_rect(app, &n, pl));
            let dragging = self.drag_note.filter(|d| d.0 == nid);
            let Some(mut r) = base.or(dragging.map(|_| rect(p.0, p.1, n.w.max(34.0), n.h.max(34.0)))) else { continue };
            if let Some((_, gx, gy, true)) = dragging {
                r.x = p.0 - gx;
                r.y = p.1 - gy;
            }
            if r.b() < -20.0 || r.y > app.ui.h + 20.0 {
                continue;
            }
            let wid = idx("note", nid as usize);
            let over = app.ui.over(r) && (app.ui.active.is_none() || app.ui.active == Some(wid));
            let hov = app.ui.anim(wid, over as i32 as f32, 14.0);
            let lift = app.ui.anim(wid ^ 7, dragging.is_some_and(|d| d.3) as i32 as f32, 16.0);
            let open = app.ui.anim_from(wid ^ 11, 0.0, 1.0, 15.0);
            app.ui.d.push_alpha(open);
            app.ui.d.push_xf((0.86 + 0.14 * open) * (1.0 + 0.035 * lift), r.cx(), r.y, 0.0, 0.0);
            let editing = self.edit_note.as_ref().is_some_and(|e| e.0 == nid);
            paper_note(app, &n, r, hov.max(lift), !editing);
            if n.w >= 0.0 {
                // botones
                let bar = hov.max(editing as i32 as f32);
                if bar > 0.02 {
                    app.ui.d.push_alpha(bar);
                    for (k, c) in POSTITS.iter().enumerate() {
                        let q = rect(r.x + 10.0 + k as f32 * 19.0, r.y + 8.0, 15.0, 15.0);
                        let cid = idx("note-c", nid as usize * 8 + k);
                        if app.ui.click(cid, q.grow(2.0)) {
                            act = Some((nid, k as u8));
                        }
                        if app.ui.over(q.grow(2.0)) {
                            app.ui.cursor = CursorIcon::Pointer;
                        }
                        app.ui.d.boxed(q, 7.5, *c, 1.0, INK.a(if n.color as usize == k { 0.7 } else { 0.25 }));
                    }
                    let b = rect(r.r() - 30.0, r.y + 4.0, 24.0, 24.0);
                    if app.ui.icon_btn_c(wid ^ 21, b, Icon::Trash, "Quitar", false, INK, INK, false) {
                        act = Some((nid, 100));
                    }
                    let b = rect(r.r() - 56.0, r.y + 4.0, 24.0, 24.0);
                    if app.ui.icon_btn_c(wid ^ 22, b, Icon::Min, "Plegar", false, INK, INK, false) {
                        act = Some((nid, 101));
                    }
                    app.ui.d.pop_alpha();
                }
                let body = rect(r.x + 15.0, r.y + 34.0, r.w - 30.0, r.h - 48.0);
                if editing {
                    let (_, e) = self.edit_note.as_mut().unwrap();
                    app.ui.clip(body.grow(3.0), 0.0);
                    if e.text.is_empty() {
                        app.ui.text(fonts().ui, 14.0, body.x, body.y + 10.0, "Escribe aquí…", INK.a(0.45));
                    }
                    app.ui.edit(idx("note-ed", nid as usize), body, e, true, fonts().ui, 14.0, INK, INK.a(0.8));
                    app.ui.unclip();
                }
                // tamaño
                let rz = rect(r.r() - 18.0, r.b() - 18.0, 20.0, 20.0);
                if app.ui.hover(rz) && app.ui.input.pressed {
                    app.ui.active = Some(wid ^ 33);
                    self.resize_note = Some((nid, p.0, p.1, n.w, n.h));
                }
                if app.ui.over(rz) || self.resize_note.is_some_and(|x| x.0 == nid) {
                    app.ui.cursor = CursorIcon::NwseResize;
                }
            }
            app.ui.d.pop_xf();
            app.ui.d.pop_alpha();
            // coger
            if app.ui.hover(r) && app.ui.input.pressed && !editing {
                app.ui.active = Some(wid);
                self.drag_note = Some((nid, p.0 - r.x, p.1 - r.y, false));
            } else if editing && app.ui.hover(r) && app.ui.input.pressed {
                app.ui.active = Some(wid);
                if p.1 < r.y + 32.0 {
                    self.drag_note = Some((nid, p.0 - r.x, p.1 - r.y, false));
                }
            }
            if over && !editing && app.ui.cursor == CursorIcon::Default {
                app.ui.cursor = CursorIcon::Grab;
            }
        }
        if let Some((nid, gx, gy, moved)) = self.drag_note {
            app.ui.redraw = true;
            let far = (p.0 - app.ui.drag_from.0).abs() + (p.1 - app.ui.drag_from.1).abs() > 5.0;
            if app.ui.input.down {
                if far {
                    self.drag_note = Some((nid, gx, gy, true));
                    app.ui.cursor = CursorIcon::Grabbing;
                }
            } else {
                self.drag_note = None;
                if moved {
                    self.anchor(app, nid, p.0 - gx, p.1 - gy);
                } else {
                    self.tap_note(app, nid);
                }
            }
        }
        if let Some((nid, x, y, w, h)) = self.resize_note {
            app.ui.redraw = true;
            app.ui.cursor = CursorIcon::NwseResize;
            if let Some(n) = self.notes.iter_mut().find(|n| n.id == nid) {
                n.w = (w + p.0 - x).clamp(150.0, 520.0);
                n.h = (h + p.1 - y).clamp(100.0, 560.0);
                if !app.ui.input.down {
                    app.db.save_note(n);
                    self.resize_note = None;
                }
            }
        }
        if let Some((nid, k)) = act {
            match k {
                100 => {
                    self.edit_note = None;
                    self.drop_note(app, nid);
                }
                101 => {
                    self.end_edit(app);
                    if let Some(n) = self.notes.iter_mut().find(|n| n.id == nid) {
                        n.w = -n.w.abs();
                        app.db.save_note(n);
                    }
                }
                c => {
                    app.set.note_color = c;
                    if let Some(n) = self.notes.iter_mut().find(|n| n.id == nid) {
                        n.color = c;
                        app.db.save_note(n);
                    }
                    if self.edit_note.is_some() {
                        app.ui.focus = Some(idx("note-ed", nid as usize));
                    }
                }
            }
        }
        // posit que se va a pegar, siguiendo al ratón
        if self.placing {
            let n = Note { id: 0, book: self.id, kind: Kind::Postit, a: 0, b: 0, color: app.set.note_color, text: String::new(), note: String::new(), x: 0.0, y: 0.0, w: NOTE_W, h: NOTE_H, created: 0 };
            app.ui.d.push_alpha(0.75);
            paper_note(app, &n, rect(p.0 - 20.0, p.1 - 14.0, NOTE_W, NOTE_H), 1.0, false);
            app.ui.d.pop_alpha();
        }
        app.ui.enabled = was;
    }
    fn tap_note(&mut self, app: &mut App, nid: i64) {
        let Some(n) = self.notes.iter_mut().find(|n| n.id == nid) else { return };
        if n.w < 0.0 {
            n.w = n.w.abs();
            app.db.save_note(n);
            app.ui.set_anim(idx("note", nid as usize) ^ 11, 0.2);
            return;
        }
        let t = n.text.clone();
        self.end_edit(app);
        self.start_edit(app, nid, &t);
    }
    pub(super) fn start_edit(&mut self, app: &mut App, nid: i64, t: &str) {
        self.edit_note = Some((nid, Edit::new(t)));
        app.ui.focus = Some(idx("note-ed", nid as usize));
        app.ui.active = Some(idx("note-ed", nid as usize) ^ 1);
        app.ui.redraw = true;
    }
    pub fn end_edit(&mut self, app: &mut App) {
        let Some((nid, e)) = self.edit_note.take() else { return };
        app.ui.focus = None;
        if let Some(n) = self.notes.iter_mut().find(|n| n.id == nid) {
            if n.kind == Kind::Postit {
                n.text = e.text;
            } else {
                n.note = e.text;
            }
            app.db.save_note(n);
        }
        app.ui.redraw = true;
    }
    /// Vuelve a pegar un posit donde se soltó.
    fn anchor(&mut self, app: &mut App, nid: i64, x: f32, y: f32) {
        let Some(pl) = self.placed.iter().find(|s| s.sheet().has((x + 20.0, y + 14.0))).or_else(|| self.placed.iter().find(|s| s.sheet().has(app.ui.input.pos))).or(self.placed.first()).cloned() else { return };
        let scroll = self.mode_seen == Some(ReadMode::ScrollV);
        let fr = frame_of(&pl, self.cfg_w(), scroll, app.ui.w);
        let Some(n) = self.notes.iter_mut().find(|n| n.id == nid) else { return };
        n.x = ((x - fr.x) / fr.w.max(1.0)).clamp(-0.4, 1.2);
        match &pl {
            Placed::Flow { flow, range, o, .. } => {
                let ly = y - o.1;
                let k = range.clone().min_by(|a, b| (flow.lines[*a].y - ly).abs().total_cmp(&(flow.lines[*b].y - ly).abs())).unwrap_or(range.start);
                n.a = flow.lines[k].start as i64;
                n.y = ly - flow.lines[k].y;
            }
            Placed::Fixed { page, r, .. } => {
                n.a = fixed_loc(*page, 0);
                n.y = ((y - r.y) / r.h).clamp(0.0, 0.97);
            }
        }
        n.b = n.a;
        app.db.save_note(n);
        self.sort_notes();
    }
    fn sort_notes(&mut self) {
        self.notes.sort_by_key(|n| (n.a, n.id));
    }
    pub(super) fn place_note(&mut self, app: &mut App, p: (f32, f32)) {
        self.placing = false;
        let mut n = Note { id: 0, book: self.id, kind: Kind::Postit, a: self.loc, b: self.loc, color: app.set.note_color, text: String::new(), note: String::new(), x: 0.0, y: 0.0, w: NOTE_W, h: NOTE_H, created: 0 };
        app.db.save_note(&mut n);
        let nid = n.id;
        self.notes.push(n);
        self.anchor(app, nid, p.0 - 20.0, p.1 - 14.0);
        self.start_edit(app, nid, "");
        self.touch();
    }
    /// Pega un posit junto a un texto concreto.
    fn note_beside(&mut self, app: &mut App, a: i64) {
        let spot = self.placed.iter().find_map(|pl| rects(pl, &self.content, a, a + 1).first().map(|r| (pl.sheet(), *r)));
        match spot {
            Some((s, r)) => {
                let x = (s.r() - NOTE_W * 0.62).min(app.ui.w - NOTE_W - 12.0);
                self.place_note(app, (x + 20.0, r.y + 14.0));
            }
            None => self.placing = true,
        }
    }
    pub fn drop_note(&mut self, app: &mut App, nid: i64) {
        if let Some(i) = self.notes.iter().position(|n| n.id == nid) {
            app.db.delete_note(nid);
            self.notes.remove(i);
        }
        if self.hot == nid {
            self.hot = 0;
        }
        self.edit_note = self.edit_note.take().filter(|e| e.0 != nid);
        app.note_n.insert(self.id, self.notes.len());
        self.touch();
        app.ui.redraw = true;
    }
    pub(super) fn mark_at(&self, l: i64) -> Option<i64> {
        self.notes.iter().rev().find(|n| matches!(n.kind, Kind::Highlight | Kind::Underline) && l >= n.a && l < n.b.max(n.a + 1)).map(|n| n.id)
    }
    pub(super) fn close_pop(&mut self) {
        self.sel = None;
        self.sel_done = false;
        self.hot = 0;
        self.pop_t = 0.0;
    }
    pub fn toggle_mark(&mut self, app: &mut App) {
        self.settle();
        match self.page_mark() {
            Some(id) => self.drop_note(app, id),
            None => {
                let (a, _) = self.visible();
                let a = a.max(self.loc);
                let end = match &self.content {
                    Content::Flow(_) => a + 90,
                    _ => a,
                };
                let text = self.content.slice(a, end).replace('\n', " ");
                let mut n = Note { id: 0, book: self.id, kind: Kind::Bookmark, a, b: a, color: 0, text: text.trim().to_string(), note: String::new(), x: 0.0, y: 0.0, w: 0.0, h: 0.0, created: 0 };
                app.db.save_note(&mut n);
                self.notes.push(n);
                self.sort_notes();
                app.ui.set_anim(id("ribbon"), 0.0);
            }
        }
        self.touch();
        app.ui.redraw = true;
    }
    /// Subraya la selección.
    pub(super) fn add_mark(&mut self, app: &mut App, kind: Kind, color: u8) -> Option<i64> {
        let (a, b) = self.sel?;
        let text = self.content.slice(a, b);
        // lo que queda debajo se sustituye
        let same: Vec<i64> = self.notes.iter().filter(|n| n.kind == kind && n.a >= a && n.b <= b && n.note.is_empty()).map(|n| n.id).collect();
        for s in same {
            self.drop_note(app, s);
        }
        let mut n = Note { id: 0, book: self.id, kind, a, b, color, text, note: String::new(), x: 0.0, y: 0.0, w: 0.0, h: 0.0, created: 0 };
        app.db.save_note(&mut n);
        let nid = n.id;
        self.notes.push(n);
        self.sort_notes();
        app.set.mark_color = color;
        app.note_n.insert(self.id, self.notes.len());
        self.sel = None;
        self.sel_done = false;
        self.touch();
        app.ui.redraw = true;
        Some(nid)
    }

    // ------------------------------------------------------------ globo
    pub(super) fn popup(&mut self, app: &mut App, over_ui: bool) {
        let tgt = match (self.hot, self.sel, self.sel_done) {
            (h, _, _) if h != 0 => self.notes.iter().find(|n| n.id == h).map(|n| (n.a, n.b)),
            (_, Some(s), true) => Some(s),
            _ => None,
        };
        let Some((a, b)) = tgt else { return };
        let mut boxes: Vec<Rect> = vec![];
        for p in &self.placed {
            boxes.extend(rects(p, &self.content, a, b));
        }
        boxes.retain(|r| r.b() > 0.0 && r.y < app.ui.h);
        let Some(first) = boxes.first().copied() else { return };
        let last = *boxes.last().unwrap();
        let mut pt = self.pop_t;
        let e = ease_out(app.ui.tween(&mut pt, true, 0.16));
        self.pop_t = pt;
        let th = app.ui.th;
        let hot = self.notes.iter().find(|n| n.id == self.hot).cloned();
        let noting = hot.as_ref().filter(|n| self.edit_note.as_ref().is_some_and(|e| e.0 == n.id) || !n.note.is_empty()).cloned();
        let icons = if hot.is_some() { 4.0 } else { 5.0 };
        let w = 5.0 * 32.0 + 16.0 + icons * 38.0 + 12.0;
        let extra = if noting.is_some() { 116.0 } else { 0.0 };
        let h = 52.0 + extra;
        let cx = first.cx().clamp(w / 2.0 + 12.0, app.ui.w - w / 2.0 - 12.0);
        let mut y = first.y - h - 12.0;
        if y < bars::TOP + 4.0 {
            y = (last.b() + 12.0).min(app.ui.h - h - 12.0);
        }
        let r = rect(cx - w / 2.0, y, w, h);
        let was = app.ui.enabled;
        app.ui.enabled = was && !over_ui;
        app.ui.d.push_alpha(e);
        app.ui.d.push_xf(0.94 + 0.06 * e, r.cx(), r.b(), 0.0, (1.0 - e) * 6.0);
        app.ui.d.shadow(r.off(0.0, 10.0), 18.0, 34.0, Color::BLACK.a(0.40));
        app.ui.d.boxed(r, 18.0, th.raised, 1.0, th.line);
        let cy = r.y + 26.0;
        let mut x = r.x + 12.0;
        let cur = hot.as_ref().map(|n| n.color);
        for (k, c) in MARKS.iter().enumerate() {
            let q = rect(x + 4.0, cy - 12.0, 24.0, 24.0);
            let cid = idx("pop-c", k);
            let click = app.ui.click(cid, q.grow(3.0));
            let ov = app.ui.over(q.grow(3.0));
            if ov {
                app.ui.cursor = CursorIcon::Pointer;
            }
            let g = app.ui.anim(cid, ov as i32 as f32, 18.0);
            app.ui.d.fill(q.grow(g * 2.0), 14.0, *c);
            if cur == Some(k as u8) {
                app.ui.icon(Icon::Check, q.cx(), q.cy(), 14.0, Color::hex(0x2A2418));
            }
            if click {
                match &hot {
                    Some(n) => {
                        let nid = n.id;
                        app.set.mark_color = k as u8;
                        if let Some(n) = self.notes.iter_mut().find(|n| n.id == nid) {
                            n.color = k as u8;
                            app.db.save_note(n);
                        }
                    }
                    None => {
                        self.add_mark(app, Kind::Highlight, k as u8);
                    }
                }
            }
            x += 32.0;
        }
        x += 8.0;
        app.ui.d.fill(rect(x, cy - 12.0, 1.0, 24.0), 0.0, th.line);
        x += 8.0;
        let mut btn = |app: &mut App, k: usize, ic: Icon, tip: &str, on: bool| {
            let q = rect(x, cy - 18.0, 36.0, 36.0);
            x += 38.0;
            app.ui.icon_btn_c(idx("pop-b", k), q, ic, tip, on, th.text, th.accent, false)
        };
        if let Some(n) = &hot {
            let nid = n.id;
            if btn(app, 0, Icon::Underline, "Subrayar o resaltar", n.kind == Kind::Underline) {
                if let Some(n) = self.notes.iter_mut().find(|n| n.id == nid) {
                    n.kind = if n.kind == Kind::Underline { Kind::Highlight } else { Kind::Underline };
                    app.db.save_note(n);
                }
            }
            if btn(app, 1, Icon::Pencil, "Escribir una nota", false) {
                self.start_edit(app, nid, &n.note);
            }
            if btn(app, 2, Icon::Copy, "Copiar", false) {
                set_clipboard(&n.text);
                app.ui.toast("Copiado");
            }
            if btn(app, 3, Icon::Trash, "Quitar", false) {
                self.drop_note(app, nid);
            }
        } else {
            if btn(app, 4, Icon::Underline, "Subrayar", false) {
                let c = app.set.mark_color;
                self.add_mark(app, Kind::Underline, c);
            }
            if btn(app, 5, Icon::Note, "Nota en un posit", false) {
                let c = app.set.mark_color;
                self.add_mark(app, Kind::Highlight, c);
                self.note_beside(app, a);
            }
            if btn(app, 6, Icon::Copy, "Copiar", false) {
                let s = self.content.slice(a, b);
                set_clipboard(&s);
                app.ui.toast("Copiado");
                self.close_pop();
            }
            if btn(app, 7, Icon::Search, "Buscar en el libro", false) {
                let s: String = self.content.slice(a, b).split_whitespace().collect::<Vec<_>>().join(" ").chars().take(60).collect();
                self.close_pop();
                self.open_side(app, Tab::Find);
                self.find.q.set(&s);
                self.run_find(app);
            }
            if btn(app, 8, Icon::Speak, "Leer en voz alta desde aquí", false) {
                self.close_pop();
                self.tts_from(app, a);
            }
        }
        if let Some(n) = noting {
            let q = rect(r.x + 12.0, r.y + 52.0, r.w - 24.0, 104.0);
            app.ui.d.fill(q, 12.0, th.text.a(0.05));
            let body = rect(q.x + 12.0, q.y + 10.0, q.w - 24.0, q.h - 20.0);
            if self.edit_note.is_none() {
                for (i, l) in app.ui.t.wrap(fonts().ui, 13.5, &n.note, body.w, 4).iter().enumerate() {
                    app.ui.text(fonts().ui, 13.5, body.x, body.y + 10.0 + i as f32 * 20.0, l, th.text);
                }
                if app.ui.click(id("pop-note"), q) {
                    self.start_edit(app, n.id, &n.note);
                }
            }
            if let Some((_, ed)) = self.edit_note.as_mut().filter(|e| e.0 == n.id) {
                app.ui.clip(body.grow(3.0), 0.0);
                if ed.text.is_empty() {
                    app.ui.text(fonts().ui, 13.5, body.x, body.y + 10.0, "Tu nota…", th.text3);
                }
                app.ui.edit(idx("note-ed", n.id as usize), body, ed, true, fonts().ui, 13.5, th.text, th.accent);
                app.ui.unclip();
            }
        }
        if app.ui.hover(r) && app.ui.input.pressed {
            app.ui.active = Some(id("pop"));
        }
        app.ui.d.pop_xf();
        app.ui.d.pop_alpha();
        app.ui.enabled = was;
    }

    // ------------------------------------------------------------ menú
    pub(super) fn context(&mut self, app: &mut App, p: (f32, f32)) {
        self.ctx_pos = p;
        self.ctx_loc = self.near(p).unwrap_or(self.loc);
        let marked = self.page_mark().is_some();
        let pin = self.sel.is_some();
        let v = vec![
            MenuItem::new(Icon::Note, "Pegar un posit aquí"),
            MenuItem::new(if marked { Icon::BookmarkFill } else { Icon::Bookmark }, if marked { "Quitar marcador" } else { "Marcar esta página" }),
            MenuItem::new(Icon::Marker, if pin { "Resaltar selección" } else { "Resaltar párrafo" }).sep(),
            MenuItem::new(Icon::Copy, if pin { "Copiar selección" } else { "Copiar párrafo" }),
            MenuItem::new(Icon::Speak, "Leer en voz alta desde aquí").sep(),
            MenuItem::new(Icon::Search, "Buscar en el libro"),
            MenuItem::new(Icon::Type, "Aspecto de la página"),
        ];
        app.ui.open_menu(id("rd-ctx"), rect(p.0, p.1 - 8.0, 0.0, 0.0), v);
    }
    pub(super) fn context_pick(&mut self, app: &mut App) {
        let Some(k) = app.ui.picked(id("rd-ctx")) else { return };
        let l = self.ctx_loc;
        match k {
            0 => {
                let p = self.ctx_pos;
                self.place_note(app, p);
            }
            1 => self.toggle_mark(app),
            2 => {
                if self.sel.is_none() {
                    self.sel = Some(self.paragraph(l));
                }
                let c = app.set.mark_color;
                self.add_mark(app, Kind::Highlight, c);
            }
            3 => {
                let (a, b) = self.sel.unwrap_or_else(|| self.paragraph(l));
                let s = self.content.slice(a, b);
                set_clipboard(&s);
                app.ui.toast("Copiado");
            }
            4 => self.tts_from(app, l),
            5 => self.open_side(app, Tab::Find),
            _ => self.style_open = true,
        }
    }
    /// Todas las notas en un archivo de texto con formato.
    pub(super) fn export(&mut self, app: &mut App) {
        let mut out = format!("# {}\n\n{}\n", self.title, self.author);
        let toc = self.toc.clone();
        let mut chap = String::new();
        for n in &self.notes {
            let c = toc.iter().rfind(|t| t.loc <= n.a).map(|t| t.title.clone()).unwrap_or_default();
            if c != chap && !c.is_empty() {
                out.push_str(&format!("\n## {c}\n"));
                chap = c;
            }
            let t = n.text.trim().replace('\n', "\n> ");
            match n.kind {
                Kind::Postit => out.push_str(&format!("\n- **Nota:** {}\n", n.text.trim())),
                Kind::Bookmark => out.push_str(&format!("\n- **Marcador:** {t}…\n")),
                _ => {
                    out.push_str(&format!("\n> {t}\n"));
                    if !n.note.trim().is_empty() {
                        out.push_str(&format!("\n{}\n", n.note.trim()));
                    }
                }
            }
        }
        let name: String = self.title.chars().map(|c| if "\\/:*?\"<>|".contains(c) { ' ' } else { c }).collect();
        app.jobs.thread(move || {
            let p = rfd::FileDialog::new().set_title("Guardar notas").set_file_name(format!("{} - notas.md", name.trim())).add_filter("Markdown", &["md"]).save_file()?;
            let ok = std::fs::write(&p, out).is_ok();
            Some(Box::new(move |a: &mut App| if ok { a.ui.toast("Notas guardadas") } else { a.ui.toast_err("No se pudo guardar el archivo") }) as Apply)
        });
    }
}

/// El papelito: sombra despegada abajo, banda de pegamento y texto.
fn paper_note(app: &mut App, n: &Note, r: Rect, lift: f32, text: bool) {
    let c = POSTITS[n.color as usize % POSTITS.len()];
    let d = &mut app.ui.d;
    if n.w < 0.0 {
        d.shadow(r.off(0.0, 3.0), 4.0, 10.0, Color::BLACK.a(0.35));
        d.grad(r, 3.0, c, c.mul(0.93), false);
        app.ui.icon(Icon::Note, r.cx(), r.cy(), 17.0, INK.a(0.75));
        if !n.text.is_empty() {
            app.ui.tooltip(r, &n.text);
        }
        return;
    }
    let k = 1.0 + lift * 0.6;
    d.shadow(rect(r.x + 4.0, r.y + 6.0, r.w - 8.0, r.h - 6.0).off(0.0, 4.0 * k), 3.0, 14.0 * k, Color::BLACK.a(0.28));
    d.shadow_rot(rect(r.x + 6.0, r.b() - 26.0, r.w * 0.5, 22.0), 4.0, 12.0 * k, -0.05, Color::BLACK.a(0.35));
    d.shadow_rot(rect(r.r() - 6.0 - r.w * 0.5, r.b() - 26.0, r.w * 0.5, 22.0), 4.0, 12.0 * k, 0.05, Color::BLACK.a(0.35));
    // el papel, sin la esquina que se dobla
    let s = 18.0;
    let (c0, c1) = (c, c.mul(0.945));
    d.grad(rect(r.x, r.y, r.w, r.h - s), 2.0, c0, c1, false);
    d.fill(rect(r.x, r.b() - s - 0.5, r.w - s, s + 0.5), 1.0, c1);
    d.grad(rect(r.x, r.y, r.w, 30.0), 2.0, Color::BLACK.a(0.055), Color::BLACK.a(0.02), false);
    let mk = |x: f32, y: f32, k: Color| MeshV { pos: [x, y, 0.0, 1.0], uv: [0.0; 2], local: [1.0; 2], size: [2.0; 2], color: k.arr(), radius: [0.0; 4], misc: [5.0, 1.0, 0.0, 0.0], clip: [0.0; 4] };
    let (x, y) = (r.r(), r.b());
    d.tris(TEX_WHITE, &[mk(x - s, y - s - 0.5, c1), mk(x, y - s - 0.5, c1), mk(x - s, y, c1)]);
    let (f0, f1) = (c.mul(0.80), c.mul(0.90));
    d.tris(TEX_WHITE, &[mk(x - s, y - s, f1), mk(x, y - s, f0), mk(x - s, y, f0)]);
    if text {
        let body = rect(r.x + 15.0, r.y + 34.0, r.w - 30.0, r.h - 46.0);
        let max = ((body.h / 20.0).floor() as usize).max(1);
        let lines: Vec<String> = n.text.split('\n').flat_map(|p| if p.is_empty() { vec![String::new()] } else { app.ui.t.wrap(fonts().ui, 14.0, p, body.w, 60) }).collect();
        for (i, l) in lines.iter().take(max).enumerate() {
            let s = if i + 1 == max && lines.len() > max { format!("{l}…") } else { l.clone() };
            app.ui.text(fonts().ui, 14.0, body.x, body.y + 10.0 + i as f32 * 20.3, &s, INK);
        }
        if n.text.is_empty() {
            app.ui.text(fonts().ui, 14.0, body.x, body.y + 10.0, "Toca para escribir", INK.a(0.42));
        }
    }
}
