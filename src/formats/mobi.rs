//! MOBI, PRC, AZW y AZW3 (KF8) sin DRM.
use super::html::{self, Css};
use super::{isbn_of, year_of, Cover, Meta};
use crate::doc::*;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

struct Pdb {
    data: Vec<u8>,
    off: Vec<usize>,
}
fn u16be(b: &[u8], o: usize) -> usize {
    b.get(o..o + 2).map_or(0, |v| (v[0] as usize) << 8 | v[1] as usize)
}
fn u32be(b: &[u8], o: usize) -> usize {
    b.get(o..o + 4).map_or(0, |v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]) as usize)
}
impl Pdb {
    fn open(p: &Path) -> Result<Pdb, String> {
        let data = std::fs::read(p).map_err(|_| "No se puede abrir el archivo")?;
        let n = u16be(&data, 76);
        let off: Vec<usize> = (0..n).map(|i| u32be(&data, 78 + i * 8)).collect();
        if n == 0 || off.windows(2).any(|w| w[1] < w[0]) || off[0] >= data.len() {
            return Err("El archivo está dañado".into());
        }
        Ok(Pdb { data, off })
    }
    fn rec(&self, i: usize) -> &[u8] {
        if i >= self.off.len() {
            return &[];
        }
        let e = self.off.get(i + 1).copied().unwrap_or(self.data.len()).min(self.data.len());
        self.data.get(self.off[i]..e).unwrap_or(&[])
    }
}
struct Head {
    base: usize,
    r0: Vec<u8>,
    compression: usize,
    text_recs: usize,
    encryption: usize,
    utf8: bool,
    version: usize,
    first_img: usize,
    extra: usize,
    exth: Vec<(usize, Vec<u8>)>,
    title: String,
}
impl Head {
    fn read(p: &Pdb, base: usize) -> Head {
        let r0 = p.rec(base).to_vec();
        let hl = u32be(&r0, 20);
        let mobi = r0.get(16..20) == Some(b"MOBI");
        let mut h = Head { base, compression: u16be(&r0, 0), text_recs: u16be(&r0, 8), encryption: u16be(&r0, 12), utf8: u32be(&r0, 28) == 65001, version: u32be(&r0, 36), first_img: u32be(&r0, 108), extra: 0, exth: vec![], title: String::new(), r0: vec![] };
        if mobi {
            if hl >= 0xE4 {
                h.extra = u16be(&r0, 242);
            }
            let (to, tl) = (u32be(&r0, 84), u32be(&r0, 88));
            if let Some(t) = r0.get(to..to + tl) {
                h.title = h.dec(t);
            }
            if u32be(&r0, 128) & 0x40 != 0 {
                let s = 16 + hl;
                if r0.get(s..s + 4) == Some(b"EXTH") {
                    let n = u32be(&r0, s + 8);
                    let mut o = s + 12;
                    for _ in 0..n.min(2000) {
                        let (t, l) = (u32be(&r0, o), u32be(&r0, o + 4));
                        if l < 8 {
                            break;
                        }
                        if let Some(v) = r0.get(o + 8..o + l) {
                            h.exth.push((t, v.to_vec()));
                        }
                        o += l;
                    }
                }
            }
        } else {
            h.utf8 = false;
            h.version = 0;
        }
        h.r0 = r0;
        h
    }
    fn dec(&self, b: &[u8]) -> String {
        if self.utf8 { String::from_utf8_lossy(b).into_owned() } else { encoding_rs::WINDOWS_1252.decode(b).0.into_owned() }
    }
    fn ex(&self, t: usize) -> Option<&[u8]> {
        self.exth.iter().find(|e| e.0 == t).map(|e| e.1.as_slice())
    }
    fn exs(&self, t: usize) -> Vec<String> {
        self.exth.iter().filter(|e| e.0 == t).map(|e| self.dec(&e.1)).collect()
    }
}
fn heads(p: &Pdb) -> (Head, Option<Head>) {
    let h = Head::read(p, 0);
    if h.version >= 8 {
        return (h, None);
    }
    let k = h.ex(121).map(|v| u32be(v, 0)).filter(|b| *b > 0 && *b < p.off.len()).map(|b| Head::read(p, b)).filter(|k| k.version >= 8);
    (h, k)
}

