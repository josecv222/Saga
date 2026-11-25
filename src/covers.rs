//! Portadas: normalizar, reducir, colorear y tener en la GPU solo lo que se ve.
use crate::db::{cover_path, thumb_path, Book};
use crate::doc::Pic;
use crate::formats::{decode, Cover};
use crate::gfx::*;
use crate::jobs::{Apply, Jobs};
use crate::text::{fonts, shape, FontId};
use image::imageops::{self, FilterType};
use image::{Rgb, RgbImage, RgbaImage};
use std::collections::HashMap;

pub const THUMB_H: u32 = 420;
const FULL_H: u32 = 1100;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Size {
    Thumb,
    Full,
    Back,
}
type Key = (i64, i32, Size);
struct Slot {
    tex: Option<TexId>,
    born: f32,
    used: u64,
}
pub struct Ready {
    key: Key,
    w: u32,
    h: u32,
    levels: Vec<Vec<u8>>,
}
#[derive(Default)]
pub struct Covers {
    slots: HashMap<Key, Slot>,
    ready: Vec<Ready>,
    frame: u64,
}
impl Covers {
    /// Textura y opacidad de aparición. Si aún no está, se pide.
    pub fn get(&mut self, jobs: &Jobs, b: &Book, size: Size, time: f32) -> Option<(TexId, f32)> {
        let key = (b.id, ver(b), size);
        let frame = self.frame;
        if let Some(s) = self.slots.get_mut(&key) {
            s.used = frame;
            return s.tex.map(|t| (t, ((time - s.born) / 0.22).clamp(0.0, 1.0)));
        }
        self.slots.insert(key, Slot { tex: None, born: 0.0, used: frame });
        let b = b.clone();
        jobs.run(if size == Size::Thumb { 1 } else { 0 }, move || {
            let img = load(&b, size);
            let (w, h) = img.dimensions();
            let levels = if size == Size::Back { vec![img.into_raw()] } else { mips(img) };
            Some(Box::new(move |a: &mut crate::app::App| a.covers.ready.push(Ready { key, w, h, levels })) as Apply)
        });
        None
    }
    /// Solo consulta: no carga nada.
    pub fn peek(&self, b: &Book, size: Size) -> Option<TexId> {
        self.slots.get(&(b.id, ver(b), size)).and_then(|s| s.tex)
    }
    /// La mejor disponible mientras llega la grande.
    pub fn best(&mut self, jobs: &Jobs, b: &Book, time: f32) -> Option<(TexId, f32)> {
        let full = self.get(jobs, b, Size::Full, time);
        match (full, self.peek(b, Size::Thumb)) {
            // la grande sustituye a la pequeña sin fundido: son la misma imagen
            (Some((f, _)), Some(_)) => Some((f, 1.0)),
            (Some(f), None) => Some(f),
            (None, _) => self.get(jobs, b, Size::Thumb, time),
        }
    }
    /// Sube a la GPU unas pocas por fotograma, para que no se note.
    pub fn frame(&mut self, gpu: &mut Gpu, time: f32) -> bool {
        self.frame += 1;
        let start = 3_500_000usize;
        let mut budget = start;
        let mut moved = false;
        while let Some(r) = self.ready.first() {
            let sz: usize = r.levels.iter().map(|l| l.len()).sum();
            if sz > budget && budget < start {
                break;
            }
            budget = budget.saturating_sub(sz);
            let r = self.ready.remove(0);
            let t = gpu.create(r.w, r.h, &r.levels);
            if let Some(s) = self.slots.get_mut(&r.key) {
                s.tex = Some(t);
                s.born = time;
            } else {
                gpu.drop_tex(t);
            }
            moved = true;
        }
        if self.frame % 64 == 0 {
            for (size, max) in [(Size::Thumb, 260), (Size::Full, 26), (Size::Back, 5)] {
                let mut v: Vec<(u64, Key)> = self.slots.iter().filter(|(k, s)| k.2 == size && s.tex.is_some()).map(|(k, s)| (s.used, *k)).collect();
                if v.len() > max {
                    v.sort_by_key(|x| x.0);
                    for (used, k) in v.iter().take(v.len() - max) {
                        if *used + 90 < self.frame {
                            if let Some(s) = self.slots.remove(k) {
                                gpu.drop_tex(s.tex.unwrap());
                            }
                        }
                    }
                }
            }
        }
        moved || !self.ready.is_empty()
    }
    pub fn forget(&mut self, gpu: &mut Gpu, id: i64) {
        let ks: Vec<Key> = self.slots.keys().filter(|k| k.0 == id).copied().collect();
        for k in ks {
            if let Some(t) = self.slots.remove(&k).and_then(|s| s.tex) {
                gpu.drop_tex(t);
            }
        }
    }
    /// Suelta lo que no se ha visto hace rato (al entrar en el lector).
    pub fn trim(&mut self, gpu: &mut Gpu, keep: i64) {
        let ks: Vec<Key> = self.slots.iter().filter(|(k, s)| k.2 != Size::Back && k.0 != keep && s.tex.is_some() && s.used + 60 < self.frame).map(|(k, _)| *k).collect();
        for k in ks {
            if let Some(t) = self.slots.remove(&k).and_then(|s| s.tex) {
                gpu.drop_tex(t);
            }
        }
    }
}
/// Las portadas compuestas dependen del título: si cambia, se rehacen.
fn ver(b: &Book) -> i32 {
    if b.cover_ver > 0 {
        return b.cover_ver;
    }
    let h = hash(&b.title) ^ hash(&b.authors).rotate_left(7);
    -((h >> 2) as i32).max(1)
}
fn load(b: &Book, size: Size) -> RgbaImage {
    let file = |p: std::path::PathBuf| std::fs::read(p).ok().and_then(|d| decode(&d));
    let img = if b.cover_ver > 0 {
        match size {
            Size::Full => file(cover_path(b.id)),
            _ => file(thumb_path(b.id)).or_else(|| file(cover_path(b.id))),
        }
    } else {
        None
    };
    let img = img.unwrap_or_else(|| generate(b, if size == Size::Full { 960 } else { 400 }));
    match size {
        Size::Back => backdrop(&img),
        Size::Thumb if img.height() > THUMB_H + 40 => {
            let w = (img.width() as f32 * THUMB_H as f32 / img.height() as f32) as u32;
            imageops::resize(&img, w.max(1), THUMB_H, FilterType::Triangle)
        }
        _ => img,
    }
}
fn backdrop(img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    // el centro de la portada, apaisado
    let ch = (w as f32 * 0.8).min(h as f32) as u32;
    let y = (h - ch) / 3;
    let c = imageops::crop_imm(img, 0, y, w, ch.max(1)).to_image();
    let s = imageops::resize(&c, 144, 90, FilterType::Triangle);
    imageops::blur(&s, 11.0)
}
pub fn mips(img: RgbaImage) -> Vec<Vec<u8>> {
    let (mut w, mut h) = img.dimensions();
    let mut out = vec![img.into_raw()];
    while w > 24 && h > 24 {
        let (nw, nh) = (w / 2, h / 2);
        let src = out.last().unwrap();
        let mut d = Vec::with_capacity((nw * nh * 4) as usize);
        for y in 0..nh as usize {
            let r0 = y * 2 * w as usize * 4;
            let r1 = r0 + w as usize * 4;
            for x in 0..nw as usize {
                let k = x * 8;
                for c in 0..4 {
                    d.push(((src[r0 + k + c] as u32 + src[r0 + k + 4 + c] as u32 + src[r1 + k + c] as u32 + src[r1 + k + 4 + c] as u32 + 2) / 4) as u8);
                }
            }
        }
        out.push(d);
        w = nw;
        h = nh;
    }
    out
}

