//! Composición tipográfica: corte de líneas óptimo (Knuth–Plass), partición
//! silábica, viudas y huérfanas. Todo en unidades lógicas.
use crate::doc::*;
use crate::formats::dims;
use crate::gfx::{rect, Rect};
use crate::text::{face, fonts, shape, FontId};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, PartialEq, Debug)]
pub struct Cfg {
    pub w: f32,
    pub h: f32,
    pub family: usize,
    pub size: f32,
    pub leading: f32,
    pub para: f32,
    pub justify: bool,
    pub hyphen: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct G {
    pub x: f32,
    pub dy: f32,
    pub adv: f32,
    pub size: f32,
    pub pos: u32,
    pub font: FontId,
    pub gid: u16,
    pub style: u16,
    /// Índice + 1 en `links`.
    pub link: u16,
}
#[derive(Clone, Debug)]
pub enum LineKind {
    Text,
    Image(String),
    Rule,
}
#[derive(Clone, Debug)]
pub struct Line {
    pub y: f32,
    pub h: f32,
    /// Línea base desde arriba.
    pub base: f32,
    pub x: f32,
    pub w: f32,
    pub kind: LineKind,
    pub glyphs: std::ops::Range<u32>,
    pub start: u32,
    pub end: u32,
    first: bool,
    last: bool,
    keep: bool,
    brk: bool,
    pub heading: bool,
    pub quote: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Page {
    pub a: u32,
    pub b: u32,
    pub y0: f32,
    pub start: u32,
}
#[derive(Default, Debug)]
pub struct Flow {
    pub lines: Vec<Line>,
    pub glyphs: Vec<G>,
    pub pages: Vec<Page>,
    pub links: Vec<Arc<str>>,
    pub height: f32,
}
pub type Dims = Mutex<HashMap<String, Option<(u32, u32)>>>;

#[derive(Clone, Copy)]
enum It {
    /// Ancho y glifos.
    Box(f32, u32, u32),
    Glue(f32),
    /// Coste y si añade guion.
    Pen(f32, bool),
    Break,
}

struct Styler {
    fam: [FontId; 4],
    size: f32,
    base_style: u16,
}
impl Styler {
    fn pick(&self, st: u16) -> (FontId, f32, f32) {
        let st = st | self.base_style;
        let mut sz = self.size;
        let mut dy = 0.0;
        if st & (SUP | SUB) != 0 {
            dy = if st & SUP != 0 { -0.36 * sz } else { 0.14 * sz };
            sz *= 0.68;
        } else if st & SMALL != 0 {
            sz *= 0.86;
        }
        if st & MONO != 0 {
            return (fonts().mono, sz * 0.84, dy);
        }
        (self.fam[((st & ITALIC != 0) as usize) + 2 * ((st & BOLD != 0) as usize)], sz, dy)
    }
}
fn lang_of(l: &str) -> Option<hypher::Lang> {
    let b = l.as_bytes();
    if b.len() < 2 {
        return Some(hypher::Lang::Spanish);
    }
    hypher::Lang::from_iso([b[0].to_ascii_lowercase(), b[1].to_ascii_lowercase()])
}
fn cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFF60)
}

pub struct Ctx<'a> {
    pub doc: &'a Doc,
    pub cfg: &'a Cfg,
    pub dims: &'a Dims,
}