fn trim(rec: &[u8], flags: usize) -> &[u8] {
    let mut n = rec.len();
    let mut f = flags >> 1;
    while f > 0 {
        if f & 1 == 1 {
            let (mut v, mut sh, mut i) = (0usize, 0, n);
            while i > 0 && sh < 28 {
                i -= 1;
                let b = rec[i] as usize;
                v |= (b & 0x7f) << sh;
                sh += 7;
                if b & 0x80 != 0 {
                    break;
                }
            }
            n = n.saturating_sub(v);
        }
        f >>= 1;
    }
    if flags & 1 == 1 && n > 0 {
        n = n.saturating_sub((rec[n - 1] & 3) as usize + 1);
    }
    &rec[..n]
}
fn palmdoc(d: &[u8], out: &mut Vec<u8>) {
    let start = out.len();
    let mut i = 0;
    while i < d.len() {
        let c = d[i];
        i += 1;
        match c {
            1..=8 => {
                let e = (i + c as usize).min(d.len());
                out.extend_from_slice(&d[i..e]);
                i = e;
            }
            0 | 9..=0x7f => out.push(c),
            0xc0..=0xff => {
                out.push(b' ');
                out.push(c ^ 0x80);
            }
            _ => {
                if i >= d.len() {
                    break;
                }
                let v = (c as usize) << 8 | d[i] as usize;
                i += 1;
                let dist = (v >> 3) & 0x7ff;
                let len = (v & 7) + 3;
                if dist == 0 || dist > out.len() - start {
                    continue;
                }
                for _ in 0..len {
                    let b = out[out.len() - dist];
                    out.push(b);
                }
            }
        }
    }
}
struct Huff {
    dict1: Vec<(usize, bool, u64)>,
    minc: [u64; 33],
    maxc: [u64; 33],
    dict: Vec<(Vec<u8>, bool)>,
}
impl Huff {
    fn new(p: &Pdb, at: usize, n: usize) -> Option<Huff> {
        let h = p.rec(at);
        if h.get(0..4) != Some(b"HUFF") {
            return None;
        }
        let (o1, o2) = (u32be(h, 8), u32be(h, 12));
        let dict1 = (0..256)
            .map(|i| {
                let v = u32be(h, o1 + i * 4) as u64;
                let len = (v & 0x1f) as usize;
                let m = v >> 8;
                let maxc = if len == 0 { 0 } else { ((m + 1) << (32 - len)).wrapping_sub(1) };
                (len, v & 0x80 != 0, maxc)
            })
            .collect();
        let (mut minc, mut maxc) = ([0u64; 33], [0xffff_ffffu64; 33]);
        for l in 1..=32usize {
            minc[l] = (u32be(h, o2 + (l - 1) * 8) as u64) << (32 - l);
            maxc[l] = ((u32be(h, o2 + (l - 1) * 8 + 4) as u64 + 1) << (32 - l)).wrapping_sub(1);
        }
        let mut dict = vec![];
        for r in 1..n {
            let c = p.rec(at + r);
            if c.get(0..4) != Some(b"CDIC") {
                return None;
            }
            let (phrases, bits) = (u32be(c, 8), u32be(c, 12));
            let cnt = (1usize << bits.min(24)).min(phrases.saturating_sub(dict.len()));
            for i in 0..cnt {
                let o = 16 + u16be(c, 16 + i * 2);
                let l = u16be(c, o);
                dict.push((c.get(o + 2..o + 2 + (l & 0x7fff)).unwrap_or(&[]).to_vec(), l & 0x8000 != 0));
            }
        }
        Some(Huff { dict1, minc, maxc, dict })
    }
    fn unpack(&mut self, d: &[u8], out: &mut Vec<u8>, depth: u32) {
        let mut bits = d.len() as i64 * 8;
        let mut buf = d.to_vec();
        buf.extend([0; 8]);
        let mut pos = 0;
        let rd = |b: &[u8], p: usize| -> u64 { b.get(p..p + 8).map_or(0, |v| u64::from_be_bytes(v.try_into().unwrap())) };
        let mut x = rd(&buf, 0);
        let mut n = 32i32;
        loop {
            if n <= 0 {
                pos += 4;
                x = rd(&buf, pos);
                n += 32;
            }
            let code = (x >> n) & 0xffff_ffff;
            let (mut len, term, mut maxc) = self.dict1[(code >> 24) as usize];
            if !term {
                while len < 33 && len > 0 && code < self.minc[len] {
                    len += 1;
                }
                if len >= 33 {
                    break;
                }
                maxc = self.maxc[len];
            }
            if len == 0 {
                break;
            }
            n -= len as i32;
            bits -= len as i64;
            if bits < 0 {
                break;
            }
            let r = (maxc.wrapping_sub(code) >> (32 - len)) as usize;
            let Some((s, done)) = self.dict.get(r).cloned() else { break };
            if done || depth > 24 {
                out.extend_from_slice(&s);
            } else {
                // frase comprimida a su vez: se resuelve una vez y se guarda
                self.dict[r].1 = true;
                let mut o = vec![];
                self.unpack(&s, &mut o, depth + 1);
                out.extend_from_slice(&o);
                self.dict[r] = (o, true);
            }
        }
    }
}
fn raw_text(p: &Pdb, h: &Head) -> Result<Vec<u8>, String> {
    if h.encryption != 0 {
        return Err("Este libro está protegido (DRM) y no se puede abrir".into());
    }
    let mut out = vec![];
    let mut huff = if h.compression == 17480 { Some(Huff::new(p, h.base + u32be(&h.r0, 112), u32be(&h.r0, 116)).ok_or("Compresión no reconocida")?) } else { None };
    for i in 1..=h.text_recs {
        let r = trim(p.rec(h.base + i), h.extra);
        match h.compression {
            1 => out.extend_from_slice(r),
            2 => palmdoc(r, &mut out),
            17480 => huff.as_mut().unwrap().unpack(r, &mut out, 0),
            _ => return Err("Compresión no reconocida".into()),
        }
    }
    Ok(out)
}

