//! Búsqueda en el libro: sin distinguir mayúsculas ni acentos.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct Hit {
    pub a: i64,
    pub b: i64,
    pub pre: String,
    pub mid: String,
    pub post: String,
}
#[derive(Default)]
pub struct Find {
    pub q: Edit,
    pub hits: Vec<Hit>,
    pub cur: Option<usize>,
    pub running: bool,
    pub ran: String,
    token: Arc<AtomicU64>,
    pub due: f32,
}
pub fn fold(c: char) -> char {
    let c = c.to_lowercase().next().unwrap_or(c);
    match c {
        'á' | 'à' | 'ä' | 'â' | 'ã' => 'a',
        'é' | 'è' | 'ë' | 'ê' => 'e',
        'í' | 'ì' | 'ï' | 'î' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
        'ú' | 'ù' | 'ü' | 'û' => 'u',
        'ñ' => 'n',
        'ç' => 'c',
        '’' | '‘' => '\'',
        '“' | '”' | '«' | '»' => '"',
        '\u{a0}' | '\n' | '\t' => ' ',
        c => c,
    }
}
/// Texto plegado con la posición original de cada letra.
type Folded = (Vec<char>, Vec<i64>, Vec<char>);
fn find_in(t: &Folded, q: &[char], end: i64, max: usize, out: &mut Vec<Hit>) {
    let (f, pos, raw) = t;
    if q.is_empty() || f.len() < q.len() {
        return;
    }
    let mut i = 0;
    while i + q.len() <= f.len() {
        if f[i] == q[0] && f[i..i + q.len()] == *q {
            let e = i + q.len();
            let s0 = i.saturating_sub(34);
            let e0 = (e + 60).min(f.len());
            let g = |a: usize, b: usize| raw[a..b].iter().collect::<String>();
            out.push(Hit { a: pos[i], b: pos.get(e).copied().unwrap_or(end), pre: format!("{}{}", if s0 > 0 { "…" } else { "" }, g(s0, i)), mid: g(i, e), post: g(e, e0) });
            if out.len() >= max {
                return;
            }
            i = e;
        } else {
            i += 1;
        }
    }
}
const MAX: usize = 1500;

impl Reader {
    pub fn run_find(&mut self, app: &mut App) {
        let q: Vec<char> = self.find.q.text.trim().chars().map(fold).collect();
        let tok = self.find.token.fetch_add(1, Ordering::SeqCst) + 1;
        self.find.ran = self.find.q.text.trim().to_string();
        self.find.hits.clear();
        self.find.cur = None;
        self.side_scroll.jump(0.0);
        if q.len() < 2 {
            self.find.running = false;
            return;
        }
        self.find.running = true;
        let (id, live, j) = (self.id, self.find.token.clone(), app.jobs.clone());
        let send = move |v: Vec<Hit>, done: bool| {
            j.send(Box::new(move |a: &mut App| {
                if let Some(r) = a.reader.as_mut().filter(|r| r.id == id && r.find.token.load(Ordering::SeqCst) == tok) {
                    r.find.hits.extend(v);
                    r.find.running = !done;
                }
            }));
        };
        match &self.content {
            Content::Flow(v) => {
                let doc = v.doc.clone();
                app.jobs.run(0, move || {
                    let mut out = vec![];
                    'a: for c in &doc.chapters {
                        for b in &c.blocks {
                            let mut t: Folded = (vec![], vec![], vec![]);
                            for (i, ch) in b.text.char_indices() {
                                if ch == '\u{ad}' {
                                    continue;
                                }
                                t.0.push(fold(ch));
                                t.1.push((b.start + i) as i64);
                                t.2.push(if ch == '\n' { ' ' } else { ch });
                            }
                            find_in(&t, &q, (b.start + b.text.len()) as i64, MAX, &mut out);
                            if out.len() >= MAX {
                                break 'a;
                            }
                        }
                    }
                    send(out, true);
                    None
                });
            }
            Content::Fixed(v) => {
                let (doc, n) = (v.doc.clone(), v.sizes.len());
                app.jobs.run(1, move || {
                    let mut total = 0;
                    for p in 0..n {
                        if live.load(Ordering::SeqCst) != tok {
                            return None;
                        }
                        let cs = doc.text(p);
                        let mut t: Folded = (vec![], vec![], vec![]);
                        for (i, c) in cs.iter().enumerate() {
                            if i > 0 {
                                let o = &cs[i - 1];
                                let line = (c.y - o.y).abs() > o.h.max(c.h) * 0.6 || c.x + c.w < o.x - o.h;
                                let gap = line || (!o.c.is_whitespace() && !c.c.is_whitespace() && c.x - (o.x + o.w) > o.h * 0.22);
                                if gap && t.2.last().is_some_and(|l| *l != ' ') {
                                    t.0.push(' ');
                                    t.1.push(fixed_loc(p, i));
                                    t.2.push(' ');
                                }
                            }
                            t.0.push(fold(c.c));
                            t.1.push(fixed_loc(p, i));
                            t.2.push(if c.c.is_control() { ' ' } else { c.c });
                        }
                        let mut out = vec![];
                        find_in(&t, &q, fixed_loc(p, cs.len()), MAX - total, &mut out);
                        total += out.len();
                        if !out.is_empty() {
                            send(out, false);
                        }
                        if total >= MAX {
                            break;
                        }
                    }
                    send(vec![], true);
                    None
                });
            }
        }
    }
    pub fn go_hit(&mut self, app: &mut App, i: usize) {
        let Some((a, b)) = self.find.hits.get(i).map(|h| (h.a, h.b)) else { return };
        self.find.cur = Some(i);
        if !self.on_screen(a) {
            self.jump(a);
        }
        self.flash = Some((a, b, app.ui.time + 6.0));
        app.ui.redraw = true;
    }
    pub fn on_screen(&self, l: i64) -> bool {
        self.placed.iter().any(|p| match p {
            Placed::Flow { flow, range, .. } => l >= flow.lines[range.start].start as i64 && l < flow.lines[range.end - 1].end as i64,
            Placed::Fixed { page, .. } => loc_page(l) == *page,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accents() {
        let s = "El camión ÁRBOL y el árbol";
        let t: Folded = (s.chars().map(fold).collect(), (0..s.chars().count() as i64).collect(), s.chars().collect());
        let q: Vec<char> = "arbol".chars().collect();
        let mut out = vec![];
        find_in(&t, &q, 99, 10, &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].mid, "ÁRBOL");
        assert_eq!(out[1].b, 99);
    }
}
