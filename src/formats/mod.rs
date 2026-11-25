//! Reconocer y abrir cada formato.
pub mod epub;
pub mod fixed;
pub mod html;
pub mod mobi;
pub mod office;
pub mod plain;

use crate::doc::{Doc, Pic, Toc};
use std::path::Path;
use std::sync::Arc;

/// Extensiones admitidas, sin punto.
pub const EXTS: &[&str] = &["epub", "kepub", "pdf", "mobi", "azw", "azw3", "prc", "fb2", "fbz", "djvu", "djv", "cbz", "cbr", "cb7", "cbt", "docx", "odt", "rtf", "txt", "md", "markdown", "html", "htm", "xhtml", "htmlz"];

/// Datos que trae el propio archivo.
#[derive(Default, Clone, Debug)]
pub struct Meta {
    pub title: String,
    pub authors: Vec<String>,
    pub series: String,
    pub series_ix: f32,
    pub publisher: String,
    pub year: i32,
    pub language: String,
    pub isbn: String,
    pub description: String,
    pub genres: Vec<String>,
    pub pages: i32,
}

/// Carácter de una página fija, en puntos de página.
#[derive(Clone, Copy, Debug)]
pub struct PChar {
    pub c: char,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub trait Fixed: Send + Sync {
    fn count(&self) -> usize;
    fn size(&self, i: usize) -> (f32, f32);
    fn render(&self, i: usize, w: u32, h: u32) -> Option<Pic>;
    fn text(&self, i: usize) -> Vec<PChar>;
    fn toc(&self) -> Vec<Toc>;
    fn is_comic(&self) -> bool {
        false
    }
}

#[derive(Clone)]
pub enum Opened {
    Flow(Arc<Doc>),
    Fixed(Arc<dyn Fixed>),
}

pub fn ext_of(p: &Path) -> String {
    let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if n.ends_with(".fb2.zip") {
        return "fbz".into();
    }
    if n.ends_with(".kepub.epub") {
        return "epub".into();
    }
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}
pub fn supported(p: &Path) -> bool {
    EXTS.contains(&ext_of(p).as_str())
}

/// El formato de verdad: se mira el contenido, no solo el nombre.
pub fn detect(p: &Path) -> Option<String> {
    use std::io::Read;
    let ext = ext_of(p);
    let mut head = [0u8; 4096];
    let n = std::fs::File::open(p).ok()?.read(&mut head).ok()?;
    let h = &head[..n];
    if h.windows(5).take(1024).any(|w| w == b"%PDF-") {
        return Some("pdf".into());
    }
    if h.starts_with(b"AT&TFORM") {
        return Some("djvu".into());
    }
    if n > 68 && (&h[60..68] == b"BOOKMOBI" || &h[60..68] == b"TEXtREAd") {
        return Some(if ext == "azw3" { "azw3" } else { "mobi" }.into());
    }
    if h.starts_with(b"Rar!") {
        return Some("cbr".into());
    }
    if h.starts_with(&[0x37, 0x7a, 0xbc, 0xaf]) {
        return Some("cb7".into());
    }
    if h.starts_with(b"{\\rtf") {
        return Some("rtf".into());
    }
    if h.starts_with(b"PK") {
        if h.windows(20).any(|w| w == b"application/epub+zip") || ext == "epub" || ext == "kepub" {
            return Some("epub".into());
        }
        return Some(match ext.as_str() {
            "docx" | "odt" | "cbz" | "fbz" | "htmlz" => ext.clone(),
            "cbr" | "cb7" => "cbz".into(),
            _ => sniff_zip(p)?.into(),
        });
    }
    let low = String::from_utf8_lossy(&h[..n.min(600)]).to_lowercase();
    if low.contains("<fictionbook") {
        return Some("fb2".into());
    }
    match ext.as_str() {
        "cbt" => Some("cbt".into()),
        "md" | "markdown" => Some("md".into()),
        "html" | "htm" | "xhtml" => Some("html".into()),
        "fb2" => Some("fb2".into()),
        _ if low.contains("<html") => Some("html".into()),
        "txt" => Some("txt".into()),
        _ => None,
    }
}
fn sniff_zip(p: &Path) -> Option<&'static str> {
    let names = epub::zip_names(&epub::zip_open(p).ok()?);
    if names.iter().any(|n| n == "META-INF/container.xml") {
        Some("epub")
    } else if names.iter().any(|n| n == "word/document.xml") {
        Some("docx")
    } else if names.iter().any(|n| n == "content.xml") {
        Some("odt")
    } else if names.iter().any(|n| n.to_lowercase().ends_with(".fb2")) {
        Some("fbz")
    } else if names.iter().any(|n| fixed::is_image(n)) {
        Some("cbz")
    } else {
        None
    }
}

