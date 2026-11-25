//! Lo que se lee: texto que se compone a medida o páginas ya hechas.
use crate::app::App;
use crate::covers::mips;
use crate::db::{Kind, Note};
use crate::doc::*;
use crate::formats::{decode, Fixed, PChar};
use crate::gfx::*;
use crate::icons::Icon;
use crate::jobs::{Apply, Jobs};
use crate::layout::{Cfg, Ctx, Dims, Flow, LineKind};
use crate::theme::MARKS;
use crate::ui::Ui;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

/// Capítulo y página; en documentos fijos el capítulo es 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Addr {
    pub c: usize,
    pub p: usize,
}
/// En documentos fijos una posición es página y carácter.
pub const PAGE_SHIFT: i64 = 20;
pub fn fixed_loc(page: usize, ch: usize) -> i64 {
    ((page as i64) << PAGE_SHIFT) | ch as i64
}
pub fn loc_page(l: i64) -> usize {
    (l >> PAGE_SHIFT).max(0) as usize
}
pub fn loc_char(l: i64) -> usize {
    (l & ((1 << PAGE_SHIFT) - 1)) as usize
}

/// Un trozo pintado: sirve para saber qué hay bajo el ratón.
#[derive(Clone)]
pub enum Placed {
    Flow { flow: Arc<Flow>, range: Range<usize>, o: (f32, f32), sheet: Rect },
    Fixed { page: usize, r: Rect, sheet: Rect },
}
impl Placed {
    pub fn sheet(&self) -> Rect {
        match self {
            Placed::Flow { sheet, .. } | Placed::Fixed { sheet, .. } => *sheet,
        }
    }
}

/// Colores de la página ya con brillo y calidez.
#[derive(Clone, Copy)]
pub struct Ink {
    pub bg: Color,
    pub ink: Color,
    pub accent: Color,
    pub dark: bool,
    pub tint: Color,
}

