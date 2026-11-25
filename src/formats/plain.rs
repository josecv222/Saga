//! TXT, Markdown, HTML suelto y RTF.
use super::epub::{zip_names, zip_open, zip_read, ZipAssets};
use super::html::{self, Css};
use super::office::split;
use super::{to_text, Cover, Meta};
use crate::doc::*;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn read(p: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(p).map_err(|_| "No se puede abrir el archivo".to_string())
}

pub fn txt(p: &Path) -> Result<Doc, String> {
    let s = to_text(&read(p)?).replace("\r\n", "\n").replace('\r', "\n");
    Ok(Doc::build(split(txt_blocks(&s)), vec![], Arc::new(NoAssets), String::new()))
}
fn txt_blocks(s: &str) -> Vec<Block> {
    // ¿Líneas cortadas a mano (Gutenberg) o un párrafo por línea?
    let lines: Vec<&str> = s.lines().collect();
    let full = lines.iter().filter(|l| !l.trim().is_empty()).count().max(1);
    let blank = lines.iter().filter(|l| l.trim().is_empty()).count();
    let long = lines.iter().filter(|l| l.chars().count() > 100).count();
    let hard = long * 20 < full && blank * 12 > full;
    let mut out: Vec<Block> = vec![];
    let mut cur = String::new();
    let mut push = |cur: &mut String| {
        let t = cur.split_whitespace().collect::<Vec<_>>().join(" ");
        cur.clear();
        if t.is_empty() {
            return;
        }
        if t.chars().all(|c| "*-_=~•·— ".contains(c)) && t.chars().count() >= 1 && !t.chars().any(char::is_alphanumeric) {
            out.push(Block::new(BlockKind::Rule));
            return;
        }
        let kind = if looks_like_chapter(&t) { BlockKind::Heading(2) } else { BlockKind::Para };
        let mut b = Block::new(kind);
        b.text = t;
        out.push(b);
    };
    for l in lines {
        if l.trim().is_empty() {
            push(&mut cur);
        } else if hard {
            // una sangría nueva también abre párrafo
            if l.starts_with("   ") && !cur.is_empty() || looks_like_chapter(l) {
                push(&mut cur);
            }
            cur.push_str(l);
            cur.push(' ');
        } else {
            cur.push_str(l);
            push(&mut cur);
        }
    }
    push(&mut cur);
    out
}

/// Carpeta del propio archivo: imágenes relativas.
struct DirAssets(PathBuf);
impl Assets for DirAssets {
    fn load(&self, n: &str) -> Option<Vec<u8>> {
        if n.contains("://") || n.contains("..") {
            return None;
        }
        std::fs::read(self.0.join(n)).ok()
    }
}
fn dir(p: &Path) -> Arc<dyn Assets> {
    Arc::new(DirAssets(p.parent().map(|p| p.to_path_buf()).unwrap_or_default()))
}
pub fn html(p: &Path) -> Result<Doc, String> {
    let o = html::parse(&to_text(&read(p)?), &Css::default(), "");
    Ok(Doc::build(split(o.blocks), vec![], dir(p), String::new()))
}
pub fn html_probe(p: &Path) -> (Meta, Cover) {
    let s = read(p).map(|b| to_text(&b)).unwrap_or_default();
    let o = html::parse(&s, &Css::default(), "");
    (Meta { title: o.title, ..Default::default() }, Cover::None)
}
pub fn htmlz(p: &Path) -> Result<Doc, String> {
    let mut z = zip_open(p)?;
    let mut css = Css::default();
    if let Some(b) = zip_read(&mut z, "style.css") {
        css.add(&to_text(&b));
    }
    let n = zip_names(&z).into_iter().find(|n| n.ends_with(".html") || n.ends_with(".htm")).ok_or("No hay HTML dentro")?;
    let o = html::parse(&to_text(&zip_read(&mut z, &n).ok_or("Archivo dañado")?), &css, &n);
    Ok(Doc::build(split(o.blocks), vec![], Arc::new(ZipAssets(Mutex::new(z))), String::new()))
}