/// Portada lista para guardar.
#[derive(Clone)]
pub struct Made {
    pub jpg: Vec<u8>,
    pub thumb: Vec<u8>,
    pub color: u32,
    pub aspect: f32,
}
pub fn make(c: &Cover) -> Option<Made> {
    let img = match c {
        Cover::None => return None,
        Cover::Encoded(b) => decode(b)?,
        Cover::Pixels(Pic { w, h, rgba }) => RgbaImage::from_raw(*w, *h, rgba.clone())?,
    };
    make_img(img)
}
pub fn make_img(img: RgbaImage) -> Option<Made> {
    let (w, h) = img.dimensions();
    if w < 24 || h < 24 {
        return None;
    }
    // sobre papel: las transparencias se ven claras
    let mut rgb = RgbImage::new(w, h);
    for (o, p) in rgb.pixels_mut().zip(img.pixels()) {
        let a = p[3] as u32;
        *o = Rgb([0, 1, 2].map(|i| ((p[i] as u32 * a + 250 * (255 - a)) / 255) as u8));
    }
    let rgb = trim_border(rgb);
    let (w, h) = rgb.dimensions();
    let full = if h > FULL_H { imageops::resize(&rgb, ((w as f32 * FULL_H as f32 / h as f32) as u32).max(1), FULL_H, FilterType::CatmullRom) } else { rgb };
    let (w, h) = full.dimensions();
    let th = THUMB_H.min(h);
    let thumb = imageops::resize(&full, ((w as f32 * th as f32 / h as f32) as u32).max(1), th, FilterType::CatmullRom);
    let enc = |i: &RgbImage, q| {
        let mut v = vec![];
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut v, q).encode_image(i).ok().map(|_| v)
    };
    let color = spine_color(&thumb);
    Some(Made { jpg: enc(&full, 90)?, thumb: enc(&thumb, 86)?, color, aspect: w as f32 / h as f32 })
}
/// Quita marcos blancos o negros de escaneos (solo si son uniformes y finos).
fn trim_border(img: RgbImage) -> RgbImage {
    let (w, h) = img.dimensions();
    let k = *img.get_pixel(0, 0);
    let l = (k[0] as i32 + k[1] as i32 + k[2] as i32) / 3;
    if l > 14 && l < 243 {
        return img;
    }
    let same = |p: &Rgb<u8>| (0..3).all(|i| (p[i] as i32 - k[i] as i32).abs() < 14);
    let row = |y: u32| (0..w).step_by(3).all(|x| same(img.get_pixel(x, y)));
    let col = |x: u32| (0..h).step_by(3).all(|y| same(img.get_pixel(x, y)));
    let (mx, my) = (w / 10, h / 10);
    let t = (0..my).take_while(|y| row(*y)).count() as u32;
    let b = (0..my).take_while(|y| row(h - 1 - *y)).count() as u32;
    let a = (0..mx).take_while(|x| col(*x)).count() as u32;
    let c = (0..mx).take_while(|x| col(w - 1 - *x)).count() as u32;
    // un margen que llega al límite es parte del diseño
    if t >= my || b >= my || a >= mx || c >= mx || t.min(b).min(a).min(c) == 0 {
        return img;
    }
    imageops::crop_imm(&img, a, t, w - a - c, h - t - b).to_image()
}
/// El lomo continúa el borde izquierdo de la portada.
fn spine_color(i: &RgbImage) -> u32 {
    let (w, h) = i.dimensions();
    let mut s = [0f32; 3];
    let mut n = 0.0;
    for y in 0..h {
        for x in 0..(w / 14).max(1) {
            let p = i.get_pixel(x, y);
            for k in 0..3 {
                s[k] += p[k] as f32;
            }
            n += 1.0;
        }
    }
    let mut c = Color(s[0] / n / 255.0, s[1] / n / 255.0, s[2] / n / 255.0, 1.0);
    // un lomo casi blanco o negro gana algo de cuerpo
    if c.lum() > 0.9 {
        c = c.mul(0.94);
    }
    if c.lum() < 0.06 {
        c = c.mix(Color::WHITE, 0.07);
    }
    c.to_u32()
}