pub fn open(p: &Path) -> Result<Opened, String> {
    let f = detect(p).ok_or("Formato no reconocido")?;
    let flow = |d: Result<Doc, String>| d.map(|d| Opened::Flow(Arc::new(d)));
    let r = match f.as_str() {
        "epub" => flow(epub::open(p)),
        "pdf" => fixed::Pdf::open(p).map(|d| Opened::Fixed(Arc::new(d))),
        "djvu" => fixed::Djvu::open(p).map(|d| Opened::Fixed(Arc::new(d))),
        "cbz" | "cbr" | "cb7" | "cbt" => fixed::Comic::open(p, &f).map(|d| Opened::Fixed(Arc::new(d))),
        "mobi" | "azw3" => flow(mobi::open(p)),
        "fb2" | "fbz" => flow(office::fb2(p, f == "fbz")),
        "docx" => flow(office::docx(p)),
        "odt" => flow(office::odt(p)),
        "rtf" => flow(plain::rtf(p)),
        "html" => flow(plain::html(p)),
        "htmlz" => flow(plain::htmlz(p)),
        "md" => flow(plain::markdown(p)),
        _ => flow(plain::txt(p)),
    }?;
    match &r {
        Opened::Flow(d) if d.len == 0 => Err("El archivo no contiene texto".into()),
        Opened::Fixed(d) if d.count() == 0 => Err("El archivo no contiene páginas".into()),
        _ => Ok(r),
    }
}