fn varint(b: &[u8], o: &mut usize) -> usize {
    let mut v = 0;
    for _ in 0..4 {
        let Some(&c) = b.get(*o) else { break };
        *o += 1;
        v = v << 7 | (c & 0x7f) as usize;
        if c & 0x80 != 0 {
            break;
        }
    }
    v
}
type Tags = HashMap<usize, Vec<usize>>;
/// Índice INDX: (etiqueta, valores) y las cadenas CNCX.
fn indx(p: &Pdb, at: usize) -> (Vec<(Vec<u8>, Tags)>, Vec<Vec<u8>>) {
    let mut out = vec![];
    let h = p.rec(at);
    if h.get(0..4) != Some(b"INDX") {
        return (out, vec![]);
    }
    let hl = u32be(h, 4);
    let n = u32be(h, 24);
    let ncncx = u32be(h, 52);
    let mut tagx = vec![];
    let mut cbytes = 0;
    if h.get(hl..hl + 4) == Some(b"TAGX") {
        let l = u32be(h, hl + 4);
        cbytes = u32be(h, hl + 8);
        let mut o = hl + 12;
        while o + 4 <= hl + l && o + 4 <= h.len() {
            tagx.push((h[o] as usize, h[o + 1] as usize, h[o + 2], h[o + 3]));
            o += 4;
        }
    }
    for r in 1..=n {
        let d = p.rec(at + r);
        if d.get(0..4) != Some(b"INDX") {
            continue;
        }
        let ix = u32be(d, 20);
        let cnt = u32be(d, 24);
        for e in 0..cnt {
            let s = u16be(d, ix + 4 + e * 2);
            let Some(&ll) = d.get(s) else { continue };
            let label = d.get(s + 1..s + 1 + ll as usize).unwrap_or(&[]).to_vec();
            let cs = s + 1 + ll as usize;
            let mut o = cs + cbytes;
            let mut ci = 0;
            let mut plan = vec![];
            for &(tag, vals, mask, end) in &tagx {
                if end == 1 {
                    ci += 1;
                    continue;
                }
                let v = d.get(cs + ci).copied().unwrap_or(0) & mask;
                if v == 0 {
                    continue;
                }
                if v == mask {
                    if mask.count_ones() > 1 {
                        let b = varint(d, &mut o);
                        plan.push((tag, vals, None, Some(b)));
                    } else {
                        plan.push((tag, vals, Some(1), None));
                    }
                } else {
                    plan.push((tag, vals, Some((v >> mask.trailing_zeros()) as usize), None));
                }
            }
            let mut tags = Tags::new();
            for (tag, vals, count, bytes) in plan {
                let mut v = vec![];
                if let Some(c) = count {
                    for _ in 0..c * vals {
                        v.push(varint(d, &mut o));
                    }
                } else if let Some(b) = bytes {
                    let end = o + b;
                    while o < end && o < d.len() {
                        v.push(varint(d, &mut o));
                    }
                }
                tags.insert(tag, v);
            }
            out.push((label, tags));
        }
    }
    let cn = (0..ncncx).map(|i| p.rec(at + n + 1 + i).to_vec()).collect();
    (out, cn)
}
fn cncx(c: &[Vec<u8>], off: usize) -> Vec<u8> {
    let Some(r) = c.get(off >> 16) else { return vec![] };
    let mut o = off & 0xffff;
    let l = varint(r, &mut o);
    r.get(o..o + l).unwrap_or(&[]).to_vec()
}
fn b32(s: &[u8]) -> usize {
    s.iter().fold(0usize, |v, c| {
        v.wrapping_mul(32) + match c {
            b'0'..=b'9' => (c - b'0') as usize,
            b'A'..=b'V' => (c - b'A') as usize + 10,
            b'a'..=b'v' => (c - b'a') as usize + 10,
            _ => 0,
        }
    })
}
/// Anclas en posiciones de byte, sin partir etiquetas.
fn anchors(raw: &[u8], base: usize, pos: &BTreeSet<usize>) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + pos.len() * 24);
    let mut last = 0;
    for &p in pos.range(base..base + raw.len().max(1)) {
        let mut q = p - base;
        // dentro de una etiqueta: se retrocede a su apertura
        let mut k = q;
        while k > 0 && q - k < 2000 {
            k -= 1;
            if raw[k] == b'>' {
                break;
            }
            if raw[k] == b'<' {
                q = k;
                break;
            }
        }
        while q < raw.len() && q > 0 && raw[q] & 0xC0 == 0x80 {
            q -= 1;
        }
        let q = q.max(last);
        out.extend_from_slice(&raw[last..q]);
        out.extend_from_slice(format!("<a id=\"fp{p}\"></a>").as_bytes());
        last = q;
    }
    out.extend_from_slice(&raw[last..]);
    out
}
fn find_all(h: &[u8], n: &[u8]) -> Vec<usize> {
    let mut v = vec![];
    let mut i = 0;
    while i + n.len() <= h.len() {
        match h[i..].windows(n.len()).position(|w| w == n) {
            Some(p) => {
                v.push(i + p);
                i += p + n.len();
            }
            None => break,
        }
    }
    v
}