pub struct Img {
    tex: Option<TexId>,
    alpha: bool,
    failed: bool,
    used: f32,
}
pub struct FlowView {
    pub doc: Arc<Doc>,
    pub cfg: Cfg,
    pub flows: Vec<Option<Arc<Flow>>>,
    pub dims: Arc<Dims>,
    want: Option<Cfg>,
    token: u64,
    pub imgs: HashMap<String, Img>,
    /// Páginas acumuladas, cuando ya está todo compuesto.
    pub base: Vec<usize>,
    pub total: usize,
}
impl FlowView {
    pub fn new(doc: Arc<Doc>, cfg: Cfg) -> FlowView {
        let n = doc.chapters.len();
        FlowView { doc, cfg, flows: vec![None; n], dims: Arc::new(Dims::default()), want: None, token: 0, imgs: HashMap::new(), base: vec![], total: 0 }
    }
    pub fn ready(&self, c: usize) -> bool {
        self.flows.get(c).is_some_and(|f| f.is_some())
    }
    /// El capítulo compuesto; si falta se compone en el momento.
    pub fn flow(&mut self, c: usize) -> Arc<Flow> {
        if self.flows[c].is_none() {
            let f = Ctx { doc: &self.doc, cfg: &self.cfg, dims: &self.dims }.chapter(c);
            self.flows[c] = Some(Arc::new(f));
        }
        self.flows[c].clone().unwrap()
    }
    /// Pide otra composición. Mientras llega se sigue viendo la anterior.
    pub fn request(&mut self, jobs: &Jobs, cfg: Cfg, cur: usize, book: i64) {
        if self.want.as_ref().unwrap_or(&self.cfg) == &cfg {
            return;
        }
        self.want = Some(cfg.clone());
        self.compose(jobs, cfg, cur, book);
    }
    pub fn compose(&mut self, jobs: &Jobs, cfg: Cfg, cur: usize, book: i64) {
        self.token += 1;
        let (doc, dims, token) = (self.doc.clone(), self.dims.clone(), self.token);
        let j = jobs.clone();
        jobs.run(0, move || {
            let cx = Ctx { doc: &doc, cfg: &cfg, dims: &dims };
            let first = Arc::new(cx.chapter(cur));
            let (c2, f2) = (cfg.clone(), first.clone());
            j.send(Box::new(move |a: &mut App| {
                if let Some(v) = flow_of(a, book, token) {
                    v.cfg = c2;
                    v.want = None;
                    v.flows = vec![None; v.doc.chapters.len()];
                    v.flows[cur] = Some(f2);
                    v.base.clear();
                    if let Some(r) = super::live(&mut a.reader, &mut a.opening) {
                        r.relaid();
                    }
                }
            }));
            use rayon::prelude::*;
            let all: Vec<Arc<Flow>> = (0..doc.chapters.len()).into_par_iter().map(|c| if c == cur { first.clone() } else { Arc::new(cx.chapter(c)) }).collect();
            Some(Box::new(move |a: &mut App| {
                if let Some(v) = flow_of(a, book, token) {
                    v.base.clear();
                    let mut n = 0;
                    for f in &all {
                        v.base.push(n);
                        n += f.pages.len();
                    }
                    v.total = n;
                    v.flows = all.into_iter().map(Some).collect();
                }
            }) as Apply)
        });
    }
    pub fn addr_of(&mut self, loc: i64) -> Addr {
        let c = self.doc.chapter_at(loc.max(0) as usize);
        let f = self.flow(c);
        Addr { c, p: f.page_of(loc.max(0) as u32) }
    }
    pub fn loc_of(&mut self, a: Addr) -> i64 {
        let f = self.flow(a.c);
        f.pages.get(a.p).map_or(0, |p| p.start as i64)
    }
    pub fn page_end(&mut self, a: Addr) -> i64 {
        let f = self.flow(a.c);
        match f.pages.get(a.p + 1) {
            Some(p) => p.start as i64,
            None => {
                let c = &self.doc.chapters[a.c];
                (c.start + c.len) as i64
            }
        }
    }
    pub fn step(&mut self, a: Addr, n: i32) -> Option<Addr> {
        let mut a = a;
        for _ in 0..n.abs() {
            if n > 0 {
                let len = self.flow(a.c).pages.len();
                if a.p + 1 < len {
                    a.p += 1;
                } else {
                    let mut c = a.c + 1;
                    while c < self.flows.len() && self.flow(c).pages.is_empty() {
                        c += 1;
                    }
                    if c >= self.flows.len() {
                        return None;
                    }
                    a = Addr { c, p: 0 };
                }
            } else if a.p > 0 {
                a.p -= 1;
            } else {
                let mut c = a.c as i32 - 1;
                while c >= 0 && self.flow(c as usize).pages.is_empty() {
                    c -= 1;
                }
                if c < 0 {
                    return None;
                }
                a = Addr { c: c as usize, p: self.flow(c as usize).pages.len() - 1 };
            }
        }
        Some(a)
    }
    pub fn image(&mut self, jobs: &Jobs, src: &str, w: f32, book: i64, time: f32) -> Option<(TexId, bool)> {
        if let Some(i) = self.imgs.get_mut(src) {
            i.used = time;
            return i.tex.map(|t| (t, i.alpha));
        }
        self.imgs.insert(src.to_string(), Img { tex: None, alpha: false, failed: false, used: time });
        let (doc, s) = (self.doc.clone(), src.to_string());
        let maxw = (w * 1.6).clamp(64.0, 2200.0) as u32;
        jobs.run(0, move || {
            let img = doc.assets.load(&s).and_then(|b| decode(&b));
            let r = img.map(|i| {
                let i = if i.width() > maxw { let h = (i.height() as f32 * maxw as f32 / i.width() as f32).max(1.0) as u32; image::imageops::resize(&i, maxw, h, image::imageops::FilterType::Triangle) } else { i };
                let alpha = i.pixels().step_by(7).filter(|p| p[3] < 128).count() * 20 > (i.width() * i.height() / 7) as usize;
                let (w, h) = i.dimensions();
                (w, h, alpha, mips(i))
            });
            Some(Box::new(move |a: &mut App| {
                let t = r.as_ref().map(|(w, h, _, m)| a.gpu.create(*w, *h, m));
                let keep = match super::live(&mut a.reader, &mut a.opening).filter(|x| x.id == book).and_then(|x| match &mut x.content {
                    Content::Flow(v) => v.imgs.get_mut(&s),
                    _ => None,
                }) {
                    Some(i) => {
                        i.tex = t;
                        i.alpha = r.as_ref().is_some_and(|r| r.2);
                        i.failed = t.is_none();
                        true
                    }
                    None => false,
                };
                if let (false, Some(t)) = (keep, t) {
                    a.gpu.drop_tex(t);
                }
            }) as Apply)
        });
        None
    }
}
fn flow_of(a: &mut App, book: i64, token: u64) -> Option<&mut FlowView> {
    match super::live(&mut a.reader, &mut a.opening).filter(|r| r.id == book).map(|r| &mut r.content) {
        Some(Content::Flow(v)) if v.token == token => Some(v),
        _ => None,
    }
}

