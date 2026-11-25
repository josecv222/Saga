//! Documento legible: capítulos, bloques y estilos. Todos los formatos de texto
//! acaban aquí, y una posición es un desplazamiento en bytes de todo el texto.
use std::collections::HashMap;
use std::sync::Arc;

pub const BOLD: u16 = 1;
pub const ITALIC: u16 = 2;
pub const MONO: u16 = 4;
pub const SUP: u16 = 8;
pub const SUB: u16 = 16;
pub const UNDER: u16 = 32;
pub const STRIKE: u16 = 64;
pub const SMALL: u16 = 128;
pub const SMALLCAPS: u16 = 256;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Align {
    Auto,
    Left,
    Center,
    Right,
}

#[derive(Clone, PartialEq, Debug)]
pub enum BlockKind {
    Para,
    Heading(u8),
    Quote,
    /// Marcador y sangrado.
    Item(String, u8),
    Pre,
    Image(String),
    Rule,
    /// Celdas separadas por tabuladores en el texto.
    Row(bool),
}

#[derive(Clone, Debug)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub style: u16,
    pub link: Option<Arc<str>>,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub kind: BlockKind,
    pub text: String,
    pub spans: Vec<Span>,
    pub align: Align,
    /// Sangría pedida por el documento (en em); None = la habitual.
    pub indent: Option<f32>,
    pub quote: u8,
    pub break_before: bool,
    pub space_before: bool,
    /// Posición global del primer byte.
    pub start: usize,
    pub ids: Vec<String>,
}
impl Block {
    pub fn new(kind: BlockKind) -> Block {
        Block { kind, text: String::new(), spans: vec![], align: Align::Auto, indent: None, quote: 0, break_before: false, space_before: false, start: 0, ids: vec![] }
    }
    /// Lo que ocupa en la numeración global (imágenes y reglas cuentan 1).
    pub fn len(&self) -> usize {
        self.text.len().max(1)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Chapter {
    pub href: String,
    pub title: String,
    pub blocks: Vec<Block>,
    pub start: usize,
    pub len: usize,
}

#[derive(Clone, Debug)]
pub struct Toc {
    pub title: String,
    pub depth: u8,
    /// Posición global (texto) o página (documentos fijos).
    pub loc: i64,
}

pub trait Assets: Send + Sync {
    fn load(&self, name: &str) -> Option<Vec<u8>>;
}
pub struct NoAssets;
impl Assets for NoAssets {
    fn load(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}
pub struct MapAssets(pub HashMap<String, Vec<u8>>);
impl Assets for MapAssets {
    fn load(&self, n: &str) -> Option<Vec<u8>> {
        self.0.get(n).cloned()
    }
}

pub struct Doc {
    pub chapters: Vec<Chapter>,
    pub toc: Vec<Toc>,
    pub assets: Arc<dyn Assets>,
    pub lang: String,
    pub len: usize,
}
impl Doc {
    /// Numera los bloques y resuelve el índice (`archivo#ancla`).
    pub fn build(mut chapters: Vec<Chapter>, raw_toc: Vec<(String, u8, String)>, assets: Arc<dyn Assets>, lang: String) -> Doc {
        chapters.retain(|c| !c.blocks.is_empty());
        let mut at = 0;
        for c in &mut chapters {
            c.start = at;
            for b in &mut c.blocks {
                b.start = at;
                at += b.len();
            }
            c.len = at - c.start;
        }
        let mut d = Doc { chapters, toc: vec![], assets, lang, len: at };
        for (title, depth, href) in raw_toc {
            if let Some(loc) = d.resolve(&href) {
                let t = title.split_whitespace().collect::<Vec<_>>().join(" ");
                if !t.is_empty() {
                    d.toc.push(Toc { title: t, depth, loc: loc as i64 });
                }
            }
        }
        if d.toc.len() < 2 {
            let g = d.guess_toc();
            if g.len() > d.toc.len() {
                d.toc = g;
            }
        }
        d
    }
    fn guess_toc(&self) -> Vec<Toc> {
        let mut v = vec![];
        let mut min = 9;
        for c in &self.chapters {
            for b in &c.blocks {
                if let BlockKind::Heading(l) = b.kind {
                    let t = b.text.trim();
                    if !t.is_empty() && t.len() < 160 && l <= 3 {
                        min = min.min(l);
                        v.push(Toc { title: t.replace('\n', " "), depth: l, loc: b.start as i64 });
                    }
                }
            }
        }
        if v.is_empty() {
            // Sin encabezados: líneas sueltas con pinta de capítulo.
            for c in &self.chapters {
                for b in &c.blocks {
                    if looks_like_chapter(&b.text) {
                        v.push(Toc { title: b.text.trim().to_string(), depth: 0, loc: b.start as i64 });
                    }
                }
            }
            min = 0;
        }
        if v.is_empty() && self.chapters.len() > 1 {
            for (i, c) in self.chapters.iter().enumerate() {
                let t = if c.title.is_empty() { format!("Parte {}", i + 1) } else { c.title.clone() };
                v.push(Toc { title: t, depth: 0, loc: c.start as i64 });
            }
            min = 0;
        }
        for t in &mut v {
            t.depth = t.depth.saturating_sub(min);
        }
        v
    }
    /// Posición global de un enlace interno.
    pub fn resolve(&self, href: &str) -> Option<usize> {
        let (file, frag) = match href.split_once('#') {
            Some((f, g)) => (f, Some(g)),
            None => (href, None),
        };
        let ch = if file.is_empty() { None } else { self.chapters.iter().find(|c| c.href == file).or_else(|| { let base = file.rsplit('/').next().unwrap_or(file); self.chapters.iter().find(|c| c.href.rsplit('/').next() == Some(base)) }) };
        if let Some(fr) = frag.filter(|f| !f.is_empty()) {
            let find = |c: &Chapter| c.blocks.iter().find(|b| b.ids.iter().any(|i| i == fr)).map(|b| b.start);
            if let Some(c) = ch {
                return Some(find(c).unwrap_or(c.start));
            }
            return self.chapters.iter().find_map(find);
        }
        ch.map(|c| c.start)
    }
    pub fn chapter_at(&self, loc: usize) -> usize {
        self.chapters.partition_point(|c| c.start <= loc).saturating_sub(1)
    }
    pub fn block_at(&self, loc: usize) -> Option<(usize, usize)> {
        let ci = self.chapter_at(loc);
        let c = self.chapters.get(ci)?;
        let bi = c.blocks.partition_point(|b| b.start <= loc).saturating_sub(1);
        Some((ci, bi))
    }
    /// Texto entre dos posiciones.
    pub fn slice(&self, a: usize, b: usize) -> String {
        let mut out = String::new();
        for c in &self.chapters {
            if c.start + c.len <= a || c.start >= b {
                continue;
            }
            for bl in &c.blocks {
                let (s, e) = (bl.start, bl.start + bl.text.len());
                if e <= a || s >= b || bl.text.is_empty() {
                    continue;
                }
                let x = floor(&bl.text, a.saturating_sub(s));
                let y = floor(&bl.text, (b - s).min(bl.text.len()));
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&bl.text[x..y]);
            }
        }
        out
    }
    pub fn words(&self) -> usize {
        self.chapters.iter().flat_map(|c| &c.blocks).map(|b| b.text.split_whitespace().count()).sum()
    }
}
pub fn floor(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

pub fn looks_like_chapter(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t.chars().count() > 70 || t.contains('\n') {
        return false;
    }
    let l = t.to_lowercase();
    let first = l.split([' ', '.', ':', ',']).next().unwrap_or("");
    const KEYS: [&str; 22] = ["capítulo", "capitulo", "chapter", "parte", "part", "libro", "book", "prólogo", "prologo", "prologue", "epílogo", "epilogo", "epilogue", "introducción", "introduccion", "introduction", "prefacio", "preface", "conclusión", "conclusion", "apéndice", "unidad"];
    if KEYS.contains(&first) {
        // «Parte de la historia…» no es un título
        let second = l.split_whitespace().nth(1).unwrap_or("");
        let numbered = second.is_empty() || second.chars().next().is_some_and(|c| c.is_ascii_digit()) || second.trim_end_matches(['.', ':']).chars().all(|c| "ivxlcdm".contains(c)) || t == t.to_uppercase() || ["primera", "segunda", "tercera", "cuarta", "primero", "segundo", "uno", "dos", "tres", "one", "two", "three", "i"].contains(&second.trim_end_matches(['.', ':']));
        return numbered || (t.chars().count() < 28 && !t.ends_with('.') && !matches!(first, "parte" | "part" | "libro" | "book"));
    }
    // numeración romana sola
    t.len() <= 6 && t.trim_end_matches('.').chars().all(|c| "IVXLC".contains(c)) && t.len() > 1
}

/// Imagen del documento ya decodificada (RGBA).
pub struct Pic {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}
