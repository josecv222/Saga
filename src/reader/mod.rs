//! Lector: modos de página, desplazamiento, paso de hoja y posición.
pub mod bars;
pub mod content;
pub mod curl;
pub mod find;
pub mod marks;
pub mod side;
pub mod tts;

use crate::app::App;
use crate::book3d::{self, Pose};
use crate::covers::Size;
use crate::db::*;
use crate::doc::floor;
use crate::formats::{self, Opened};
use crate::gfx::*;
use crate::jobs::Apply;
use crate::layout::Cfg;
use crate::text::fonts;
use crate::theme::{Theme, PAPERS};
use crate::ui::*;
use content::*;
use std::time::Instant;
use winit::keyboard::NamedKey;
use winit::window::CursorIcon;

// ---------------------------------------------------------------- apertura
pub struct Opening {
    pub id: i64,
    loc: Option<i64>,
    t: f32,
    closing: bool,
    doc: Option<Result<Opened, String>>,
    from: (Pose, (f32, f32)),
    token: Instant,
    /// El lector ya compuesto: sus páginas se ven mientras el libro se abre.
    reader: Option<Box<Reader>>,
    /// Las últimas páginas, para cerrarlo con ellas.
    texs: Vec<TexId>,
    /// Hoja del lector: ancho, alto y centro.
    dims: Option<(f32, f32, f32)>,
}

impl App {
    pub fn open_book(&mut self, id: i64) {
        self.open_at(id, None);
    }
    pub fn open_book_at(&mut self, id: i64, loc: i64) {
        self.open_at(id, Some(loc));
    }
    fn open_at(&mut self, id: i64, loc: Option<i64>) {
        if self.opening.is_some() || self.reader.is_some() {
            return;
        }
        let Some(b) = self.book(id) else { return };
        let path = b.path.clone();
        if !std::path::Path::new(&path).exists() {
            self.relocate(id);
            return;
        }
        let from = match &self.sheet {
            Some(s) if s.id == id => s.pose(self),
            _ => self.home_pose(id),
        };
        let token = Instant::now();
        self.opening = Some(Opening { id, loc, t: 0.0, closing: false, doc: None, from, token, reader: None, texs: vec![], dims: None });
        self.ui.redraw = true;
        self.jobs.run(0, move || {
            let r = std::panic::catch_unwind(|| formats::open(std::path::Path::new(&path))).unwrap_or_else(|_| Err("El archivo está dañado".into()));
            Some(Box::new(move |a: &mut App| {
                if let Some(o) = a.opening.as_mut().filter(|o| o.token == token) {
                    o.doc = Some(r);
                }
            }) as Apply)
        });
    }
    /// El archivo se movió: se busca el nuevo sitio.
    fn relocate(&mut self, id: i64) {
        self.ask("No se encuentra el archivo", "Puede que se haya movido o borrado. ¿Quieres indicar dónde está ahora?", "Buscar", move |a| {
            a.jobs.thread(move || {
                let p = rfd::FileDialog::new().set_title("¿Dónde está el libro?").add_filter("Libros", formats::EXTS).pick_file()?;
                Some(Box::new(move |a: &mut App| {
                    a.edit_book(id, |b| {
                        b.path = p.to_string_lossy().to_string();
                        b.missing = false;
                    });
                    a.open_book(id);
                }) as Apply)
            });
        });
    }
}

/// El lector abierto o el que se está abriendo.
pub fn live<'a>(r: &'a mut Option<Reader>, o: &'a mut Option<Opening>) -> Option<&'a mut Reader> {
    match r {
        Some(r) => Some(r),
        None => o.as_mut().and_then(|o| o.reader.as_deref_mut()),
    }
}
pub fn frame(app: &mut App) {
    if let Some(mut o) = app.opening.take() {
        let keep = opening(app, &mut o);
        if keep {
            app.opening = Some(o);
        } else {
            for t in o.texs.drain(..) {
                app.gpu.drop_tex(t);
            }
        }
    }
    if let Some(mut r) = app.reader.take() {
        if r.frame(app) {
            app.reader = Some(r);
        } else {
            r.content.free(&mut app.gpu);
            r.free(app);
        }
    }
}
fn opening(app: &mut App, o: &mut Opening) -> bool {
    let calm = app.set.calm;
    let secs = if o.closing { 0.56 } else { 0.78 };
    // al cerrar, espera a que el lector se desvanezca
    let hold = o.closing && o.texs.is_empty() && app.reader.as_ref().is_some_and(|r| r.t > 0.35);
    if !hold {
        let open = !o.closing;
        let mut t = o.t;
        app.ui.tween(&mut t, open, secs);
        o.t = t;
    } else {
        app.ui.redraw = true;
    }
    if o.closing && o.t <= 0.0 {
        return false;
    }
    if app.reader.is_some() && !o.closing && app.reader.as_ref().is_some_and(|r| r.covers_all()) {
        return false;
    }
    let Some(bi) = app.index.get(&o.id).copied() else { return false };
    let full = app.ui.full();
    let ink = ink(app);
    let t = o.t;
    let fly = ease_io((t / 0.62).min(1.0));
    let open = smooth(((t - 0.36) / 0.64).clamp(0.0, 1.0));
    app.ui.d.push_alpha(smooth((t / 0.7).min(1.0)));
    app.ui.d.fill(full, 0.0, desk(&ink));
    app.ui.d.pop_alpha();
    let mut l = app.look(bi, Size::Full);
    l.paper = l.paper.mix(ink.bg.mul(1.0 / 0.96), smooth((t / 0.40).min(1.0)));
    // el libro abierto coincide con las hojas del lector
    let fixed = app.books[bi].format == "pdf" || app.books[bi].format.starts_with("cb") || app.books[bi].format == "djvu";
    let mode = if fixed { app.set.fixed_mode } else { app.set.mode };
    let paged = matches!(mode, ReadMode::Single | ReadMode::Double);
    if !o.closing && paged && o.reader.is_none() && app.reader.is_none() && o.t < 0.5 {
        match o.doc.take() {
            Some(Ok(d)) => o.reader = Some(Box::new(Reader::new(app, o.id, d, o.loc))),
            x => o.doc = x,
        }
    }
    if let Some(r) = &mut o.reader {
        r.bake(app, mode);
    }
    if !o.closing && paged {
        if let Some(r) = o.reader.as_deref().or(app.reader.as_ref()).filter(|r| r.texs.len() > 1) {
            let s = r.geo(app, mode, None).sheets[0];
            o.dims = Some((s.w, s.h, s.cy()));
            l.inner = [Some(r.texs[0]), Some(r.texs[1])];
        }
    }
    if o.closing && o.texs.len() > 1 {
        l.inner = [Some(o.texs[0]), Some(o.texs[1])];
    }
    if mode != ReadMode::Double {
        l.inner[0] = None;
        l.lid = 1.0 - smooth(((open - 0.5) / 0.45).clamp(0.0, 1.0));
    }
    let (pw, ph) = page_size(app, if mode == ReadMode::ScrollV { ReadMode::Single } else { mode }, full.w, full.h);
    let (pw, ph, cy) = o.dims.unwrap_or((pw, ph, full.h / 2.0));
    let h = ph * (1.0 - 0.19 * (std::f32::consts::PI * open).sin());
    let double = mode == ReadMode::Double;
    if !fixed || o.dims.is_some() {
        l.aspect = lerp(l.aspect, pw / ph, smooth((t / 0.62).min(1.0)));
    }
    let w = ph * l.aspect;
    // con la tapa abierta el lomo queda en el centro
    let cx = if double { full.w / 2.0 + w / 2.0 * open } else { full.w / 2.0 + w * 0.5 * (std::f32::consts::PI * open).sin() * 0.4 };
    let to = Pose { cx, cy, h, yaw: 0.0, pitch: 0.0, z: -h * l.thick * 0.5 * open, open };
    let (from, vpf) = match (&app.sheet, o.closing) {
        (Some(s), true) if s.id == o.id => s.pose(app),
        (None, true) => app.home_pose(o.id),
        _ => o.from,
    };
    let mut p = from.lerp(&to, fly);
    p.open = open * book3d::FLAT;
    let vp = (lerp(vpf.0, full.cx(), fly), lerp(vpf.1, full.cy(), fly));
    book3d::draw(&mut app.ui, &p, &l, vp);
    if o.closing {
        return true;
    }
    if t >= 1.0 && o.doc.is_none() && o.reader.is_none() && app.reader.is_none() {
        crate::views::editor::spinner(app, full.cx() + if double { w / 2.0 } else { 0.0 }, full.cy(), 40.0);
    }
    if app.reader.is_none() && t >= if calm { 0.5 } else if o.reader.is_some() { 1.0 } else { 0.94 } {
        let ready = match o.reader.take() {
            Some(mut r) => {
                r.t = 1.0;
                Some(Ok(*r))
            }
            None => match o.doc.take() {
                Some(Ok(d)) => Some(Ok(Reader::new(app, o.id, d, o.loc))),
                Some(Err(e)) => Some(Err(e)),
                None => None,
            },
        };
        match ready {
            Some(Ok(mut r)) => {
                r.start(app);
                app.reader = Some(r);
                app.ui.focus = None;
                let id = o.id;
                app.covers.trim(&mut app.gpu, id);
            }
            Some(Err(e)) => {
                app.ui.toast_err(e);
                o.closing = true;
            }
            None => {}
        }
    }
    true
}

