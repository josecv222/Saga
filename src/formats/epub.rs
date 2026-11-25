//! EPUB 2 y 3.
use super::html::{self, join, percent, xml, Css, Node};
use super::{isbn_of, to_text, year_of, Cover, Meta};
use crate::doc::*;
use parking_lot::Mutex;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use zip::ZipArchive;

pub type Zip = ZipArchive<std::io::BufReader<File>>;
pub fn zip_open(p: &Path) -> Result<Zip, String> {
    let f = File::open(p).map_err(|_| "No se puede abrir el archivo".to_string())?;
    ZipArchive::new(std::io::BufReader::new(f)).map_err(|_| "El archivo está dañado".to_string())
}
pub fn zip_names(z: &Zip) -> Vec<String> {
    z.file_names().flatten().map(|n| n.to_string()).collect()
}
pub fn zip_read(z: &mut Zip, name: &str) -> Option<Vec<u8>> {
    let real = if z.index_for_name(name).is_some() {
        name.to_string()
    } else {
        let low = name.to_lowercase();
        zip_names(z).into_iter().find(|n| n.to_lowercase() == low)?
    };
    let mut f = z.by_name(&real).ok()?;
    if f.size() > 400 << 20 {
        return None;
    }
    let mut v = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut v).ok()?;
    Some(v)
}
pub struct ZipAssets(pub Mutex<Zip>);
impl Assets for ZipAssets {
    fn load(&self, n: &str) -> Option<Vec<u8>> {
        zip_read(&mut self.0.lock(), n)
    }
}

struct Opf {
    base: String,
    root: Node,
    /// id, ruta, tipo, propiedades
    items: Vec<(String, String, String, String)>,
}
fn opf(z: &mut Zip) -> Result<Opf, String> {
    let c = zip_read(z, "META-INF/container.xml").map(|b| to_text(&b));
    let path = c
        .and_then(|c| xml(&c).first("rootfile").and_then(|n| n.attr("full-path").map(percent)))
        .filter(|p| z.index_for_name(p).is_some())
        .or_else(|| zip_names(z).into_iter().find(|n| n.ends_with(".opf")))
        .ok_or("El EPUB no tiene índice interno")?;
    let root = xml(&to_text(&zip_read(z, &path).ok_or("El EPUB está dañado")?));
    let mut items = vec![];
    if let Some(m) = root.first("manifest") {
        for i in m.all("item") {
            items.push((i.attr("id").unwrap_or("").to_string(), join(&path, i.attr("href").unwrap_or("")), i.attr("media-type").unwrap_or("").to_string(), i.attr("properties").unwrap_or("").to_string()));
        }
    }
    Ok(Opf { base: path, root, items })
}

pub fn open(p: &Path) -> Result<Doc, String> {
    let mut z = zip_open(p)?;
    let o = opf(&mut z)?;
    if let Some(e) = zip_read(&mut z, "META-INF/encryption.xml") {
        let e = xml(&to_text(&e));
        let mut refs = vec![];
        e.deep("CipherReference", &mut refs);
        e.deep("cipherreference", &mut refs);
        if refs.iter().any(|r| r.attr("uri").is_some_and(|u| { let u = u.to_lowercase(); u.ends_with("html") || u.ends_with("htm") || u.ends_with("xml") })) {
            return Err("Este libro está protegido (DRM) y no se puede abrir".into());
        }
    }
    let mut css = Css::default();
    for (_, path, ty, _) in &o.items {
        if ty == "text/css" || path.ends_with(".css") {
            if let Some(b) = zip_read(&mut z, path) {
                css.add(&to_text(&b));
            }
        }
    }
    let mut order = vec![];
    if let Some(s) = o.root.first("spine") {
        for r in s.all("itemref") {
            if let Some(i) = o.items.iter().find(|i| Some(i.0.as_str()) == r.attr("idref")) {
                order.push(i.1.clone());
            }
        }
    }
    if order.is_empty() {
        order = o.items.iter().filter(|i| i.2.contains("html")).map(|i| i.1.clone()).collect();
    }
    let srcs: Vec<(String, String)> = order.iter().filter_map(|h| zip_read(&mut z, h).map(|b| (h.clone(), to_text(&b)))).collect();
    use rayon::prelude::*;
    let mut chapters: Vec<Chapter> = srcs
        .par_iter()
        .map(|(h, s)| {
            let out = html::parse(s, &css, h);
            Chapter { href: h.clone(), title: out.title, blocks: out.blocks, ..Default::default() }
        })
        .collect();
    for c in &mut chapters {
        if let Some(b) = c.blocks.first_mut() {
            b.break_before = false;
        }
    }
    let mut toc = vec![];
    if let Some(nav) = o.items.iter().find(|i| i.3.split_whitespace().any(|p| p == "nav")) {
        if let Some(b) = zip_read(&mut z, &nav.1) {
            toc = nav_toc(&to_text(&b), &nav.1);
        }
    }
    if toc.is_empty() {
        let id = o.root.first("spine").and_then(|s| s.attr("toc")).unwrap_or("ncx");
        let ncx = o.items.iter().find(|i| i.0 == id && i.1.ends_with("ncx")).or_else(|| o.items.iter().find(|i| i.2 == "application/x-dtbncx+xml"));
        if let Some(n) = ncx {
            if let Some(b) = zip_read(&mut z, &n.1) {
                let x = xml(&to_text(&b));
                if let Some(m) = x.first("navmap") {
                    ncx_walk(m, 0, &n.1, &mut toc);
                }
            }
        }
    }
    let lang = o.root.first("metadata").and_then(|m| m.find("language")).map(|n| n.full()).unwrap_or_default();
    Ok(Doc::build(chapters, toc, Arc::new(ZipAssets(Mutex::new(z))), lang))
}
fn ncx_walk(n: &Node, depth: u8, base: &str, out: &mut Vec<(String, u8, String)>) {
    for p in n.all("navpoint") {
        let t = p.find("navlabel").map(|l| l.full()).unwrap_or_default();
        let h = p.find("content").and_then(|c| c.attr("src")).unwrap_or("");
        out.push((t, depth, join(base, h)));
        ncx_walk(p, depth + 1, base, out);
    }
}
fn nav_toc(src: &str, base: &str) -> Vec<(String, u8, String)> {
    let x = xml(src);
    let mut navs = vec![];
    x.deep("nav", &mut navs);
    let nav = navs.iter().find(|n| n.attr("type").is_some_and(|t| t.contains("toc"))).or(navs.first());
    let mut out = vec![];
    fn walk(n: &Node, depth: u8, base: &str, out: &mut Vec<(String, u8, String)>) {
        for k in &n.kids {
            match html::local(&k.name) {
                "li" => {
                    if let Some(a) = k.find("a").or_else(|| k.find("span")) {
                        if let Some(h) = a.attr("href") {
                            out.push((a.full(), depth, join(base, h)));
                        }
                    }
                    if let Some(ol) = k.find("ol") {
                        walk(ol, depth + 1, base, out);
                    }
                }
                _ => walk(k, depth, base, out),
            }
        }
    }
    if let Some(n) = nav {
        walk(n, 0, base, &mut out);
    }
    out
}