struct Recs {
    pdb: Pdb,
    first: usize,
}
impl Assets for Recs {
    fn load(&self, n: &str) -> Option<Vec<u8>> {
        let i = if let Some(r) = n.strip_prefix("rec:") {
            r.trim_start_matches('0').parse::<usize>().ok()?
        } else {
            let r = n.strip_prefix("kindle:embed:")?;
            b32(r.split('?').next()?.as_bytes())
        };
        let d = self.pdb.rec(self.first + i.checked_sub(1)?);
        (!d.is_empty()).then(|| d.to_vec())
    }
}

pub fn open(p: &Path) -> Result<Doc, String> {
    let pdb = Pdb::open(p)?;
    let (h6, h8) = heads(&pdb);
    let first = h6.first_img;
    let (h, kf8) = match h8 {
        Some(k) => (k, true),
        None => {
            let v = h6.version >= 8;
            (h6, v)
        }
    };
    let raw = raw_text(&pdb, &h)?;
    let lang = h.ex(524).map(|b| h.dec(b)).unwrap_or_default();
    let (ch, toc) = if kf8 { kf8_chapters(&pdb, &h, raw) } else { mobi6_chapters(&pdb, &h, raw) };
    Ok(Doc::build(ch, toc, Arc::new(Recs { pdb, first }), lang))
}