// ---------------------------------------------------------------- colores
pub const VPAD: f32 = 24.0;
pub fn ink(app: &App) -> Ink {
    let p = PAPERS[app.set.paper.min(PAPERS.len() - 1)];
    let b = app.set.brightness.clamp(0.35, 1.0);
    let w = app.set.warmth;
    let f = |c: Color| Color(c.0 * b, c.1 * b * (1.0 - 0.10 * w), c.2 * b * (1.0 - 0.32 * w), c.3);
    let accent = if p.dark { if app.ui.base_th().dark { app.ui.base_th().accent } else { Color::hex(0xE8C08A) } } else if app.ui.base_th().dark { Color::hex(0x8F4F22) } else { app.ui.base_th().accent };
    let tint = f(if p.dark { Color(0.90, 0.90, 0.90, 1.0) } else { Color::WHITE });
    Ink { bg: f(p.bg), ink: f(p.ink), accent: f(accent), dark: p.dark, tint }
}
/// La mesa sobre la que descansa el libro.
pub fn desk(i: &Ink) -> Color {
    if i.dark {
        if i.bg.lum() < 0.02 { Color::hex(0x000000) } else { i.bg.mul(0.62) }
    } else {
        i.bg.mul(0.86)
    }
}
fn theme_of(i: &Ink) -> Theme {
    let raised = if i.dark { i.bg.mix(Color::WHITE, 0.10) } else { i.bg.mix(Color::WHITE, 0.55) };
    Theme {
        name: "",
        dark: i.dark,
        bg: i.bg,
        panel: if i.dark { i.bg.mix(Color::WHITE, 0.045) } else { i.bg.mix(Color::WHITE, 0.35) },
        raised,
        line: i.ink.a(0.12),
        text: i.ink,
        text2: i.ink.mix(i.bg, 0.22),
        text3: i.ink.mix(i.bg, 0.42),
        accent: i.accent,
        on_accent: if i.accent.lum() > 0.5 { Color::hex(0x1B160E) } else { Color::hex(0xFFF8EC) },
        glow: 0.0,
    }
}