impl Ctx<'_> {
    pub fn chapter(&self, ci: usize) -> Flow {
        let c = &self.doc.chapters[ci];
        let mut f = Flow::default();
        let em = self.cfg.size;
        let lh = (em * self.cfg.leading).round();
        let mut y = 0.0;
        let mut prev: Option<&Block> = None;
        let mut no_indent = true;
        let mut i = 0;
        while i < c.blocks.len() {
            let b = &c.blocks[i];
            let mut gap = match &b.kind {
                BlockKind::Heading(l) => em * if *l <= 2 { 1.9 } else { 1.4 },
                BlockKind::Rule | BlockKind::Image(_) | BlockKind::Pre => em * 0.8,
                BlockKind::Row(_) => {
                    if prev.is_some_and(|p| matches!(p.kind, BlockKind::Row(_))) { em * 0.35 } else { em }
                }
                BlockKind::Quote => em * if prev.is_some_and(|p| p.quote == b.quote) { self.cfg.para } else { 0.9 },
                BlockKind::Item(..) => em * if prev.is_some_and(|p| matches!(p.kind, BlockKind::Item(..))) { 0.3 } else { 0.7 },
                BlockKind::Para => em * if prev.is_some_and(|p| p.kind != BlockKind::Para) && !no_indent { 0.8 } else { self.cfg.para },
            };
            if let Some(Block { kind: BlockKind::Heading(_), .. }) = prev {
                gap = if matches!(b.kind, BlockKind::Heading(_)) { em * 0.45 } else { em * 0.95 };
            }
            if let Some(Block { kind: BlockKind::Image(_) | BlockKind::Rule, .. }) = prev {
                gap = gap.max(em * 0.8);
            }
            if b.space_before && prev.is_some() {
                gap = gap.max(em * 1.2);
            }
            if prev.is_none() {
                // el título de un capítulo respira desde arriba
                gap = if matches!(b.kind, BlockKind::Heading(1..=3)) { (self.cfg.h * 0.10).min(em * 4.0) } else { 0.0 };
            }
            y += gap.round();
            let y0 = y;
            let first_line = f.lines.len();
            match &b.kind {
                BlockKind::Image(src) => {
                    let Some((iw, ih)) = self.dims_of(src) else {
                        i += 1;
                        continue;
                    };
                    let k = em / 18.0;
                    let (mut w, mut h) = (iw as f32 * k, ih as f32 * k);
                    let max_h = self.cfg.h * 0.94;
                    // las ilustraciones ocupan el ancho; los adornos, su tamaño
                    if iw > 400 || ih > 500 {
                        let s = self.cfg.w / w;
                        w *= s;
                        h *= s;
                    }
                    let s = (self.cfg.w / w).min(max_h / h).min(1.0);
                    let (w, h) = ((w * s).round(), (h * s).round());
                    f.lines.push(Line { y, h, base: h, x: (self.cfg.w - w) / 2.0, w, kind: LineKind::Image(src.clone()), glyphs: 0..0, start: b.start as u32, end: b.start as u32 + 1, first: true, last: true, keep: false, brk: b.break_before, heading: false, quote: false });
                    y += h;
                    no_indent = true;
                }
                BlockKind::Rule => {
                    f.lines.push(Line { y, h: lh, base: lh, x: 0.0, w: self.cfg.w, kind: LineKind::Rule, glyphs: 0..0, start: b.start as u32, end: b.start as u32 + 1, first: true, last: true, keep: false, brk: b.break_before, heading: false, quote: false });
                    y += lh;
                    no_indent = true;
                }
                BlockKind::Row(_) => {
                    let mut e = i;
                    while e < c.blocks.len() && matches!(c.blocks[e].kind, BlockKind::Row(_)) {
                        e += 1;
                    }
                    // anchos comunes a toda la tabla
                    let n = c.blocks[i..e].iter().map(|b| b.text.split('\t').count()).max().unwrap_or(1);
                    let mut wt = vec![4.0f32; n];
                    for r in &c.blocks[i..e] {
                        for (k, cell) in r.text.split('\t').enumerate() {
                            wt[k] = wt[k].max((cell.chars().count() as f32).min(42.0));
                        }
                    }
                    let tot: f32 = wt.iter().sum();
                    let cols: Vec<f32> = wt.iter().map(|w| w / tot * self.cfg.w).collect();
                    for r in &c.blocks[i..e] {
                        self.row(&mut f, r, &cols, &mut y, lh);
                        y += (em * 0.35).round();
                    }
                    i = e;
                    prev = Some(&c.blocks[e - 1]);
                    no_indent = true;
                    continue;
                }
                _ => {
                    let (size, st, lead) = match &b.kind {
                        BlockKind::Heading(l) => (em * [1.75, 1.45, 1.24, 1.1, 1.0, 1.0][(*l as usize - 1).min(5)], BOLD, 1.25),
                        BlockKind::Quote => (em * 0.96, 0, self.cfg.leading),
                        BlockKind::Pre => (em * 0.94, MONO, 1.4),
                        _ => (em, 0, self.cfg.leading),
                    };
                    let q = b.quote as f32 * em * 1.5;
                    let (mut left, mut right) = (q.min(self.cfg.w * 0.3), if b.quote > 0 { em } else { 0.0 });
                    if let BlockKind::Item(_, d) = &b.kind {
                        left += (*d as f32).min(5.0) * em * 1.45;
                    }
                    if self.cfg.w < em * 16.0 {
                        left *= 0.5;
                        right = 0.0;
                    }
                    let heading = matches!(b.kind, BlockKind::Heading(_));
                    let align = match b.align {
                        Align::Auto if heading || b.kind == BlockKind::Pre || !self.cfg.justify => Align::Left,
                        a => a,
                    };
                    let plain = b.kind == BlockKind::Para && matches!(align, Align::Auto | Align::Left);
                    let indent = if plain && !no_indent && self.cfg.para < 0.7 { em * 1.35 } else { 0.0 };
                    let lh2 = (size * lead).round();
                    self.para(&mut f, b, &b.text, b.start, 0, Par { size, st, left, right, indent, align, lh: lh2, heading, quote: b.quote > 0 }, &mut y);
                    if let BlockKind::Item(m, _) = &b.kind {
                        if let Some(l) = f.lines.get_mut(first_line) {
                            // marcador colgado
                            let fam = fonts().families[self.cfg.family.min(fonts().families.len() - 1)].f[0];
                            let sh = shape(fam, m);
                            let mut x = -(sh.w * size) - em * 0.5;
                            let gs = l.glyphs.start as usize;
                            let mk: Vec<G> = sh
                                .g
                                .iter()
                                .map(|g| {
                                    let r = G { x, dy: 0.0, adv: g.adv * size, size, pos: l.start, font: g.font, gid: g.gid, style: 0, link: 0 };
                                    x += g.adv * size;
                                    r
                                })
                                .collect();
                            let n = mk.len() as u32;
                            f.glyphs.splice(gs..gs, mk);
                            l.glyphs.end += n;
                            for k in first_line + 1..f.lines.len() {
                                let g = &mut f.lines[k].glyphs;
                                *g = g.start + n..g.end + n;
                            }
                        }
                    }
                    no_indent = heading;
                }
            }
            let _ = y0;
            if let Some(l) = f.lines.get_mut(first_line) {
                l.brk |= b.break_before && first_line > 0;
            }
            prev = Some(b);
            i += 1;
        }
        f.height = y;
        self.paginate(&mut f);
        f
    }
    fn dims_of(&self, src: &str) -> Option<(u32, u32)> {
        if let Some(d) = self.dims.lock().get(src) {
            return *d;
        }
        let d = self.doc.assets.load(src).and_then(|b| dims(&b));
        self.dims.lock().insert(src.to_string(), d);
        d
    }
    fn row(&self, f: &mut Flow, b: &Block, cols: &[f32], y: &mut f32, _lh: f32) {
        let size = self.cfg.size * 0.88;
        let lh = (size * 1.4).round();
        let head = b.kind == BlockKind::Row(true);
        let mut t = Flow::default();
        let mut x = 0.0;
        let mut off = 0;
        let mut maxh: f32 = lh;
        let gs = f.glyphs.len() as u32;
        let asc = (lh + size * 0.5) / 2.0 + size * 0.06;
        for (k, cell) in b.text.split('\t').enumerate() {
            let w = cols.get(k).copied().unwrap_or(60.0);
            let mut cy = 0.0;
            t.lines.clear();
            t.glyphs.clear();
            let tmp = Block::new(BlockKind::Para);
            self.para(&mut t, &tmp, cell, b.start + off, off, Par { size, st: if head { BOLD } else { 0 }, left: 0.0, right: 10.0, indent: 0.0, align: Align::Left, lh, heading: true, quote: false }, &mut cy);
            let _ = &b.spans;
            self.take(f, &t, x, w);
            maxh = maxh.max(cy);
            x += w;
            off += cell.len() + 1;
        }
        let ge = f.glyphs.len() as u32;
        f.lines.push(Line { y: *y, h: maxh + 6.0, base: asc, x: 0.0, w: self.cfg.w, kind: LineKind::Text, glyphs: gs..ge, start: b.start as u32, end: (b.start + b.len()) as u32, first: true, last: true, keep: head, brk: false, heading: false, quote: false });
        *y += maxh + 6.0;
    }
    fn take(&self, f: &mut Flow, t: &Flow, x: f32, w: f32) {
        let b0 = t.lines.first().map_or(0.0, |l| l.y + l.base);
        for l in &t.lines {
            for g in &t.glyphs[l.glyphs.start as usize..l.glyphs.end as usize] {
                let mut g = *g;
                // la celda se compuso al ancho de página: aquí se recorta a su columna
                g.x += x + l.x;
                g.dy += l.y + l.base - b0;
                let _ = w;
                f.glyphs.push(g);
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn para(&self, f: &mut Flow, b: &Block, text: &str, gstart: usize, _cell: usize, p: Par, y: &mut f32) {
        let fam = fonts().families[self.cfg.family.min(fonts().families.len() - 1)].f;
        let sty = Styler { fam, size: p.size, base_style: p.st };
        let width = (self.cfg.w - p.left - p.right).max(40.0);
        let lang = if self.cfg.hyphen && !p.heading { lang_of(&self.doc.lang) } else { None };
        let mut items: Vec<It> = vec![];
        let mut gl: Vec<G> = vec![];
        let pre = b.kind == BlockKind::Pre;
        // palabras
        let bytes = text.as_bytes();
        let mut i = 0;
        let mut si = 0;
        let style_at = |pos: usize, si: &mut usize| -> (u16, usize, Option<usize>) {
            while *si < b.spans.len() && b.spans[*si].end <= pos {
                *si += 1;
            }
            match b.spans.get(*si) {
                Some(s) if s.start <= pos => (s.style, s.end, s.link.as_ref().map(|_| *si)),
                Some(s) => (0, s.start, None),
                None => (0, usize::MAX, None),
            }
        };
        let mut links: HashMap<usize, u16> = HashMap::new();
        while i < text.len() {
            match bytes[i] {
                b' ' => {
                    let (st, _, _) = style_at(i, &mut si);
                    let (fnt, sz, _) = sty.pick(st);
                    let w = shape(fnt, " ").w * sz;
                    if pre {
                        gl.push(G { x: 0.0, dy: 0.0, adv: w, size: sz, pos: (gstart + i) as u32, font: fnt, gid: 0, style: 0xffff, link: 0 });
                        items.push(It::Box(w, gl.len() as u32 - 1, gl.len() as u32));
                        items.push(It::Pen(0.0, false));
                    } else {
                        items.push(It::Glue(w));
                    }
                    i += 1;
                }
                b'\n' => {
                    items.push(It::Break);
                    i += 1;
                }
                _ => {
                    let mut e = i;
                    while e < text.len() && bytes[e] != b' ' && bytes[e] != b'\n' {
                        e += 1;
                    }
                    let word = &text[i..e];
                    // cortes posibles dentro de la palabra
                    let mut cuts: Vec<(usize, bool)> = vec![];
                    let mut prev_alnum = false;
                    for (o, c) in word.char_indices() {
                        let n = o + c.len_utf8();
                        if c == '\u{ad}' {
                            cuts.push((n, true));
                        } else if matches!(c, '-' | '–' | '—' | '/' | '‐') && prev_alnum && word[n..].starts_with(char::is_alphanumeric) {
                            cuts.push((n, false));
                        } else if cjk(c) && n < word.len() && !word[n..].starts_with(['。', '、', '，', '！', '？', '」', '』', '）']) {
                            cuts.push((n, false));
                        }
                        prev_alnum = c.is_alphanumeric();
                    }
                    if let (Some(l), true) = (lang, cuts.is_empty() && word.chars().count() >= 6) {
                        let core = word.trim_matches(|c: char| !c.is_alphabetic());
                        if core.chars().count() >= 6 && core.chars().all(char::is_alphabetic) && !core.chars().all(char::is_uppercase) {
                            let off = word.find(core).unwrap_or(0);
                            let low = core.to_lowercase();
                            if low.len() == core.len() {
                                let mut o = 0;
                                for s in hypher::hyphenate(&low, l) {
                                    o += s.len();
                                    if o < core.len() && core[..o].chars().count() >= 2 && core[o..].chars().count() >= 3 {
                                        cuts.push((off + o, true));
                                    }
                                }
                            }
                        }
                    }
                    // glifos por tramos de estilo
                    let g0 = gl.len();
                    let mut k = i;
                    while k < e {
                        let (st, until, link) = style_at(k, &mut si);
                        let ke = until.min(e);
                        let piece = &text[k..ke];
                        let (fnt, sz, dy) = sty.pick(st);
                        let li = match link {
                            Some(s) => *links.entry(s).or_insert_with(|| {
                                f.links.push(b.spans[s].link.clone().unwrap());
                                f.links.len() as u16
                            }),
                            None => 0,
                        };
                        let mut add = |t: &str, at: usize, sz: f32| {
                            for g in shape(fnt, t).g.iter() {
                                gl.push(G { x: g.dx * sz, dy: dy - g.dy * sz, adv: g.adv * sz, size: sz, pos: (gstart + at + g.cluster as usize) as u32, font: g.font, gid: g.gid, style: st, link: li });
                            }
                        };
                        if st & SMALLCAPS != 0 {
                            for (o, c) in piece.char_indices() {
                                let up: String = c.to_uppercase().collect();
                                let low = c.is_lowercase();
                                for u in up.chars() {
                                    add(u.encode_utf8(&mut [0; 4]), k + o, if low { sz * 0.8 } else { sz });
                                }
                            }
                        } else {
                            add(piece, k, sz);
                        }
                        k = ke;
                    }
                    // cajas entre cortes
                    let mut start = g0;
                    cuts.push((word.len(), false));
                    for (n, (c, hy)) in cuts.iter().enumerate() {
                        let lim = (gstart + i + c) as u32;
                        let end = if n + 1 == cuts.len() { gl.len() } else { gl[start..].iter().position(|g| g.pos >= lim).map_or(gl.len(), |p| start + p) };
                        if end > start {
                            let w: f32 = gl[start..end].iter().map(|g| g.adv).sum();
                            items.push(It::Box(w, start as u32, end as u32));
                            if n + 1 < cuts.len() {
                                items.push(It::Pen(if *hy { 55.0 } else { 8.0 }, *hy));
                            }
                        }
                        start = end;
                    }
                    i = e;
                }
            }
        }
        if items.is_empty() {
            return;
        }
        // líneas
        let hyph = { let (fnt, sz, _) = sty.pick(0); (fnt, sz, shape(fnt, "-")) };
        let hw = hyph.2.w * hyph.1;
        let just = p.align == Align::Auto;
        let mut segs: Vec<(usize, usize, bool)> = vec![]; // desde, hasta, última del tramo
        let mut s0 = 0;
        for k in 0..=items.len() {
            if k == items.len() || matches!(items[k], It::Break) {
                let ind = if s0 == 0 { p.indent } else { 0.0 };
                let cuts = breaks(&items[s0..k], width, ind, hw, just, pre);
                let mut a = s0;
                for (n, c) in cuts.iter().enumerate() {
                    segs.push((a, s0 + c, n + 1 == cuts.len()));
                    a = s0 + c;
                }
                s0 = k + 1;
            }
        }
        let fc = face(sty.pick(0).0);
        let asc = fc.ascent.min(1.05) * p.size;
        let desc = fc.descent.min(0.4) * p.size;
        let base = ((p.lh + asc - desc) / 2.0).round();
        let nseg = segs.len();
        let first_ix = f.lines.len();
        for (n, (a, z, last)) in segs.into_iter().enumerate() {
            // recorta espacios en los extremos
            let (mut a, mut z) = (a, z);
            let hyphen = z > a && z < items.len() && matches!(items[z], It::Pen(_, true)) || (z > a && matches!(items[z - 1], It::Pen(_, true)) && !last);
            while a < z && matches!(items[a], It::Glue(_) | It::Pen(..) | It::Break) {
                a += 1;
            }
            while z > a && matches!(items[z - 1], It::Glue(_) | It::Pen(..) | It::Break) {
                z -= 1;
            }
            let indent = if n == 0 { p.indent } else { 0.0 };
            let (mut bw, mut gw) = (0.0, 0.0);
            for it in &items[a..z] {
                match it {
                    It::Box(w, ..) => bw += w,
                    It::Glue(w) => gw += w,
                    _ => {}
                }
            }
            if hyphen {
                bw += hw;
            }
            let free = width - indent - bw - gw;
            let k = if just && !last && gw > 0.0 {
                // nunca se abre una línea casi vacía
                (1.0 + free / gw).clamp(0.6, 3.2)
            } else if gw > 0.0 && free < 0.0 {
                (1.0 + free / gw).max(0.6)
            } else {
                1.0
            };
            let used = bw + gw * k;
            let x0 = p.left
                + match p.align {
                    Align::Center => (width - used) / 2.0,
                    Align::Right => width - used,
                    _ => indent,
                };
            let gs = f.glyphs.len() as u32;
            let mut x = 0.0;
            let mut start = u32::MAX;
            let mut end = 0;
            for it in &items[a..z] {
                match it {
                    It::Box(_, s, e) => {
                        for g in &gl[*s as usize..*e as usize] {
                            let mut g = *g;
                            let adv = g.adv;
                            g.x += x;
                            x += adv;
                            start = start.min(g.pos);
                            end = end.max(g.pos);
                            if g.style != 0xffff {
                                f.glyphs.push(g);
                            }
                        }
                    }
                    It::Glue(w) => x += w * k,
                    _ => {}
                }
            }
            if hyphen {
                for g in hyph.2.g.iter() {
                    f.glyphs.push(G { x, dy: 0.0, adv: g.adv * hyph.1, size: hyph.1, pos: end, font: g.font, gid: g.gid, style: 0, link: 0 });
                    x += g.adv * hyph.1;
                }
            }
            if start == u32::MAX {
                start = f.lines.last().map_or(gstart as u32, |l| l.end);
                end = start;
            } else {
                // hasta el final del último carácter
                let o = end as usize - gstart;
                end += text.get(o..).and_then(|t| t.chars().next()).map_or(1, |c| c.len_utf8()) as u32;
            }
            f.lines.push(Line { y: *y, h: p.lh, base, x: x0, w: x, kind: LineKind::Text, glyphs: gs..f.glyphs.len() as u32, start, end, first: n == 0, last: n + 1 == nseg, keep: p.heading, brk: false, heading: p.heading, quote: p.quote });
            *y += p.lh;
        }
        if let Some(l) = f.lines.get_mut(first_ix) {
            l.start = l.start.min(gstart as u32);
        }
        if let Some(l) = f.lines.last_mut() {
            l.end = l.end.max((gstart + text.len()) as u32);
        }
    }
    fn paginate(&self, f: &mut Flow) {
        let h = self.cfg.h;
        let n = f.lines.len();
        let mut a = 0;
        while a < n {
            let y0 = f.lines[a].y;
            let mut e = a + 1;
            while e < n && f.lines[e].y + f.lines[e].h - y0 <= h && !f.lines[e].brk {
                e += 1;
            }
            if e < n && !f.lines[e].brk {
                // viudas, huérfanas y títulos al pie
                let mut k = e;
                for _ in 0..4 {
                    let cut = &f.lines[k];
                    let before = &f.lines[k - 1];
                    let bad = (cut.last && !cut.first) || (before.first && !before.last) || before.keep;
                    if bad && k > a + 1 {
                        k -= 1;
                    } else {
                        break;
                    }
                }
                if k > a + (e - a) / 2 {
                    e = k;
                }
            }
            // el primer título conserva su aire: solo la primera página arranca en 0
            let y0 = if a == 0 { 0.0 } else { y0 };
            f.pages.push(Page { a: a as u32, b: e as u32, y0, start: f.lines[a].start });
            a = e;
        }
        // una página 1 con aire arriba puede pasarse: se comprueba y se quita
        if let Some(p) = f.pages.first_mut() {
            let l = &f.lines[p.b as usize - 1];
            if l.y + l.h > h {
                p.y0 = f.lines[0].y;
            }
        }
    }
}
struct Par {
    size: f32,
    st: u16,
    left: f32,
    right: f32,
    indent: f32,
    align: Align,
    lh: f32,
    heading: bool,
    quote: bool,
}

/// Índices de corte (el último es `items.len()`).
fn breaks(items: &[It], width: f32, indent: f32, hw: f32, just: bool, pre: bool) -> Vec<usize> {
    if items.is_empty() {
        return vec![0];
    }
    if just && !pre {
        if let Some(v) = knuth(items, width, indent, hw) {
            return v;
        }
    }
    // a la vista: cada línea con lo que quepa
    let mut out = vec![];
    let mut w = 0.0;
    let mut glue = 0.0;
    let mut last: Option<(usize, f32)> = None;
    let mut lim = width - indent;
    let mut i = 0;
    let mut a = 0;
    while i < items.len() {
        match items[i] {
            It::Box(bw, ..) => {
                if w + glue + bw > lim + 0.01 && i > a {
                    let cut = match last {
                        Some((c, _)) if c > a => c,
                        _ => i,
                    };
                    out.push(cut);
                    a = cut;
                    i = cut;
                    w = 0.0;
                    glue = 0.0;
                    last = None;
                    lim = width;
                    // sin espacios al principio
                    while i < items.len() && matches!(items[i], It::Glue(_) | It::Pen(..)) {
                        i += 1;
                    }
                    if i >= items.len() {
                        break;
                    }
                    a = a.max(i.min(items.len()));
                    continue;
                }
                w += glue + bw;
                glue = 0.0;
            }
            It::Glue(g) => {
                glue += g;
                last = Some((i, w));
            }
            It::Pen(_, hy) => {
                if !hy || w + hw <= lim {
                    last = Some((i, w));
                }
            }
            It::Break => {}
        }
        i += 1;
    }
    out.push(items.len());
    out
}

fn knuth(items: &[It], width: f32, indent: f32, hw: f32) -> Option<Vec<usize>> {
    #[derive(Clone, Copy)]
    struct Node {
        at: usize,
        w: f32,
        g: f32,
        dem: f64,
        prev: u32,
        hy: bool,
        first: bool,
    }
    let mut nodes: Vec<Node> = vec![Node { at: 0, w: 0.0, g: 0.0, dem: 0.0, prev: u32::MAX, hy: false, first: true }];
    let mut active: Vec<u32> = vec![0];
    let (mut w, mut g) = (0.0f32, 0.0f32);
    let mut cand: Vec<(u32, f64, bool)> = vec![];
    let n = items.len();
    for i in 0..=n {
        let (legal, cost, hy, end) = if i == n {
            (true, 0.0, false, true)
        } else {
            match items[i] {
                It::Glue(_) => (i > 0 && matches!(items[i - 1], It::Box(..)), 0.0, false, false),
                It::Pen(c, h) => (true, c, h, false),
                _ => (false, 0.0, false, false),
            }
        };
        if legal {
            cand.clear();
            let mut keep = Vec::with_capacity(active.len());
            let mut over_best: Option<(u32, f64)> = None;
            for &ai in &active {
                let a = nodes[ai as usize];
                let lw = width - if a.first { indent } else { 0.0 };
                let bw = w - a.w + if hy { hw } else { 0.0 };
                let gw = g - a.g;
                let free = lw - bw - gw;
                let r = if free >= 0.0 {
                    if gw > 0.0 { free / (gw * 0.55) } else if free < 1.0 { 0.0 } else { 9.0 }
                } else if gw > 0.0 {
                    free / (gw * 0.28)
                } else {
                    -9.0
                };
                if r < -1.0 {
                    // ya no cabe desde aquí: deja de estar activa
                    if over_best.is_none_or(|o| a.dem < o.1) {
                        over_best = Some((ai, a.dem));
                    }
                } else {
                    keep.push(ai);
                    let bad = if end { if r < 0.0 { 100.0 * (-r as f64).powi(3) } else { 0.0 } } else { 100.0 * (r.abs() as f64).powi(3) };
                    let mut d = (1.0 + bad + cost as f64).powi(2);
                    if hy && a.hy {
                        d += 6000.0;
                    }
                    if end && a.hy {
                        d += 3000.0;
                    }
                    if r > 6.0 && !end {
                        d += 1e7;
                    }
                    cand.push((ai, a.dem + d, hy));
                }
            }
            if cand.is_empty() {
                // nada encaja: se fuerza el menos malo
                let (ai, dem) = over_best?;
                if keep.is_empty() {
                    // se vuelve atrás hasta el último corte posible de esa línea
                    cand.push((ai, dem + 1e9, hy));
                }
            }
            if let Some(best) = cand.iter().min_by(|x, y| x.1.total_cmp(&y.1)).copied() {
                // lo que venga después empieza tras los espacios
                let (mut nw, mut ng) = (w, g);
                let mut k = i;
                while k < n {
                    match items[k] {
                        It::Glue(x) => ng += x,
                        It::Pen(..) => {}
                        _ => break,
                    }
                    k += 1;
                }
                if !matches!(items.get(i), Some(It::Glue(_))) {
                    nw = w;
                    ng = g;
                }
                nodes.push(Node { at: i, w: nw, g: ng, dem: best.1, prev: best.0, hy: best.2, first: false });
                keep.push(nodes.len() as u32 - 1);
            }
            active = keep;
            if active.len() > 60 {
                active.sort_by(|a, b| nodes[*a as usize].dem.total_cmp(&nodes[*b as usize].dem));
                active.truncate(40);
            }
        }
        if i < n {
            match items[i] {
                It::Box(x, ..) => w += x,
                It::Glue(x) => g += x,
                _ => {}
            }
        }
    }
    let mut cur = nodes.len() as u32 - 1;
    if nodes[cur as usize].at != n {
        return None;
    }
    let mut v = vec![];
    while cur != u32::MAX && nodes[cur as usize].prev != u32::MAX {
        v.push(nodes[cur as usize].at);
        cur = nodes[cur as usize].prev;
    }
    v.reverse();
    Some(v)
}

impl Flow {
    pub fn page_of(&self, loc: u32) -> usize {
        self.pages.partition_point(|p| p.start <= loc).saturating_sub(1)
    }
    pub fn line_of(&self, loc: u32) -> usize {
        self.lines.partition_point(|l| l.start <= loc).saturating_sub(1)
    }
    /// Posición bajo un punto, relativo al origen del flujo.
    pub fn hit(&self, range: std::ops::Range<usize>, x: f32, y: f32) -> Option<(u32, bool)> {
        let mut best: Option<(f32, usize)> = None;
        for i in range {
            let l = &self.lines[i];
            let d = if y < l.y { l.y - y } else if y > l.y + l.h { y - l.y - l.h } else { 0.0 };
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, i));
            }
        }
        let (dy, i) = best?;
        let l = &self.lines[i];
        if !matches!(l.kind, LineKind::Text) {
            return Some((l.start, false));
        }
        let lx = x - l.x;
        let gs = &self.glyphs[l.glyphs.start as usize..l.glyphs.end as usize];
        let inside = dy == 0.0 && lx >= -2.0 && lx <= l.w + 2.0;
        for (k, g) in gs.iter().enumerate() {
            if lx < g.x + g.adv / 2.0 {
                return Some((g.pos, inside));
            }
            let nx = gs.get(k + 1).map_or(g.x + g.adv, |n| n.x);
            if lx < nx {
                return Some((gs.get(k + 1).map_or(l.end, |n| n.pos), inside));
            }
        }
        Some((l.end, inside))
    }
    /// Glifo exacto bajo el punto (para enlaces).
    pub fn glyph_at(&self, range: std::ops::Range<usize>, x: f32, y: f32) -> Option<&G> {
        for l in &self.lines[range] {
            if y >= l.y && y < l.y + l.h {
                let lx = x - l.x;
                return self.glyphs[l.glyphs.start as usize..l.glyphs.end as usize].iter().find(|g| lx >= g.x - 1.0 && lx < g.x + g.adv + 1.0);
            }
        }
        None
    }
    /// Rectángulos del tramo [a, b) entre unas líneas.
    pub fn rects(&self, range: std::ops::Range<usize>, a: u32, b: u32) -> Vec<Rect> {
        let mut out = vec![];
        for l in &self.lines[range] {
            if l.end <= a || l.start >= b {
                continue;
            }
            if !matches!(l.kind, LineKind::Text) {
                continue;
            }
            let gs = &self.glyphs[l.glyphs.start as usize..l.glyphs.end as usize];
            let x0 = gs.iter().find(|g| g.pos >= a).map(|g| g.x);
            let x1 = gs.iter().rev().find(|g| g.pos < b).map(|g| g.x + g.adv);
            if let (Some(x0), Some(x1)) = (x0, x1) {
                if x1 > x0 {
                    out.push(rect(l.x + x0, l.y, x1 - x0, l.h));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn doc(s: &str) -> Doc {
        let mut b = Block::new(BlockKind::Para);
        b.text = s.into();
        Doc::build(vec![Chapter { blocks: vec![b], ..Default::default() }], vec![], Arc::new(NoAssets), "es".into())
    }
    fn cfg(just: bool) -> Cfg {
        Cfg { w: 320.0, h: 400.0, family: 0, size: 18.0, leading: 1.5, para: 0.0, justify: just, hyphen: true }
    }
    const T: &str = "En un lugar de la Mancha, de cuyo nombre no quiero acordarme, no ha mucho tiempo que vivía un hidalgo de los de lanza en astillero, adarga antigua, rocín flaco y galgo corredor. Una olla de algo más vaca que carnero, salpicón las más noches, duelos y quebrantos los sábados, lentejas los viernes, algún palomino de añadidura los domingos, consumían las tres partes de su hacienda.";
    #[test]
    fn fills_width_and_keeps_text() {
        for just in [true, false] {
            let d = doc(T);
            let c = cfg(just);
            let dm = Dims::default();
            let f = Ctx { doc: &d, cfg: &c, dims: &dm }.chapter(0);
            assert!(f.lines.len() > 8);
            for (i, l) in f.lines.iter().enumerate() {
                assert!(l.x + l.w <= c.w + 1.5, "línea {i} se sale: {} {just}", l.x + l.w);
                if just && i + 1 < f.lines.len() {
                    assert!((l.x + l.w - c.w).abs() < 1.5, "línea {i} no llega: {}", l.x + l.w);
                }
                if i > 0 {
                    assert!(f.lines[i - 1].end <= l.start + 1);
                }
            }
            // todos los caracteres visibles están
            let n = f.glyphs.iter().filter(|g| g.style == 0).count();
            assert!(n >= T.chars().filter(|c| *c != ' ').count());
        }
    }
    #[test]
    fn long_word_and_empty() {
        for s in ["", "Supercalifragilísticoespialidosootravezsupercalifragilístico", "a", "—¿?", "x\ny\n\nz"] {
            let d = doc(s);
            let c = cfg(true);
            let dm = Dims::default();
            let _ = Ctx { doc: &d, cfg: &c, dims: &dm }.chapter(0);
        }
    }
}