pub struct PageTex {
    pub tex: TexId,
    pub w: u32,
    used: f32,
    born: f32,
}
pub struct FixedView {
    pub doc: Arc<dyn Fixed>,
    pub sizes: Vec<(f32, f32)>,
    pub toc: Vec<Toc>,
    tex: HashMap<usize, PageTex>,
    loading: HashSet<(usize, u32)>,
    pub chars: HashMap<usize, Arc<Vec<PChar>>>,
    char_wait: HashSet<usize>,
}
impl FixedView {
    pub fn new(doc: Arc<dyn Fixed>) -> FixedView {
        let sizes = (0..doc.count()).map(|i| { let (w, h) = doc.size(i); (w.max(1.0), h.max(1.0)) }).collect();
        let toc = doc.toc();
        FixedView { doc, sizes, toc, tex: HashMap::new(), loading: HashSet::new(), chars: HashMap::new(), char_wait: HashSet::new() }
    }
    pub fn page(&mut self, jobs: &Jobs, i: usize, want: u32, book: i64, time: f32, prio: usize) -> Option<(TexId, f32)> {
        let (pw, ph) = self.sizes[i];
        // ni diminuta ni enorme
        let max = (4200.0 / ph * pw).min(3600.0);
        let want = (want as f32).min(max).max(64.0) as u32;
        let have = self.tex.get(&i).map(|t| t.w);
        if have != Some(want) && !self.loading.contains(&(i, want)) && self.loading.iter().filter(|l| l.0 == i).count() < 2 {
            self.loading.insert((i, want));
            let doc = self.doc.clone();
            let h = (want as f32 * ph / pw).round().max(1.0) as u32;
            jobs.run(prio, move || {
                let pic = doc.render(i, want, h);
                Some(Box::new(move |a: &mut App| {
                    let time = a.ui.time;
                    let t = pic.map(|p| (a.gpu.create(p.w, p.h, &[p.rgba]), p.w));
                    let Some(Content::Fixed(v)) = super::live(&mut a.reader, &mut a.opening).filter(|r| r.id == book).map(|r| &mut r.content) else {
                        if let Some((t, _)) = t {
                            a.gpu.drop_tex(t);
                        }
                        return;
                    };
                    v.loading.remove(&(i, want));
                    if let Some((t, _)) = t {
                        let born = v.tex.get(&i).map_or(time, |o| o.born);
                        if let Some(o) = v.tex.insert(i, PageTex { tex: t, w: want, used: time, born }) {
                            a.gpu.drop_tex(o.tex);
                        }
                    }
                }) as Apply)
            });
        }
        let t = self.tex.get_mut(&i)?;
        t.used = time;
        Some((t.tex, ((time - t.born) / 0.16).clamp(0.0, 1.0)))
    }
    pub fn trim(&mut self, gpu: &mut Gpu, time: f32) {
        let max = 12;
        if self.tex.len() > max {
            let mut v: Vec<(usize, f32)> = self.tex.iter().map(|(k, t)| (*k, t.used)).collect();
            v.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (k, used) in v.iter().take(self.tex.len() - max) {
                if time - used > 0.5 {
                    if let Some(t) = self.tex.remove(k) {
                        gpu.drop_tex(t.tex);
                    }
                }
            }
        }
    }
    pub fn text(&mut self, jobs: &Jobs, i: usize, book: i64) -> Option<Arc<Vec<PChar>>> {
        if let Some(c) = self.chars.get(&i) {
            return Some(c.clone());
        }
        if self.char_wait.insert(i) {
            let doc = self.doc.clone();
            jobs.run(1, move || {
                let c = Arc::new(doc.text(i));
                Some(Box::new(move |a: &mut App| {
                    if let Some(Content::Fixed(v)) = super::live(&mut a.reader, &mut a.opening).filter(|r| r.id == book).map(|r| &mut r.content) {
                        v.chars.insert(i, c);
                    }
                }) as Apply)
            });
        }
        None
    }
}