fn ncx(p: &Pdb, h: &Head) -> Vec<(String, u8, usize, Option<(usize, usize)>)> {
    let at = u32be(&h.r0, 244);
    if at == 0xffff_ffff || at == 0 {
        return vec![];
    }
    let (e, c) = indx(p, h.base + at);
    e.iter()
        .map(|(_, t)| {
            let g = |k: usize| t.get(&k).and_then(|v| v.first()).copied();
            let label = g(3).map(|o| h.dec(&cncx(&c, o))).unwrap_or_default();
            let fid = t.get(&6).filter(|v| v.len() >= 2).map(|v| (v[0], v[1]));
            (html::unescape(&label), g(4).unwrap_or(0).min(8) as u8, g(1).unwrap_or(0), fid)
        })
        .collect()
}

fn mobi6_chapters(p: &Pdb, h: &Head, raw: Vec<u8>) -> (Vec<Chapter>, Vec<(String, u8, String)>) {
    let mut pos = BTreeSet::new();
    for i in find_all(&raw, b"filepos=") {
        let d: Vec<u8> = raw[i + 8..].iter().copied().skip_while(|c| *c == b'"' || *c == b'\'').take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = String::from_utf8_lossy(&d).parse::<usize>() {
            pos.insert(n);
        }
    }
    let nc = ncx(p, h);
    for e in &nc {
        pos.insert(e.2);
    }
    let body = anchors(&raw, 0, &pos);
    let mut s = h.dec(&body);
    // filepos="000123" → href="#fp123"; recindex → src
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(i) = rest.find("filepos=") {
        out.push_str(&rest[..i]);
        rest = rest[i + 8..].trim_start_matches(['"', '\'']);
        let n = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
        out.push_str(&format!("href=\"#fp{}\"", rest[..n].parse::<usize>().unwrap_or(0)));
        rest = rest[n..].trim_start_matches(['"', '\'']);
    }
    out.push_str(rest);
    s = out.replace("recindex=\"", "src=\"rec:").replace("recindex='", "src='rec:");
    let o = html::parse(&s, &Css::default(), "");
    let mut ch: Vec<Chapter> = vec![];
    for b in o.blocks {
        if ch.is_empty() || b.break_before {
            ch.push(Chapter::default());
        }
        ch.last_mut().unwrap().blocks.push(b);
    }
    if ch.len() == 1 {
        let b = std::mem::take(&mut ch[0].blocks);
        ch = super::office::split(b);
        for c in &mut ch {
            c.href = String::new();
        }
    }
    let toc = nc.into_iter().map(|e| (e.0, e.1, format!("#fp{}", e.2))).collect();
    (ch, toc)
}

