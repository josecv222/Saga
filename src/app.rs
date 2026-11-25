//! Estado de la aplicación y reparto de cada fotograma.
use crate::book3d::{self, Look, Pose};
use crate::covers::{Covers, Size};
use crate::db::*;
use crate::gfx::*;
use crate::icons::Icon;
use crate::jobs::Jobs;
use crate::reader::{Opening, Reader};
use crate::text::fonts;
use crate::ui::*;
use crate::views::{editor::Editor, home, home::Home, panels::Panel, sheet::Sheet};
use std::collections::HashMap;
use winit::keyboard::NamedKey;
use winit::window::{CursorIcon, ResizeDirection};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum WinReq {
    Close,
    Minimize,
    Maximize,
    Drag,
    Resize(ResizeDirection),
    Fullscreen(bool),
}

pub struct App {
    pub gpu: Gpu,
    pub ui: Ui,
    pub db: Db,
    pub set: Settings,
    saved: String,
    pub jobs: Jobs,
    pub books: Vec<Book>,
    pub index: HashMap<i64, usize>,
    pub colls: Vec<Collection>,
    pub note_n: HashMap<i64, usize>,
    pub covers: Covers,
    pub off: Vec<(TexId, Draw, Color)>,
    pub home: Home,
    pub sheet: Option<Sheet>,
    pub editor: Option<Editor>,
    pub panel: Option<Panel>,
    pub opening: Option<Opening>,
    pub reader: Option<Reader>,
    pub confirm: Option<Confirm>,
    pub win: Vec<WinReq>,
    pub maximized: bool,
    pub fullscreen: bool,
    pub dialog: bool,
    pub import_done: usize,
    pub import_total: usize,
    pub drop_hover: bool,
    drop_t: f32,
    pub headless: bool,
}
pub struct Confirm {
    title: String,
    body: String,
    yes: String,
    act: Option<Box<dyn FnOnce(&mut App)>>,
    t: f32,
    closing: bool,
}

pub const STRIP: f32 = 38.0;