pub enum Content {
    Flow(FlowView),
    Fixed(FixedView),
}
impl Content {
    pub fn is_flow(&self) -> bool {
        matches!(self, Content::Flow(_))
    }
    pub fn toc(&self) -> Vec<Toc> {
        match self {
            Content::Flow(v) => v.doc.toc.clone(),
            Content::Fixed(v) => v.toc.iter().map(|t| Toc { loc: fixed_loc(t.loc.max(0) as usize, 0), ..t.clone() }).collect(),
        }
    }
    pub fn addr_of(&mut self, loc: i64) -> Addr {
        match self {
            Content::Flow(v) => v.addr_of(loc),
            Content::Fixed(v) => Addr { c: 0, p: loc_page(loc).min(v.sizes.len() - 1) },
        }
    }
    pub fn loc_of(&mut self, a: Addr) -> i64 {
        match self {
            Content::Flow(v) => v.loc_of(a),
            Content::Fixed(_) => fixed_loc(a.p, 0),
        }
    }
    pub fn page_end(&mut self, a: Addr) -> i64 {
        match self {
            Content::Flow(v) => v.page_end(a),
            Content::Fixed(_) => fixed_loc(a.p + 1, 0),
        }
    }
    pub fn step(&mut self, a: Addr, n: i32) -> Option<Addr> {
        match self {
            Content::Flow(v) => v.step(a, n),
            Content::Fixed(v) => {
                let p = a.p as i64 + n as i64;
                (p >= 0 && (p as usize) < v.sizes.len()).then_some(Addr { c: 0, p: p as usize })
            }
        }
    }
    /// Las dos páginas enfrentadas a las que pertenece una.
    pub fn spread(&mut self, a: Addr) -> [Option<Addr>; 2] {
        match self {
            Content::Flow(v) => {
                let l = Addr { c: a.c, p: a.p & !1 };
                let n = v.flow(a.c).pages.len();
                [Some(l), (l.p + 1 < n).then_some(Addr { c: a.c, p: l.p + 1 })]
            }
            Content::Fixed(v) => {
                if a.p == 0 {
                    return [None, Some(a)];
                }
                let l = (a.p - 1) / 2 * 2 + 1;
                let n = v.sizes.len();
                [Some(Addr { c: 0, p: l }), (l + 1 < n).then_some(Addr { c: 0, p: l + 1 })]
            }
        }
    }
    pub fn progress(&self, loc: i64) -> f32 {
        match self {
            Content::Flow(v) => loc as f32 / v.doc.len.max(1) as f32,
            Content::Fixed(v) => loc_page(loc) as f32 / (v.sizes.len().max(2) - 1) as f32,
        }
    }
    pub fn loc_at(&self, p: f32) -> i64 {
        match self {
            Content::Flow(v) => (p.clamp(0.0, 1.0) * (v.doc.len.max(1) - 1) as f32) as i64,
            Content::Fixed(v) => fixed_loc((p.clamp(0.0, 1.0) * (v.sizes.len() - 1) as f32).round() as usize, 0),
        }
    }
    pub fn slice(&mut self, a: i64, b: i64) -> String {
        match self {
            Content::Flow(v) => v.doc.slice(a.max(0) as usize, b.max(0) as usize),
            Content::Fixed(v) => {
                let mut out = String::new();
                for p in loc_page(a)..=loc_page(b) {
                    let Some(cs) = v.chars.get(&p).cloned().or_else(|| (p < v.sizes.len()).then(|| Arc::new(v.doc.text(p)))) else { continue };
                    let s = if p == loc_page(a) { loc_char(a) } else { 0 };
                    let e = if p == loc_page(b) { loc_char(b) } else { cs.len() };
                    out.push_str(&join(&cs[s.min(cs.len())..e.min(cs.len())]));
                    if p != loc_page(b) {
                        out.push(' ');
                    }
                }
                out
            }
        }
    }
    pub fn free(&mut self, gpu: &mut Gpu) {
        match self {
            Content::Flow(v) => {
                for i in v.imgs.values() {
                    if let Some(t) = i.tex {
                        gpu.drop_tex(t);
                    }
                }
                v.imgs.clear();
            }
            Content::Fixed(v) => {
                for t in v.tex.values() {
                    gpu.drop_tex(t.tex);
                }
                v.tex.clear();
            }
        }
    }
}
/// Caracteres de una página fija a texto: los saltos de línea pasan a espacio y
/// los guiones de fin de línea se unen.
pub fn join(cs: &[PChar]) -> String {
    let mut out = String::new();
    for (i, c) in cs.iter().enumerate() {
        if i > 0 {
            let p = &cs[i - 1];
            let newline = (c.y - p.y).abs() > p.h.max(c.h) * 0.6 || c.x + c.w < p.x - p.h;
            if newline {
                if out.ends_with('-') && c.c.is_lowercase() {
                    out.pop();
                } else if !out.ends_with(' ') {
                    out.push(' ');
                }
            } else if !p.c.is_whitespace() && !c.c.is_whitespace() && c.x - (p.x + p.w) > p.h * 0.22 {
                out.push(' ');
            }
        }
        if !c.c.is_control() {
            out.push(c.c);
        }
    }
    out
}

