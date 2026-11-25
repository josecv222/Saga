//! Letras: registro de tipografías, composición de palabras y atlas de glifos.
use crate::gfx::{rect, Color, Draw, Gpu, Rect, ATLAS};
use crate::icons::Icon;
use parking_lot::RwLock;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Cap, Format, Join, Mask, Stroke, Transform, Vector};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FontId(pub u16);

pub struct Face {
    rb: rustybuzz::Face<'static>,
    sw: swash::FontRef<'static>,
    upem: f32,
    /// En unidades de 1 em; descendente positivo.
    pub ascent: f32,
    pub descent: f32,
    pub xheight: f32,
}

static REG: RwLock<Vec<&'static Face>> = RwLock::new(Vec::new());

fn leak_face(data: &'static [u8], index: u32) -> Option<&'static Face> {
    let rb = rustybuzz::Face::from_slice(data, index)?;
    let sw = swash::FontRef::from_index(data, index as usize)?;
    let upem = rb.units_per_em() as f32;
    let ascent = rb.ascender() as f32 / upem;
    let descent = -(rb.descender() as f32) / upem;
    let xheight = rb.x_height().unwrap_or((upem * 0.5) as i16) as f32 / upem;
    Some(Box::leak(Box::new(Face { rb, sw, upem, ascent, descent, xheight })))
}
fn add_face(f: &'static Face) -> FontId {
    let mut r = REG.write();
    r.push(f);
    FontId(r.len() as u16 - 1)
}
pub fn face(id: FontId) -> &'static Face {
    REG.read()[id.0 as usize]
}

macro_rules! emb {
    ($n:literal) => {
        include_bytes!(concat!("../assets/fonts/", $n, ".ttf")) as &'static [u8]
    };
}

/// Regular, cursiva, negrita y negrita cursiva.
#[derive(Clone, Copy, Debug)]
pub struct Family {
    pub name: &'static str,
    pub f: [FontId; 4],
}