impl App {
    pub fn new(gpu: Gpu, db: Db, set: Settings, jobs: Jobs, w: f32, h: f32, dpi: f32) -> App {
        let mut ui = Ui::new(w, h, dpi);
        ui.set_theme(set.theme, true);
        ui.calm = set.calm;
        let books = db.books();
        let mut a = App {
            gpu,
            ui,
            saved: serde_json::to_string(&set).unwrap_or_default(),
            set,
            jobs,
            books,
            index: HashMap::new(),
            colls: db.collections(),
            note_n: db.note_counts(),
            db,
            covers: Covers::default(),
            off: vec![],
            home: Home::new(),
            sheet: None,
            editor: None,
            panel: None,
            opening: None,
            reader: None,
            confirm: None,
            win: vec![],
            maximized: false,
            fullscreen: false,
            dialog: false,
            import_done: 0,
            import_total: 0,
            drop_hover: false,
            drop_t: 0.0,
            headless: false,
        };
        a.reindex();
        a
    }
    pub fn reindex(&mut self) {
        self.index = self.books.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
    }
    pub fn book(&self, id: i64) -> Option<&Book> {
        self.index.get(&id).map(|i| &self.books[*i])
    }
    pub fn book_mut(&mut self, id: i64) -> Option<&mut Book> {
        self.index.get(&id).map(|i| &mut self.books[*i])
    }
    pub fn edit_book(&mut self, id: i64, f: impl FnOnce(&mut Book)) {
        if let Some(b) = self.book_mut(id) {
            f(b);
            let b = b.clone();
            self.db.save(&b);
            self.home.dirty = true;
            self.ui.redraw = true;
        }
    }
    pub fn set_status(&mut self, id: i64, s: Status) {
        self.edit_book(id, |b| {
            b.status = s;
            if s == Status::Done {
                b.progress = 1.0;
                b.finished = now();
            }
        });
    }
    pub fn remove_book(&mut self, id: i64) {
        self.db.delete(id);
        self.covers.forget(&mut self.gpu, id);
        self.books.retain(|b| b.id != id);
        for c in &mut self.colls {
            c.books.retain(|b| *b != id);
        }
        self.reindex();
        self.home.dirty = true;
        if self.sheet.as_ref().is_some_and(|s| s.id == id) {
            self.sheet = None;
        }
        self.ui.toast("Libro quitado de la biblioteca");
    }
    pub fn ask(&mut self, title: &str, body: &str, yes: &str, act: impl FnOnce(&mut App) + 'static) {
        self.confirm = Some(Confirm { title: title.into(), body: body.into(), yes: yes.into(), act: Some(Box::new(act)), t: 0.0, closing: false });
        self.ui.redraw = true;
    }
    pub fn ask_remove(&mut self, id: i64) {
        let Some(b) = self.book(id) else { return };
        let t = format!("¿Quitar «{}»?", b.title);
        self.ask(&t, "Se borran sus notas, subrayados y progreso. El archivo del libro se queda donde está.", "Quitar", move |a| a.remove_book(id));
    }
    pub fn look(&mut self, i: usize, size: Size) -> Look {
        let b = &self.books[i];
        let time = self.ui.time;
        let tex = match size {
            Size::Full => self.covers.best(&self.jobs, b, time),
            s => self.covers.get(&self.jobs, b, s, time),
        };
        if tex.is_some_and(|t| t.1 < 1.0) {
            self.ui.redraw = true;
        }
        let img = if b.cover_ver > 0 { b.aspect } else { 2.0 / 3.0 };
        Look {
            tex,
            img_aspect: img,
            aspect: book3d::book_aspect(img),
            thick: book3d::thickness(b.pages),
            color: Color::hex(if b.cover_ver > 0 { b.color } else { crate::covers::gen_color(&b.title) }),
            title: b.title.clone(),
            author: b.author_list().first().map(|s| s.to_string()).unwrap_or_default(),
            dim: 1.0,
            spine_text: true,
            paper: Color::hex(0xE2D8C4),
            inner: [None, None],
            lid: 1.0,
        }
    }
    /// Libro que se está mostrando fuera de la vista (ficha o apertura).
    pub fn lifted(&self, id: i64) -> bool {
        self.sheet.as_ref().is_some_and(|s| s.id == id) || self.opening.as_ref().is_some_and(|o| o.id == id)
    }
    pub fn home_pose(&self, id: i64) -> (Pose, (f32, f32)) {
        self.home.poses.get(&id).copied().unwrap_or((Pose { cx: self.ui.w / 2.0, cy: self.ui.h * 0.45, h: 120.0, yaw: 0.4, pitch: 0.0, z: 0.0, open: 0.0 }, (self.ui.w / 2.0, self.ui.h * 0.45)))
    }