/// Lo que se pinta además del texto.
pub struct Over<'a> {
    pub notes: &'a [Note],
    pub sel: Option<(i64, i64)>,
    pub find: Option<(i64, i64)>,
    pub speak: Option<(i64, i64)>,
    pub hot: i64,
}

#[allow(clippy::too_many_arguments)]
pub fn draw_flow(ui: &mut Ui, jobs: &Jobs, v: &mut FlowView, flow: &Flow, range: Range<usize>, o: (f32, f32), ink: &Ink, ov: &Over, book: i64, view: Rect) {
    if range.is_empty() {
        return;
    }
    let (a, b) = (flow.lines[range.start].start as i64, flow.lines[range.end - 1].end as i64);
    let mk = if ink.dark { 0.30 } else { 0.48 };
    let band = |ui: &mut Ui, x: i64, y: i64, c: Color, under: bool| {
        if y <= a || x >= b {
            return;
        }
        for r in flow.rects(range.clone(), x.max(0) as u32, y.max(0) as u32) {
            let r = r.off(o.0, o.1);
            if under {
                ui.d.fill(rect(r.x, r.b() - r.h * 0.14, r.w, 2.2), 1.1, c.a(0.95));
            } else {
                // alto del texto, no del interlineado
                let k = (r.h * 0.09).min(5.0);
                ui.d.fill(rect(r.x - 1.5, r.y + k, r.w + 3.0, r.h - 2.0 * k), 3.0, c);
            }
        }
    };
    for n in ov.notes {
        let c = MARKS[n.color as usize % 5];
        match n.kind {
            Kind::Highlight => band(ui, n.a, n.b, c.a(if n.id == ov.hot { mk + 0.14 } else { mk }), false),
            Kind::Underline => band(ui, n.a, n.b, c, true),
            _ => {}
        }
    }
    if let Some((x, y)) = ov.speak {
        band(ui, x, y, ink.accent.a(0.22), false);
    }
    if let Some((x, y)) = ov.find {
        band(ui, x, y, ink.accent.a(0.50), false);
    }
    if let Some((x, y)) = ov.sel {
        band(ui, x.min(y), x.max(y), ink.accent.a(0.34), false);
    }
    let time = ui.time;
    for li in range {
        let l = &flow.lines[li];
        let y = o.1 + l.y;
        if y > view.b() || y + l.h < view.y {
            continue;
        }
        match &l.kind {
            LineKind::Rule => {
                for k in -1..=1 {
                    let cx = o.0 + l.w / 2.0 + k as f32 * 22.0;
                    ui.d.fill(rect(cx - 2.0, y + l.h / 2.0 - 2.0, 4.0, 4.0), 2.0, ink.ink.a(0.5));
                }
            }
            LineKind::Image(src) => {
                let r = rect(o.0 + l.x, y, l.w, l.h);
                match v.image(jobs, src, l.w * ui.d.dpi, book, time) {
                    Some((t, alpha)) => {
                        if alpha && ink.dark {
                            ui.d.fill(r.grow(6.0), 8.0, Color::hex(0xE4DDCF).mul(0.9));
                        }
                        ui.d.image(t, r, if l.w > 200.0 { 4.0 } else { 0.0 }, [0.0, 0.0, 1.0, 1.0], ink.tint);
                    }
                    None => {
                        if v.imgs.get(src).is_some_and(|i| i.failed) {
                            ui.d.boxed(r, 6.0, ink.ink.a(0.03), 1.0, ink.ink.a(0.12));
                            ui.icon(Icon::Image, r.cx(), r.cy(), 22.0, ink.ink.a(0.3));
                        } else {
                            ui.d.fill(r, 6.0, ink.ink.a(0.05));
                        }
                    }
                }
            }
            LineKind::Text => {
                let base = y + l.base;
                let ox = o.0 + l.x;
                let gs = &flow.glyphs[l.glyphs.start as usize..l.glyphs.end as usize];
                let c = if l.quote { ink.ink.a(0.86) } else { ink.ink };
                for g in gs {
                    let col = if g.link > 0 { ink.accent } else { c };
                    ui.t.glyph(&mut ui.d, g.font, g.gid, g.size, ox + g.x, base + g.dy, col);
                }
                // subrayado y tachado del propio libro
                let mut k = 0;
                while k < gs.len() {
                    let st = gs[k].style & (UNDER | STRIKE);
                    if st == 0 {
                        k += 1;
                        continue;
                    }
                    let mut e = k;
                    while e + 1 < gs.len() && gs[e + 1].style & (UNDER | STRIKE) == st {
                        e += 1;
                    }
                    let (x0, x1) = (ox + gs[k].x, ox + gs[e].x + gs[e].adv);
                    let col = if gs[k].link > 0 { ink.accent } else { c };
                    let sz = gs[k].size;
                    if st & UNDER != 0 {
                        ui.d.fill(rect(x0, base + sz * 0.13, x1 - x0, 1.0), 0.0, col.a(0.7));
                    }
                    if st & STRIKE != 0 {
                        ui.d.fill(rect(x0, base - sz * 0.26, x1 - x0, 1.0), 0.0, col.a(0.8));
                    }
                    k = e + 1;
                }
                if l.quote {
                    ui.d.fill(rect(ox.min(o.0 + l.x) - 16.0, y, 2.0, l.h), 0.0, ink.accent.a(0.35));
                }
            }
        }
    }
}