// ------------------------------------------------------ portada tipográfica
const PALS: [(u32, u32, u32); 10] = [
    (0x233B40, 0xE9DDC3, 0xC9A063),
    (0x5A2E2B, 0xF0E1CB, 0xD9A066),
    (0x2C3550, 0xE8E4D8, 0xC7A45D),
    (0x3C4A33, 0xEDE6D0, 0xBFA760),
    (0x6A4A2C, 0xF4E9D4, 0xE3BC7B),
    (0x40304A, 0xEBE0EA, 0xCFA3C4),
    (0x1F4A4A, 0xE6EBE0, 0xD9B36C),
    (0x7A3B2E, 0xF6E6D2, 0xF0BF7F),
    (0x2E2E33, 0xE9E5DB, 0xB79B6B),
    (0x4B5358, 0xF0EEE6, 0xD8B98A),
];
pub fn gen_color(title: &str) -> u32 {
    PALS[(hash(title) % PALS.len() as u32) as usize].0
}
fn hash(s: &str) -> u32 {
    let h = s.bytes().fold(2166136261u32, |h, c| (h ^ c as u32).wrapping_mul(16777619));
    h ^ (h >> 15)
}
fn wrap(font: FontId, size: f32, s: &str, w: f32) -> Vec<String> {
    let mut out = vec![];
    let mut line = String::new();
    for word in s.split_whitespace() {
        let t = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if shape(font, &t).w * size > w && !line.is_empty() {
            out.push(std::mem::replace(&mut line, word.to_string()));
        } else {
            line = t;
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}
/// Tela, filete dorado y el título: una encuadernación sobria para lo que no trae portada.
pub fn generate(b: &Book, h: u32) -> RgbaImage {
    let w = h * 2 / 3;
    let k = h as f32 / 960.0;
    let (bg, fg, gold) = PALS[(hash(&b.title) % PALS.len() as u32) as usize];
    let (bg, fg, gold) = (Color::hex(bg), Color::hex(fg), Color::hex(gold));
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
            // trama de tela
            let n = ((x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263)).wrapping_mul(1274126177) >> 24) as f32 / 255.0;
            let weave = (((x as f32 / k * 1.3).sin() + (y as f32 / k * 1.3).sin()) * 0.5) * 0.012;
            let l = 1.10 - 0.22 * v - 0.10 * (u - 0.4).abs() + weave + (n - 0.5) * 0.035;
            let c = bg.mul(l);
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
            img.put_pixel(x, y, image::Rgba([q(c.0), q(c.1), q(c.2), 255]));
        }
    }
    for (inset, th, a) in [(44.0f32, 2.6f32, 0.85f32), (56.0, 1.2, 0.6)] {
        let (x0, y0, x1, y1) = (inset * k + 14.0 * k, inset * k, w as f32 - inset * k, h as f32 - inset * k);
        for y in y0 as u32..y1 as u32 {
            for x in x0 as u32..x1 as u32 {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let d = (fx - x0).min(x1 - fx).min(fy - y0).min(y1 - fy);
                let cov = (th * k - d + 0.5).clamp(0.0, 1.0);
                if cov > 0.0 {
                    blend(img.get_pixel_mut(x, y), gold, cov * a);
                }
            }
        }
    }
    let cx = w as f32 / 2.0 + 7.0 * k;
    let inner = w as f32 - 190.0 * k;
    let f = fonts().title;
    let title = if b.title.is_empty() { "Sin título" } else { &b.title };
    let mut size = 78.0 * k;
    let mut lines = wrap(f, size, title, inner);
    while size > 30.0 * k && (lines.len() > 5 || lines.iter().any(|l| shape(f, l).w * size > inner)) {
        size *= 0.93;
        lines = wrap(f, size, title, inner);
    }
    lines.truncate(8);
    let lh = size * 1.12;
    let mut y = h as f32 * 0.40 - lines.len() as f32 * lh / 2.0 + size * 0.8;
    let mut t = crate::text::Raster::default();
    for l in &lines {
        draw_text(&mut t, &mut img, f, size, cx, y, l, fg);
        y += lh;
    }
    // adorno
    let oy = y + 4.0 * k;
    let half = 60.0 * k;
    for dx in -(half as i32)..=half as i32 {
        let t = 1.0 - (dx as f32 / half).abs();
        let (x, yy) = ((cx + dx as f32) as i32, oy as i32);
        if x > 0 && (x as u32) < w && yy > 0 && (yy as u32) < h {
            blend(img.get_pixel_mut(x as u32, yy as u32), gold, t);
        }
    }
    if !b.authors.is_empty() {
        let au = b.author_line();
        let fi = fonts().title_i;
        let mut asz = 38.0 * k;
        while asz > 16.0 * k && shape(fi, &au).w * asz > inner {
            asz *= 0.94;
        }
        draw_text(&mut t, &mut img, fi, asz, cx, h as f32 - 118.0 * k, &au, gold.mix(fg, 0.35));
    }
    img
}
fn blend(p: &mut image::Rgba<u8>, c: Color, a: f32) {
    for (i, v) in [c.0, c.1, c.2].into_iter().enumerate() {
        p[i] = (p[i] as f32 * (1.0 - a) + v * 255.0 * a).round() as u8;
    }
}
#[allow(clippy::too_many_arguments)]
fn draw_text(t: &mut crate::text::Raster, img: &mut RgbaImage, f: FontId, size: f32, cx: f32, base: f32, s: &str, c: Color) {
    let sh = shape(f, s);
    let mut x = cx - sh.w * size / 2.0;
    for g in sh.g.iter() {
        if let Some((l, top, gw, gh, data)) = t.glyph(g.font, g.gid, size) {
            let (ox, oy) = ((x + g.dx * size).round() as i32 + l, (base - g.dy * size).round() as i32 - top);
            for yy in 0..gh as i32 {
                for xx in 0..gw as i32 {
                    let (px, py) = (ox + xx, oy + yy);
                    if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                        let v = data[(yy * gw as i32 + xx) as usize] as f32 / 255.0;
                        if v > 0.0 {
                            blend(img.get_pixel_mut(px as u32, py as u32), c, v);
                        }
                    }
                }
            }
        }
        x += g.adv * size;
    }
}
