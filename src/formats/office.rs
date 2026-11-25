//! FB2, DOCX y ODT.
use super::epub::{zip_names, zip_open, zip_read, ZipAssets};
use super::html::{self, local, xml, Node};
use super::{to_text, year_of, Cover, Meta};
use crate::doc::*;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

struct W {
    blocks: Vec<Block>,
}
impl W {
    fn run(b: &mut Block, s: &str, style: u16, link: Option<Arc<str>>) {
        let st = b.text.len();
        for c in s.chars() {
            let c = if matches!(c, '\r' | '\t') { ' ' } else { c };
            if c == ' ' && (b.text.is_empty() || b.text.ends_with(' ')) {
                continue;
            }
            b.text.push(c);
        }
        let e = b.text.len();
        if e > st && (style != 0 || link.is_some()) {
            match b.spans.last_mut() {
                Some(l) if l.end == st && l.style == style && l.link == link => l.end = e,
                _ => b.spans.push(Span { start: st, end: e, style, link }),
            }
        }
    }
    fn push(&mut self, mut b: Block) {
        let k = b.text.trim_end().len();
        b.text.truncate(k);
        for s in &mut b.spans {
            s.end = s.end.min(k);
        }
        b.spans.retain(|s| s.start < s.end);
        if !b.text.is_empty() || matches!(b.kind, BlockKind::Image(_) | BlockKind::Rule) {
            self.blocks.push(b);
        }
    }
}
/// Parte por encabezados grandes para que cada capítulo empiece página.
pub fn split(blocks: Vec<Block>) -> Vec<Chapter> {
    let top = blocks.iter().filter_map(|b| if let BlockKind::Heading(l) = b.kind { Some(l) } else { None }).min().unwrap_or(0);
    let n = blocks.iter().filter(|b| b.kind == BlockKind::Heading(top)).count();
    let mut out: Vec<Chapter> = vec![];
    for b in blocks {
        let cut = out.is_empty() || (n > 1 && b.kind == BlockKind::Heading(top));
        if cut {
            out.push(Chapter { href: format!("c{}", out.len()), ..Default::default() });
        }
        out.last_mut().unwrap().blocks.push(b);
    }
    out
}