/// Rectángulos de un tramo de una página fija.
pub fn fixed_rects(cs: &[PChar], a: usize, b: usize, r: Rect, size: (f32, f32)) -> Vec<Rect> {
    let k = r.w / size.0;
    let mut out: Vec<Rect> = vec![];
    for c in &cs[a.min(cs.len())..b.min(cs.len())] {
        if c.w <= 0.0 && c.c.is_whitespace() {
            continue;
        }
        let q = rect(r.x + c.x * k, r.y + c.y * k, c.w * k, c.h * k);
        if let Some(l) = out.last_mut() {
            if (l.cy() - q.cy()).abs() < l.h.max(q.h) * 0.5 && q.x >= l.x - 1.0 && q.x - l.r() < l.h * 1.6 {
                let y0 = l.y.min(q.y);
                let y1 = l.b().max(q.b());
                *l = rect(l.x, y0, q.r() - l.x, y1 - y0);
                continue;
            }
        }
        out.push(q);
    }
    out
}
#[allow(clippy::too_many_arguments)]
pub fn draw_fixed(ui: &mut Ui, jobs: &Jobs, v: &mut FixedView, page: usize, r: Rect, ink: &Ink, strength: f32, ov: &Over, book: i64) {
    let want = (r.w * ui.d.dpi).round() as u32;
    let base = if strength > 0.0 { ink.bg } else { Color::hex(0xF4F2EE).mul(ink.tint.lum()) };
    ui.d.fill(r, 0.0, base);
    let time = ui.time;
    match v.page(jobs, page, want, book, time, 0) {
        Some((t, a)) => {
            if a < 1.0 {
                ui.redraw = true;
            }
            ui.d.push_alpha(a);
            if strength > 0.0 {
                ui.d.page(t, r, [0.0, 0.0, 1.0, 1.0], ink.bg, ink.ink, strength);
            } else {
                ui.d.image(t, r, 0.0, [0.0, 0.0, 1.0, 1.0], ink.tint);
            }
            ui.d.pop_alpha();
        }
        None => {
            let c = if base.lum() > 0.5 { Color::BLACK } else { Color::WHITE };
            ui.icon(Icon::Clock, r.cx(), r.cy(), 26.0, c.a(0.22));
        }
    }
    let cs = v.text(jobs, page, book);
    if let Some(cs) = cs {
        let sz = v.sizes[page];
        let mk = if ink.dark && strength > 0.0 { 0.30 } else { 0.40 };
        let band = |ui: &mut Ui, x: i64, y: i64, c: Color, under: bool| {
            let (x, y) = (x.min(y), x.max(y));
            if loc_page(x) > page || loc_page(y) < page {
                return;
            }
            let s = if loc_page(x) == page { loc_char(x) } else { 0 };
            let e = if loc_page(y) == page { loc_char(y) } else { cs.len() };
            for q in fixed_rects(&cs, s, e, r, sz) {
                if under {
                    ui.d.fill(rect(q.x, q.b() - 1.0, q.w, 2.0), 1.0, c.a(0.95));
                } else {
                    ui.d.fill(q.grow(1.0), 2.5, c);
                }
            }
        };
        for n in ov.notes {
            let c = MARKS[n.color as usize % 5];
            match n.kind {
                Kind::Highlight => band(ui, n.a, n.b, c.a(if n.id == ov.hot { mk + 0.14 } else { mk }), false),
                Kind::Underline => band(ui, n.a, n.b, c, true),
                _ => {}
            }
        }
        if let Some((x, y)) = ov.speak {
            band(ui, x, y, ink.accent.a(0.22), false);
        }
        if let Some((x, y)) = ov.find {
            band(ui, x, y, ink.accent.a(0.50), false);
        }
        if let Some((x, y)) = ov.sel {
            band(ui, x, y, ink.accent.a(0.34), false);
        }
    }
}

