//! HTML/XHTML y XML tolerantes a errores → bloques de lectura.
use crate::doc::*;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Open { name: String, attrs: Vec<(String, String)>, close: bool },
    Close(String),
    Text(String),
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.char_indices().take(12).find(|c| c.1 == ';').map(|c| c.0);
        let mut done = false;
        if let Some(e) = end {
            let name = &rest[1..e];
            if let Some(c) = entity(name) {
                out.push_str(&c);
                rest = &rest[e + 1..];
                done = true;
            }
        }
        if !done {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}
fn entity(n: &str) -> Option<String> {
    if let Some(d) = n.strip_prefix('#') {
        let v = if let Some(h) = d.strip_prefix(['x', 'X']) { u32::from_str_radix(h, 16).ok()? } else { d.parse().ok()? };
        // Windows-1252 colado como numérico
        if (0x80..=0x9f).contains(&v) {
            return Some(encoding_rs::WINDOWS_1252.decode(&[v as u8]).0.into_owned());
        }
        return char::from_u32(v).map(|c| c.to_string());
    }
    let c = match n {
        "amp" => '&', "lt" => '<', "gt" => '>', "quot" => '"', "apos" => '\'', "nbsp" => '\u{a0}', "shy" => '\u{ad}',
        "ndash" => '–', "mdash" => '—', "hellip" => '…', "lsquo" => '‘', "rsquo" => '’', "ldquo" => '“', "rdquo" => '”', "sbquo" => '‚', "bdquo" => '„',
        "laquo" => '«', "raquo" => '»', "lsaquo" => '‹', "rsaquo" => '›', "bull" => '•', "middot" => '·', "copy" => '©', "reg" => '®', "trade" => '™', "deg" => '°', "sect" => '§', "para" => '¶', "dagger" => '†', "Dagger" => '‡',
        "iexcl" => '¡', "iquest" => '¿', "ordf" => 'ª', "ordm" => 'º', "euro" => '€', "pound" => '£', "yen" => '¥', "cent" => '¢', "times" => '×', "divide" => '÷', "plusmn" => '±', "frac12" => '½', "frac14" => '¼', "frac34" => '¾', "sup1" => '¹', "sup2" => '²', "sup3" => '³', "micro" => 'µ', "not" => '¬', "brvbar" => '¦', "curren" => '¤', "minus" => '−', "infin" => '∞', "ne" => '≠', "le" => '≤', "ge" => '≥', "larr" => '←', "rarr" => '→', "uarr" => '↑', "darr" => '↓', "harr" => '↔', "hearts" => '♥', "lozenge" => '◊', "loz" => '◊',
        "ensp" => '\u{2002}', "emsp" => '\u{2003}', "thinsp" => '\u{2009}', "zwnj" => '\u{200c}', "zwj" => '\u{200d}', "lrm" => '\u{200e}', "rlm" => '\u{200f}',
        "aacute" => 'á', "eacute" => 'é', "iacute" => 'í', "oacute" => 'ó', "uacute" => 'ú', "Aacute" => 'Á', "Eacute" => 'É', "Iacute" => 'Í', "Oacute" => 'Ó', "Uacute" => 'Ú', "ntilde" => 'ñ', "Ntilde" => 'Ñ', "uuml" => 'ü', "Uuml" => 'Ü', "auml" => 'ä', "ouml" => 'ö', "Auml" => 'Ä', "Ouml" => 'Ö', "euml" => 'ë', "iuml" => 'ï', "yuml" => 'ÿ', "agrave" => 'à', "egrave" => 'è', "igrave" => 'ì', "ograve" => 'ò', "ugrave" => 'ù', "Agrave" => 'À', "Egrave" => 'È', "acirc" => 'â', "ecirc" => 'ê', "icirc" => 'î', "ocirc" => 'ô', "ucirc" => 'û', "atilde" => 'ã', "otilde" => 'õ', "ccedil" => 'ç', "Ccedil" => 'Ç', "aring" => 'å', "Aring" => 'Å', "aelig" => 'æ', "AElig" => 'Æ', "oelig" => 'œ', "OElig" => 'Œ', "oslash" => 'ø', "Oslash" => 'Ø', "szlig" => 'ß', "eth" => 'ð', "thorn" => 'þ', "yacute" => 'ý', "scaron" => 'š', "Scaron" => 'Š',
        "alpha" => 'α', "beta" => 'β', "gamma" => 'γ', "delta" => 'δ', "epsilon" => 'ε', "theta" => 'θ', "lambda" => 'λ', "mu" => 'μ', "pi" => 'π', "sigma" => 'σ', "phi" => 'φ', "omega" => 'ω', "Omega" => 'Ω', "Delta" => 'Δ', "Sigma" => 'Σ',
        _ => return None,
    };
    Some(c.to_string())
}