pub struct Fonts {
    pub ui: FontId,
    pub ui_m: FontId,
    pub ui_b: FontId,
    pub title: FontId,
    pub title_i: FontId,
    pub mono: FontId,
    pub families: Vec<Family>,
}
static FONTS: OnceLock<Fonts> = OnceLock::new();
pub fn fonts() -> &'static Fonts {
    FONTS.get_or_init(|| {
        let e = |d: &'static [u8]| add_face(leak_face(d, 0).expect("tipografía"));
        let ui = e(emb!("Lexend_400Regular"));
        let ui_m = e(emb!("Lexend_500Medium"));
        let ui_b = e(emb!("Lexend_600SemiBold"));
        let title = e(emb!("Newsreader_500Medium"));
        let title_i = e(emb!("Newsreader_500Medium_Italic"));
        let mono = e(emb!("JetBrainsMono_400Regular"));
        let mut families = vec![
            Family { name: "Literata", f: [e(emb!("Literata_400Regular")), e(emb!("Literata_400Regular_Italic")), e(emb!("Literata_700Bold")), e(emb!("Literata_700Bold_Italic"))] },
            Family { name: "Newsreader", f: [e(emb!("Newsreader_400Regular")), e(emb!("Newsreader_400Regular_Italic")), e(emb!("Newsreader_700Bold")), e(emb!("Newsreader_700Bold_Italic"))] },
            Family { name: "Atkinson", f: [e(emb!("AtkinsonHyperlegible_400Regular")), e(emb!("AtkinsonHyperlegible_400Regular_Italic")), e(emb!("AtkinsonHyperlegible_700Bold")), e(emb!("AtkinsonHyperlegible_700Bold_Italic"))] },
            Family { name: "Lexend", f: [ui, ui, ui_b, ui_b] },
        ];
        // Las del sistema, si están.
        for (name, files) in [
            ("Georgia", ["georgia.ttf", "georgiai.ttf", "georgiab.ttf", "georgiaz.ttf"]),
            ("Palatino", ["pala.ttf", "palai.ttf", "palab.ttf", "palabi.ttf"]),
            ("Bookman", ["BOOKOS.TTF", "BOOKOSI.TTF", "BOOKOSB.TTF", "BOOKOSBI.TTF"]),
            ("Constantia", ["constan.ttf", "constani.ttf", "constanb.ttf", "constanz.ttf"]),
        ] {
            let ids: Vec<FontId> = files.iter().filter_map(|f| sys_face(f, 0)).map(add_face).collect();
            if let [a, b, c, d] = ids[..] {
                families.push(Family { name, f: [a, b, c, d] });
            }
        }
        Fonts { ui, ui_m, ui_b, title, title_i, mono, families }
    })
}
fn sys_face(file: &str, index: u32) -> Option<&'static Face> {
    let dir = std::env::var("WINDIR").unwrap_or("C:\\Windows".into());
    let data = std::fs::read(format!("{dir}\\Fonts\\{file}")).ok()?;
    leak_face(Box::leak(data.into_boxed_slice()), index)
}
static FALLBACK: RwLock<Vec<(&'static str, Option<FontId>)>> = RwLock::new(Vec::new());
const FALLBACK_FILES: &[&str] = &["segoeui.ttf", "seguisym.ttf", "YuGothR.ttc", "malgun.ttf", "msyh.ttc", "seguiemj.ttf", "Nirmala.ttc", "ebrima.ttf"];

fn fallback_for(ch: char) -> Option<FontId> {
    let mut i = 0;
    loop {
        let have = FALLBACK.read().get(i).copied();
        let id = match have {
            Some((_, id)) => id,
            None => {
                let file = FALLBACK_FILES.get(i)?;
                let id = sys_face(file, 0).map(add_face);
                FALLBACK.write().push((file, id));
                id
            }
        };
        if let Some(id) = id {
            if face(id).rb.glyph_index(ch).is_some() {
                return Some(id);
            }
        }
        i += 1;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Gl {
    pub font: FontId,
    pub gid: u16,
    /// Byte dentro de la palabra.
    pub cluster: u16,
    pub adv: f32,
    pub dx: f32,
    pub dy: f32,
}
#[derive(Debug)]
pub struct Shaped {
    pub g: Vec<Gl>,
    pub w: f32,
}

thread_local! {
    static CACHE: RefCell<HashMap<FontId, HashMap<Box<str>, Rc<Shaped>>>> = RefCell::new(HashMap::new());
}

/// Compone una palabra en unidades de 1 em: cambiar el tamaño no exige repetirla.
pub fn shape(font: FontId, s: &str) -> Rc<Shaped> {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let m = c.entry(font).or_default();
        if let Some(r) = m.get(s) {
            return r.clone();
        }
        if m.len() > 60_000 {
            m.clear();
        }
        let r = Rc::new(shape_raw(font, s));
        m.insert(s.into(), r.clone());
        r
    })
}
fn shape_raw(font: FontId, s: &str) -> Shaped {
    let mut out = Shaped { g: Vec::with_capacity(s.len()), w: 0.0 };
    let main = face(font);
    let pick = |ch: char| -> FontId {
        if ch.is_control() || main.rb.glyph_index(ch).is_some() || matches!(ch, '\u{ad}' | '\u{200b}'..='\u{200f}' | '\u{feff}' | '\u{300}'..='\u{36f}' | '\u{fe00}'..='\u{fe0f}') {
            font
        } else {
            fallback_for(ch).unwrap_or(font)
        }
    };
    let mut run_start = 0;
    let mut cur: Option<FontId> = None;
    let mut runs = vec![];
    for (i, ch) in s.char_indices() {
        let f = pick(ch);
        if cur != Some(f) {
            if let Some(c) = cur {
                runs.push((c, run_start, i));
            }
            cur = Some(f);
            run_start = i;
        }
    }
    if let Some(c) = cur {
        runs.push((c, run_start, s.len()));
    }
    for (f, a, b) in runs {
        let fc = face(f);
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(&s[a..b]);
        buf.guess_segment_properties();
        let rtl = buf.direction() == rustybuzz::Direction::RightToLeft;
        let res = rustybuzz::shape(&fc.rb, &[], buf);
        let mut run: Vec<Gl> = res
            .glyph_infos()
            .iter()
            .zip(res.glyph_positions())
            .map(|(i, p)| Gl { font: f, gid: i.glyph_id as u16, cluster: (a + i.cluster as usize) as u16, adv: p.x_advance as f32 / fc.upem, dx: p.x_offset as f32 / fc.upem, dy: p.y_offset as f32 / fc.upem })
            .collect();
        if rtl {
            // orden visual ya correcto dentro de la palabra
            run.shrink_to_fit();
        }
        out.w += run.iter().map(|g| g.adv).sum::<f32>();
        out.g.extend(run);
    }
    out
}

#[derive(Clone, Copy)]
struct Slot {
    uv: [f32; 4],
    left: f32,
    top: f32,
    w: f32,
    h: f32,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    font: u16,
    gid: u16,
    size: u16,
    sub: u8,
}

/// Atlas de glifos e iconos: cada uno se dibuja una vez por tamaño.
pub struct Text {
    ctx: ScaleContext,
    slots: HashMap<Key, Option<Slot>>,
    shelf: (u32, u32, u32),
    pending: Vec<(u32, u32, u32, u32, Vec<u8>)>,
    full: bool,
    measures: HashMap<(FontId, u32, Box<str>), f32>,
}
impl Text {
    pub fn new() -> Text {
        fonts();
        Text { ctx: ScaleContext::new(), slots: HashMap::new(), shelf: (1, 1, 0), pending: vec![], full: false, measures: HashMap::new() }
    }
    /// Sube lo nuevo. Devuelve true si hubo que vaciar el atlas y conviene repintar.
    pub fn flush(&mut self, gpu: &Gpu) -> bool {
        for (x, y, w, h, d) in self.pending.drain(..) {
            gpu.upload_atlas(x, y, w, h, &d);
        }
        if self.measures.len() > 20_000 {
            self.measures.clear();
        }
        if self.full {
            self.full = false;
            self.slots.clear();
            self.shelf = (1, 1, 0);
            return true;
        }
        false
    }
    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let (mut x, mut y, mut sh) = self.shelf;
        if x + w + 1 > ATLAS {
            x = 1;
            y += sh + 1;
            sh = 0;
        }
        if y + h + 1 > ATLAS || w + 2 > ATLAS {
            self.full = true;
            return None;
        }
        self.shelf = (x + w + 1, y, sh.max(h));
        Some((x, y))
    }
    fn put(&mut self, key: Key, left: f32, top: f32, w: u32, h: u32, data: Vec<u8>) -> Option<Slot> {
        if w == 0 || h == 0 {
            self.slots.insert(key, None);
            return None;
        }
        let (x, y) = self.alloc(w, h)?;
        // un píxel vacío alrededor para que el filtrado no arrastre vecinos
        self.pending.push((x, y, w, h, data));
        let a = ATLAS as f32;
        let s = Slot { uv: [x as f32 / a, y as f32 / a, (x + w) as f32 / a, (y + h) as f32 / a], left, top, w: w as f32, h: h as f32 };
        self.slots.insert(key, Some(s));
        Some(s)
    }
    fn slot(&mut self, font: FontId, gid: u16, px: f32, sub: u8) -> Option<Slot> {
        let key = Key { font: font.0, gid, size: (px * 4.0) as u16, sub };
        if let Some(s) = self.slots.get(&key) {
            return *s;
        }
        if self.full {
            return None;
        }
        let size = key.size as f32 / 4.0;
        let f = face(font);
        let mut sc = self.ctx.builder(f.sw).size(size).hint(size < 26.0).build();
        let img = Render::new(&[Source::Outline]).format(Format::Alpha).offset(Vector::new(sub as f32 / 4.0, 0.0)).render(&mut sc, gid);
        match img {
            Some(i) => {
                // margen de 1 px para interpolar sin cortes
                let (w, h) = (i.placement.width, i.placement.height);
                if w == 0 || h == 0 {
                    self.slots.insert(key, None);
                    return None;
                }
                let (pw, ph) = (w + 2, h + 2);
                let mut d = vec![0u8; (pw * ph) as usize];
                for y in 0..h {
                    let o = ((y + 1) * pw + 1) as usize;
                    d[o..o + w as usize].copy_from_slice(&i.data[(y * w) as usize..((y + 1) * w) as usize]);
                }
                self.put(key, i.placement.left as f32 - 1.0, i.placement.top as f32 + 1.0, pw, ph, d)
            }
            None => {
                self.slots.insert(key, None);
                None
            }
        }
    }
    /// Un glifo con su origen (x, línea base) en unidades lógicas.
    #[allow(clippy::too_many_arguments)]
    pub fn glyph(&mut self, d: &mut Draw, font: FontId, gid: u16, size: f32, x: f32, y: f32, c: Color) {
        let (s, tx, ty) = d.xf();
        let dpi = d.dpi;
        let px = size * dpi;
        let plain = (s - dpi).abs() < 0.0005;
        let (fx, fy) = (x * s + tx, y * s + ty);
        if plain {
            let sub = (((fx - fx.floor()) * 4.0) as u8).min(3);
            let Some(g) = self.slot(font, gid, px, sub) else { return };
            let r = rect(fx.floor() + g.left, (fy + 0.5).floor() - g.top, g.w, g.h);
            d.glyph(r, g.uv, c, None);
        } else {
            let k = s / dpi;
            let Some(g) = self.slot(font, gid, px, 0) else { return };
            let r = rect(fx + g.left * k, fy - g.top * k, g.w * k, g.h * k);
            d.glyph(r, g.uv, c, None);
        }
    }
    /// Cuadros de cada glifo en unidades lógicas, para ponerlos en una malla 3D.
    pub fn quads(&mut self, font: FontId, size: f32, s: &str, spacing: f32, dpi: f32) -> (Vec<(Rect, [f32; 4])>, f32) {
        let mut out = vec![];
        let mut x = 0.0;
        for (i, word) in split_keep(s).enumerate() {
            let _ = i;
            for g in shape(font, word).g.iter() {
                if let Some(sl) = self.slot(g.font, g.gid, size * dpi, 0) {
                    out.push((rect(x + g.dx * size + sl.left / dpi, -sl.top / dpi - g.dy * size, sl.w / dpi, sl.h / dpi), sl.uv));
                }
                x += g.adv * size + spacing;
            }
        }
        (out, x)
    }
    /// Texto centrado verticalmente en `cy`. Devuelve el ancho.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, d: &mut Draw, font: FontId, size: f32, x: f32, cy: f32, s: &str, c: Color) -> f32 {
        self.draw_sp(d, font, size, x, cy, s, c, 0.0)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn draw_sp(&mut self, d: &mut Draw, font: FontId, size: f32, x: f32, cy: f32, s: &str, c: Color, spacing: f32) -> f32 {
        let f = face(font);
        // Se centra sobre la altura de la x y las mayúsculas, que es lo que ve el ojo.
        let base = cy + size * (f.xheight * 0.5 + 0.10);
        self.draw_base(d, font, size, x, base, s, c, spacing)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn draw_base(&mut self, d: &mut Draw, font: FontId, size: f32, x: f32, base: f32, s: &str, c: Color, spacing: f32) -> f32 {
        let mut px = x;
        for word in split_keep(s) {
            let sh = shape(font, word);
            for g in sh.g.iter() {
                self.glyph(d, g.font, g.gid, size, px + g.dx * size, base - g.dy * size, c);
                px += g.adv * size + spacing;
            }
        }
        px - x
    }
    pub fn measure(&mut self, font: FontId, size: f32, s: &str) -> f32 {
        if let Some(w) = self.measures.get(&(font, 0, s.into())) {
            return w * size;
        }
        let w: f32 = split_keep(s).map(|w| shape(font, w).w).sum();
        self.measures.insert((font, 0, s.into()), w);
        w * size
    }
    /// Recorta con puntos suspensivos si no cabe.
    pub fn fit(&mut self, font: FontId, size: f32, s: &str, w: f32) -> String {
        if self.measure(font, size, s) <= w {
            return s.to_string();
        }
        let el = shape(font, "…").w * size;
        let mut x = 0.0;
        let mut end = 0;
        'o: for (off, word) in split_keep_idx(s) {
            for g in shape(font, word).g.iter() {
                if x + g.adv * size + el > w {
                    break 'o;
                }
                x += g.adv * size;
                let mut e = off + g.cluster as usize;
                e += s[e..].chars().next().map_or(0, |c| c.len_utf8());
                end = end.max(e);
            }
        }
        format!("{}…", s[..end].trim_end())
    }
    /// Parte un texto en líneas.
    pub fn wrap(&mut self, font: FontId, size: f32, s: &str, width: f32, max_lines: usize) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for para in s.split('\n') {
            let mut line = String::new();
            let mut w = 0.0;
            for word in split_keep(para) {
                let ww = shape(font, word).w * size;
                let tw = shape(font, word.trim_end()).w * size;
                if w + tw > width && !line.is_empty() {
                    out.push(std::mem::take(&mut line).trim_end().to_string());
                    w = 0.0;
                }
                line.push_str(word);
                w += ww;
            }
            out.push(line.trim_end().to_string());
        }
        if max_lines > 0 && out.len() > max_lines {
            out.truncate(max_lines);
            let last = out.pop().unwrap();
            let t = self.fit(font, size, &format!("{last}……………………"), width);
            let t = t.trim_end_matches('…').trim_end().to_string();
            out.push(format!("{t}…"));
        }
        out
    }
    /// Icono centrado en (cx, cy).
    pub fn icon(&mut self, d: &mut Draw, ic: Icon, cx: f32, cy: f32, size: f32, c: Color) {
        let (s, tx, ty) = d.xf();
        let k = s / d.dpi;
        let px = size * d.dpi;
        let key = Key { font: u16::MAX, gid: ic as u16, size: (px * 4.0) as u16, sub: 0 };
        let slot = match self.slots.get(&key) {
            Some(s) => *s,
            None => {
                if self.full {
                    return;
                }
                let (path, filled) = ic.path();
                let sc = px / 24.0;
                let mut st = Stroke::new(1.7);
                st.cap(Cap::Round).join(Join::Round);
                let mut m = Mask::new(path);
                m.transform(Some(Transform::scale(sc, sc)));
                if filled {
                    m.style(swash::zeno::Fill::NonZero);
                } else {
                    m.style(st);
                }
                let n = px.ceil() as u32 + 4;
                m.size(n, n).offset(Vector::new(2.0, 2.0));
                let (data, _) = m.render();
                self.put(key, 0.0, 0.0, n, n, data)
            }
        };
        let Some(g) = slot else { return };
        let (fx, fy) = (cx * s + tx, cy * s + ty);
        let half = px * k / 2.0 + 2.0 * k;
        let (mut x, mut y) = (fx - half, fy - half);
        if (k - 1.0).abs() < 0.0005 {
            x = x.round();
            y = y.round();
        }
        d.glyph(rect(x, y, g.w * k, g.h * k), g.uv, c, None);
    }
}