pub fn markdown(p: &Path) -> Result<Doc, String> {
    let s = to_text(&read(p)?).replace("\r\n", "\n");
    let o = html::parse(&md_html(&s), &Css::default(), "");
    Ok(Doc::build(split(o.blocks), vec![], dir(p), String::new()))
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
fn md_inline(s: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    let (mut b, mut e) = (false, false);
    while i < s.len() {
        let r = &s[i..];
        if r.starts_with('\\') && r.len() > 1 {
            let c = r[1..].chars().next().unwrap();
            out.push_str(&esc(&c.to_string()));
            i += 1 + c.len_utf8();
        } else if let Some(x) = r.strip_prefix('`') {
            if let Some(n) = x.find('`') {
                out.push_str(&format!("<code>{}</code>", esc(&x[..n])));
                i += n + 2;
            } else {
                out.push('`');
                i += 1;
            }
        } else if r.starts_with("**") || r.starts_with("__") {
            out.push_str(if b { "</b>" } else { "<b>" });
            b = !b;
            i += 2;
        } else if r.starts_with('*') || (r.starts_with('_') && (e || s[..i].ends_with(' ') || i == 0)) {
            out.push_str(if e { "</i>" } else { "<i>" });
            e = !e;
            i += 1;
        } else if r.starts_with("![") || r.starts_with('[') {
            let img = r.starts_with('!');
            let st = if img { 2 } else { 1 };
            let m = r.find("](").and_then(|m| r[m..].find(')').map(|c| (m, m + c)));
            if let Some((m, c)) = m {
                let t = &r[st..m];
                let u = r[m + 2..c].split(' ').next().unwrap_or("");
                out.push_str(&if img { format!("<img src=\"{u}\"/>") } else { format!("<a href=\"{u}\">{}</a>", md_inline(t)) });
                i += c + 1;
            } else {
                out.push_str(&esc(&r[..st]));
                i += st;
            }
        } else {
            let c = r.chars().next().unwrap();
            out.push_str(&esc(c.encode_utf8(&mut [0; 4])));
            i += c.len_utf8();
        }
    }
    if b {
        out.push_str("</b>");
    }
    if e {
        out.push_str("</i>");
    }
    out
}
fn md_html(s: &str) -> String {
    let mut out = String::new();
    let mut para: Vec<String> = vec![];
    let mut list: Option<&str> = None;
    let mut code = false;
    let mut quote = false;
    macro_rules! end_para {
        () => {
            if !para.is_empty() {
                out.push_str(&format!("<p>{}</p>", md_inline(&para.join(" "))));
                para.clear();
            }
        };
    }
    macro_rules! end_list {
        () => {
            if let Some(l) = list.take() {
                out.push_str(&format!("</{l}>"));
            }
        };
    }
    for l in s.lines() {
        let t = l.trim();
        if t.starts_with("```") {
            end_para!();
            end_list!();
            out.push_str(if code { "</pre>" } else { "<pre>" });
            code = !code;
            continue;
        }
        if code {
            out.push_str(&esc(l));
            out.push('\n');
            continue;
        }
        let q = t.starts_with('>');
        if q != quote {
            end_para!();
            end_list!();
            out.push_str(if q { "<blockquote>" } else { "</blockquote>" });
            quote = q;
        }
        let t = if q { t[1..].trim() } else { t };
        if t.is_empty() {
            end_para!();
            end_list!();
        } else if t.starts_with('#') && t.trim_start_matches('#').starts_with(' ') {
            end_para!();
            end_list!();
            let n = t.len() - t.trim_start_matches('#').len();
            let n = n.min(6);
            out.push_str(&format!("<h{n}>{}</h{n}>", md_inline(t.trim_matches('#').trim())));
        } else if t.len() >= 3 && (t.chars().all(|c| c == '-' || c == ' ') || t.chars().all(|c| c == '*' || c == ' ') || t.chars().all(|c| c == '_')) {
            end_para!();
            end_list!();
            out.push_str("<hr/>");
        } else if t.starts_with('|') && t.ends_with('|') && t.len() > 2 {
            end_para!();
            end_list!();
            let cells: Vec<&str> = t.trim_matches('|').split('|').map(|c| c.trim()).collect();
            if !cells.iter().all(|c| !c.is_empty() && c.chars().all(|c| c == '-' || c == ':')) {
                out.push_str("<tr>");
                for c in cells {
                    out.push_str(&format!("<td>{}</td>", esc(c)));
                }
                out.push_str("</tr>");
            }
        } else if let Some((tag, rest)) = item(t) {
            end_para!();
            if list != Some(tag) {
                end_list!();
                out.push_str(&format!("<{tag}>"));
                list = Some(tag);
            }
            out.push_str(&format!("<li>{}</li>", md_inline(rest)));
        } else {
            para.push(t.to_string());
            if l.ends_with("  ") {
                end_para!();
            }
        }
    }
    end_para!();
    end_list!();
    out
}
fn item(t: &str) -> Option<(&'static str, &str)> {
    for m in ["- ", "* ", "+ "] {
        if let Some(r) = t.strip_prefix(m) {
            return Some(("ul", r));
        }
    }
    let d = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if d > 0 && d < 4 && (t[d..].starts_with(". ") || t[d..].starts_with(") ")) {
        return Some(("ol", &t[d + 2..]));
    }
    None
}

pub fn rtf(p: &Path) -> Result<Doc, String> {
    Ok(Doc::build(split(rtf_blocks(&read(p)?)), vec![], Arc::new(NoAssets), String::new()))
}
fn rtf_blocks(b: &[u8]) -> Vec<Block> {
    #[derive(Clone, Default)]
    struct S {
        style: u16,
        skip: bool,
        center: bool,
        uc: usize,
    }
    let mut st = vec![S { uc: 1, ..Default::default() }];
    let mut out: Vec<Block> = vec![];
    let mut cur = Block::new(BlockKind::Para);
    let mut enc = encoding_rs::WINDOWS_1252;
    let mut i = 0;
    let mut skip_chars = 0;
    let mut raw: Vec<u8> = vec![];
    fn add(cur: &mut Block, s: &str, style: u16) {
        let a = cur.text.len();
        cur.text.push_str(s);
        let e = cur.text.len();
        if style != 0 && e > a {
            match cur.spans.last_mut() {
                Some(l) if l.end == a && l.style == style => l.end = e,
                _ => cur.spans.push(Span { start: a, end: e, style, link: None }),
            }
        }
    }
    macro_rules! flush_raw {
        () => {
            if !raw.is_empty() {
                let s = enc.decode(&raw).0.into_owned();
                let t = st.last().unwrap();
                if !t.skip {
                    add(&mut cur, &s, t.style);
                }
                raw.clear();
            }
        };
    }
    while i < b.len() {
        match b[i] {
            b'{' => {
                flush_raw!();
                let t = st.last().unwrap().clone();
                st.push(t);
                i += 1;
            }
            b'}' => {
                flush_raw!();
                if st.len() > 1 {
                    st.pop();
                }
                i += 1;
            }
            b'\\' => {
                i += 1;
                if i >= b.len() {
                    break;
                }
                let c = b[i];
                if c == b'\'' {
                    let h = std::str::from_utf8(b.get(i + 1..i + 3).unwrap_or(b"")).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                    if let Some(v) = h {
                        if skip_chars > 0 {
                            skip_chars -= 1;
                        } else {
                            raw.push(v);
                        }
                    }
                    i += 3;
                } else if c.is_ascii_alphabetic() {
                    flush_raw!();
                    let s = i;
                    while i < b.len() && b[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let w = std::str::from_utf8(&b[s..i]).unwrap_or("");
                    let ns = i;
                    if i < b.len() && b[i] == b'-' {
                        i += 1;
                    }
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                    let num: Option<i32> = std::str::from_utf8(&b[ns..i]).ok().and_then(|s| s.parse().ok());
                    if i < b.len() && b[i] == b' ' {
                        i += 1;
                    }
                    let t = st.last_mut().unwrap();
                    let flag = |v: &mut u16, f: u16| if num == Some(0) { *v &= !f } else { *v |= f };
                    match w {
                        "par" | "sect" | "page" => {
                            if !t.skip {
                                let mut b = std::mem::replace(&mut cur, Block::new(BlockKind::Para));
                                if t.center {
                                    b.align = Align::Center;
                                }
                                let k = b.text.trim_end().len();
                                b.text.truncate(k);
                                b.spans.retain(|s| s.start < k);
                                for s in &mut b.spans {
                                    s.end = s.end.min(k);
                                }
                                if looks_like_chapter(&b.text) {
                                    b.kind = BlockKind::Heading(2);
                                }
                                if !b.text.trim().is_empty() {
                                    out.push(b);
                                }
                            }
                        }
                        "line" => add(&mut cur, "\n", 0),
                        "tab" => add(&mut cur, " ", 0),
                        "b" => flag(&mut t.style, BOLD),
                        "i" => flag(&mut t.style, ITALIC),
                        "ul" => flag(&mut t.style, UNDER),
                        "ulnone" => t.style &= !UNDER,
                        "super" => t.style |= SUP,
                        "sub" => t.style |= SUB,
                        "nosupersub" => t.style &= !(SUP | SUB),
                        "plain" => t.style = 0,
                        "qc" => t.center = true,
                        "pard" | "ql" | "qj" => t.center = false,
                        "uc" => t.uc = num.unwrap_or(1).max(0) as usize,
                        "ansicpg" => {
                            if let Some(e) = num.and_then(|n| encoding_rs::Encoding::for_label(format!("windows-{n}").as_bytes())) {
                                enc = e;
                            }
                        }
                        "u" => {
                            let v = num.unwrap_or(0);
                            let v = if v < 0 { v + 65536 } else { v };
                            if let Some(c) = char::from_u32(v as u32) {
                                if !t.skip {
                                    let s = t.style;
                                    add(&mut cur, c.encode_utf8(&mut [0; 4]), s);
                                }
                            }
                            skip_chars = t.uc;
                        }
                        "emdash" => add(&mut cur, "—", 0),
                        "endash" => add(&mut cur, "–", 0),
                        "lquote" => add(&mut cur, "‘", 0),
                        "rquote" => add(&mut cur, "’", 0),
                        "ldblquote" => add(&mut cur, "“", 0),
                        "rdblquote" => add(&mut cur, "”", 0),
                        "bullet" => add(&mut cur, "•", 0),
                        "fonttbl" | "colortbl" | "stylesheet" | "info" | "pict" | "header" | "footer" | "headerl" | "headerr" | "footerl" | "footerr" | "object" | "themedata" | "datastore" | "latentstyles" | "listtable" | "listoverridetable" | "generator" | "fldinst" => t.skip = true,
                        _ => {}
                    }
                } else {
                    match c {
                        b'*' => st.last_mut().unwrap().skip = true,
                        b'~' => add(&mut cur, "\u{a0}", 0),
                        b'\\' | b'{' | b'}' => raw.push(c),
                        _ => {}
                    }
                    i += 1;
                }
            }
            b'\r' | b'\n' => i += 1,
            c => {
                if skip_chars > 0 {
                    skip_chars -= 1;
                } else if !st.last().unwrap().skip {
                    raw.push(c);
                }
                i += 1;
            }
        }
    }
    flush_raw!();
    if !cur.text.trim().is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rtf_basic() {
        let b = rtf_blocks(b"{\\rtf1\\ansi{\\fonttbl{\\f0 Arial;}}Hola \\b mundo\\b0 \\'f1\\par Dos \\u241?o\\par}");
        assert_eq!(b[0].text, "Hola mundoñ");
        assert_eq!(b[1].text, "Dos ño");
        assert_eq!(b[0].spans[0].style, BOLD);
    }
    #[test]
    fn md() {
        let h = md_html("# T\n\nHola **a** [x](http://y)\n\n- uno\n- dos\n");
        assert!(h.contains("<h1>T</h1>") && h.contains("<b>a</b>") && h.contains("<li>dos</li>"));
    }
    #[test]
    fn txt_hard() {
        let b = txt_blocks("CAPÍTULO I\n\nEsta línea sigue\nen esta otra.\n\nOtro párrafo\nmás.\n");
        assert_eq!(b.len(), 3);
        assert_eq!(b[1].text, "Esta línea sigue en esta otra.");
    }
}