pub fn tokens(s: &str) -> Vec<Tok> {
    let mut out = vec![];
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' {
            if s[i..].starts_with("<!--") {
                i = s[i..].find("-->").map_or(b.len(), |e| i + e + 3);
                continue;
            }
            if s[i..].starts_with("<![CDATA[") {
                let e = s[i..].find("]]>").map_or(b.len(), |e| i + e);
                out.push(Tok::Text(s[i + 9..e.max(i + 9)].to_string()));
                i = (e + 3).min(b.len());
                continue;
            }
            if i + 1 < b.len() && (b[i + 1] == b'!' || b[i + 1] == b'?') {
                // el DOCTYPE puede traer un bloque interno entre corchetes
                let mut depth = 0;
                let mut j = i;
                while j < b.len() {
                    match b[j] {
                        b'[' => depth += 1,
                        b']' => depth -= 1,
                        b'>' if depth <= 0 => break,
                        _ => {}
                    }
                    j += 1;
                }
                i = (j + 1).min(b.len());
                continue;
            }
            if i + 1 < b.len() && (b[i + 1] == b'/' || b[i + 1].is_ascii_alphabetic()) {
                let closing = b[i + 1] == b'/';
                let mut j = i + 1 + closing as usize;
                let st = j;
                while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' && b[j] != b'/' {
                    j += 1;
                }
                let name = s[st..j].to_ascii_lowercase();
                let mut attrs = vec![];
                let mut selfc = false;
                loop {
                    while j < b.len() && (b[j].is_ascii_whitespace() || b[j] == b'/') {
                        selfc = b[j] == b'/';
                        j += 1;
                    }
                    if j >= b.len() || b[j] == b'>' {
                        j += 1;
                        break;
                    }
                    selfc = false;
                    let ks = j;
                    while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'=' && b[j] != b'>' && b[j] != b'/' {
                        j += 1;
                    }
                    if j == ks {
                        j += 1;
                        continue;
                    }
                    let k = s.get(ks..j).unwrap_or("").to_ascii_lowercase();
                    while j < b.len() && b[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    let mut v = String::new();
                    if j < b.len() && b[j] == b'=' {
                        j += 1;
                        while j < b.len() && b[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
                            let q = b[j];
                            let vs = j + 1;
                            j = vs;
                            while j < b.len() && b[j] != q {
                                j += 1;
                            }
                            v = unescape(&s[vs..j]);
                            j += 1;
                        } else {
                            let vs = j;
                            while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' {
                                j += 1;
                            }
                            v = unescape(&s[vs..j]);
                        }
                    }
                    attrs.push((k, v));
                }
                i = j.min(b.len());
                if closing {
                    out.push(Tok::Close(name));
                } else {
                    if (name == "script" || name == "style") && !selfc {
                        let end = format!("</{name}");
                        let low = s[i..].to_ascii_lowercase();
                        let e = low.find(&end).map_or(b.len(), |e| i + e);
                        out.push(Tok::Open { name: name.clone(), attrs, close: false });
                        out.push(Tok::Text(s[i..e].to_string()));
                        i = e;
                        continue;
                    }
                    out.push(Tok::Open { name, attrs, close: selfc });
                }
                continue;
            }
        }
        let mut j = i + 1;
        while j < b.len() && b[j] != b'<' {
            j += 1;
        }
        out.push(Tok::Text(unescape(&s[i..j])));
        i = j;
    }
    out
}
pub fn attr<'a>(a: &'a [(String, String)], k: &str) -> Option<&'a str> {
    a.iter().find(|x| x.0 == k || x.0.rsplit(':').next() == Some(k)).map(|x| x.1.as_str())
}
/// Nombre sin prefijo de espacio de nombres.
pub fn local(n: &str) -> &str {
    n.rsplit(':').next().unwrap_or(n)
}