pub fn probe(p: &Path) -> (Meta, Cover) {
    let mut m = Meta::default();
    let Ok(mut z) = zip_open(p) else { return (m, Cover::None) };
    let Ok(o) = opf(&mut z) else { return (m, Cover::None) };
    let mut cover_id = None;
    if let Some(md) = o.root.first("metadata") {
        meta_from(md, &mut m);
        for n in md.all("meta") {
            match n.attr("name") {
                Some("cover") => cover_id = n.attr("content").map(|s| s.to_string()),
                Some("calibre:series") => m.series = n.attr("content").unwrap_or("").into(),
                Some("calibre:series_index") => m.series_ix = n.attr("content").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                _ => {}
            }
            match n.attr("property") {
                Some("belongs-to-collection") if m.series.is_empty() => m.series = n.full(),
                Some("group-position") if m.series_ix == 0.0 => m.series_ix = n.full().parse().unwrap_or(0.0),
                _ => {}
            }
        }
    }
    let is_img = |i: &&(String, String, String, String)| i.2.starts_with("image/") && !i.2.contains("svg");
    let it = o
        .items
        .iter()
        .filter(is_img)
        .find(|i| i.3.contains("cover-image"))
        .or_else(|| cover_id.as_ref().and_then(|c| o.items.iter().filter(is_img).find(|i| &i.0 == c || i.1.ends_with(c.as_str()))))
        .or_else(|| o.items.iter().filter(is_img).find(|i| i.0.to_lowercase().contains("cover") || i.1.to_lowercase().contains("cover") || i.1.to_lowercase().contains("portada")));
    let mut data = it.and_then(|i| zip_read(&mut z, &i.1));
    if data.is_none() {
        // primera imagen de las dos primeras páginas
        if let Some(s) = o.root.first("spine") {
            for r in s.all("itemref").take(2) {
                let Some(i) = o.items.iter().find(|i| Some(i.0.as_str()) == r.attr("idref")) else { continue };
                let Some(b) = zip_read(&mut z, &i.1) else { continue };
                let out = html::parse(&to_text(&b), &Css::default(), &i.1);
                if let Some(BlockKind::Image(src)) = out.blocks.iter().map(|b| &b.kind).find(|k| matches!(k, BlockKind::Image(_))) {
                    // una ilustración apaisada no es una portada
                    data = zip_read(&mut z, src).filter(|d| image::ImageReader::new(std::io::Cursor::new(d)).with_guessed_format().ok().and_then(|r| r.into_dimensions().ok()).is_some_and(|(w, h)| h as f32 > w as f32 * 1.15 && h > 300));
                    break;
                }
            }
        }
    }
    let _ = &o.base;
    (m, data.map_or(Cover::None, Cover::Encoded))
}
pub fn meta_from(md: &Node, m: &mut Meta) {
    m.title = md.find("title").map(|n| n.full()).unwrap_or_default();
    for c in md.all("creator") {
        let role = c.attr("role").unwrap_or("aut");
        if role == "aut" || role.is_empty() {
            m.authors.push(c.full());
        }
    }
    m.publisher = md.find("publisher").map(|n| n.full()).unwrap_or_default();
    m.language = md.find("language").map(|n| n.full()).unwrap_or_default();
    m.description = md.find("description").map(|n| html::plain(&html::unescape(&n.full()))).unwrap_or_default();
    m.genres = md.all("subject").flat_map(|n| n.full().split([',', ';']).map(|s| s.trim().to_string()).collect::<Vec<_>>()).filter(|s| !s.is_empty() && s.len() < 40).take(6).collect();
    let dates: Vec<String> = md.all("date").map(|n| n.full()).collect();
    m.year = dates.iter().map(|d| year_of(d)).filter(|y| *y > 0).min().unwrap_or(0);
    for i in md.all("identifier") {
        let t = i.full();
        let scheme = i.attr("scheme").unwrap_or("").to_lowercase();
        if scheme == "isbn" || t.to_lowercase().contains("isbn") || (t.starts_with("978") && t.len() >= 13) {
            m.isbn = isbn_of(&t);
        }
    }
}