/// Posición del texto bajo un punto y si de verdad está encima de una letra.
pub fn hit(p: &Placed, c: &Content, pos: (f32, f32)) -> Option<(i64, bool)> {
    match (p, c) {
        (Placed::Flow { flow, range, o, .. }, _) => flow.hit(range.clone(), pos.0 - o.0, pos.1 - o.1).map(|(l, i)| (l as i64, i)),
        (Placed::Fixed { page, r, .. }, Content::Fixed(v)) => {
            let cs = v.chars.get(page)?;
            if cs.is_empty() {
                return None;
            }
            let k = r.w / v.sizes[*page].0;
            let (x, y) = ((pos.0 - r.x) / k, (pos.1 - r.y) / k);
            let mut best = (f32::MAX, 0usize, false);
            for (i, c) in cs.iter().enumerate() {
                let dy = if y < c.y { c.y - y } else if y > c.y + c.h { y - c.y - c.h } else { 0.0 };
                let dx = if x < c.x { c.x - x } else if x > c.x + c.w { x - c.x - c.w } else { 0.0 };
                let d = dy * 4.0 + dx;
                if d < best.0 {
                    let after = x > c.x + c.w / 2.0;
                    best = (d, i + after as usize, dy <= 1.0 && dx < c.h * 0.6);
                }
            }
            Some((fixed_loc(*page, best.1), best.2))
        }
        _ => None,
    }
}
/// Rectángulos en pantalla de un tramo dentro de un trozo pintado.
pub fn rects(p: &Placed, c: &Content, a: i64, b: i64) -> Vec<Rect> {
    match (p, c) {
        (Placed::Flow { flow, range, o, .. }, _) => flow.rects(range.clone(), a.max(0) as u32, b.max(0) as u32).into_iter().map(|r| r.off(o.0, o.1)).collect(),
        (Placed::Fixed { page, r, .. }, Content::Fixed(v)) => {
            let Some(cs) = v.chars.get(page) else { return vec![] };
            if loc_page(a) > *page || loc_page(b) < *page {
                return vec![];
            }
            let s = if loc_page(a) == *page { loc_char(a) } else { 0 };
            let e = if loc_page(b) == *page { loc_char(b) } else { cs.len() };
            fixed_rects(cs, s, e, *r, v.sizes[*page])
        }
        _ => vec![],
    }
}