// ---------------------------------------------------------------- lector
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tab {
    Toc,
    Notes,
    Marks,
    Find,
}
pub struct Turning {
    /// Anterior y siguiente; el avance va de uno a otro.
    a: Addr,
    b: Addr,
    fwd: bool,
    t: f32,
    v: f32,
    drag: bool,
    /// Progreso al que va al soltar.
    goal: f32,
    drawn: bool,
    tilt: f32,
    /// Desde dónde y cuánto lleva el tramo sin mano.
    from: f32,
    el: f32,
    /// Tras soltar ya va lanzada: solo frena.
    thrown: bool,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Press {
    Text { start: i64, moved: bool },
    Link,
    Margin { x: f32, y: f32, moved: bool },
    Pan { x: f32, y: f32, px: f32, py: f32 },
    Other,
}
pub struct Reader {
    pub id: i64,
    pub title: String,
    pub author: String,
    pub content: Content,
    pub t: f32,
    pub closing: bool,
    pub addr: Addr,
    /// Posición de lectura: ancla de todo lo demás.
    pub loc: i64,
    // desplazamiento: elemento y distancia dentro de él
    pub sc: usize,
    pub sy: Spring,
    pub sy_t: f32,
    hx: Spring,
    hx_t: f32,
    pub turning: Option<Turning>,
    texs: Vec<TexId>,
    tex_size: (u32, u32),
    pub zoom: f32,
    zoom_to: f32,
    pan: (f32, f32),
    pub placed: Vec<Placed>,
    pub press: Option<Press>,
    // interfaz
    pub pinned: bool,
    pub bar: f32,
    pub last_move: f32,
    pub side: Option<Tab>,
    pub side_t: f32,
    pub side_tab: Tab,
    pub side_scroll: Scroll,
    pub style_open: bool,
    pub style_t: f32,
    pub style_scroll: Scroll,
    pub find: find::Find,
    // marcas
    pub notes: Vec<Note>,
    pub sel: Option<(i64, i64)>,
    pub sel_done: bool,
    pub hot: i64,
    pub pop_t: f32,
    pub edit_note: Option<(i64, Edit)>,
    pub placing: bool,
    pub drag_note: Option<(i64, f32, f32, bool)>,
    pub resize_note: Option<(i64, f32, f32, f32, f32)>,
    pub ctx_loc: i64,
    pub ctx_pos: (f32, f32),
    pub history: Vec<i64>,
    pub auto: bool,
    pub tts: Option<tts::Tts>,
    pub speak: Option<(i64, i64)>,
    pub toc: Vec<crate::doc::Toc>,
    /// Trozo destacado un momento (tras buscar).
    pub flash: Option<(i64, i64, f32)>,
    begun: i64,
    secs: i64,
    tick: Instant,
    saved: Instant,
    dirty: bool,
    w_h: (f32, f32),
    mode_seen: Option<ReadMode>,
    /// Fundido al cambiar de modo (1 = hecho).
    swap: f32,
    wheel_acc: f32,
    wheel_at: f32,
    pub slider_drag: Option<f32>,
    start_t: f32,
    full_before: bool,
    /// Este clic solo quita la selección.
    calm_tap: bool,
    /// Cerrándose con las páginas puestas en el libro.
    ghost: bool,
}

pub(super) struct Geo {
    pub sheets: Vec<Rect>,
    pub text: Vec<Rect>,
}

impl Reader {
    pub fn new(app: &mut App, id: i64, d: Opened, at: Option<i64>) -> Reader {
        let b = app.book(id).cloned().unwrap_or_default();
        let loc = at.unwrap_or(b.loc).max(0);
        let content = match d {
            Opened::Flow(d) => Content::Flow(FlowView::new(d, Cfg { w: 1.0, h: 1.0, family: 0, size: 20.0, leading: 1.5, para: 0.0, justify: true, hyphen: true })),
            Opened::Fixed(d) => Content::Fixed(FixedView::new(d)),
        };
        let toc = content.toc();
        let notes = app.db.notes(id);
        let mut r = Reader {
            id,
            title: b.title.clone(),
            author: b.author_line(),
            content,
            t: 0.0,
            closing: false,
            addr: Addr::default(),
            loc,
            sc: 0,
            sy: Spring::default(),
            sy_t: 0.0,
            hx: Spring::default(),
            hx_t: 0.0,
            turning: None,
            texs: vec![],
            tex_size: (0, 0),
            zoom: 1.0,
            zoom_to: 1.0,
            pan: (0.0, 0.0),
            placed: vec![],
            press: None,
            pinned: false,
            bar: 1.0,
            last_move: app.ui.time,
            side: None,
            side_t: 0.0,
            side_tab: Tab::Toc,
            side_scroll: Scroll::default(),
            style_open: false,
            style_t: 0.0,
            style_scroll: Scroll::default(),
            find: find::Find::default(),
            notes,
            sel: None,
            sel_done: false,
            hot: 0,
            pop_t: 0.0,
            edit_note: None,
            placing: false,
            drag_note: None,
            resize_note: None,
            ctx_loc: 0,
            ctx_pos: (0.0, 0.0),
            history: vec![],
            auto: false,
            tts: None,
            speak: None,
            toc,
            flash: None,
            begun: now(),
            secs: 0,
            tick: Instant::now(),
            saved: Instant::now(),
            dirty: false,
            w_h: (0.0, 0.0),
            mode_seen: None,
            swap: 1.0,
            wheel_acc: 0.0,
            wheel_at: 0.0,
            slider_drag: None,
            start_t: app.ui.time,
            full_before: false,
            calm_tap: false,
            ghost: false,
        };
        let (fw, fh) = (app.ui.w, app.ui.h);
        let mode = r.mode(app);
        r.w_h = (fw, fh);
        if let Content::Flow(v) = &mut r.content {
            let cfg = cfg_for(app, mode, fw, fh);
            v.cfg = cfg.clone();
            let cur = v.doc.chapter_at(loc as usize);
            v.compose(&app.jobs, cfg, cur, id);
            // lo primero que se verá, ya
            v.flow(cur);
        }
        r.mode_seen = Some(mode);
        r.seek(loc);
        r
    }
    /// Empieza la lectura de verdad.
    fn start(&mut self, app: &mut App) {
        self.start_t = app.ui.time;
        self.last_move = app.ui.time;
        self.bar = 0.0;
        self.tick = Instant::now();
        self.begun = now();
        app.edit_book(self.id, |b| {
            b.opened = now();
            if matches!(b.status, Status::ToRead | Status::Paused) {
                b.status = Status::Reading;
            }
        });
    }
    fn targets(&mut self, app: &mut App, sz: (u32, u32)) {
        if self.tex_size != sz || self.texs.len() < 4 {
            for t in self.texs.drain(..) {
                app.gpu.drop_tex(t);
            }
            self.tex_size = sz;
            for _ in 0..4 {
                self.texs.push(app.gpu.create_target(sz.0.max(1), sz.1.max(1)));
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn bake_one(&mut self, app: &mut App, k: usize, ad: Option<Addr>, s: Rect, tx: Rect, side: i32, ik: &Ink) {
        let sz = self.tex_size;
        let mut d = Draw::new(sz.0 as f32, sz.1 as f32, app.ui.d.dpi);
        d.push_xf(1.0, 0.0, 0.0, -s.x, -s.y);
        std::mem::swap(&mut app.ui.d, &mut d);
        self.draw_page(app, ad, s, tx, ik, side, false);
        self.page_notes(app, ad, s, tx, ik);
        std::mem::swap(&mut app.ui.d, &mut d);
        app.off.push((self.texs[k], d, desk(ik)));
    }
    /// Lo que se ve ahora, a textura: 0 izquierda, 1 derecha (o única).
    fn bake(&mut self, app: &mut App, mode: ReadMode) {
        let ik = ink(app);
        let g = self.geo(app, mode, None);
        let dpi = app.ui.d.dpi;
        self.targets(app, ((g.sheets[0].w * dpi).round() as u32, (g.sheets[0].h * dpi).round() as u32));
        if mode == ReadMode::Double {
            let s = self.content.spread(self.addr);
            self.bake_one(app, 0, s[0], g.sheets[0], g.text[0], -1, &ik);
            self.bake_one(app, 1, s[1], g.sheets[1], g.text[1], 1, &ik);
        } else {
            self.bake_one(app, 1, Some(self.addr), g.sheets[0], g.text[0], 0, &ik);
        }
    }
    pub fn mode(&self, app: &App) -> ReadMode {
        if self.content.is_flow() { app.set.mode } else { app.set.fixed_mode }
    }
    pub fn covers_all(&self) -> bool {
        self.t >= 1.0 && !self.closing
    }
    pub fn bg(&self, app: &App) -> Color {
        desk(&ink(app))
    }
    pub fn ink(&self, app: &App) -> Color {
        ink(app).ink
    }
    pub fn chrome_alpha(&self) -> f32 {
        self.bar
    }
    pub fn strength(&self, app: &App) -> f32 {
        match &self.content {
            Content::Fixed(v) if v.doc.is_comic() => app.set.recolor_comics as i32 as f32,
            Content::Fixed(_) => app.set.recolor as i32 as f32,
            _ => 1.0,
        }
    }
    /// Se recompuso el texto: todo se vuelve a anclar a la posición.
    pub fn relaid(&mut self) {
        self.turning = None;
        if self.swap < 1.0 {
            self.swap = 0.0;
        }
        let l = self.loc;
        self.seek(l);
    }
    /// Va directo a una posición.
    pub fn seek(&mut self, loc: i64) {
        let max = match &self.content {
            Content::Flow(v) => v.doc.len as i64 - 1,
            Content::Fixed(v) => fixed_loc(v.sizes.len() - 1, 0xFFFFF),
        };
        self.loc = loc.clamp(0, max.max(0));
        let l = self.loc;
        self.addr = self.content.addr_of(l);
        self.hx = Spring::at(0.0);
        self.hx_t = 0.0;
        match &mut self.content {
            Content::Flow(v) => {
                let c = self.addr.c;
                let f = v.flow(c);
                let li = f.line_of(l as u32);
                self.sc = c;
                let y = f.lines.get(li).map_or(0.0, |x| if li == 0 { -TOP_PAD } else { x.y - 70.0 });
                self.sy = Spring::at(y);
            }
            Content::Fixed(_) => {
                self.sc = self.addr.p;
                self.sy = Spring::at(if self.sc == 0 { -20.0 } else { -14.0 });
            }
        }
        self.sy_t = self.sy.x;
        self.pan = (0.0, 0.0);
    }
    /// Salto con vuelta atrás.
    pub fn jump(&mut self, loc: i64) {
        self.history.push(self.loc);
        if self.history.len() > 50 {
            self.history.remove(0);
        }
        self.turning = None;
        self.seek(loc);
        self.touch();
    }
    pub fn touch(&mut self) {
        self.dirty = true;
    }
    pub fn save(&mut self, app: &mut App) {
        self.count();
        let p = self.progress();
        let (loc, secs) = (self.loc, self.secs);
        let sitting = self.secs;
        let _ = sitting;
        if let Some(b) = app.book_mut(self.id) {
            b.loc = loc;
            b.opened = now();
            if b.status == Status::Done {
                b.progress = b.progress.max(p);
            } else {
                b.progress = p;
            }
            if p >= 0.995 && b.status != Status::Done {
                b.status = Status::Done;
                b.finished = now();
                b.progress = 1.0;
            }
            b.secs += secs;
            let b = b.clone();
            app.db.save(&b);
        }
        if secs >= 5 {
            app.db.add_session(self.id, self.begun, secs);
        }
        self.begun = now();
        self.secs = 0;
        self.dirty = false;
        self.saved = Instant::now();
        app.note_n.insert(self.id, self.notes.len());
    }
    fn count(&mut self) {
        // pausas largas no cuentan como lectura
        let d = self.tick.elapsed().as_secs();
        if d >= 1 {
            self.secs += d.min(120) as i64;
            self.tick = Instant::now();
        }
    }
    pub fn progress(&mut self) -> f32 {
        let (a, l) = (self.addr, self.loc);
        // la última página cuenta como fin
        let last = match &mut self.content {
            Content::Flow(v) => v.page_end(a) >= v.doc.len as i64 - 2,
            Content::Fixed(v) => a.p + 1 >= v.sizes.len(),
        };
        if last { 1.0 } else { self.content.progress(l).clamp(0.0, 0.994) }
    }
    pub fn free(&mut self, app: &mut App) {
        for t in self.texs.drain(..) {
            app.gpu.drop_tex(t);
        }
        if let Some(t) = &self.tts {
            t.stop();
        }
    }
    pub fn close(&mut self, app: &mut App) {
        if self.closing {
            return;
        }
        self.save(app);
        if let Some(t) = self.tts.take() {
            t.stop();
        }
        self.closing = true;
        app.home.dirty = true;
        let from = app.home_pose(self.id);
        let mode = self.mode(app);
        let (mut texs, mut dims) = (vec![], None);
        if matches!(mode, ReadMode::Single | ReadMode::Double) && self.zoom <= 1.01 {
            self.settle();
            self.sel = None;
            self.bake(app, mode);
            let s = self.geo(app, mode, None).sheets[0];
            dims = Some((s.w, s.h, s.cy()));
            texs = std::mem::take(&mut self.texs);
            self.ghost = true;
        }
        app.opening = Some(Opening { id: self.id, loc: None, t: 1.0, closing: true, doc: None, from, token: Instant::now(), reader: None, texs, dims });
        if app.fullscreen && !self.full_before {
            app.win.push(crate::app::WinReq::Fullscreen(false));
        }
        app.ui.focus = None;
    }

    // ------------------------------------------------------------ geometría
    pub(super) fn geo(&self, app: &App, mode: ReadMode, at: Option<Addr>) -> Geo {
        let (w, h) = (app.ui.w, app.ui.h);
        let dpi = app.ui.d.dpi;
        let sn = |v: f32| (v * dpi).round() / dpi;
        match &self.content {
            Content::Flow(_) => {
                let (pw, ph) = page_size(app, mode, w, h);
                let (mx, mt, mb) = margins(app, pw);
                let y = sn(VPAD);
                let mk = |x: f32| {
                    let s = rect(sn(x), y, sn(pw), sn(ph));
                    (s, rect(s.x + mx, s.y + mt, s.w - 2.0 * mx, s.h - mt - mb))
                };
                let v: Vec<(Rect, Rect)> = match mode {
                    ReadMode::Double => vec![mk(w / 2.0 - pw), mk(w / 2.0)],
                    _ => vec![mk(w / 2.0 - pw / 2.0)],
                };
                Geo { sheets: v.iter().map(|x| x.0).collect(), text: v.iter().map(|x| x.1).collect() }
            }
            Content::Fixed(v) => {
                let size = |a: Option<Addr>| a.and_then(|a| v.sizes.get(a.p)).copied();
                let a = at.unwrap_or(self.addr);
                let z = self.zoom;
                let ah = h - 2.0 * VPAD;
                if mode == ReadMode::Double {
                    let l = if a.p == 0 { None } else { Some(Addr { c: 0, p: (a.p - 1) / 2 * 2 + 1 }) };
                    let rr = Addr { c: 0, p: if a.p == 0 { 0 } else { l.unwrap().p + 1 } };
                    let base = size(l).or(size(Some(rr))).unwrap_or((600.0, 800.0));
                    let k = (ah / base.1).min((w - 40.0) / 2.0 / base.0) * z;
                    let (pw, ph) = (sn(base.0 * k), sn(base.1 * k));
                    let y = sn((h - ph) / 2.0 + self.pan.1);
                    let cx = sn(w / 2.0 + self.pan.0);
                    let s = vec![rect(cx - pw, y, pw, ph), rect(cx, y, pw, ph)];
                    Geo { sheets: s.clone(), text: s }
                } else {
                    let base = size(Some(a)).unwrap_or((600.0, 800.0));
                    let k = (ah / base.1).min((w - 60.0) / base.0) * z;
                    let (pw, ph) = (sn(base.0 * k), sn(base.1 * k));
                    let s = vec![rect(sn((w - pw) / 2.0 + self.pan.0), sn((h - ph) / 2.0 + self.pan.1), pw, ph)];
                    Geo { sheets: s.clone(), text: s }
                }
            }
        }
    }

    // ------------------------------------------------------------ fotograma
    fn frame(&mut self, app: &mut App) -> bool {
        let open = !self.closing;
        let mut t = self.t;
        app.ui.tween(&mut t, open, if open { 0.28 } else { 0.22 });
        self.t = t;
        if self.closing && t <= 0.0 {
            return false;
        }
        let ik = ink(app);
        app.ui.th = theme_of(&ik);
        let was = app.ui.enabled;
        app.ui.enabled = was && open;
        self.count();
        if self.saved.elapsed().as_secs() > 25 && self.dirty {
            self.save(app);
        }
        let time = app.ui.time;
        if app.ui.input.moved {
            self.last_move = time;
        }
        let mode = self.mode(app);
        let (w, h) = (app.ui.w, app.ui.h);
        // cambios de tamaño, letra o modo
        if let Content::Flow(v) = &mut self.content {
            let cfg = cfg_for(app, mode, w, h);
            let cur = v.doc.chapter_at(self.loc.max(0) as usize);
            let dragging = app.ui.input.down && app.ui.active.is_some();
            if !dragging || (w, h) != self.w_h {
                v.request(&app.jobs, cfg, cur, self.id);
            }
        }
        if self.mode_seen != Some(mode) || (w, h) != self.w_h {
            if self.mode_seen.is_some_and(|m| m != mode) {
                self.swap = 0.0;
            }
            self.mode_seen = Some(mode);
            self.w_h = (w, h);
            self.turning = None;
            let l = self.loc;
            self.seek(l);
        }
        // zoom suave
        if (self.zoom - self.zoom_to).abs() > 0.001 {
            let k = 1.0 - (-18.0 * app.ui.dt).exp();
            self.zoom += (self.zoom_to - self.zoom) * k;
            if (self.zoom - self.zoom_to).abs() < 0.002 {
                self.zoom = self.zoom_to;
            }
            app.ui.redraw = true;
        }
        if self.zoom_to <= 1.0 && mode != ReadMode::ScrollV {
            self.pan.0 *= 0.85;
            self.pan.1 *= 0.85;
        }
        app.ui.d.push_alpha(smooth(t));
        // al cerrarse, las páginas ya van en el libro: solo se retira la interfaz
        let ghost = self.closing && self.ghost;
        if ghost {
            self.bars(app, &ik);
            self.side_frame(app, &ik);
            self.style_frame(app);
            app.ui.d.pop_alpha();
            app.ui.enabled = was;
            return true;
        }
        app.ui.d.fill(app.ui.full(), 0.0, if mode == ReadMode::ScrollV && self.content.is_flow() { ik.bg } else { desk(&ik) });
        self.placed.clear();
        self.bars_logic(app);
        let ui_free = app.ui.enabled;
        // al cambiar de modo, lo nuevo aparece en un fundido breve
        let sw = smooth(self.swap);
        if self.swap < 1.0 {
            self.swap = (self.swap + app.ui.dt / 0.22).min(1.0);
            app.ui.redraw = true;
        }
        app.ui.d.push_alpha(sw);
        match mode {
            ReadMode::ScrollV => self.scroll_v(app, &ik),
            ReadMode::ScrollH => self.scroll_h(app, &ik),
            _ => self.paged(app, &ik, mode),
        }
        app.ui.d.pop_alpha();
        if let Content::Fixed(v) = &mut self.content {
            v.trim(&mut app.gpu, time);
        }
        if let Some((a, b, until)) = self.flash {
            let _ = (a, b);
            if time > until {
                self.flash = None;
                app.ui.redraw = true;
            } else {
                app.ui.wake = app.ui.wake.min(until - time + 0.01);
            }
        }
        // lo de encima decide antes que la página
        app.ui.enabled = ui_free;
        let over_ui = self.over_ui(app);
        self.postits(app, &ik, over_ui);
        self.ruler(app, &ik);
        self.popup(app, over_ui);
        self.bars(app, &ik);
        self.tts_bar(app);
        self.side_frame(app, &ik);
        self.style_frame(app);
        self.context_pick(app);
        self.input(app, mode, over_ui);
        self.keys(app, mode);
        self.tts_frame(app);
        app.ui.d.pop_alpha();
        app.ui.enabled = was;
        // primera vez: se enseña la interfaz un instante
        if time - self.start_t < 2.5 {
            app.ui.wake = app.ui.wake.min(2.6 - (time - self.start_t));
        }
        true
    }
    pub(super) fn overlay<'a>(&'a self) -> (Option<(i64, i64)>, Option<(i64, i64)>, Option<(i64, i64)>, i64) {
        (self.sel, self.flash.map(|f| (f.0, f.1)), self.speak, self.hot)
    }

    // ------------------------------------------------------------ páginas
    /// Pinta una página en su hoja. `rec` la apunta para el ratón.
    #[allow(clippy::too_many_arguments)]
    fn draw_page(&mut self, app: &mut App, a: Option<Addr>, sheet: Rect, text: Rect, ik: &Ink, side: i32, rec: bool) {
        let (sel, find, speak, hot) = self.overlay();
        let strength = self.strength(app);
        let Reader { content, notes, id, placed, title, toc, .. } = self;
        let ov = Over { notes, sel, find, speak, hot };
        match content {
            Content::Flow(v) => {
                app.ui.d.fill(sheet, 0.0, ik.bg);
                if let Some(a) = a {
                    let flow = v.flow(a.c);
                    if let Some(p) = flow.pages.get(a.p).copied() {
                        let o = (text.x, text.y - p.y0);
                        let range = p.a as usize..p.b as usize;
                        draw_flow(&mut app.ui, &app.jobs, v, &flow, range.clone(), o, ik, &ov, *id, sheet);
                        if rec {
                            placed.push(Placed::Flow { flow: flow.clone(), range, o, sheet });
                        }
                        if app.set.footer {
                            let dim = ik.ink.a(0.42);
                            let first = a.p == 0;
                            if !first {
                                let head = if side < 0 { title.clone() } else { toc.iter().rfind(|t| t.loc <= p.start as i64).map(|t| t.title.clone()).unwrap_or_else(|| title.clone()) };
                                let s = app.ui.t.fit(fonts().title_i, 13.5, &head, text.w * 0.8);
                                app.ui.text_c(fonts().title_i, 13.5, text.cx(), sheet.y + 27.0, &s, dim);
                            }
                            if !v.base.is_empty() {
                                let n = v.base[a.c] + a.p + 1;
                                app.ui.text_c(fonts().title, 13.5, text.cx(), sheet.b() - 24.0, &n.to_string(), dim);
                            }
                        }
                    }
                }
                // sombra del lomo
                if side != 0 {
                    let w = (sheet.w * 0.075).min(60.0);
                    let k = if ik.dark { 0.30 } else { 0.16 };
                    if side < 0 {
                        app.ui.d.grad(rect(sheet.r() - w, sheet.y, w, sheet.h), 0.0, Color::BLACK.a(0.0), Color::BLACK.a(k), true);
                    } else {
                        app.ui.d.grad(rect(sheet.x, sheet.y, w, sheet.h), 0.0, Color::BLACK.a(k), Color::BLACK.a(0.0), true);
                    }
                }
            }
            Content::Fixed(v) => match a {
                Some(a) => {
                    // cada página en su proporción, pegada al lomo
                    let (pw, ph) = v.sizes[a.p];
                    let k = (sheet.w / pw).min(sheet.h / ph);
                    let (rw, rh) = (pw * k, ph * k);
                    let x = match side {
                        -1 => sheet.r() - rw,
                        1 => sheet.x,
                        _ => sheet.x + (sheet.w - rw) / 2.0,
                    };
                    let r = rect(x, sheet.y + (sheet.h - rh) / 2.0, rw, rh);
                    if rw + 1.0 < sheet.w || rh + 1.0 < sheet.h {
                        app.ui.d.fill(sheet, 0.0, if strength > 0.0 { ik.bg } else { Color::hex(0xF4F2EE).mul(ik.tint.lum()) });
                    }
                    draw_fixed(&mut app.ui, &app.jobs, v, a.p, r, ik, strength, &ov, *id);
                    if rec {
                        placed.push(Placed::Fixed { page: a.p, r, sheet });
                    }
                    if side != 0 {
                        let w = (sheet.w * 0.05).min(40.0);
                        if side < 0 {
                            app.ui.d.grad(rect(sheet.r() - w, sheet.y, w, sheet.h), 0.0, Color::BLACK.a(0.0), Color::BLACK.a(0.18), true);
                        } else {
                            app.ui.d.grad(rect(sheet.x, sheet.y, w, sheet.h), 0.0, Color::BLACK.a(0.18), Color::BLACK.a(0.0), true);
                        }
                    }
                }
                None => app.ui.d.fill(sheet, 0.0, desk(ik)),
            },
        }
    }
    fn stack(&mut self, app: &mut App, g: &Geo, ik: &Ink, left: bool, right: bool) {
        // el taco de hojas a cada lado crece o mengua con lo leído
        let p = self.content.progress(self.loc).clamp(0.0, 1.0);
        let k = ((app.ui.time - self.start_t) / 0.30).clamp(0.0, 1.0);
        if k < 1.0 {
            app.ui.redraw = true;
        }
        app.ui.d.push_alpha(smooth(k));
        let (a, b) = (g.sheets[0], *g.sheets.last().unwrap());
        let all = rect(a.x, a.y, b.r() - a.x, a.h);
        app.ui.d.shadow(all.off(0.0, 10.0).grow(2.0), 6.0, 46.0, Color::BLACK.a(if ik.dark { 0.50 } else { 0.28 }));
        let edge = if ik.dark { ik.bg.mix(Color::WHITE, 0.07) } else { ik.bg.mul(0.93) };
        let dark = if ik.dark { Color::BLACK.a(0.5) } else { Color::hex(0x5A4630).a(0.30) };
        let mut pile = |n: usize, s: f32, r: Rect| {
            for k in (1..=n).rev() {
                let o = k as f32 * 2.2 * s;
                let q = rect(r.x + o.min(0.0), r.y + k as f32 * 0.8, r.w + o.abs(), r.h - k as f32 * 1.6);
                app.ui.d.boxed(q, 1.5, edge.mul(1.0 - 0.03 * k as f32), 1.0, dark);
            }
        };
        if left {
            pile((p * 4.0).ceil() as usize, -1.0, a);
        }
        if right {
            pile(((1.0 - p) * 4.0).ceil() as usize, 1.0, b);
        }
        app.ui.d.pop_alpha();
    }
    fn paged(&mut self, app: &mut App, ik: &Ink, mode: ReadMode) {
        let double = mode == ReadMode::Double;
        // avance del giro
        if let Some(tn) = &mut self.turning {
            if !tn.drag {
                let span = (tn.goal - tn.from).abs();
                let base = if app.set.calm {
                    0.16
                } else if !double {
                    0.34
                } else {
                    match app.set.turn {
                        Turn::Curl => 0.62,
                        Turn::Slide => 0.42,
                        _ => 0.26,
                    }
                };
                let dur = (base * span.sqrt()).max(0.12);
                tn.el += app.ui.dt;
                let u = (tn.el / dur).clamp(0.0, 1.0);
                // tecla: entra y sale suave; soltada: ya va lanzada y solo frena
                let e = if tn.thrown { 1.0 - (1.0 - u).powi(3) } else { 0.5 - 0.5 * (u * std::f32::consts::PI).cos() };
                tn.t = tn.from + (tn.goal - tn.from) * e;
                app.ui.redraw = true;
                if u >= 1.0 {
                    let to = if tn.goal > 0.5 { tn.b } else { tn.a };
                    self.turning = None;
                    self.land(to);
                }
            } else {
                app.ui.redraw = true;
            }
        }
        let g = self.geo(app, mode, None);
        let Some(tn) = self.turning.as_ref().map(|t| (t.a, t.b, t.t, t.drawn, t.tilt)) else {
            self.stack(app, &g, ik, double, true);
            let pages = if double { self.content.spread(self.addr).to_vec() } else { vec![Some(self.addr)] };
            for (i, a) in pages.iter().enumerate() {
                let side = if double { if i == 0 { -1 } else { 1 } } else { 0 };
                self.draw_page(app, *a, g.sheets[i], g.text[i], ik, side, true);
            }
            self.ribbon(app, &g, ik);
            self.preload(app, mode, &g);
            return;
        };
        let (a, b, t, drawn, tilt) = tn;
        let ga = self.geo(app, mode, Some(a));
        let gb = self.geo(app, mode, Some(b));
        let (pa, pb) = if double { (self.content.spread(a), self.content.spread(b)) } else { ([None, Some(a)], [None, Some(b)]) };
        let turn = app.set.turn;
        if turn == Turn::None || app.set.calm {
            // fundido muy corto
        }
        // texturas de las hojas
        let dpi = app.ui.d.dpi;
        let sz = (((ga.sheets[0].w.max(gb.sheets[0].w)) * dpi).round() as u32, ((ga.sheets[0].h.max(gb.sheets[0].h)) * dpi).round() as u32);
        self.targets(app, sz);
        // 0: A izquierda, 1: A derecha, 2: B izquierda, 3: B derecha
        let fixed = !self.content.is_flow();
        if !drawn || fixed {
            let list: Vec<(usize, Option<Addr>, Rect, Rect, i32)> = if double {
                vec![(0, pa[0], ga.sheets[0], ga.text[0], -1), (1, pa[1], ga.sheets[1], ga.text[1], 1), (2, pb[0], gb.sheets[0], gb.text[0], -1), (3, pb[1], gb.sheets[1], gb.text[1], 1)]
            } else {
                vec![(1, pa[1], ga.sheets[0], ga.text[0], 0), (3, pb[1], gb.sheets[0], gb.text[0], 0)]
            };
            for (k, ad, s, tx, side) in list {
                self.bake_one(app, k, ad, s, tx, side, ik);
            }
            if let Some(t) = &mut self.turning {
                t.drawn = true;
            }
        }
        let full_uv = [0.0, 0.0, 1.0, 1.0];
        let tx = self.texs.clone();
        let blank = |ad: Option<Addr>| ad.is_none() && fixed;
        if !double && turn != Turn::None && !app.set.calm {
            {
                let e = t.clamp(0.0, 1.0);
                let clip = app.ui.full();
                let gap = (ga.sheets[0].h.max(gb.sheets[0].h) + 48.0).max(app.ui.h * 0.5);
                app.ui.d.push_clip(clip, 0.0);
                app.ui.d.fill(clip, 0.0, desk(ik));
                for (g2, tid, dy) in [(&ga, tx[1], -gap * e), (&gb, tx[3], gap * (1.0 - e))] {
                    let s = g2.sheets[0].off(0.0, dy);
                    if s.b() > 0.0 && s.y < app.ui.h {
                        app.ui.d.shadow(s.off(0.0, 4.0), 2.0, 22.0, Color::BLACK.a(0.30));
                        app.ui.d.image(tid, s, 0.0, full_uv, Color::WHITE);
                    }
                }
                app.ui.d.pop_clip();
            }
            return;
        }
        match turn {
            Turn::Curl if !app.set.calm => {
                self.stack(app, &ga, ik, double, true);
                let d = &mut app.ui.d;
                if double {
                    d.image(tx[0], ga.sheets[0], 0.0, full_uv, Color::WHITE);
                    d.image(tx[3], gb.sheets[1], 0.0, full_uv, Color::WHITE);
                    curl::draw(d, &curl::Sheet { r: ga.sheets[1], flip: false, front: tx[1], back: Some(tx[2]), paper: ik.bg, t, tilt }, None);
                } else {
                    d.image(tx[3], gb.sheets[0], 0.0, full_uv, Color::WHITE);
                    let s = ga.sheets[0];
                    curl::draw(d, &curl::Sheet { r: s, flip: false, front: tx[1], back: None, paper: ik.bg, t: t * 0.96, tilt }, Some(rect(-10.0, 0.0, app.ui.w + 20.0, app.ui.h)));
                }
                let _ = blank;
            }
            Turn::Slide if !app.set.calm => {
                // la nueva entra por debajo mientras la actual se retira
                let e = t.clamp(0.0, 1.0);
                let w = app.ui.w;
                let clip = app.ui.full();
                app.ui.d.push_clip(clip, 0.0);
                let shift = -w * e;
                let under = w * 0.22 * (1.0 - e);
                let k = if double { [2usize, 3] } else { [3, 3] };
                app.ui.d.fill(clip, 0.0, desk(ik));
                for i in 0..gb.sheets.len() {
                    let tid = if double { tx[k[i]] } else { tx[3] };
                    app.ui.d.image(tid, gb.sheets[i].off(under, 0.0), 0.0, full_uv, Color::WHITE.mix(Color(0.6, 0.6, 0.6, 1.0), 1.0 - e));
                }
                let all = rect(0.0, 0.0, w, app.ui.h).off(shift, 0.0);
                app.ui.d.shadow(all, 0.0, 50.0, Color::BLACK.a(0.45 * (1.0 - e)));
                app.ui.d.fill(all, 0.0, desk(ik));
                for i in 0..ga.sheets.len() {
                    let tid = if double { tx[i] } else { tx[1] };
                    app.ui.d.image(tid, ga.sheets[i].off(shift, 0.0), 0.0, full_uv, Color::WHITE);
                }
                app.ui.d.pop_clip();
            }
            _ => {
                self.stack(app, &ga, ik, double, true);
                let e = smooth(if turn == Turn::None { (t * 30.0).min(1.0) } else { t });
                for (g2, ids, al) in [(&ga, [0usize, 1], 1.0), (&gb, [2, 3], e)] {
                    app.ui.d.push_alpha(al);
                    for i in 0..g2.sheets.len() {
                        let tid = if double { tx[ids[i]] } else { tx[ids[1]] };
                        app.ui.d.image(tid, g2.sheets[i], 0.0, full_uv, Color::WHITE);
                    }
                    app.ui.d.pop_alpha();
                }
            }
        }
    }
    fn preload(&mut self, app: &mut App, mode: ReadMode, g: &Geo) {
        let a = self.addr;
        let (id, time) = (self.id, app.ui.time);
        if let Content::Fixed(v) = &mut self.content {
            let k = if mode == ReadMode::Double { 2 } else { 1 };
            let w = (g.sheets[0].w * app.ui.d.dpi).round() as u32;
            for d in [k, k + 1, -k, -k + 1] {
                let p = a.p as i32 + d;
                if p >= 0 && (p as usize) < v.sizes.len() {
                    v.page(&app.jobs, p as usize, w, id, time, 1);
                }
            }
        } else if let Content::Flow(v) = &mut self.content {
            // las ilustraciones de alrededor, ya listas para pasar la hoja
            for d in [-2, -1, 1, 2, 3] {
                if let Some(x) = v.step(a, d) {
                    let f = v.flow(x.c);
                    if let Some(p) = f.pages.get(x.p) {
                        for l in &f.lines[p.a as usize..p.b as usize] {
                            if let crate::layout::LineKind::Image(src) = &l.kind {
                                v.image(&app.jobs, src, l.w * app.ui.d.dpi, id, time);
                            }
                        }
                    }
                }
            }
        }
    }
    /// La hoja en curso aterriza ya.
    pub fn settle(&mut self) {
        if let Some(t) = self.turning.take() {
            self.land(if t.goal > 0.5 { t.b } else { t.a });
        }
    }
    fn land(&mut self, a: Addr) {
        self.addr = a;
        self.loc = self.content.loc_of(a);
        self.sel = None;
        self.sel_done = false;
        self.touch();
    }
    /// Una hoja adelante (1) o atrás (-1).
    pub fn turn(&mut self, app: &mut App, dir: i32) {
        let mode = self.mode(app);
        match mode {
            ReadMode::ScrollV => {
                self.sy_t += dir as f32 * (app.ui.h - 130.0);
                app.ui.redraw = true;
            }
            ReadMode::ScrollH => {
                let g = self.geo(app, mode, None);
                self.hx_t = (self.hx_t / (g.sheets[0].w + HGAP)).round() * (g.sheets[0].w + HGAP) + dir as f32 * (g.sheets[0].w + HGAP);
                app.ui.redraw = true;
            }
            _ => {
                if let Some(t) = self.turning.take() {
                    // pulsaciones seguidas: la hoja en curso aterriza ya
                    self.land(if t.goal > 0.5 { t.b } else { t.a });
                }
                if let Some((a, b)) = self.neighbours(mode, dir) {
                    if app.set.turn == Turn::None || (app.set.calm && app.set.turn != Turn::Fade) {
                        self.land(if dir > 0 { b } else { a });
                    } else {
                        let fwd = dir > 0;
                        let t0 = if fwd { 0.0 } else { 1.0 };
                        self.turning = Some(Turning { a, b, fwd, t: t0, v: 0.0, drag: false, goal: 1.0 - t0, drawn: false, tilt: 0.16, from: t0, el: 0.0, thrown: false });
                    }
                }
                app.ui.redraw = true;
            }
        }
        self.sel = None;
        self.touch();
    }
    fn neighbours(&mut self, mode: ReadMode, dir: i32) -> Option<(Addr, Addr)> {
        let cur = self.addr;
        if mode == ReadMode::Double {
            let sp = self.content.spread(cur);
            let first = sp[0].or(sp[1])?;
            let last = sp[1].or(sp[0])?;
            let other = if dir > 0 { self.content.step(last, 1)? } else { self.content.step(first, -1)? };
            let os = self.content.spread(other);
            let o = os[0].or(os[1])?;
            Some(if dir > 0 { (first, o) } else { (o, first) })
        } else {
            let o = self.content.step(cur, dir)?;
            Some(if dir > 0 { (cur, o) } else { (o, cur) })
        }
    }
    fn ribbon(&mut self, app: &mut App, g: &Geo, ik: &Ink) {
        if self.page_mark().is_none() {
            return;
        }
        let s = *g.sheets.last().unwrap();
        let x = s.r() - 46.0;
        let t = app.ui.anim_from(id("ribbon"), 0.0, 1.0, 12.0);
        let h = 52.0 * t;
        app.ui.d.shadow(rect(x, s.y, 20.0, h), 1.0, 8.0, Color::BLACK.a(0.4));
        app.ui.d.grad(rect(x, s.y - 4.0, 20.0, h), 1.0, ik.accent.mul(0.8), ik.accent, false);
        app.ui.d.rotated(rect(x + 3.0, s.y + h - 11.0, 14.2, 14.2), 0.0, std::f32::consts::FRAC_PI_4, ik.bg, ik.bg);
    }
    /// Marcador de la página visible.
    pub fn page_mark(&mut self) -> Option<i64> {
        let (a, b) = self.visible();
        self.notes.iter().find(|n| n.kind == Kind::Bookmark && n.a >= a && n.a < b).map(|n| n.id)
    }
    /// Tramo que se está viendo.
    pub fn visible(&mut self) -> (i64, i64) {
        let a = self.loc;
        match self.mode_seen {
            Some(ReadMode::Double) => {
                let s = self.content.spread(self.addr);
                let f = s[0].or(s[1]).unwrap_or(self.addr);
                let l = s[1].or(s[0]).unwrap_or(self.addr);
                (self.content.loc_of(f), self.content.page_end(l))
            }
            Some(ReadMode::ScrollV) => {
                let mut e = a + 1;
                for p in &self.placed {
                    match p {
                        Placed::Flow { flow, range, .. } => e = e.max(flow.lines[range.end - 1].end as i64),
                        Placed::Fixed { page, .. } => e = e.max(fixed_loc(page + 1, 0)),
                    }
                }
                (a, e)
            }
            _ => {
                let ad = self.addr;
                (self.content.loc_of(ad), self.content.page_end(ad))
            }
        }
    }

    // ------------------------------------------------------------ desplazamiento vertical
    fn item_h(&mut self, app: &App, i: usize) -> f32 {
        match &mut self.content {
            Content::Flow(v) => v.flow(i).height + v.cfg.size * 5.0,
            Content::Fixed(v) => {
                let (pw, ph) = v.sizes[i];
                fixed_w(app, self.zoom, pw) * ph / pw + 14.0
            }
        }
    }
    fn items(&self) -> usize {
        match &self.content {
            Content::Flow(v) => v.flows.len(),
            Content::Fixed(v) => v.sizes.len(),
        }
    }
    fn scroll_v(&mut self, app: &mut App, ik: &Ink) {
        let h = app.ui.h;
        if self.auto && self.press.is_none() {
            let v = 12.0 + app.set.auto_scroll * 90.0;
            self.sy_t += v * app.ui.dt;
            self.sy.x += v * app.ui.dt;
            app.ui.redraw = true;
            self.touch();
        }
        // principio y fin
        if self.sc == 0 {
            self.sy_t = self.sy_t.max(-TOP_PAD);
        }
        let dt = app.ui.dt;
        let t = self.sy_t;
        if self.sy.step(t, 19.0, dt, 0.12) {
            app.ui.redraw = true;
        }
        // normaliza al elemento visible
        let n = self.items();
        for _ in 0..400 {
            if self.sy.x < 0.0 && self.sc > 0 {
                self.sc -= 1;
                let ih = self.item_h(app, self.sc);
                self.sy.x += ih;
                self.sy_t += ih;
            } else {
                let ih = self.item_h(app, self.sc);
                if self.sy.x >= ih && self.sc + 1 < n {
                    self.sc += 1;
                    self.sy.x -= ih;
                    self.sy_t -= ih;
                } else {
                    break;
                }
            }
        }
        // tope inferior: suma de lo que queda
        let mut rest = -self.sy_t;
        let mut i = self.sc;
        while i < n && rest < h * 2.0 {
            rest += self.item_h(app, i);
            i += 1;
        }
        if i >= n && rest < h * 0.72 {
            self.sy_t -= h * 0.72 - rest;
            if self.sc == 0 {
                self.sy_t = self.sy_t.max(-TOP_PAD);
            }
        }
        let (sel, find, speak, hot) = self.overlay();
        let strength = self.strength(app);
        let dpi = app.ui.d.dpi;
        let mut y = -self.sy.x;
        let mut i = self.sc;
        let w = app.ui.w;
        let mut first: Option<i64> = None;
        while i < n && y < h {
            let ih = self.item_h(app, i);
            let zoom = self.zoom;
            let Reader { content, notes, id, placed, .. } = self;
            let ov = Over { notes, sel, find, speak, hot };
            match content {
                Content::Flow(v) => {
                    let flow = v.flow(i);
                    let x = (((w - v.cfg.w) / 2.0) * dpi).round() / dpi;
                    let top = -y;
                    let a = flow.lines.partition_point(|l| l.y + l.h < top - 4.0);
                    let b = flow.lines.partition_point(|l| l.y < top + h);
                    if a < b {
                        let o = (x, y);
                        let sheet = rect(0.0, y.max(0.0), w, (y + ih).min(h) - y.max(0.0));
                        draw_flow(&mut app.ui, &app.jobs, v, &flow, a..b, o, ik, &ov, *id, rect(0.0, 0.0, w, h));
                        // primera línea bajo el borde superior
                        if first.is_none() {
                            let k = flow.lines[a..b].iter().position(|l| y + l.y >= 40.0).map_or(b - 1, |k| a + k);
                            first = Some(flow.lines[k].start as i64);
                            self.addr = Addr { c: i, p: flow.page_of(flow.lines[k].start) };
                        }
                        placed.push(Placed::Flow { flow, range: a..b, o, sheet });
                    }
                }
                Content::Fixed(v) => {
                    let (pw, ph) = v.sizes[i];
                    let rw = fixed_w(app, zoom, pw);
                    let r = rect((((w - rw) / 2.0 + self.pan.0) * dpi).round() / dpi, (y * dpi).round() / dpi, rw, rw * ph / pw);
                    app.ui.d.shadow(r.off(0.0, 4.0), 2.0, 22.0, Color::BLACK.a(0.30));
                    draw_fixed(&mut app.ui, &app.jobs, v, i, r, ik, strength, &ov, *id);
                    if first.is_none() && r.b() > h * 0.35 {
                        first = Some(fixed_loc(i, 0));
                        self.addr = Addr { c: 0, p: i };
                    }
                    placed.push(Placed::Fixed { page: i, r, sheet: r });
                }
            }
            y += ih;
            i += 1;
        }
        if let Content::Fixed(v) = &mut self.content {
            let time = app.ui.time;
            for k in [i, i + 1, self.sc.wrapping_sub(1)] {
                if k < n {
                    let want = (fixed_w(app, self.zoom, v.sizes[k].0) * dpi).round() as u32;
                    v.page(&app.jobs, k, want, self.id, time, 1);
                }
            }
        }
        if let Some(f) = first {
            if f != self.loc {
                self.loc = f;
                self.touch();
            }
        }
        // bordes que se desvanecen
        if self.content.is_flow() {
            app.ui.d.grad(rect(0.0, 0.0, w, 34.0), 0.0, ik.bg, ik.bg.a(0.0), false);
            app.ui.d.grad(rect(0.0, h - 34.0, w, 34.0), 0.0, ik.bg.a(0.0), ik.bg, false);
        }
    }

    // ------------------------------------------------------------ desplazamiento horizontal
    fn scroll_h(&mut self, app: &mut App, ik: &Ink) {
        let g = self.geo(app, ReadMode::ScrollH, None);
        let step = g.sheets[0].w + HGAP;
        let dt = app.ui.dt;
        let t = self.hx_t;
        if self.hx.step(t, 16.0, dt, 0.1) {
            app.ui.redraw = true;
        }
        // normaliza alrededor de la página actual
        for _ in 0..60 {
            if self.hx.x > step * 0.5 {
                match self.content.step(self.addr, 1) {
                    Some(a) => {
                        self.addr = a;
                        self.hx.x -= step;
                        self.hx_t -= step;
                    }
                    None => break,
                }
            } else if self.hx.x < -step * 0.5 {
                match self.content.step(self.addr, -1) {
                    Some(a) => {
                        self.addr = a;
                        self.hx.x += step;
                        self.hx_t += step;
                    }
                    None => break,
                }
            } else {
                break;
            }
        }
        if self.content.step(self.addr, 1).is_none() {
            self.hx_t = self.hx_t.min(0.0);
        }
        if self.content.step(self.addr, -1).is_none() {
            self.hx_t = self.hx_t.max(0.0);
        }
        let l = self.content.loc_of(self.addr);
        if l != self.loc {
            self.loc = l;
            self.touch();
        }
        let n = (app.ui.w / step / 2.0).ceil() as i32 + 1;
        for k in -n..=n {
            let Some(a) = (if k == 0 { Some(self.addr) } else { self.content.step(self.addr, k) }) else { continue };
            let dx = k as f32 * step - self.hx.x;
            let mut s = g.sheets[0].off(dx, 0.0);
            let mut tx = g.text[0].off(dx, 0.0);
            if s.x > app.ui.w || s.r() < 0.0 {
                continue;
            }
            let dpi = app.ui.d.dpi;
            let fx = (s.x * dpi).round() / dpi - s.x;
            s.x += fx;
            tx.x += fx;
            app.ui.d.shadow(s.off(0.0, 6.0), 2.0, 30.0, Color::BLACK.a(if ik.dark { 0.4 } else { 0.22 }));
            self.draw_page(app, Some(a), s, tx, ik, 0, true);
        }
    }

    // ------------------------------------------------------------ ratón
    fn over_ui(&self, app: &App) -> bool {
        let p = app.ui.input.pos;
        if app.ui.menu_blocks() {
            return true;
        }
        if self.bar > 0.3 && (p.1 < bars::TOP || bars::bottom_rect(app).has(p)) {
            return true;
        }
        if self.side_t > 0.0 && p.0 < self.side_t * side::W {
            return true;
        }
        self.style_t > 0.0 && side::style_rect(app).has(p)
    }
    fn at(&self, p: (f32, f32)) -> Option<(usize, i64, bool)> {
        let i = self.placed.iter().position(|x| x.sheet().has(p))?;
        let (l, inside) = hit(&self.placed[i], &self.content, p)?;
        Some((i, l, inside))
    }
    /// Si el punto cae fuera de todo, el trozo más cercano.
    fn near(&self, p: (f32, f32)) -> Option<i64> {
        if let Some(x) = self.at(p) {
            return Some(x.1);
        }
        let d = |r: Rect| {
            let dx = (r.x - p.0).max(p.0 - r.r()).max(0.0);
            let dy = (r.y - p.1).max(p.1 - r.b()).max(0.0);
            dx + dy
        };
        let b = self.placed.iter().min_by(|a, b| d(a.sheet()).total_cmp(&d(b.sheet())))?;
        hit(b, &self.content, p).map(|x| x.0)
    }
    fn link_at(&self, p: (f32, f32)) -> Option<std::sync::Arc<str>> {
        for x in &self.placed {
            if let Placed::Flow { flow, range, o, sheet } = x {
                if sheet.has(p) {
                    let g = flow.glyph_at(range.clone(), p.0 - o.0, p.1 - o.1)?;
                    return (g.link > 0).then(|| flow.links[g.link as usize - 1].clone());
                }
            }
        }
        None
    }
    fn input(&mut self, app: &mut App, mode: ReadMode, over_ui: bool) {
        let p = app.ui.input.pos;
        let free = app.ui.enabled && !over_ui && app.ui.active.is_none() && self.drag_note.is_none() && self.resize_note.is_none();
        // rueda
        if app.ui.enabled && !over_ui && (app.ui.input.wheel.1 != 0.0 || app.ui.input.wheel.0 != 0.0) {
            let (wx, wy) = app.ui.input.wheel;
            app.ui.input.wheel = (0.0, 0.0);
            if app.ui.input.mods.ctrl {
                self.zoom_by(app, if wy > 0.0 { 1 } else { -1 });
            } else {
                match mode {
                    ReadMode::ScrollV => {
                        self.sy_t -= wy * 1.15;
                        self.pan.0 += wx;
                    }
                    ReadMode::ScrollH => self.hx_t -= (wy + wx) * 1.5,
                    _ if self.zoom_to > 1.0 && !self.content.is_flow() => self.pan.1 += wy,
                    _ => {
                        // una hoja por muesca
                        if app.ui.time - self.wheel_at > 0.25 {
                            self.wheel_acc = 0.0;
                        }
                        self.wheel_at = app.ui.time;
                        self.wheel_acc += wy + wx;
                        if self.wheel_acc.abs() >= 90.0 {
                            let d = if self.wheel_acc > 0.0 { -1 } else { 1 };
                            self.wheel_acc = 0.0;
                            self.turn(app, d);
                        }
                    }
                }
                app.ui.redraw = true;
            }
        }
        if app.ui.enabled && app.ui.input.back {
            self.go_back(app);
        }
        // cursor
        if free && self.press.is_none() && !self.placing {
            if self.link_at(p).is_some() {
                app.ui.cursor = CursorIcon::Pointer;
            } else if self.at(p).is_some_and(|x| x.2) {
                app.ui.cursor = CursorIcon::Text;
            }
        }
        if self.placing {
            app.ui.cursor = CursorIcon::Copy;
            if free && app.ui.input.pressed {
                self.place_note(app, p);
                return;
            }
        }
        if free && app.ui.input.rpressed {
            self.context(app, p);
        }
        // pulsar
        if free && app.ui.input.pressed && (app.fullscreen || p.1 > 24.0) {
            let had = self.sel.is_some() || self.hot != 0;
            self.calm_tap = had;
            self.close_pop();
            let hit = self.at(p);
            let on_text = hit.is_some_and(|h| h.2);
            self.press = Some(if self.link_at(p).is_some() {
                Press::Link
            } else if let (true, Some((_, l, _))) = (on_text && self.turning.is_none(), hit) {
                if app.ui.input.clicks == 2 {
                    let (a, b) = self.word(l);
                    self.sel = Some((a, b));
                    self.sel_done = true;
                    Press::Other
                } else if app.ui.input.clicks == 3 {
                    let (a, b) = self.paragraph(l);
                    self.sel = Some((a, b));
                    self.sel_done = true;
                    Press::Other
                } else {
                    if app.ui.input.mods.shift {
                        if let Some((a, _)) = self.sel {
                            self.sel = Some((a, l));
                        }
                    }
                    Press::Text { start: l, moved: false }
                }
            } else if self.zoom_to > 1.0 && !self.content.is_flow() {
                Press::Pan { x: p.0, y: p.1, px: self.pan.0, py: self.pan.1 }
            } else if had {
                Press::Other
            } else {
                Press::Margin { x: p.0, y: p.1, moved: false }
            });
            app.ui.redraw = true;
        }
        let Some(pr) = self.press else { return };
        let down = app.ui.input.down;
        match pr {
            Press::Text { start, moved } => {
                let far = (p.0 - app.ui.drag_from.0).abs() + (p.1 - app.ui.drag_from.1).abs() > 4.0;
                if down && (moved || far) {
                    if let Some(l) = self.near(p) {
                        self.sel = Some((start, l));
                        self.sel_done = false;
                    }
                    self.press = Some(Press::Text { start, moved: true });
                    app.ui.cursor = CursorIcon::Text;
                    // arrastrar hasta el borde sigue bajando
                    if mode == ReadMode::ScrollV {
                        let m = 46.0;
                        let v = if p.1 < m { (p.1 - m) * 0.25 } else if p.1 > app.ui.h - m { (p.1 - app.ui.h + m) * 0.25 } else { 0.0 };
                        if v != 0.0 {
                            self.sy_t += v;
                            app.ui.redraw = true;
                        }
                    }
                }
                if !down {
                    if moved {
                        if let Some((a, b)) = self.sel {
                            if a == b {
                                self.sel = None;
                            } else {
                                // palabras enteras
                                let (mut a, mut b) = (a.min(b), a.max(b));
                                let (w0, w1) = (self.word(a), self.word(b));
                                if w0.0 < a && a < w0.1 {
                                    a = w0.0;
                                }
                                if w1.0 < b && b < w1.1 {
                                    b = w1.1;
                                }
                                self.sel = Some((a, b));
                                self.sel_done = true;
                                self.pop_t = 0.0;
                            }
                        }
                    } else {
                        // clic simple: ¿sobre un subrayado?
                        match self.mark_at(start) {
                            Some(id) => {
                                self.hot = id;
                                self.pop_t = 0.0;
                            }
                            None => self.tap(app, mode, p),
                        }
                    }
                    self.press = None;
                }
            }
            Press::Link => {
                if !down {
                    self.press = None;
                    if let Some(h) = self.link_at(p) {
                        self.follow(app, &h);
                    }
                }
            }
            Press::Margin { x, y, moved } => {
                let dx = p.0 - x;
                let far = dx.abs() > 8.0;
                if mode == ReadMode::Double && down && app.set.turn != Turn::None && !app.set.calm {
                    self.drag_turn(app, mode, x, y, p, moved || far);
                    if far {
                        self.press = Some(Press::Margin { x, y, moved: true });
                    }
                } else if mode == ReadMode::ScrollH && down && (moved || far) {
                    self.press = Some(Press::Margin { x: p.0, y, moved: true });
                    self.hx_t -= dx;
                    app.ui.cursor = CursorIcon::Grabbing;
                } else if mode == ReadMode::ScrollV && down && (moved || (p.1 - y).abs() > 8.0) {
                    self.press = Some(Press::Margin { x, y: p.1, moved: true });
                    self.sy_t -= p.1 - y;
                    app.ui.cursor = CursorIcon::Grabbing;
                }
                if !down {
                    self.press = None;
                    if let Some(t) = self.turning.as_mut().filter(|t| t.drag) {
                        t.drag = false;
                        // según el empuje o por dónde iba
                        let g = if t.v.abs() > 0.9 { t.v > 0.0 } else { t.t > if t.fwd { 0.35 } else { 0.65 } };
                        t.goal = g as i32 as f32;
                        t.v = t.v.clamp(-3.0, 3.0);
                        t.from = t.t;
                        t.el = 0.0;
                        t.thrown = true;
                    } else if !moved {
                        self.tap(app, mode, p);
                    }
                }
            }
            Press::Pan { x, y, px, py } => {
                if down {
                    self.pan.0 = px + p.0 - x;
                    if mode == ReadMode::ScrollV {
                        self.sy_t -= p.1 - y;
                        self.press = Some(Press::Pan { x, y: p.1, px, py });
                    } else {
                        self.pan.1 = py + p.1 - y;
                    }
                    app.ui.cursor = CursorIcon::Grabbing;
                    app.ui.redraw = true;
                } else {
                    self.press = None;
                    if (p.0 - app.ui.drag_from.0).abs() + (p.1 - app.ui.drag_from.1).abs() < 5.0 {
                        self.tap(app, mode, p);
                    }
                }
            }
            Press::Other => {
                if !down {
                    self.press = None;
                }
            }
        }
    }
    /// Clic sin más: los lados pasan página, el centro muestra u oculta la interfaz.
    fn tap(&mut self, app: &mut App, mode: ReadMode, p: (f32, f32)) {
        if std::mem::take(&mut self.calm_tap) {
            return;
        }
        if self.style_open || self.side.is_some() {
            self.style_open = false;
            self.side = None;
            return;
        }
        let w = app.ui.w;
        if matches!(mode, ReadMode::Single | ReadMode::Double) {
            let z = 0.24;
            if p.0 < w * z {
                self.turn(app, -1);
                return;
            }
            if p.0 > w * (1.0 - z) {
                self.turn(app, 1);
                return;
            }
        }
        self.pinned = if self.bar > 0.5 { false } else { true };
        self.last_move = if self.pinned { app.ui.time } else { -100.0 };
        app.ui.redraw = true;
    }
    fn drag_turn(&mut self, app: &mut App, mode: ReadMode, x: f32, y: f32, p: (f32, f32), far: bool) {
        let g = self.geo(app, mode, None);
        let r = *g.sheets.last().unwrap();
        let double = mode == ReadMode::Double;
        let spine = if double { r.x } else { r.x };
        if self.turning.is_none() && far {
            let dir = if p.0 < x { 1 } else { -1 };
            if let Some((a, b)) = self.neighbours(mode, dir) {
                // inclinación según se tire de arriba o de abajo
                let tilt = ((y - app.ui.h / 2.0) / app.ui.h * 0.6).clamp(-0.3, 0.3);
                let t0 = if dir > 0 { 0.0 } else { 1.0 };
                self.turning = Some(Turning { a, b, fwd: dir > 0, t: t0, v: 0.0, drag: true, goal: 0.0, drawn: false, tilt, from: t0, el: 0.0, thrown: true });
            }
        }
        let dt = app.ui.dt.max(0.001);
        if let Some(t) = self.turning.as_mut().filter(|t| t.drag) {
            // el borde de la hoja sigue al ratón
            let want = if double { (r.r() - p.0) / (2.0 * r.w) } else { (r.r() - p.0 + (spine - spine)) / (r.w * 1.15) };
            let want = want.clamp(0.0, 1.0);
            let k = 1.0 - (-26.0 * dt).exp();
            let nt = t.t + (want - t.t) * k;
            t.v = t.v * 0.6 + (nt - t.t) / dt * 0.4;
            t.t = nt;
            app.ui.cursor = CursorIcon::Grabbing;
            app.ui.redraw = true;
        }
    }
    fn zoom_by(&mut self, app: &mut App, d: i32) {
        if self.content.is_flow() {
            app.set.size = (app.set.size + d as f32).clamp(11.0, 44.0);
        } else {
            self.zoom_to = (self.zoom_to * if d > 0 { 1.12 } else { 1.0 / 1.12 }).clamp(0.4, 4.0);
            if (self.zoom_to - 1.0).abs() < 0.04 {
                self.zoom_to = 1.0;
            }
        }
        app.ui.redraw = true;
    }
    fn follow(&mut self, app: &mut App, h: &str) {
        if h.starts_with("http://") || h.starts_with("https://") || h.starts_with("mailto:") {
            let u = h.to_string();
            let s = format!("Se abrirá en tu navegador: {u}");
            app.ask("¿Abrir el enlace?", &s, "Abrir", move |_| {
                let _ = std::process::Command::new("rundll32.exe").arg("url.dll,FileProtocolHandler").arg(&u).spawn();
            });
            return;
        }
        let to = match &self.content {
            Content::Flow(v) => v.doc.resolve(h),
            _ => None,
        };
        if let Some(l) = to {
            self.jump(l as i64);
            app.ui.redraw = true;
        }
    }
    pub fn go_back(&mut self, app: &mut App) {
        if let Some(l) = self.history.pop() {
            self.turning = None;
            self.seek(l);
            app.ui.redraw = true;
        }
    }
    pub fn word(&mut self, l: i64) -> (i64, i64) {
        match &self.content {
            Content::Flow(v) => {
                let Some((c, b)) = v.doc.block_at(l as usize) else { return (l, l) };
                let bl = &v.doc.chapters[c].blocks[b];
                let o = floor(&bl.text, (l as usize).saturating_sub(bl.start));
                let t = &bl.text;
                let w = |c: char| c.is_alphanumeric() || c == '\'' || c == '’' || c == '\u{ad}';
                let s = t[..o].char_indices().rev().take_while(|x| w(x.1)).last().map_or(o, |x| x.0);
                let e = t[o..].char_indices().take_while(|x| w(x.1)).last().map_or(o, |x| o + x.0 + x.1.len_utf8());
                ((bl.start + s) as i64, (bl.start + e) as i64)
            }
            Content::Fixed(v) => {
                let Some(cs) = v.chars.get(&loc_page(l)) else { return (l, l) };
                let i = loc_char(l).min(cs.len().saturating_sub(1));
                let ok = |k: usize, p: usize| {
                    let (a, b) = (&cs[p], &cs[k]);
                    b.c.is_alphanumeric() && (a.y - b.y).abs() < a.h * 0.6 && (b.x - a.x - a.w).abs() < a.h * 0.3 && (a.x - b.x - b.w).abs() < a.h * 2.0 + a.w
                };
                let (mut s, mut e) = (i, i);
                while s > 0 && cs[s - 1].c.is_alphanumeric() && (cs[s - 1].y - cs[s].y).abs() < cs[s].h * 0.6 && cs[s].x - cs[s - 1].x - cs[s - 1].w < cs[s].h * 0.25 {
                    s -= 1;
                }
                while e < cs.len() && cs[e].c.is_alphanumeric() && (e == i || ok(e, e - 1)) {
                    e += 1;
                }
                (fixed_loc(loc_page(l), s), fixed_loc(loc_page(l), e))
            }
        }
    }
    fn paragraph(&mut self, l: i64) -> (i64, i64) {
        match &self.content {
            Content::Flow(v) => match v.doc.block_at(l as usize) {
                Some((c, b)) => {
                    let bl = &v.doc.chapters[c].blocks[b];
                    (bl.start as i64, (bl.start + bl.text.len()) as i64)
                }
                None => (l, l),
            },
            _ => self.word(l),
        }
    }

    // ------------------------------------------------------------ teclado
    fn keys(&mut self, app: &mut App, mode: ReadMode) {
        if !app.ui.enabled || app.confirm.is_some() {
            return;
        }
        if app.ui.ctrl("f") {
            self.open_side(app, Tab::Find);
        }
        if app.ui.ctrl("d") {
            self.toggle_mark(app);
        }
        if app.ui.ctrl("c") && app.ui.focus.is_none() {
            if let Some((a, b)) = self.sel {
                let s = self.content.slice(a, b);
                set_clipboard(&s);
                app.ui.toast("Copiado");
            }
        }
        if app.ui.ctrl("+") || app.ui.ctrl("=") {
            self.zoom_by(app, 1);
        }
        if app.ui.ctrl("-") {
            self.zoom_by(app, -1);
        }
        if app.ui.ctrl("0") {
            self.zoom_to = 1.0;
            self.pan = (0.0, 0.0);
        }
        if app.ui.key(NamedKey::F11) {
            let on = !app.fullscreen;
            app.win.push(crate::app::WinReq::Fullscreen(on));
        }
        if app.ui.key(NamedKey::Escape) {
            if self.edit_note.is_some() {
                self.end_edit(app);
            } else if app.ui.focus.is_some() {
                app.ui.focus = None;
                self.side = None;
            } else if self.placing {
                self.placing = false;
            } else if self.sel.is_some() || self.hot != 0 {
                self.close_pop();
            } else if self.style_open || self.side.is_some() {
                self.style_open = false;
                self.side = None;
            } else if app.fullscreen {
                app.win.push(crate::app::WinReq::Fullscreen(false));
            } else {
                self.close(app);
            }
            app.ui.redraw = true;
            return;
        }
        if app.ui.focus.is_some() {
            return;
        }
        let shift = app.ui.input.mods.shift;
        let vertical = mode == ReadMode::ScrollV;
        if app.ui.key(NamedKey::ArrowRight) || app.ui.key(NamedKey::PageDown) {
            self.turn(app, 1);
        }
        if app.ui.key(NamedKey::ArrowLeft) || app.ui.key(NamedKey::PageUp) {
            self.turn(app, -1);
        }
        if app.ui.key(NamedKey::Space) {
            if self.tts.is_some() {
                self.tts_pause(app);
            } else {
                self.turn(app, if shift { -1 } else { 1 });
            }
        }
        for (k, d) in [(NamedKey::ArrowDown, 1.0), (NamedKey::ArrowUp, -1.0)] {
            if app.ui.key(k) {
                if vertical {
                    self.sy_t += d * 90.0;
                } else {
                    self.turn(app, d as i32);
                }
            }
        }
        if app.ui.key(NamedKey::Home) {
            self.jump(0);
        }
        if app.ui.key(NamedKey::End) {
            let l = self.content.loc_at(1.0);
            self.jump(l);
        }
        if app.ui.chr("t") || app.ui.chr("i") {
            self.toggle_side(app, Tab::Toc);
        }
        if app.ui.chr("n") {
            self.placing = !self.placing;
        }
        if app.ui.chr("m") {
            self.toggle_mark(app);
        }
        if app.ui.chr("a") && vertical {
            self.auto = !self.auto;
        }
        if app.ui.chr("f") {
            let on = !app.fullscreen;
            app.win.push(crate::app::WinReq::Fullscreen(on));
        }
        if app.ui.chr("h") {
            self.pinned = !self.pinned;
            self.last_move = -100.0;
        }
        for k in 0..4 {
            if app.ui.chr(&(k + 1).to_string()) {
                self.set_mode(app, [ReadMode::Double, ReadMode::Single, ReadMode::ScrollV, ReadMode::ScrollH][k]);
            }
        }
    }
    pub fn set_mode(&mut self, app: &mut App, m: ReadMode) {
        if self.content.is_flow() {
            app.set.mode = m;
        } else {
            app.set.fixed_mode = m;
            self.zoom_to = 1.0;
            self.zoom = 1.0;
        }
        self.pan = (0.0, 0.0);
        app.ui.redraw = true;
    }
}
pub const TOP_PAD: f32 = 96.0;
pub const HGAP: f32 = 26.0;
fn fixed_w(app: &App, zoom: f32, _pw: f32) -> f32 {
    ((app.ui.w - 60.0).min(app.ui.h * 1.02).max(320.0) * zoom).round()
}
/// Tamaño de la hoja según el modo.
pub fn page_size(app: &App, mode: ReadMode, w: f32, h: f32) -> (f32, f32) {
    let ph = h - 2.0 * VPAD;
    let em = app.set.size;
    let pw = match mode {
        ReadMode::Double => ((w - 64.0) / 2.0).min(ph * 0.86).min(em * 40.0 + 120.0),
        ReadMode::ScrollH => (w - 120.0).min(ph * 0.80).min(em * 40.0 + 120.0),
        _ => (w - 64.0).min((ph * 0.84).max(em * 34.0)).min(em * 44.0 + 140.0),
    };
    (pw.max(260.0).round(), ph.round())
}
pub fn margins(app: &App, pw: f32) -> (f32, f32, f32) {
    let m = app.set.margin.clamp(0.0, 1.0);
    let mx = pw * lerp(0.055, 0.17, m);
    let v = lerp(0.0, 22.0, m);
    let f = app.set.footer;
    (mx, if f { 50.0 + v } else { 34.0 + v }, if f { 50.0 + v } else { 34.0 + v })
}
pub fn cfg_for(app: &App, mode: ReadMode, w: f32, h: f32) -> Cfg {
    let s = &app.set;
    let (pw, ph) = page_size(app, mode, w, h);
    let (mx, mt, mb) = margins(app, pw);
    let (cw, ch) = if mode == ReadMode::ScrollV {
        ((w - 80.0).min(s.size * lerp(46.0, 26.0, s.margin)), h - 160.0)
    } else {
        (pw - 2.0 * mx, ph - mt - mb)
    };
    Cfg { w: cw.max(160.0).round(), h: ch.max(160.0).round(), family: s.family, size: s.size, leading: s.leading, para: s.para, justify: s.justify, hyphen: s.hyphen }
}