// ---------------------------------------------------------------- FB2
fn fb2_src(p: &Path, zipped: bool) -> Result<String, String> {
    let b = if zipped {
        let mut z = zip_open(p)?;
        let n = zip_names(&z).into_iter().find(|n| n.to_lowercase().ends_with(".fb2")).ok_or("No hay ningún FB2 dentro")?;
        zip_read(&mut z, &n).ok_or("El archivo está dañado")?
    } else {
        std::fs::read(p).map_err(|_| "No se puede abrir el archivo")?
    };
    // la codificación suele venir en la cabecera XML
    let head = String::from_utf8_lossy(&b[..b.len().min(200)]).to_lowercase();
    if let Some(i) = head.find("encoding=\"") {
        let e = &head[i + 10..];
        if let Some(enc) = e.find('"').and_then(|j| encoding_rs::Encoding::for_label(e[..j].as_bytes())) {
            return Ok(enc.decode(&b).0.into_owned());
        }
    }
    Ok(to_text(&b))
}
fn b64(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut n) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        acc = acc << 6 | v as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
            acc &= (1 << n) - 1;
        }
    }
    out
}
fn fb2_inline(n: &Node, b: &mut Block, style: u16, link: Option<Arc<str>>) {
    for k in &n.kids {
        match local(&k.name) {
            "#" => W::run(b, &k.text.replace('\n', " "), style, link.clone()),
            "emphasis" => fb2_inline(k, b, style | ITALIC, link.clone()),
            "strong" => fb2_inline(k, b, style | BOLD, link.clone()),
            "sup" => fb2_inline(k, b, style | SUP, link.clone()),
            "sub" => fb2_inline(k, b, style | SUB, link.clone()),
            "code" => fb2_inline(k, b, style | MONO, link.clone()),
            "strikethrough" => fb2_inline(k, b, style | STRIKE, link.clone()),
            "a" => fb2_inline(k, b, style | if k.attr("type") == Some("note") { SUP } else { 0 }, k.attr("href").map(|h| h.into())),
            _ => fb2_inline(k, b, style, link.clone()),
        }
    }
}
fn fb2_walk(n: &Node, depth: u8, quote: u8, w: &mut W, ids: &mut Vec<String>) {
    for k in &n.kids {
        let mut mk = |kind: BlockKind| {
            let mut b = Block::new(kind);
            b.quote = quote;
            b.ids = std::mem::take(ids);
            if let Some(i) = k.attr("id") {
                b.ids.push(i.into());
            }
            b
        };
        match local(&k.name) {
            "title" => {
                let mut b = mk(BlockKind::Heading(depth.clamp(1, 6)));
                b.align = Align::Center;
                let ps: Vec<&Node> = k.all("p").collect();
                for (i, p) in ps.iter().enumerate() {
                    if i > 0 {
                        b.text.push('\n');
                    }
                    fb2_inline(p, &mut b, 0, None);
                }
                w.push(b);
            }
            "subtitle" => {
                let mut b = mk(BlockKind::Heading(5));
                b.align = Align::Center;
                fb2_inline(k, &mut b, 0, None);
                w.push(b);
            }
            "p" | "v" | "text-author" => {
                let mut b = mk(if quote > 0 { BlockKind::Quote } else { BlockKind::Para });
                let st = if local(&k.name) == "text-author" { ITALIC } else { 0 };
                fb2_inline(k, &mut b, st, None);
                w.push(b);
            }
            "empty-line" => w.push(mk(BlockKind::Rule)),
            "image" => {
                let h = k.attr("href").unwrap_or("").trim_start_matches('#').to_string();
                w.push(mk(BlockKind::Image(h)));
            }
            "epigraph" | "cite" | "poem" | "stanza" | "annotation" => fb2_walk(k, depth, quote.max(1), w, ids),
            "section" | "body" => {
                if let Some(i) = k.attr("id") {
                    ids.push(i.into());
                }
                fb2_walk(k, depth + 1, quote, w, ids);
            }
            "tr" => {
                let mut b = mk(BlockKind::Row(false));
                b.text = k.kids.iter().filter(|c| matches!(local(&c.name), "td" | "th")).map(|c| c.full()).collect::<Vec<_>>().join("\t");
                w.push(b);
            }
            "table" => fb2_walk(k, depth, quote, w, ids),
            _ => {}
        }
    }
}
pub fn fb2(p: &Path, zipped: bool) -> Result<Doc, String> {
    let x = xml(&fb2_src(p, zipped)?);
    let mut pics = HashMap::new();
    let mut bins = vec![];
    x.deep("binary", &mut bins);
    for b in bins {
        if let Some(i) = b.attr("id") {
            pics.insert(i.to_string(), b64(&b.full()));
        }
    }
    let mut w = W { blocks: vec![] };
    let root = x.first("fictionbook").unwrap_or(&x);
    let mut ids = vec![];
    if let Some(c) = root.first("coverpage").and_then(|c| c.first("image")).and_then(|i| i.attr("href")) {
        w.push(Block::new(BlockKind::Image(c.trim_start_matches('#').into())));
    }
    for b in root.all("body") {
        fb2_walk(b, 1, 0, &mut w, &mut ids);
    }
    let lang = root.first("lang").map(|l| l.full()).unwrap_or_default();
    Ok(Doc::build(split(w.blocks), vec![], Arc::new(MapAssets(pics)), lang))
}
pub fn fb2_probe(p: &Path, zipped: bool) -> (Meta, Cover) {
    let mut m = Meta::default();
    let Ok(s) = fb2_src(p, zipped) else { return (m, Cover::None) };
    let x = xml(&s);
    if let Some(t) = x.first("title-info") {
        m.title = t.find("book-title").map(|n| n.full()).unwrap_or_default();
        for a in t.all("author") {
            let part = |k: &str| a.find(k).map(|n| n.full()).unwrap_or_default();
            m.authors.push([part("first-name"), part("middle-name"), part("last-name")].join(" ").split_whitespace().collect::<Vec<_>>().join(" "));
        }
        m.genres = t.all("genre").map(|g| g.full()).collect();
        m.language = t.find("lang").map(|n| n.full()).unwrap_or_default();
        m.year = t.find("date").map(|n| year_of(&n.full())).unwrap_or(0);
        m.description = t.find("annotation").map(|n| n.all("p").map(|p| p.full()).collect::<Vec<_>>().join("\n\n")).unwrap_or_default();
        if let Some(s) = t.find("sequence") {
            m.series = s.attr("name").unwrap_or("").into();
            m.series_ix = s.attr("number").and_then(|n| n.parse().ok()).unwrap_or(0.0);
        }
    }
    if let Some(pi) = x.first("publish-info") {
        m.publisher = pi.find("publisher").map(|n| n.full()).unwrap_or_default();
        m.isbn = pi.find("isbn").map(|n| super::isbn_of(&n.full())).unwrap_or_default();
    }
    let id = x.first("coverpage").and_then(|c| c.first("image")).and_then(|i| i.attr("href")).map(|h| h.trim_start_matches('#').to_string());
    let mut bins = vec![];
    x.deep("binary", &mut bins);
    let c = id.and_then(|i| bins.into_iter().find(|b| b.attr("id") == Some(i.as_str()))).map(|b| b64(&b.full()));
    (m, c.map_or(Cover::None, Cover::Encoded))
}