#[derive(Clone, Default, Debug)]
struct Decl {
    align: Option<Align>,
    italic: Option<bool>,
    bold: Option<bool>,
    indent: Option<f32>,
    caps: Option<bool>,
    brk: bool,
    under: Option<bool>,
    strike: bool,
    hidden: bool,
    raise: i8,
    mono: bool,
    space: bool,
}
impl Decl {
    fn parse(body: &str) -> Decl {
        let mut d = Decl::default();
        for p in body.split(';') {
            let Some((k, v)) = p.split_once(':') else { continue };
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim().to_ascii_lowercase();
            let v = v.trim_end_matches("!important").trim();
            match k.as_str() {
                "text-align" => d.align = Some(match v {
                    "center" => Align::Center,
                    "right" | "end" => Align::Right,
                    "left" | "start" => Align::Left,
                    _ => Align::Auto,
                }),
                "font-style" => d.italic = Some(v == "italic" || v == "oblique"),
                "font-weight" => d.bold = Some(v == "bold" || v == "bolder" || v.parse::<i32>().is_ok_and(|n| n >= 600)),
                "font-variant" | "font-variant-caps" => d.caps = Some(v.contains("small-caps")),
                "text-indent" => {
                    let n: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
                    if let Ok(n) = n.parse::<f32>() {
                        let em = if v.ends_with("em") { n } else if v.ends_with('%') { n / 3.0 } else if v.ends_with("pt") { n / 12.0 } else { n / 16.0 };
                        d.indent = Some(em.clamp(-2.0, 4.0));
                    }
                }
                "page-break-before" | "break-before" => d.brk = matches!(v, "always" | "left" | "right" | "page"),
                "text-decoration" | "text-decoration-line" => {
                    d.under = Some(v.contains("underline"));
                    d.strike = v.contains("line-through");
                }
                "display" => d.hidden = v == "none",
                "vertical-align" => d.raise = if v == "super" { 1 } else if v == "sub" { -1 } else { 0 },
                "font-family" => d.mono = v.contains("mono") || v.contains("courier"),
                "margin-top" => {
                    let n: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                    d.space = n.parse::<f32>().is_ok_and(|n| (v.ends_with("em") && n >= 0.9) || (v.ends_with("px") && n >= 14.0));
                }
                _ => {}
            }
        }
        d
    }
    fn over(&mut self, o: &Decl) {
        if o.align.is_some() { self.align = o.align; }
        if o.italic.is_some() { self.italic = o.italic; }
        if o.bold.is_some() { self.bold = o.bold; }
        if o.indent.is_some() { self.indent = o.indent; }
        if o.caps.is_some() { self.caps = o.caps; }
        if o.under.is_some() { self.under = o.under; }
        self.brk |= o.brk;
        self.strike |= o.strike;
        self.hidden |= o.hidden;
        self.mono |= o.mono;
        self.space |= o.space;
        if o.raise != 0 { self.raise = o.raise; }
    }
}
/// Solo lo que importa para leer: alineación, estilo, sangría y saltos.
#[derive(Default, Clone)]
pub struct Css {
    rules: HashMap<String, Decl>,
    /// Selectores con antecesores (`.intro p`): solo valen dentro de ellos.
    deep: Vec<(Vec<String>, String, Decl)>,
}
/// ¿Casa `div.intro`, `.intro` o `div` con esta etiqueta y clases?
fn compound(sel: &str, tag: &str, class: &str) -> bool {
    let mut parts = sel.split('.');
    let t = parts.next().unwrap_or("");
    if !t.is_empty() && t != tag {
        return false;
    }
    parts.all(|c| class.split_whitespace().any(|k| k.eq_ignore_ascii_case(c)))
}
impl Css {
    pub fn add(&mut self, src: &str) {
        let mut s = String::with_capacity(src.len());
        let mut rest = src;
        while let Some(i) = rest.find("/*") {
            s.push_str(&rest[..i]);
            rest = rest[i..].find("*/").map_or("", |e| &rest[i + e + 2..]);
        }
        s.push_str(rest);
        let mut i = 0;
        while let Some(o) = s[i..].find('{') {
            let sel = &s[i..i + o];
            let start = i + o + 1;
            if sel.trim_start().starts_with("@media") {
                i = start;
                continue;
            }
            let Some(c) = s[start..].find('}') else { break };
            if !sel.trim_start().starts_with('@') {
                let d = Decl::parse(&s[start..start + c]);
                for one in sel.split(',') {
                    let one = one.trim().to_ascii_lowercase();
                    // sin pseudoclases ni atributos; `>` se toma como descendiente
                    let chain: Vec<String> = one.split([' ', '>']).map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
                    let Some(last) = chain.last().cloned() else { continue };
                    if chain.iter().any(|s| s.contains([':', '[', '#', '*', '+', '~'])) {
                        continue;
                    }
                    if chain.len() == 1 {
                        self.rules.entry(last).or_default().over(&d);
                    } else {
                        self.deep.push((chain[..chain.len() - 1].to_vec(), last, d.clone()));
                    }
                }
            }
            i = start + c + 1;
        }
    }
    fn get(&self, tag: &str, class: &str, style: &str, anc: &[(String, String)]) -> Decl {
        let mut d = Decl::default();
        if !self.rules.is_empty() {
            if let Some(r) = self.rules.get(tag) {
                d.over(r);
            }
            for c in class.split_whitespace() {
                let c = c.to_ascii_lowercase();
                if let Some(r) = self.rules.get(&format!(".{c}")) {
                    d.over(r);
                }
                if let Some(r) = self.rules.get(&format!("{tag}.{c}")) {
                    d.over(r);
                }
            }
        }
        for (up, last, r) in &self.deep {
            if !compound(last, tag, class) {
                continue;
            }
            // cada antecesor del selector, de dentro afuera, en algún nivel por encima
            let mut k = anc.len();
            let ok = up.iter().rev().all(|s| {
                while k > 0 {
                    k -= 1;
                    if compound(s, &anc[k].0, &anc[k].1) {
                        return true;
                    }
                }
                false
            });
            if ok {
                d.over(r);
            }
        }
        if !style.is_empty() {
            d.over(&Decl::parse(style));
        }
        d
    }
}

