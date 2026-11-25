//! Documentos de páginas fijas: PDF, DjVu y cómics.
use super::{year_of, Cover, Fixed, Meta, PChar};
use crate::doc::{looks_like_chapter, Pic, Toc};
use hayro::hayro_interpret::font::{Glyph, GlyphRun};
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::hayro_syntax;
use hayro::hayro_interpret::{interpret_page, BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache, InterpreterSettings, SoftMask};
use hayro::kurbo::{Affine, BezPath, Point, Rect as KRect};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};
use hayro_syntax::object::{Array, Dict, MaybeRef, Name, Object, ObjectIdentifier, String as PStr};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn guard<T>(f: impl FnOnce() -> T) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok()
}

// ---------------------------------------------------------------- PDF
pub struct Pdf {
    pdf: hayro_syntax::Pdf,
    sizes: Vec<(f32, f32)>,
    toc: Vec<Toc>,
}
impl Pdf {
    fn load(p: &Path) -> Result<hayro_syntax::Pdf, String> {
        let data = std::fs::read(p).map_err(|_| "No se puede abrir el archivo")?;
        match guard(|| hayro_syntax::Pdf::new(Arc::new(data))) {
            Some(Ok(p)) => Ok(p),
            Some(Err(hayro_syntax::LoadPdfError::Decryption(_))) => Err("Este PDF está protegido con contraseña".into()),
            _ => Err("El PDF está dañado".into()),
        }
    }
    pub fn open(p: &Path) -> Result<Pdf, String> {
        let pdf = Self::load(p)?;
        let sizes = pdf.pages().iter().map(|p| p.render_dimensions()).collect();
        let toc = guard(|| outline(&pdf)).unwrap_or_default();
        Ok(Pdf { pdf, sizes, toc })
    }
    pub fn probe(p: &Path) -> (Meta, Cover) {
        let mut m = Meta::default();
        let Ok(d) = Self::open(p) else { return (m, Cover::None) };
        let md = d.pdf.metadata();
        let s = |b: &Option<Box<[u8]>>| b.as_deref().map(pdf_str).unwrap_or_default();
        m.title = s(&md.title);
        m.authors = s(&md.author).split(';').map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();
        m.description = s(&md.subject);
        m.year = md.creation_date.as_ref().map(|d| year_of(&format!("{d:?}"))).unwrap_or(0);
        m.pages = d.count() as i32;
        let (w, h) = d.size(0);
        let k = 1100.0 / h.max(1.0);
        let c = d.render(0, (w * k) as u32, 1100).map_or(Cover::None, Cover::Pixels);
        (m, c)
    }
    fn lines(&self, i: usize) -> Vec<(String, f32)> {
        let mut out: Vec<(String, f32)> = vec![];
        let mut cur = (String::new(), 0.0f32);
        for c in self.text(i) {
            if c.c == '\n' {
                if !cur.0.trim().is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                cur = (String::new(), 0.0);
            } else {
                cur.0.push(c.c);
                cur.1 = cur.1.max(c.h);
            }
        }
        if !cur.0.trim().is_empty() {
            out.push(cur);
        }
        out
    }
    /// Sin marcadores: títulos por tamaño de letra o por su forma.
    fn guess(&self) -> Vec<Toc> {
        use rayon::prelude::*;
        let n = self.count().min(1200);
        let pages: Vec<Vec<(String, f32)>> = (0..n).into_par_iter().map(|i| guard(|| self.lines(i)).unwrap_or_default()).collect();
        let mut hs: Vec<i32> = pages.iter().flatten().flat_map(|l| std::iter::repeat_n((l.1 * 2.0) as i32, (l.0.len() / 10).max(1))).collect();
        if hs.is_empty() {
            return vec![];
        }
        hs.sort_unstable();
        let body = hs[hs.len() / 2] as f32 / 2.0;
        let mut v = vec![];
        for (i, p) in pages.iter().enumerate() {
            for (t, h) in p.iter().take(12) {
                let t = t.trim();
                let big = *h > body * 1.35 && t.chars().count() > 2 && t.chars().count() < 80 && t.chars().filter(|c| c.is_alphabetic()).count() > 2;
                if looks_like_chapter(t) && *h >= body * 0.95 || big {
                    v.push((Toc { title: t.split_whitespace().collect::<Vec<_>>().join(" "), depth: 0, loc: i as i64 }, *h));
                    break;
                }
            }
        }
        // cabeceras repetidas no son títulos
        let mut count: HashMap<String, usize> = HashMap::new();
        for (t, _) in &v {
            *count.entry(t.title.clone()).or_default() += 1;
        }
        let v: Vec<Toc> = v.into_iter().filter(|(t, _)| count[&t.title] < 3).map(|x| x.0).collect();
        // si casi cada página «tiene título» son diapositivas: también sirve de guía
        v
    }
}
impl Fixed for Pdf {
    fn count(&self) -> usize {
        self.sizes.len()
    }
    fn size(&self, i: usize) -> (f32, f32) {
        self.sizes.get(i).copied().unwrap_or((595.0, 842.0))
    }
    fn render(&self, i: usize, w: u32, h: u32) -> Option<Pic> {
        guard(|| {
            let page = self.pdf.pages().get(i)?;
            let (pw, ph) = self.size(i);
            let (w, h) = (w.clamp(8, 6000), h.clamp(8, 6000));
            let s = PixmapSettings { x_scale: (w as f32 + 0.01) / pw, y_scale: (h as f32 + 0.01) / ph, bg_color: WHITE };
            let cache = RenderCache::new();
            let pm = render(page, &cache, &InterpreterSettings::default(), &RenderSettings::default(), &s);
            let (w, h) = (pm.width() as u32, pm.height() as u32);
            let mut rgba = pm.data_as_u8_slice().to_vec();
            for p in rgba.chunks_exact_mut(4) {
                p[3] = 255;
            }
            Some(Pic { w, h, rgba })
        })
        .flatten()
    }
    fn text(&self, i: usize) -> Vec<PChar> {
        guard(|| {
            let Some(page) = self.pdf.pages().get(i) else { return vec![] };
            let t = page.initial_transform(true).to_kurbo_();
            let (w, h) = self.size(i);
            let cache = InterpreterCache::new();
            let mut ctx = Context::new(t, KRect::new(0.0, 0.0, w as f64, h as f64), &cache, page.xref(), InterpreterSettings::default());
            let mut dev = TextDev::default();
            interpret_page(page, &mut ctx, &mut dev);
            tidy(dev.out, w, h)
        })
        .unwrap_or_default()
    }
    fn toc(&self) -> Vec<Toc> {
        if self.toc.is_empty() {
            return guard(|| self.guess()).unwrap_or_default();
        }
        self.toc.clone()
    }
}
trait ToK {
    fn to_kurbo_(&self) -> Affine;
}
impl ToK for hayro_syntax::transform::Transform {
    fn to_kurbo_(&self) -> Affine {
        use hayro::hayro_interpret::util::TransformExt;
        self.to_kurbo()
    }
}
#[derive(Default)]
struct TextDev {
    out: Vec<PChar>,
}
impl<'a> Device<'a> for TextDev {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        for g in run.glyphs() {
            let t = props.transform * g.transform();
            let o = t * Point::new(0.0, 0.0);
            let adv = match &**g {
                Glyph::Outline(o) => o.advance_width().unwrap_or(500.0),
                Glyph::Type3(t) => t.advance_width().unwrap_or(500.0),
            } as f64;
            let e = t * Point::new(adv, 0.0);
            let up = t * Point::new(0.0, 1000.0);
            let h = ((up.x - o.x).powi(2) + (up.y - o.y).powi(2)).sqrt() as f32;
            // solo texto horizontal: lo girado no se puede elegir línea a línea
            let w = (e.x - o.x).abs() as f32;
            if h < 0.5 || h > 400.0 {
                continue;
            }
            let chars: Vec<char> = match g.as_unicode() {
                Some(BfString::Char(c)) => vec![c],
                Some(BfString::String(s)) => s.chars().collect(),
                None => continue,
            };
            let n = chars.len().max(1) as f32;
            for (i, c) in chars.into_iter().enumerate() {
                if c.is_control() || c == '\u{fffd}' {
                    continue;
                }
                let c = if c == '\u{a0}' { ' ' } else { c };
                self.out.push(PChar { c, x: o.x.min(e.x) as f32 + w / n * i as f32, y: o.y as f32 - h * 0.78, w: w / n, h });
            }
        }
    }
}
/// Ordena en líneas y repone los espacios que el PDF no escribe.
fn tidy(chars: Vec<PChar>, w: f32, h: f32) -> Vec<PChar> {
    let mut out: Vec<PChar> = Vec::with_capacity(chars.len() + chars.len() / 5);
    for c in chars {
        if c.x < -5.0 || c.y < -20.0 || c.x > w + 5.0 || c.y > h + 5.0 {
            continue;
        }
        if let Some(l) = out.last().copied() {
            let same = (l.y + l.h * 0.78 - (c.y + c.h * 0.78)).abs() < l.h.max(c.h) * 0.55;
            let gap = c.x - (l.x + l.w);
            if !same || gap < -l.h * 2.0 {
                if l.c == ' ' {
                    out.pop();
                }
                if let Some(l) = out.last().copied() {
                    out.push(PChar { c: '\n', x: l.x + l.w, y: l.y, w: 0.0, h: l.h });
                }
            } else if gap > c.h * 0.17 && l.c != ' ' && c.c != ' ' {
                out.push(PChar { c: ' ', x: l.x + l.w, y: l.y, w: gap, h: l.h });
            }
        }
        // negrita falsa: el mismo carácter pintado dos veces
        if let Some(l) = out.last() {
            if l.c == c.c && (l.x - c.x).abs() < c.w * 0.3 && (l.y - c.y).abs() < 1.0 {
                continue;
            }
            if l.c == ' ' && c.c == ' ' {
                continue;
            }
        }
        out.push(c);
    }
    out
}
fn pdf_str(b: &[u8]) -> String {
    if b.starts_with(&[0xfe, 0xff]) {
        return encoding_rs::UTF_16BE.decode(&b[2..]).0.trim_matches('\0').trim().to_string();
    }
    if b.starts_with(&[0xff, 0xfe]) {
        return encoding_rs::UTF_16LE.decode(&b[2..]).0.trim().to_string();
    }
    if b.starts_with(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(&b[3..]).trim().to_string();
    }
    match std::str::from_utf8(b) {
        Ok(s) => s.trim().to_string(),
        Err(_) => encoding_rs::WINDOWS_1252.decode(b).0.trim().to_string(),
    }
}
fn outline(pdf: &hayro_syntax::Pdf) -> Vec<Toc> {
    let x = pdf.xref();
    let mut pages: HashMap<ObjectIdentifier, usize> = HashMap::new();
    for (i, p) in pdf.pages().iter().enumerate() {
        if let Some(id) = p.raw().obj_id() {
            pages.insert(id, i);
        }
    }
    let Some(cat) = x.get::<Dict<'_>>(x.root_id()) else { return vec![] };
    let mut names: HashMap<Vec<u8>, usize> = HashMap::new();
    let dest_arr = |a: &Array<'_>| -> Option<usize> {
        match a.raw_iter().next()? {
            MaybeRef::Ref(r) => pages.get(&r.into()).copied(),
            MaybeRef::NotRef(Object::Number(n)) => Some(n.as_f64().max(0.0) as usize),
            _ => None,
        }
    };
    let dest_obj = |o: Object<'_>| -> Option<usize> {
        match o {
            Object::Array(a) => dest_arr(&a),
            Object::Dict(d) => d.get::<Array<'_>>("D").and_then(|a| dest_arr(&a)),
            _ => None,
        }
    };
    if let Some(d) = cat.get::<Dict<'_>>("Dests") {
        for k in d.keys() {
            if let Some(p) = d.get::<Object<'_>>(&*k).and_then(dest_obj) {
                names.insert(k.to_vec(), p);
            }
        }
    }
    fn tree(d: &Dict<'_>, f: &mut dyn FnMut(Vec<u8>, Object<'_>), depth: u32) {
        if depth > 24 {
            return;
        }
        if let Some(a) = d.get::<Array<'_>>("Names") {
            let v: Vec<Object<'_>> = a.iter::<Object<'_>>().collect();
            for p in v.chunks_exact(2) {
                if let Object::String(s) = &p[0] {
                    f(s.to_vec(), p[1].clone());
                }
            }
        }
        if let Some(k) = d.get::<Array<'_>>("Kids") {
            for c in k.iter::<Dict<'_>>() {
                tree(&c, f, depth + 1);
            }
        }
    }
    if let Some(d) = cat.get::<Dict<'_>>("Names").and_then(|n| n.get::<Dict<'_>>("Dests")) {
        tree(
            &d,
            &mut |k, o| {
                if let Some(p) = dest_obj(o) {
                    names.insert(k, p);
                }
            },
            0,
        );
    }
    let Some(root) = cat.get::<Dict<'_>>("Outlines") else { return vec![] };
    let mut out = vec![];
    let target = |d: &Dict<'_>| -> Option<usize> {
        let o = d.get::<Object<'_>>("Dest").or_else(|| d.get::<Dict<'_>>("A").and_then(|a| a.get::<Object<'_>>("D")))?;
        match &o {
            Object::String(s) => names.get(&s.to_vec()).copied(),
            Object::Name(n) => names.get(&n.to_vec()).copied(),
            _ => dest_obj(o),
        }
    };
    fn walk<'a>(first: Option<Dict<'a>>, depth: u8, target: &dyn Fn(&Dict<'_>) -> Option<usize>, out: &mut Vec<Toc>) {
        let mut cur = first;
        let mut n = 0;
        while let Some(d) = cur {
            n += 1;
            if out.len() > 5000 || n > 5000 {
                return;
            }
            let t = d.get::<PStr<'_>>("Title").map(|s| pdf_str(&s)).unwrap_or_default();
            if let (Some(p), false) = (target(&d), t.is_empty()) {
                out.push(Toc { title: t.split_whitespace().collect::<Vec<_>>().join(" "), depth, loc: p as i64 });
            }
            if depth < 8 {
                walk(d.get::<Dict<'_>>("First"), depth + 1, target, out);
            }
            cur = d.get::<Dict<'_>>("Next");
        }
    }
    walk(root.get::<Dict<'_>>("First"), 0, &target, &mut out);
    let _: Option<Name<'_>> = None;
    out
}

// ---------------------------------------------------------------- DjVu
pub struct Djvu {
    doc: Mutex<djvu_rs::DjVuDocument>,
    sizes: Vec<(f32, f32)>,
    toc: Vec<Toc>,
}
impl Djvu {
    pub fn open(p: &Path) -> Result<Djvu, String> {
        let data = std::fs::read(p).map_err(|_| "No se puede abrir el archivo")?;
        let doc = guard(|| djvu_rs::DjVuDocument::parse(&data)).and_then(|r| r.ok()).ok_or("El DjVu está dañado")?;
        let mut sizes = vec![];
        for i in 0..doc.page_count() {
            let s = doc.page(i).map(|p| (p.width() as f32 * 72.0 / p.dpi().max(1) as f32, p.height() as f32 * 72.0 / p.dpi().max(1) as f32)).unwrap_or((595.0, 842.0));
            sizes.push(s);
        }
        let mut toc = vec![];
        fn walk(b: &[djvu_rs::DjVuBookmark], d: u8, n: usize, out: &mut Vec<Toc>) {
            for x in b {
                let pg = x.url.trim_start_matches('#').chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<usize>().unwrap_or(1);
                out.push(Toc { title: x.title.clone(), depth: d, loc: pg.saturating_sub(1).min(n.saturating_sub(1)) as i64 });
                walk(&x.children, d + 1, n, out);
            }
        }
        walk(doc.bookmarks(), 0, sizes.len(), &mut toc);
        Ok(Djvu { doc: Mutex::new(doc), sizes, toc })
    }
    pub fn probe(p: &Path) -> (Meta, Cover) {
        let Ok(d) = Self::open(p) else { return (Meta::default(), Cover::None) };
        let (w, h) = d.size(0);
        let m = Meta { pages: d.count() as i32, ..Default::default() };
        (m, d.render(0, (w * 1100.0 / h.max(1.0)) as u32, 1100).map_or(Cover::None, Cover::Pixels))
    }
}
impl Fixed for Djvu {
    fn count(&self) -> usize {
        self.sizes.len()
    }
    fn size(&self, i: usize) -> (f32, f32) {
        self.sizes.get(i).copied().unwrap_or((595.0, 842.0))
    }
    fn render(&self, i: usize, w: u32, h: u32) -> Option<Pic> {
        let d = self.doc.lock();
        guard(|| {
            let page = d.page(i).ok()?;
            let o = djvu_rs::djvu_render::RenderOptions::new(w.clamp(8, 6000), h.clamp(8, 6000));
            let p = djvu_rs::djvu_render::render_pixmap(page, &o).ok()?;
            Some(Pic { w: p.width, h: p.height, rgba: p.data.clone() })
        })
        .flatten()
    }
    fn text(&self, i: usize) -> Vec<PChar> {
        let d = self.doc.lock();
        guard(|| {
            let Ok(page) = d.page(i) else { return vec![] };
            let Ok(Some(t)) = page.text_layer() else { return vec![] };
            let k = 72.0 / page.dpi().max(1) as f32;
            let mut out: Vec<PChar> = vec![];
            fn walk(z: &djvu_rs::TextZone, k: f32, out: &mut Vec<PChar>) {
                if !z.children.is_empty() {
                    for c in &z.children {
                        walk(c, k, out);
                    }
                    return;
                }
                let t = z.text.trim();
                let n = t.chars().count().max(1) as f32;
                let (x, y, w, h) = (z.rect.x as f32 * k, z.rect.y as f32 * k, z.rect.width as f32 * k, z.rect.height as f32 * k);
                if let Some(l) = out.last().copied() {
                    let nl = y > l.y + l.h * 0.6 || x < l.x - l.h;
                    out.push(PChar { c: if nl { '\n' } else { ' ' }, x: l.x + l.w, y: l.y, w: if nl { 0.0 } else { (x - l.x - l.w).max(0.0) }, h: l.h });
                }
                for (i, c) in t.chars().enumerate() {
                    out.push(PChar { c, x: x + w / n * i as f32, y, w: w / n, h });
                }
            }
            for z in &t.zones {
                walk(z, k, &mut out);
            }
            out
        })
        .unwrap_or_default()
    }
    fn toc(&self) -> Vec<Toc> {
        self.toc.clone()
    }
}

// ---------------------------------------------------------------- cómics
pub fn is_image(n: &str) -> bool {
    let l = n.to_lowercase();
    !l.contains("__macosx") && [".jpg", ".jpeg", ".png", ".webp", ".gif", ".bmp"].iter().any(|e| l.ends_with(e))
}
/// Orden natural: «pág 2» antes que «pág 10».
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let num = |i: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut v = 0u64;
                    while let Some(c) = i.peek().and_then(|c| c.to_digit(10)) {
                        v = v.saturating_mul(10).saturating_add(c as u64);
                        i.next();
                    }
                    v
                };
                let o = num(&mut x).cmp(&num(&mut y));
                if o.is_ne() {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.to_lowercase().cmp(d.to_lowercase());
                if o.is_ne() {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}
enum Store {
    Zip(Mutex<super::epub::Zip>),
    /// RAR, 7z y TAR se abren enteros: no permiten saltar a una página.
    Mem(HashMap<String, Vec<u8>>),
}
pub struct Comic {
    store: Store,
    names: Vec<String>,
    sizes: Vec<(f32, f32)>,
}
impl Comic {
    fn store(p: &Path, f: &str, only_first: bool) -> Result<(Store, Vec<String>), String> {
        let bad = || "El cómic está dañado".to_string();
        let store = match f {
            "cbz" => Store::Zip(Mutex::new(super::epub::zip_open(p)?)),
            "cbr" => {
                let mut m = HashMap::new();
                let mut best: Option<String> = None;
                if only_first {
                    // solo la primera página, para la portada
                    for e in unrar::Archive::new(p).open_for_listing().map_err(|_| bad())?.flatten() {
                        let n = e.filename.to_string_lossy().to_string();
                        if is_image(&n) && best.as_ref().is_none_or(|b| natural(&n, b).is_lt()) {
                            best = Some(n);
                        }
                    }
                }
                let mut a = unrar::Archive::new(p).open_for_processing().map_err(|_| bad())?;
                while let Some(h) = a.read_header().map_err(|_| bad())? {
                    let n = h.entry().filename.to_string_lossy().to_string();
                    let want = is_image(&n) && best.as_ref().is_none_or(|b| b == &n);
                    if want && h.entry().is_file() {
                        let (d, r) = h.read().map_err(|_| bad())?;
                        m.insert(n, d);
                        a = r;
                        if only_first {
                            break;
                        }
                    } else {
                        a = h.skip().map_err(|_| bad())?;
                    }
                }
                Store::Mem(m)
            }
            "cb7" => {
                let mut m = HashMap::new();
                let mut r = sevenz_rust2::ArchiveReader::open(p, sevenz_rust2::Password::empty()).map_err(|_| bad())?;
                r.for_each_entries(|e, d| {
                    let n = e.name().to_string();
                    let mut v = vec![];
                    d.read_to_end(&mut v)?;
                    if is_image(&n) {
                        m.insert(n, v);
                    }
                    Ok(true)
                })
                .map_err(|_| bad())?;
                Store::Mem(m)
            }
            _ => Store::Mem(untar(&std::fs::read(p).map_err(|_| bad())?)),
        };
        let mut names: Vec<String> = match &store {
            Store::Zip(z) => super::epub::zip_names(&z.lock()).into_iter().filter(|n| is_image(n)).collect(),
            Store::Mem(m) => m.keys().filter(|n| is_image(n)).cloned().collect(),
        };
        names.sort_by(|a, b| natural(a, b));
        Ok((store, names))
    }
    pub fn open(p: &Path, f: &str) -> Result<Comic, String> {
        let (store, names) = Self::store(p, f, false)?;
        let mut c = Comic { store, names, sizes: vec![] };
        c.sizes = (0..c.names.len()).map(|i| c.bytes(i).and_then(|b| super::dims(&b)).map_or((800.0, 1200.0), |d| (d.0 as f32, d.1 as f32))).collect();
        Ok(c)
    }
    fn bytes(&self, i: usize) -> Option<Vec<u8>> {
        let n = self.names.get(i)?;
        match &self.store {
            Store::Zip(z) => super::epub::zip_read(&mut z.lock(), n),
            Store::Mem(m) => m.get(n).cloned(),
        }
    }
    pub fn probe(p: &Path, f: &str) -> (Meta, Cover) {
        let mut m = Meta::default();
        let Ok((store, names)) = Self::store(p, f, true) else { return (m, Cover::None) };
        let c = Comic { store, sizes: vec![], names };
        m.pages = if matches!(c.store, Store::Zip(_)) { c.names.len() as i32 } else { 0 };
        if let Store::Zip(z) = &c.store {
            let mut z = z.lock();
            let info = super::epub::zip_names(&z).into_iter().find(|n| n.to_lowercase().ends_with("comicinfo.xml"));
            if let Some(b) = info.and_then(|n| super::epub::zip_read(&mut z, &n)) {
                let x = super::html::xml(&super::to_text(&b));
                let g = |k: &str| x.first(k).map(|n| n.full()).unwrap_or_default();
                m.series = g("series");
                m.series_ix = g("number").parse().unwrap_or(0.0);
                m.title = g("title");
                if m.title.is_empty() && !m.series.is_empty() {
                    m.title = format!("{} {}", m.series, g("number")).trim().to_string();
                }
                m.authors = g("writer").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                m.publisher = g("publisher");
                m.year = year_of(&g("year"));
                m.description = g("summary");
                m.genres = g("genre").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                m.language = g("languageiso");
            }
        }
        if m.genres.is_empty() {
            m.genres = vec!["Cómic".into()];
        }
        (m, c.bytes(0).map_or(Cover::None, Cover::Encoded))
    }
}
impl Fixed for Comic {
    fn count(&self) -> usize {
        self.names.len()
    }
    fn size(&self, i: usize) -> (f32, f32) {
        self.sizes.get(i).copied().unwrap_or((800.0, 1200.0))
    }
    fn render(&self, i: usize, w: u32, h: u32) -> Option<Pic> {
        let img = guard(|| super::decode(&self.bytes(i)?)).flatten()?;
        let (iw, ih) = img.dimensions();
        let img = if w < iw && h < ih { image::imageops::resize(&img, w.max(1), h.max(1), image::imageops::FilterType::CatmullRom) } else { img };
        Some(Pic { w: img.width(), h: img.height(), rgba: img.into_raw() })
    }
    fn text(&self, _: usize) -> Vec<PChar> {
        vec![]
    }
    fn toc(&self) -> Vec<Toc> {
        // carpetas internas = capítulos
        let mut v: Vec<Toc> = vec![];
        for (i, n) in self.names.iter().enumerate() {
            let d = PathBuf::from(n).parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if !d.is_empty() && v.last().is_none_or(|t| t.title != d) {
                v.push(Toc { title: d, depth: 0, loc: i as i64 });
            }
        }
        if v.len() < 2 {
            v.clear();
        }
        v
    }
    fn is_comic(&self) -> bool {
        true
    }
}
fn untar(b: &[u8]) -> HashMap<String, Vec<u8>> {
    let mut m = HashMap::new();
    let mut o = 0;
    while o + 512 <= b.len() {
        let h = &b[o..o + 512];
        if h[0] == 0 {
            break;
        }
        let z = |s: &[u8]| String::from_utf8_lossy(s).trim_end_matches(['\0', ' ']).trim().to_string();
        let name = z(&h[..100]);
        let size = usize::from_str_radix(&z(&h[124..136]), 8).unwrap_or(0);
        o += 512;
        if let (true, Some(d)) = (is_image(&name), b.get(o..o + size)) {
            m.insert(name, d.to_vec());
        }
        o += size.div_ceil(512) * 512;
    }
    m
}