/// Metadatos y portada (imagen codificada o ya pintada).
pub enum Cover {
    None,
    Encoded(Vec<u8>),
    Pixels(Pic),
}
pub fn probe(p: &Path) -> (String, Meta, Cover) {
    let f = detect(p).unwrap_or_else(|| ext_of(p));
    let (mut m, c) = match f.as_str() {
        "epub" => epub::probe(p),
        "pdf" => fixed::Pdf::probe(p),
        "djvu" => fixed::Djvu::probe(p),
        "cbz" | "cbr" | "cb7" | "cbt" => fixed::Comic::probe(p, &f),
        "mobi" | "azw3" => mobi::probe(p),
        "fb2" | "fbz" => office::fb2_probe(p, f == "fbz"),
        "docx" => office::docx_probe(p),
        "odt" => office::odt_probe(p),
        "html" => plain::html_probe(p),
        _ => (Meta::default(), Cover::None),
    };
    let (ft, fa) = from_filename(p);
    let junk = |t: &str| {
        let l = t.trim().to_lowercase();
        l.is_empty() || l == "untitled" || l == "unknown" || l == "sin título" || l == "title" || l.starts_with("microsoft word") || l.starts_with("powerpoint") || l.ends_with(".doc") || l.ends_with(".docx") || l.ends_with(".indd") || l.ends_with(".qxd") || l.ends_with(".dvi") || l.ends_with(".tex") || l.ends_with(".ppt") || l.ends_with(".pptx") || l == "documento1" || l.starts_with("presentación de powerpoint") || l.contains('\u{fffd}')
    };
    if junk(&m.title) {
        m.title = ft;
        if m.authors.is_empty() || f == "pdf" {
            if let Some(a) = fa {
                m.authors = vec![a];
            }
        }
    }
    m.title = m.title.split_whitespace().collect::<Vec<_>>().join(" ");
    m.authors = m.authors.iter().map(|a| flip_author(a)).filter(|a| !a.is_empty() && !["unknown", "desconocido", "autor desconocido", "user", "usuario", "admin"].contains(&a.to_lowercase().as_str())).collect();
    m.language = m.language.chars().take(2).collect::<String>().to_lowercase();
    (f, m, c)
}
/// «Apellido, Nombre» → «Nombre Apellido».
pub fn flip_author(a: &str) -> String {
    let a = a.split_whitespace().collect::<Vec<_>>().join(" ");
    let a = a.trim_matches([';', ',', ' ']).to_string();
    match a.split_once(", ") {
        Some((s, n)) if !n.contains(',') && s.split(' ').count() <= 3 && n.split(' ').count() <= 3 => format!("{n} {s}"),
        _ => a,
    }
}
/// «Autor - Título (2004)» o «Título - Autor»: solo se fía del primer guion.
pub fn from_filename(p: &Path) -> (String, Option<String>) {
    let mut s = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    for e in [".fb2", ".kepub"] {
        s = s.trim_end_matches(e).to_string();
    }
    let s = s.replace('_', " ");
    // quita paréntesis y corchetes de bibliotecas
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let t = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = if t.is_empty() { s.trim().to_string() } else { t };
    if let Some((a, b)) = t.split_once(" - ") {
        let (a, b) = (a.trim(), b.trim());
        let name = |x: &str| {
            let w: Vec<&str> = x.split(' ').collect();
            (2..=4).contains(&w.len()) && w.iter().all(|w| w.chars().next().is_some_and(|c| c.is_uppercase()) && !w.chars().any(|c| c.is_ascii_digit())) && !w.iter().any(|w| ["El", "La", "Los", "Las", "The", "Un", "Una", "Capítulo", "Guía"].contains(w))
        };
        if name(a) && !b.is_empty() {
            return (b.to_string(), Some(a.to_string()));
        }
        if name(b) && !a.is_empty() {
            return (a.to_string(), Some(b.to_string()));
        }
    }
    (t, None)
}
pub fn year_of(s: &str) -> i32 {
    let d: Vec<char> = s.chars().collect();
    for w in d.windows(4) {
        if w.iter().all(|c| c.is_ascii_digit()) {
            let y: i32 = w.iter().collect::<String>().parse().unwrap_or(0);
            if (1000..2100).contains(&y) {
                return y;
            }
        }
    }
    0
}
pub fn isbn_of(s: &str) -> String {
    let d: String = s.chars().filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x').collect();
    let n = if d.len() >= 13 && (d.starts_with("978") || d.starts_with("979")) { &d[..13] } else if d.len() == 10 { &d[..] } else { "" };
    n.to_uppercase()
}

pub fn decode(bytes: &[u8]) -> Option<image::RgbaImage> {
    let mut r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut l = image::Limits::default();
    l.max_alloc = Some(512 << 20);
    r.limits(l);
    Some(r.decode().ok()?.to_rgba8())
}
/// Ancho y alto sin decodificar la imagen entera.
pub fn dims(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()
}
/// Bytes a texto, adivinando la codificación.
pub fn to_text(b: &[u8]) -> String {
    if let Some((e, n)) = encoding_rs::Encoding::for_bom(b) {
        return e.decode(&b[n..]).0.into_owned();
    }
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let mut d = chardetng::EncodingDetector::new();
            d.feed(&b[..b.len().min(200_000)], true);
            d.guess(None, true).decode(b).0.into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        assert_eq!(from_filename(Path::new("Gabriel García Márquez - Cien años de soledad (1967).epub")), ("Cien años de soledad".into(), Some("Gabriel García Márquez".into())));
        assert_eq!(flip_author("Borges, Jorge Luis"), "Jorge Luis Borges");
        assert_eq!(year_of("2004-05-01"), 2004);
        assert_eq!(isbn_of("urn:isbn:978-84-376-0494-7"), "9788437604947");
    }
}