// ---------------------------------------------------------------- DOCX
fn core_meta(x: &Node) -> Meta {
    let g = |k: &str| x.first(k).map(|n| n.full()).unwrap_or_default();
    Meta { title: g("title"), authors: g("creator").split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(), description: g("description"), language: g("language"), year: year_of(&g("created")), ..Default::default() }
}
pub fn docx_probe(p: &Path) -> (Meta, Cover) {
    let Ok(mut z) = zip_open(p) else { return (Meta::default(), Cover::None) };
    let m = zip_read(&mut z, "docProps/core.xml").map(|b| core_meta(&xml(&to_text(&b)))).unwrap_or_default();
    let c = zip_read(&mut z, "docProps/thumbnail.jpeg").map_or(Cover::None, Cover::Encoded);
    (m, c)
}
pub fn docx(p: &Path) -> Result<Doc, String> {
    let mut z = zip_open(p)?;
    let x = xml(&to_text(&zip_read(&mut z, "word/document.xml").ok_or("El documento está dañado")?));
    let mut rels: HashMap<String, String> = HashMap::new();
    if let Some(r) = zip_read(&mut z, "word/_rels/document.xml.rels") {
        let r = xml(&to_text(&r));
        let mut v = vec![];
        r.deep("relationship", &mut v);
        for n in v {
            if let (Some(i), Some(t)) = (n.attr("id"), n.attr("target")) {
                rels.insert(i.into(), if t.contains("://") { t.into() } else { html::join("word/document.xml", t) });
            }
        }
    }
    let mut heads: HashMap<String, u8> = HashMap::new();
    if let Some(s) = zip_read(&mut z, "word/styles.xml") {
        let s = xml(&to_text(&s));
        let mut v = vec![];
        s.deep("style", &mut v);
        for n in v {
            let id = n.attr("styleid").unwrap_or("").to_string();
            let name = n.find("name").and_then(|x| x.attr("val")).unwrap_or("").to_lowercase();
            if let Some(d) = name.strip_prefix("heading ").and_then(|d| d.trim().parse().ok()) {
                heads.insert(id, d);
            } else if name == "title" {
                heads.insert(id, 1);
            }
        }
    }
    let mut w = W { blocks: vec![] };
    if let Some(body) = x.first("body") {
        docx_body(body, &rels, &heads, &mut w);
    }
    Ok(Doc::build(split(w.blocks), vec![], Arc::new(ZipAssets(Mutex::new(z))), String::new()))
}
fn on(n: Option<&Node>) -> bool {
    n.is_some_and(|n| !matches!(n.attr("val"), Some("0" | "false" | "none")))
}
fn docx_body(body: &Node, rels: &HashMap<String, String>, heads: &HashMap<String, u8>, w: &mut W) {
    for k in &body.kids {
        match local(&k.name) {
            "p" => {
                let pr = k.find("ppr");
                let st = pr.and_then(|p| p.find("pstyle")).and_then(|s| s.attr("val")).unwrap_or("");
                let lvl = heads.get(st).copied().or_else(|| pr.and_then(|p| p.find("outlinelvl")).and_then(|o| o.attr("val")).and_then(|v| v.parse::<u8>().ok()).filter(|v| *v < 6).map(|v| v + 1));
                let num = pr.and_then(|p| p.find("numpr"));
                let kind = match (lvl, num) {
                    (Some(l), _) => BlockKind::Heading(l.clamp(1, 6)),
                    (None, Some(n)) => BlockKind::Item("•".into(), 1 + n.find("ilvl").and_then(|i| i.attr("val")).and_then(|v| v.parse().ok()).unwrap_or(0u8)),
                    _ => BlockKind::Para,
                };
                let mut b = Block::new(kind);
                b.align = match pr.and_then(|p| p.find("jc")).and_then(|j| j.attr("val")) {
                    Some("center") => Align::Center,
                    Some("right" | "end") => Align::Right,
                    _ => Align::Auto,
                };
                b.break_before = on(pr.and_then(|p| p.find("pagebreakbefore")));
                let mut pics = vec![];
                docx_runs(k, rels, &mut b, None, &mut pics);
                w.push(b);
                for p in pics {
                    w.push(Block::new(BlockKind::Image(p)));
                }
            }
            "tbl" => {
                let mut first = true;
                for tr in k.all("tr") {
                    let mut b = Block::new(BlockKind::Row(first));
                    b.text = tr.all("tc").map(|c| c.all("p").map(|p| { let mut t = Block::new(BlockKind::Para); docx_runs(p, rels, &mut t, None, &mut vec![]); t.text }).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("\t");
                    first = false;
                    w.push(b);
                }
            }
            "sdt" | "sdtcontent" | "ins" => docx_body(k, rels, heads, w),
            _ => {}
        }
    }
}
fn docx_runs(n: &Node, rels: &HashMap<String, String>, b: &mut Block, link: Option<Arc<str>>, pics: &mut Vec<String>) {
    for k in &n.kids {
        match local(&k.name) {
            "r" => {
                let pr = k.find("rpr");
                let mut s = 0;
                if let Some(p) = pr {
                    if on(p.find("b")) { s |= BOLD; }
                    if on(p.find("i")) { s |= ITALIC; }
                    if on(p.find("u")) { s |= UNDER; }
                    if on(p.find("strike")) { s |= STRIKE; }
                    if on(p.find("smallcaps")) { s |= SMALLCAPS; }
                    match p.find("vertalign").and_then(|v| v.attr("val")) {
                        Some("superscript") => s |= SUP,
                        Some("subscript") => s |= SUB,
                        _ => {}
                    }
                }
                if matches!(b.kind, BlockKind::Heading(_)) {
                    s &= !BOLD;
                }
                for t in &k.kids {
                    match local(&t.name) {
                        "t" => {
                            let txt: String = t.kids.iter().map(|x| x.text.as_str()).collect();
                            W::run(b, &txt, s, link.clone());
                        }
                        "tab" => W::run(b, " ", s, None),
                        "br" | "cr" => {
                            if t.attr("type") != Some("page") && !b.text.is_empty() {
                                b.text.push('\n');
                            }
                        }
                        "drawing" | "pict" | "object" => {
                            let mut v = vec![];
                            t.deep("blip", &mut v);
                            t.deep("imagedata", &mut v);
                            for i in v {
                                if let Some(r) = i.attr("embed").or(i.attr("id")).and_then(|r| rels.get(r)) {
                                    pics.push(r.clone());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            "hyperlink" => {
                let l = k.attr("id").and_then(|i| rels.get(i)).map(|s| s.as_str().into()).or_else(|| k.attr("anchor").map(|a| format!("#{a}").into()));
                docx_runs(k, rels, b, l, pics);
            }
            "bookmarkstart" => {
                if let Some(n) = k.attr("name") {
                    b.ids.push(n.into());
                }
            }
            "ins" | "smarttag" | "sdt" | "sdtcontent" => docx_runs(k, rels, b, link.clone(), pics),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- ODT
pub fn odt_probe(p: &Path) -> (Meta, Cover) {
    let Ok(mut z) = zip_open(p) else { return (Meta::default(), Cover::None) };
    let mut m = Meta::default();
    if let Some(b) = zip_read(&mut z, "meta.xml") {
        let x = xml(&to_text(&b));
        m = core_meta(&x);
        if m.authors.is_empty() {
            m.authors = x.first("initial-creator").map(|n| vec![n.full()]).unwrap_or_default();
        }
    }
    let c = zip_read(&mut z, "Thumbnails/thumbnail.png").map_or(Cover::None, Cover::Encoded);
    (m, c)
}
pub fn odt(p: &Path) -> Result<Doc, String> {
    let mut z = zip_open(p)?;
    let x = xml(&to_text(&zip_read(&mut z, "content.xml").ok_or("El documento está dañado")?));
    let mut styles: HashMap<String, (u16, Align)> = HashMap::new();
    let mut v = vec![];
    x.deep("style", &mut v);
    for s in v {
        let mut f = 0;
        let mut al = Align::Auto;
        if let Some(t) = s.find("text-properties") {
            if t.attr("font-weight") == Some("bold") { f |= BOLD; }
            if t.attr("font-style") == Some("italic") { f |= ITALIC; }
            if t.attr("text-underline-style").is_some_and(|u| u != "none") { f |= UNDER; }
            if let Some(t) = t.attr("text-position") {
                if t.starts_with("super") { f |= SUP; }
                if t.starts_with("sub") { f |= SUB; }
            }
        }
        if let Some(t) = s.find("paragraph-properties") {
            match t.attr("text-align") {
                Some("center") => al = Align::Center,
                Some("end" | "right") => al = Align::Right,
                _ => {}
            }
        }
        styles.insert(s.attr("name").unwrap_or("").into(), (f, al));
    }
    fn inline(n: &Node, st: &HashMap<String, (u16, Align)>, b: &mut Block, style: u16, link: Option<Arc<str>>, pics: &mut Vec<String>) {
        for k in &n.kids {
            match local(&k.name) {
                "#" => W::run(b, &k.text.replace('\n', " "), style, link.clone()),
                "s" | "tab" => W::run(b, " ", style, None),
                "line-break" => b.text.push('\n'),
                "a" => inline(k, st, b, style, k.attr("href").map(|h| h.into()), pics),
                "image" => {
                    if let Some(h) = k.attr("href") {
                        pics.push(h.into());
                    }
                }
                "note" | "annotation" => {}
                _ => {
                    let s = k.attr("style-name").and_then(|s| st.get(s)).map_or(0, |s| s.0);
                    inline(k, st, b, style | s, link.clone(), pics);
                }
            }
        }
    }
    fn walk(n: &Node, st: &HashMap<String, (u16, Align)>, depth: u8, w: &mut W) {
        for k in &n.kids {
            match local(&k.name) {
                t @ ("p" | "h") => {
                    let kind = if t == "h" { BlockKind::Heading(k.attr("outline-level").and_then(|l| l.parse().ok()).unwrap_or(1u8).clamp(1, 6)) } else if depth > 0 { BlockKind::Item("•".into(), depth) } else { BlockKind::Para };
                    let mut b = Block::new(kind);
                    let s = k.attr("style-name").and_then(|s| st.get(s)).copied().unwrap_or((0, Align::Auto));
                    b.align = s.1;
                    let mut pics = vec![];
                    inline(k, st, &mut b, if t == "h" { 0 } else { s.0 }, None, &mut pics);
                    w.push(b);
                    for p in pics {
                        w.push(Block::new(BlockKind::Image(p)));
                    }
                }
                "list" => walk(k, st, depth + 1, w),
                "table-row" => {
                    let mut b = Block::new(BlockKind::Row(false));
                    b.text = k.all("table-cell").map(|c| c.full()).collect::<Vec<_>>().join("\t");
                    w.push(b);
                }
                "#" => {}
                _ => walk(k, st, depth, w),
            }
        }
    }
    let mut w = W { blocks: vec![] };
    if let Some(t) = x.first("text") {
        walk(t, &styles, 0, &mut w);
    }
    Ok(Doc::build(split(w.blocks), vec![], Arc::new(ZipAssets(Mutex::new(z))), String::new()))
}