    pub fn frame(&mut self, w: f32, h: f32, dpi: f32, dt: f32) -> bool {
        self.ui.begin(w, h, dpi, dt);
        self.ui.calm = self.set.calm;
        self.ui.th = self.ui.base_th();
        self.ui.set_theme(self.set.theme, false);
        while let Some(a) = self.jobs.next() {
            a(self);
            self.ui.redraw = true;
        }
        let time = self.ui.time;
        if self.covers.frame(&mut self.gpu, time) {
            self.ui.redraw = true;
        }
        self.off.clear();
        let base = self.ui.enabled;
        let modal = self.editor.is_some() || self.panel.is_some() || self.confirm.is_some();
        let reading = self.reader.as_ref().is_some_and(|r| r.covers_all());
        let th = self.ui.th;
        if !reading {
            self.ui.enabled = base && !modal && self.sheet.as_ref().is_none_or(|s| s.closing) && self.opening.is_none() && self.reader.is_none();
            home::frame(self);
            self.ui.enabled = base && !modal && self.opening.is_none() && self.reader.is_none();
            if let Some(mut s) = self.sheet.take() {
                if s.frame(self) && self.sheet.is_none() {
                    self.sheet = Some(s);
                }
            }
        }
        self.ui.enabled = base && self.confirm.is_none();
        if let Some(mut e) = self.editor.take() {
            if e.frame(self) {
                self.editor = Some(e);
            }
        }
        if let Some(mut p) = self.panel.take() {
            if p.frame(self) && self.panel.is_none() {
                self.panel = Some(p);
            }
        }
        self.ui.enabled = base && self.confirm.is_none();
        crate::reader::frame(self);
        self.ui.enabled = base;
        self.confirm_frame();
        self.chrome();
        self.drop_frame();
        if self.import_total > 0 {
            self.import_pill();
        }
        self.ui.end();
        if self.ui.t.flush(&self.gpu) {
            self.ui.redraw = true;
        }
        if !self.ui.input.down {
            let s = serde_json::to_string(&self.set).unwrap_or_default();
            if s != self.saved {
                self.db.save_settings(&self.set);
                self.saved = s;
            }
        }
        let off = std::mem::take(&mut self.off);
        let clear = match &self.reader {
            Some(r) if reading => r.bg(self),
            _ => th.bg,
        };
        self.gpu.render(&off, &self.ui.d, clear, time % 1000.0);
        self.ui.redraw
    }
    pub fn shutdown(&mut self) {
        if let Some(mut r) = self.reader.take() {
            r.save(self);
        }
        self.db.save_settings(&self.set);
    }
    fn import_pill(&mut self) {
        let th = self.ui.th;
        let s = format!("Añadiendo {} de {}…", self.import_done.min(self.import_total), self.import_total);
        let w = self.ui.t.measure(fonts().ui, 13.0, &s) + 64.0;
        let r = rect(self.ui.w - w - 24.0, self.ui.h - 64.0, w, 40.0);
        self.ui.d.shadow(r.off(0.0, 6.0), 20.0, 24.0, Color::BLACK.a(0.4));
        self.ui.d.boxed(r, 20.0, th.raised, 1.0, th.line);
        let p = self.import_done as f32 / self.import_total.max(1) as f32;
        let p = self.ui.anim(id("import"), p, 10.0);
        self.ui.d.arc(rect(r.x + 12.0, r.cy() - 10.0, 20.0, 20.0), 3.0, p.max(0.02), th.accent, th.text.a(0.12));
        self.ui.text(fonts().ui, 13.0, r.x + 44.0, r.cy(), &s, th.text);
    }
    fn drop_frame(&mut self) {
        let open = self.drop_hover;
        let mut t = self.drop_t;
        let e = ease_out(self.ui.tween(&mut t, open, 0.2));
        self.drop_t = t;
        if t <= 0.0 {
            return;
        }
        let th = self.ui.th;
        let f = self.ui.full();
        self.ui.d.push_alpha(e);
        self.ui.d.fill(f, 0.0, th.bg.a(0.82));
        let r = f.inset(28.0);
        self.ui.d.stroke(r, 28.0, 2.0, th.accent.a(0.7));
        self.ui.icon(Icon::Book, f.cx(), f.cy() - 40.0, 64.0, th.accent);
        self.ui.text_c(fonts().title, 32.0, f.cx(), f.cy() + 30.0, "Suelta aquí tus libros", th.text);
        self.ui.d.pop_alpha();
    }
    fn confirm_frame(&mut self) {
        let Some(mut c) = self.confirm.take() else { return };
        let open = !c.closing;
        let t = self.ui.tween(&mut c.t, open, 0.18);
        if c.closing && t <= 0.0 {
            return;
        }
        let th = self.ui.th;
        let e = ease_out(t);
        let f = self.ui.full();
        self.ui.enabled = open;
        self.ui.d.push_alpha(e);
        self.ui.d.fill(f, 0.0, Color::BLACK.a(0.5));
        let lines = self.ui.t.wrap(fonts().ui, 14.5, &c.body, 380.0, 6);
        let hh = 150.0 + lines.len() as f32 * 22.0;
        let r = rect(f.cx() - 220.0, f.cy() - hh / 2.0, 440.0, hh);
        self.ui.d.push_xf(0.96 + 0.04 * e, r.cx(), r.cy(), 0.0, 0.0);
        self.ui.card(r, 22.0);
        self.ui.text_fit(fonts().title, 23.0, r.x + 30.0, r.y + 40.0, &c.title, r.w - 60.0, th.text);
        for (i, l) in lines.iter().enumerate() {
            self.ui.text(fonts().ui, 14.5, r.x + 30.0, r.y + 78.0 + i as f32 * 22.0, l, th.text2);
        }
        let yes = self.ui.button(id("cf-yes"), rect(r.r() - 150.0, r.b() - 62.0, 120.0, 42.0), Btn::Danger, None, &c.yes) || (open && self.ui.key(NamedKey::Enter));
        let no = self.ui.button(id("cf-no"), rect(r.r() - 280.0, r.b() - 62.0, 120.0, 42.0), Btn::Soft, None, "Cancelar") || (open && self.ui.key(NamedKey::Escape)) || (self.ui.input.pressed && !r.has(self.ui.input.pos) && open);
        self.ui.d.pop_xf();
        self.ui.d.pop_alpha();
        if open && (yes || no) {
            c.closing = true;
            if yes {
                if let Some(f) = c.act.take() {
                    f(self);
                }
            }
        }
        if self.confirm.is_none() {
            self.confirm = Some(c);
        }
    }
    /// Botones de ventana, zona de arrastre y bordes para redimensionar.
    fn chrome(&mut self) {
        let w = self.ui.w;
        let h = self.ui.h;
        let (vis, fg) = match &self.reader {
            Some(r) if r.covers_all() => (r.chrome_alpha(), r.ink(self)),
            _ => (1.0, self.ui.th.text),
        };
        if vis > 0.01 && !self.fullscreen {
            self.ui.d.push_alpha(vis);
            let mut x = w - 46.0;
            for (k, ic, req) in [("wx", Icon::Close, WinReq::Close), ("wm", if self.maximized { Icon::Restore } else { Icon::Max }, WinReq::Maximize), ("wn", Icon::Min, WinReq::Minimize)] {
                let r = rect(x, 0.0, 46.0, STRIP - 4.0);
                let i = id(k);
                let c = self.ui.click(i, r);
                let over = self.ui.over(r);
                let a = self.ui.anim(i, over as i32 as f32, 18.0);
                let red = k == "wx";
                self.ui.d.fill(r, 0.0, if red { Color::hex(0xC4382B).a(a) } else { fg.a(0.10 * a) });
                let col = if red { fg.a(0.75).mix(Color::WHITE, a) } else { fg.a(0.75 + 0.25 * a) };
                self.ui.icon(ic, r.cx(), r.cy(), 15.0, col);
                if c {
                    self.win.push(req);
                }
                x -= 46.0;
            }
            self.ui.d.pop_alpha();
        }
        // arrastrar por la franja superior
        let strip = rect(0.0, 0.0, w - 46.0 * 3.0, STRIP);
        if !self.fullscreen && self.ui.hover(strip) && self.ui.input.pressed && self.ui.active.is_none() {
            if self.ui.input.clicks >= 2 {
                self.win.push(WinReq::Maximize);
            } else {
                self.win.push(WinReq::Drag);
            }
            self.ui.input.pressed = false;
        }
        if !self.maximized && !self.fullscreen && self.ui.active.is_none() {
            let m = 5.0;
            let (x, y) = self.ui.input.pos;
            let (l, r, t, b) = (x < m, x > w - m, y < m, y > h - m);
            use ResizeDirection::*;
            let dir = match (l, r, t, b) {
                (true, _, true, _) => Some((NorthWest, CursorIcon::NwseResize)),
                (_, true, true, _) => Some((NorthEast, CursorIcon::NeswResize)),
                (true, _, _, true) => Some((SouthWest, CursorIcon::NeswResize)),
                (_, true, _, true) => Some((SouthEast, CursorIcon::NwseResize)),
                (true, ..) => Some((West, CursorIcon::EwResize)),
                (_, true, ..) => Some((East, CursorIcon::EwResize)),
                (_, _, true, _) => Some((North, CursorIcon::NsResize)),
                (_, _, _, true) => Some((South, CursorIcon::NsResize)),
                _ => None,
            };
            if let Some((d, c)) = dir {
                if x >= 0.0 && y >= 0.0 && x <= w && y <= h {
                    self.ui.cursor = c;
                    if self.ui.input.pressed {
                        self.win.push(WinReq::Resize(d));
                    }
                }
            }
        }
    }
}