/// Trozos "palabra + espacios", que es la unidad de la caché.
pub fn split_keep(s: &str) -> impl Iterator<Item = &str> {
    split_keep_idx(s).map(|x| x.1)
}
pub fn split_keep_idx(s: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut pos = 0;
    std::iter::from_fn(move || {
        if pos >= s.len() {
            return None;
        }
        let rest = &s[pos..];
        let mut end = rest.len();
        let mut in_space = false;
        for (i, c) in rest.char_indices() {
            if c == ' ' {
                in_space = true;
            } else if in_space {
                end = i;
                break;
            }
        }
        let r = (pos, &rest[..end]);
        pos += end;
        Some(r)
    })
}

/// Glifos sueltos en memoria, para componer imágenes fuera de la GPU.
pub struct Raster {
    ctx: ScaleContext,
}
impl Default for Raster {
    fn default() -> Self {
        Raster { ctx: ScaleContext::new() }
    }
}
impl Raster {
    /// Desplazamiento izquierdo, superior, ancho, alto y cobertura.
    pub fn glyph(&mut self, font: FontId, gid: u16, px: f32) -> Option<(i32, i32, u32, u32, Vec<u8>)> {
        let mut sc = self.ctx.builder(face(font).sw).size(px).hint(false).build();
        let i = Render::new(&[Source::Outline]).format(Format::Alpha).render(&mut sc, gid)?;
        Some((i.placement.left, i.placement.top, i.placement.width, i.placement.height, i.data))
    }
}