pub fn join(base: &str, rel: &str) -> String {
    let rel = percent(rel);
    if rel.contains("://") || rel.starts_with("mailto:") || rel.starts_with("data:") {
        return rel;
    }
    let (path, frag) = match rel.split_once('#') {
        Some((p, f)) => (p.to_string(), Some(f.to_string())),
        None => (rel, None),
    };
    let mut out: Vec<&str> = vec![];
    let full;
    if path.is_empty() {
        full = base.to_string();
    } else if let Some(p) = path.strip_prefix('/') {
        full = p.to_string();
    } else {
        full = match base.rfind('/') {
            Some(i) => format!("{}/{}", &base[..i], path),
            None => path,
        };
    }
    for seg in full.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    let p = out.join("/");
    match frag {
        Some(f) => format!("{p}#{f}"),
        None => p,
    }
}
pub fn percent(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 1) {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Frame {
    name: String,
    class: String,
    style: u16,
    align: Align,
    indent: Option<f32>,
    link: Option<Arc<str>>,
    hidden: bool,
    quote: u8,
    pre: bool,
    list: Option<(bool, u32)>,
}

pub struct Out {
    pub blocks: Vec<Block>,
    pub title: String,
}

/// Convierte un capítulo. `base` es la ruta del archivo dentro del libro.
pub fn parse(src: &str, css: &Css, base: &str) -> Out {
    let toks = tokens(src);
    let mut css = css.clone();
    let mut out: Vec<Block> = vec![];
    let mut stack: Vec<Frame> = vec![Frame { name: String::new(), class: String::new(), style: 0, align: Align::Auto, indent: None, link: None, hidden: false, quote: 0, pre: false, list: None }];
    let mut cur: Option<Block> = None;
    let mut ids: Vec<String> = vec![];
    let mut title = String::new();
    let mut in_title = false;
    let mut kind_next: Option<BlockKind> = None;
    let mut brk = false;
    let mut space = false;
    let mut row: Option<(Vec<String>, bool)> = None;
    let mut pending_space = false;

    fn flush(cur: &mut Option<Block>, out: &mut Vec<Block>, ids: &mut Vec<String>, pending: &mut bool) {
        *pending = false;
        if let Some(mut b) = cur.take() {
            let keep = b.text.trim_end_matches([' ', '\n']).len();
            if b.kind != BlockKind::Pre {
                b.text.truncate(keep);
            } else {
                let k = b.text.trim_end_matches('\n').len();
                b.text.truncate(k);
            }
            let l = b.text.len();
            for s in &mut b.spans {
                s.end = s.end.min(l);
            }
            b.spans.retain(|s| s.start < s.end);
            if !b.text.is_empty() {
                out.push(b);
            } else {
                // el ancla pasa al bloque siguiente
                ids.extend(b.ids);
            }
        }
    }

    for t in toks {
        match t {
            Tok::Open { name, attrs, close } => {
                let n = local(&name).to_string();
                if n == "title" {
                    in_title = !close;
                    continue;
                }
                let top = stack.last().unwrap();
                let class = attr(&attrs, "class").unwrap_or("").to_string();
                let anc: Vec<(String, String)> = stack.iter().map(|f| (f.name.clone(), f.class.clone())).collect();
                let d = css.get(&n, &class, attr(&attrs, "style").unwrap_or(""), &anc);
                let mut f = Frame { name: n.clone(), class, style: top.style, align: top.align, indent: top.indent, link: top.link.clone(), hidden: top.hidden || d.hidden, quote: top.quote, pre: top.pre, list: None };
                if matches!(n.as_str(), "head" | "script" | "noscript" | "template") {
                    f.hidden = true;
                }
                if n == "style" {
                    f.hidden = true;
                }
                if let Some(i) = attr(&attrs, "id").or(if n == "a" { attr(&attrs, "name") } else { None }) {
                    match &mut cur {
                        Some(c) if !is_block(&n) => c.ids.push(i.to_string()),
                        _ => ids.push(i.to_string()),
                    }
                }
                match n.as_str() {
                    "b" | "strong" => f.style |= BOLD,
                    "i" | "em" | "cite" | "dfn" | "var" | "address" => f.style |= ITALIC,
                    "code" | "kbd" | "samp" | "tt" => f.style |= MONO,
                    "sup" => f.style |= SUP,
                    "sub" => f.style |= SUB,
                    "u" | "ins" => f.style |= UNDER,
                    "s" | "strike" | "del" => f.style |= STRIKE,
                    "small" => f.style |= SMALL,
                    "big" => f.style &= !SMALL,
                    "center" => f.align = Align::Center,
                    "a" => {
                        if let Some(h) = attr(&attrs, "href") {
                            f.link = Some(join(base, h).into());
                        }
                    }
                    _ => {}
                }
                if let Some(i) = d.italic { if i { f.style |= ITALIC } else { f.style &= !ITALIC } }
                if let Some(b) = d.bold { if b { f.style |= BOLD } else if !n.starts_with('h') { f.style &= !BOLD } }
                if let Some(c) = d.caps { if c { f.style |= SMALLCAPS } else { f.style &= !SMALLCAPS } }
                if d.under == Some(true) && n != "a" { f.style |= UNDER; }
                if d.strike { f.style |= STRIKE; }
                if d.mono && !is_block(&n) { f.style |= MONO; }
                if d.raise > 0 && !is_block(&n) { f.style |= SUP; }
                if d.raise < 0 && !is_block(&n) { f.style |= SUB; }
                if let Some(a) = d.align { f.align = a; }
                if let Some(a) = attr(&attrs, "align") {
                    f.align = match a { "center" => Align::Center, "right" => Align::Right, "left" => Align::Left, _ => f.align };
                }
                if d.indent.is_some() { f.indent = d.indent; }
                if is_block(&n) {
                    if row.is_none() {
                        flush(&mut cur, &mut out, &mut ids, &mut pending_space);
                    } else {
                        pending_space = true;
                    }
                    brk |= d.brk;
                    space |= d.space;
                    match n.as_str() {
                        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                            kind_next = Some(BlockKind::Heading(n.as_bytes()[1] - b'0'));
                            f.style &= !(BOLD);
                        }
                        "blockquote" => f.quote += 1,
                        "ul" | "menu" => f.list = Some((false, 0)),
                        "ol" => f.list = Some((true, attr(&attrs, "start").and_then(|s| s.parse::<u32>().ok()).unwrap_or(1).saturating_sub(1))),
                        "li" => {
                            let depth = stack.iter().filter(|s| s.list.is_some()).count().max(1) as u8;
                            let mut m = "•".to_string();
                            if let Some(l) = stack.iter_mut().rev().find_map(|s| s.list.as_mut()) {
                                l.1 += 1;
                                if l.0 {
                                    m = format!("{}.", l.1);
                                }
                            }
                            kind_next = Some(BlockKind::Item(m, depth));
                        }
                        "dd" => f.quote += 1,
                        "pre" => {
                            f.pre = true;
                            f.style |= MONO;
                        }
                        "hr" => {
                            let mut b = Block::new(BlockKind::Rule);
                            b.ids = std::mem::take(&mut ids);
                            out.push(b);
                        }
                        "tr" => row = Some((vec![], false)),
                        "pagebreak" => brk = true,
                        _ => {}
                    }
                }
                match n.as_str() {
                    "td" | "th" => {
                        if let Some((cells, head)) = &mut row {
                            cells.push(String::new());
                            if n == "th" {
                                *head = true;
                            }
                        }
                    }
                    "br" => {
                        if let Some((cells, _)) = &mut row {
                            if let Some(c) = cells.last_mut() {
                                c.push(' ');
                            }
                        } else if let Some(c) = &mut cur {
                            c.text.push('\n');
                        }
                        pending_space = false;
                    }
                    "img" | "image" => {
                        let src = attr(&attrs, "src").or(attr(&attrs, "href"));
                        if let (Some(s), false) = (src, f.hidden) {
                            let s = join(base, s);
                            // imágenes sueltas en medio del texto: bloque propio
                            flush(&mut cur, &mut out, &mut ids, &mut pending_space);
                            let mut b = Block::new(BlockKind::Image(s));
                            b.ids = std::mem::take(&mut ids);
                            b.break_before = std::mem::take(&mut brk);
                            out.push(b);
                        }
                    }
                    _ => {}
                }
                let void = matches!(n.as_str(), "br" | "img" | "hr" | "meta" | "link" | "input" | "image" | "col" | "wbr" | "base" | "source");
                if !close && !void {
                    // un <p> o <li> sin cerrar termina al abrir el siguiente
                    if (n == "p" || n == "li") && stack.last().is_some_and(|s| s.name == n) {
                        stack.pop();
                    }
                    stack.push(f);
                }
            }
            Tok::Close(name) => {
                let n = local(&name).to_string();
                if n == "title" {
                    in_title = false;
                    continue;
                }
                if is_block(&n) {
                    match n.as_str() {
                        "tr" => {
                            if let Some((cells, head)) = row.take() {
                                let t: Vec<String> = cells.iter().map(|c| c.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
                                if t.iter().any(|c| !c.is_empty()) {
                                    let mut b = Block::new(BlockKind::Row(head));
                                    b.text = t.join("\t");
                                    b.ids = std::mem::take(&mut ids);
                                    out.push(b);
                                }
                            }
                        }
                        _ if row.is_none() => flush(&mut cur, &mut out, &mut ids, &mut pending_space),
                        _ => {}
                    }
                    if n.starts_with('h') || n == "li" {
                        kind_next = None;
                    }
                }
                if let Some(p) = stack.iter().rposition(|s| s.name == n) {
                    if p > 0 {
                        stack.truncate(p);
                    }
                }
            }
            Tok::Text(s) => {
                if in_title {
                    title.push_str(&s);
                    continue;
                }
                let f = stack.last().unwrap();
                if f.name == "style" {
                    css.add(&s);
                    continue;
                }
                if f.hidden {
                    continue;
                }
                if let Some((cells, _)) = &mut row {
                    if let Some(c) = cells.last_mut() {
                        if pending_space {
                            c.push(' ');
                            pending_space = false;
                        }
                        c.push_str(&s);
                        continue;
                    }
                }
                let ws = |c: char| matches!(c, ' ' | '\n' | '\t' | '\r');
                if cur.is_none() && !f.pre && s.chars().all(ws) {
                    continue;
                }
                if cur.is_none() {
                    let kind = kind_next.take().unwrap_or(if f.pre { BlockKind::Pre } else if f.quote > 0 { BlockKind::Quote } else { BlockKind::Para });
                    let mut b = Block::new(kind);
                    b.align = f.align;
                    b.indent = f.indent;
                    b.quote = f.quote;
                    b.ids = std::mem::take(&mut ids);
                    b.break_before = std::mem::take(&mut brk);
                    b.space_before = std::mem::take(&mut space);
                    cur = Some(b);
                    pending_space = false;
                }
                let b = cur.as_mut().unwrap();
                let mut start = b.text.len();
                if f.pre {
                    b.text.push_str(&s.replace("\r\n", "\n").replace('\t', "    "));
                } else {
                    for c in s.chars() {
                        if ws(c) {
                            pending_space = true;
                        } else {
                            if pending_space {
                                if !b.text.is_empty() && !b.text.ends_with('\n') && !b.text.ends_with(' ') {
                                    b.text.push(' ');
                                }
                                pending_space = false;
                            }
                            if c != '\u{feff}' {
                                b.text.push(c);
                            }
                        }
                    }
                }
                let end = b.text.len();
                while !f.pre && start < end && b.text.as_bytes()[start] == b' ' {
                    start += 1;
                }
                if end > start && (f.style != 0 || f.link.is_some()) {
                    match b.spans.last_mut() {
                        Some(l) if l.end == start && l.style == f.style && l.link == f.link => l.end = end,
                        _ => b.spans.push(Span { start, end, style: f.style, link: f.link.clone() }),
                    }
                }
            }
        }
    }
    flush(&mut cur, &mut out, &mut ids, &mut pending_space);
    if !ids.is_empty() {
        if let Some(l) = out.last_mut() {
            l.ids.extend(ids);
        }
    }
    Out { blocks: out, title: title.split_whitespace().collect::<Vec<_>>().join(" ") }
}

fn is_block(n: &str) -> bool {
    matches!(n, "p" | "div" | "section" | "article" | "header" | "footer" | "aside" | "nav" | "main" | "figure" | "figcaption" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "li" | "dl" | "dt" | "dd" | "blockquote" | "pre" | "table" | "tr" | "td" | "th" | "tbody" | "thead" | "hr" | "center" | "address" | "body" | "caption" | "svg" | "menu" | "form" | "fieldset" | "details" | "summary" | "mbp:pagebreak" | "pagebreak")
}

/// Texto plano de un fragmento HTML (descripciones).
pub fn plain(s: &str) -> String {
    if !s.contains('<') && !s.contains('&') {
        return s.trim().to_string();
    }
    let o = parse(s, &Css::default(), "");
    o.blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n\n")
}

// ------------------------------------------------------------ árbol XML mínimo
#[derive(Debug, Default, Clone)]
pub struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub kids: Vec<Node>,
    pub text: String,
}
impl Node {
    pub fn find(&self, n: &str) -> Option<&Node> {
        self.kids.iter().find(|k| local(&k.name) == n)
    }
    pub fn all<'a>(&'a self, n: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.kids.iter().filter(move |k| local(&k.name) == n)
    }
    pub fn deep<'a>(&'a self, n: &str, out: &mut Vec<&'a Node>) {
        for k in &self.kids {
            if local(&k.name) == n {
                out.push(k);
            }
            k.deep(n, out);
        }
    }
    pub fn first(&self, n: &str) -> Option<&Node> {
        for k in &self.kids {
            if local(&k.name) == n {
                return Some(k);
            }
            if let Some(f) = k.first(n) {
                return Some(f);
            }
        }
        None
    }
    pub fn attr(&self, k: &str) -> Option<&str> {
        attr(&self.attrs, k)
    }
    pub fn full(&self) -> String {
        let mut s = String::new();
        self.gather(&mut s);
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }
    fn gather(&self, s: &mut String) {
        if self.name == "#" {
            s.push_str(&self.text);
        }
        for k in &self.kids {
            k.gather(s);
        }
    }
}
pub fn xml(src: &str) -> Node {
    let mut stack = vec![Node::default()];
    for t in tokens(src) {
        match t {
            Tok::Open { name, attrs, close } => {
                let n = Node { name, attrs, ..Default::default() };
                if close {
                    stack.last_mut().unwrap().kids.push(n);
                } else {
                    stack.push(n);
                }
            }
            Tok::Close(name) => {
                if let Some(p) = stack.iter().rposition(|s| s.name == name) {
                    while stack.len() > p.max(1) {
                        let n = stack.pop().unwrap();
                        stack.last_mut().unwrap().kids.push(n);
                    }
                }
            }
            Tok::Text(s) => stack.last_mut().unwrap().kids.push(Node { name: "#".into(), text: s, ..Default::default() }),
        }
    }
    while stack.len() > 1 {
        let n = stack.pop().unwrap();
        stack.last_mut().unwrap().kids.push(n);
    }
    stack.pop().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocks_and_spans() {
        let o = parse("<html><head><title>T</title></head><body><h1 id='a'>Uno</h1><p>Hola <em>mundo</em>,\n bien.</p><ul><li>x<li>y</ul><p>a<br/>b</p></body>", &Css::default(), "x/y.html");
        assert_eq!(o.title, "T");
        assert_eq!(o.blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), ["Uno", "Hola mundo, bien.", "x", "y", "a\nb"]);
        assert_eq!(&o.blocks[1].text[o.blocks[1].spans[0].start..o.blocks[1].spans[0].end], "mundo");
        assert_eq!(o.blocks[0].ids, ["a"]);
    }
    #[test]
    fn descendants() {
        let mut css = Css::default();
        css.add("p { text-align:justify } .intro p { text-align:right } div > em.x { font-weight:bold }");
        let o = parse("<body><div class='intro'><p>a</p></div><p>b</p></body>", &css, "");
        assert!(matches!(o.blocks[0].align, Align::Right));
        assert!(matches!(o.blocks[1].align, Align::Auto));
    }
    #[test]
    fn paths() {
        assert_eq!(join("OEBPS/text/a.xhtml", "../img/b%20c.png"), "OEBPS/img/b c.png");
        assert_eq!(join("a/b.html", "#n"), "a/b.html#n");
    }
    #[test]
    fn bad_input() {
        for s in ["<", "<p", "<p a='", "&#xZZ;", "</>", "<a b=c", "&amp", "<!DOCTYPE [", "<![CDATA["] {
            parse(s, &Css::default(), "");
        }
    }
}