fn kf8_chapters(p: &Pdb, h: &Head, raw: Vec<u8>) -> (Vec<Chapter>, Vec<(String, u8, String)>) {
    // flujos: 0 = texto, el resto CSS y SVG
    let mut flows = vec![];
    let fd = u32be(&h.r0, 192);
    let f = p.rec(h.base + fd);
    if f.get(0..4) == Some(b"FDST") {
        let n = u32be(f, 8);
        for i in 0..n.min(500) {
            flows.push((u32be(f, 12 + i * 8), u32be(f, 16 + i * 8)));
        }
    }
    let text: &[u8] = flows.first().and_then(|f| raw.get(f.0..f.1.min(raw.len()))).unwrap_or(&raw);
    let mut css = Css::default();
    for fl in flows.iter().skip(1) {
        if let Some(b) = raw.get(fl.0..fl.1.min(raw.len())) {
            let s = h.dec(b);
            if !s.trim_start().starts_with('<') {
                css.add(&s);
            }
        }
    }
    let (fi, si) = (u32be(&h.r0, 248), u32be(&h.r0, 252));
    let frags: Vec<(usize, usize)> = if fi != 0xffff_ffff {
        indx(p, h.base + fi).0.iter().map(|(l, t)| (String::from_utf8_lossy(l).parse().unwrap_or(0), t.get(&6).and_then(|v| v.get(1)).copied().unwrap_or(0))).collect()
    } else {
        vec![]
    };
    let skels: Vec<(usize, usize, usize)> = if si != 0xffff_ffff {
        indx(p, h.base + si).0.iter().map(|(_, t)| (t.get(&1).and_then(|v| v.first()).copied().unwrap_or(0), t.get(&6).and_then(|v| v.first()).copied().unwrap_or(0), t.get(&6).and_then(|v| v.get(1)).copied().unwrap_or(0))).collect()
    } else {
        vec![]
    };
    let mut parts: Vec<(usize, Vec<u8>)> = vec![];
    let mut fp = 0;
    for (n, sp, sl) in skels {
        let mut bp = sp + sl;
        let Some(sk) = text.get(sp..bp) else { break };
        let mut sk = sk.to_vec();
        for _ in 0..n {
            let Some(&(ins, len)) = frags.get(fp) else { break };
            fp += 1;
            let Some(sl) = text.get(bp..bp + len) else { break };
            let at = ins.saturating_sub(sp).min(sk.len());
            sk.splice(at..at, sl.iter().copied());
            bp += len;
        }
        parts.push((sp, sk));
    }
    if parts.is_empty() {
        parts.push((0, text.to_vec()));
    }
    let posof = |fid: usize, off: usize| frags.get(fid).map(|f| f.0 + off);
    let mut pos = BTreeSet::new();
    for (_, b) in &parts {
        for i in find_all(b, b"kindle:pos:fid:") {
            if let Some(r) = b.get(i + 15..i + 34) {
                if let Some(q) = posof(b32(&r[..4]), b32(&r[9..19])) {
                    pos.insert(q);
                }
            }
        }
    }
    let nc = ncx(p, h);
    let mut toc = vec![];
    for e in nc {
        let q = e.3.and_then(|f| posof(f.0, f.1)).unwrap_or(e.2);
        pos.insert(q);
        toc.push((e.0, e.1, format!("#fp{q}")));
    }
    let mut ch = vec![];
    for (i, (base, b)) in parts.iter().enumerate() {
        let body = anchors(b, *base, &pos);
        let s = h.dec(&body);
        let mut out = String::with_capacity(s.len());
        let mut rest = s.as_str();
        while let Some(k) = rest.find("kindle:pos:fid:") {
            out.push_str(&rest[..k]);
            let r = &rest[k + 15..];
            let rb = r.as_bytes();
            if rb.len() >= 19 && rb[..19].is_ascii() {
                let q = posof(b32(&rb[..4]), b32(&rb[9..19])).unwrap_or(0);
                out.push_str(&format!("#fp{q}"));
                rest = &r[19..];
            } else {
                rest = r;
            }
        }
        out.push_str(rest);
        let o = html::parse(&out, &css, "");
        ch.push(Chapter { href: format!("part{i}"), title: o.title, blocks: o.blocks, ..Default::default() });
    }
    (ch, toc)
}

pub fn probe(p: &Path) -> (Meta, Cover) {
    let Ok(pdb) = Pdb::open(p) else { return (Meta::default(), Cover::None) };
    let (h, _) = heads(&pdb);
    let mut m = Meta { title: h.exs(503).into_iter().next().unwrap_or(h.title.clone()), ..Default::default() };
    m.authors = h.exs(100).iter().flat_map(|a| a.split(['&', ';']).map(|s| s.trim().to_string()).collect::<Vec<_>>()).collect();
    m.publisher = h.exs(101).join(", ");
    m.description = html::plain(&h.exs(103).join("\n"));
    m.isbn = h.exs(104).first().map(|s| isbn_of(s)).unwrap_or_default();
    m.genres = h.exs(105).iter().flat_map(|s| s.split([';', ',']).map(|x| x.trim().to_string()).collect::<Vec<_>>()).filter(|s| !s.is_empty()).take(6).collect();
    m.year = h.exs(106).first().map(|s| year_of(s)).unwrap_or(0);
    m.language = h.exs(524).into_iter().next().unwrap_or_default();
    m.title = html::unescape(&m.title);
    let c = h.ex(201).map(|v| u32be(v, 0)).filter(|v| *v != 0xffff_ffff).map(|o| pdb.rec(h.first_img + o).to_vec()).filter(|d| super::dims(d).is_some());
    (m, c.map_or(Cover::None, Cover::Encoded))
}
