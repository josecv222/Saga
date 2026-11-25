//! Editor de un libro: campos, búsqueda de datos en línea y galería de portadas.
use crate::app::App;
use crate::book3d::{self, fill_uv, Pose};
use crate::covers::{self, Made, Size};
use crate::db::Book;
use crate::formats;
use crate::gfx::*;
use crate::icons::Icon;
use crate::jobs::Apply;
use crate::meta::{self, Cand};
use crate::text::fonts;
use crate::ui::*;
use std::sync::atomic::{AtomicU64, Ordering};
use winit::keyboard::NamedKey;
use winit::window::CursorIcon;

const F: [(&str, &str); 11] = [
    ("Título", "Título del libro"),
    ("Subtítulo", ""),
    ("Autores", "Separados por ;"),
    ("Serie", ""),
    ("Número en la serie", ""),
    ("Editorial", ""),
    ("Año", ""),
    ("Páginas", ""),
    ("Idioma", "es, en…"),
    ("ISBN", ""),
    ("Géneros", "Separados por ;"),
];
static NEXT: AtomicU64 = AtomicU64::new(1);

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Info,
    Meta,
    Cover,
}
pub struct Pick {
    thumb: String,
    full: String,
    source: &'static str,
    tex: Option<TexId>,
    size: (u32, u32),
    failed: bool,
    local: Option<Made>,
}
pub struct Editor {
    id: i64,
    token: u64,
    t: f32,
    closing: bool,
    tab: Tab,
    f: Vec<Edit>,
    desc: Edit,
    q_title: Edit,
    q_author: Edit,
    cands: Vec<Cand>,
    cand_tex: Vec<Option<(TexId, u32, u32)>>,
    searching: bool,
    searched: bool,
    picks: Vec<Pick>,
    picking: bool,
    picked_once: bool,
    cover_busy: bool,
    chosen: Option<Made>,
    chosen_tex: Option<(TexId, f32)>,
    chosen_from: Option<usize>,
    reset_cover: bool,
    scroll: Scroll,
    scroll2: Scroll,
    scroll3: Scroll,
    /// Espera antes de buscar mientras se escribe.
    typing: f32,
    applied: Option<usize>,
}

impl Editor {
    pub fn new(app: &mut App, id: i64, search: bool) -> Option<Editor> {
        let b = app.book(id)?.clone();
        let mut e = Editor {
            id,
            token: NEXT.fetch_add(1, Ordering::Relaxed),
            t: 0.0,
            closing: false,
            tab: if search { Tab::Meta } else { Tab::Info },
            f: vec![],
            desc: Edit::new(&b.description),
            q_title: Edit::new(&b.title),
            q_author: Edit::new(b.author_list().first().unwrap_or(&"")),
            cands: vec![],
            cand_tex: vec![],
            searching: false,
            searched: false,
            picks: vec![],
            picking: false,
            picked_once: false,
            cover_busy: false,
            chosen: None,
            chosen_tex: None,
            chosen_from: None,
            reset_cover: false,
            scroll: Scroll::default(),
            scroll2: Scroll::default(),
            scroll3: Scroll::default(),
            typing: -1.0,
            applied: None,
        };
        e.load(&b);
        app.ui.focus = None;
        if search {
            e.search(app);
        }
        Some(e)
    }
    fn load(&mut self, b: &Book) {
        let n = |v: i32| if v == 0 { String::new() } else { v.to_string() };
        self.f = [b.title.clone(), b.subtitle.clone(), b.authors.clone(), b.series.clone(), if b.series_ix > 0.0 { super::home::num(b.series_ix) } else { String::new() }, b.publisher.clone(), n(b.year), n(b.pages), b.language.clone(), b.isbn.clone(), b.genres.clone()].iter().map(|s| Edit::new(s)).collect();
    }
    fn store(&self, b: &mut Book) {
        let g = |i: usize| self.f[i].text.trim().to_string();
        if !g(0).is_empty() {
            b.title = g(0);
        }
        b.subtitle = g(1);
        b.authors = g(2);
        b.series = g(3);
        b.series_ix = g(4).replace(',', ".").parse().unwrap_or(0.0);
        b.publisher = g(5);
        b.year = g(6).parse().unwrap_or(0);
        b.pages = g(7).parse().unwrap_or(0);
        b.language = g(8).to_lowercase();
        b.isbn = g(9);
        b.genres = g(10);
        b.description = self.desc.text.trim().to_string();
    }
    fn search(&mut self, app: &mut App) {
        let (t, a) = (self.q_title.text.trim().to_string(), self.q_author.text.trim().to_string());
        if t.is_empty() {
            return;
        }
        self.searching = true;
        self.searched = true;
        self.typing = -1.0;
        self.token = NEXT.fetch_add(1, Ordering::Relaxed);
        let tok = self.token;
        let isbn = if formats::isbn_of(&t).is_empty() { String::new() } else { formats::isbn_of(&t) };
        app.jobs.net(0, move || {
            let v = if isbn.is_empty() { meta::search(&t, &a, "") } else { meta::search("", "", &isbn) };
            Some(Box::new(move |app: &mut App| {
                // el editor puede estar fuera de su sitio: se reparte en el siguiente fotograma
                app.jobs.send(Box::new(move |app: &mut App| Editor::got(app, tok, v)));
            }) as Apply)
        });
    }
    fn got(app: &mut App, tok: u64, v: Vec<Cand>) {
        let jobs = app.jobs.clone();
        let Some(e) = app.editor.as_mut().filter(|e| e.token == tok) else { return };
        e.searching = false;
        for c in e.cand_tex.drain(..).flatten() {
            app.gpu.drop_tex(c.0);
        }
        e.cand_tex = vec![None; v.len()];
        e.applied = None;
        e.scroll2.jump(0.0);
        for (i, c) in v.iter().enumerate() {
            if let Some((th, _)) = c.cover.clone() {
                jobs.net(1, move || {
                    let img = formats::decode(&meta::fetch(&th)?)?;
                    let (w, h) = img.dimensions();
                    let m = covers::mips(img);
                    Some(Box::new(move |app: &mut App| {
                        if app.editor.as_ref().is_some_and(|e| e.token == tok) {
                            let t = app.gpu.create(w, h, &m);
                            let e = app.editor.as_mut().unwrap();
                            if let Some(s) = e.cand_tex.get_mut(i) {
                                *s = Some((t, w, h));
                            }
                        }
                    }) as Apply)
                });
            }
        }
        e.cands = v;
    }
    /// Reúne portadas de todas partes.
    fn find_covers(&mut self, app: &mut App) {
        for p in self.picks.drain(..) {
            if let Some(t) = p.tex {
                app.gpu.drop_tex(t);
            }
        }
        self.picking = true;
        self.picked_once = true;
        self.chosen_from = None;
        let (t, a, i) = (self.f[0].text.clone(), self.f[2].text.clone(), self.f[9].text.clone());
        let path = app.book(self.id).map(|b| b.path.clone()).unwrap_or_default();
        let id = self.id;
        app.jobs.run(0, move || {
            // la que trae el propio archivo
            let (_, _, c) = formats::probe(std::path::Path::new(&path));
            let m = covers::make(&c)?;
            let img = formats::decode(&m.thumb)?;
            let (w, h) = img.dimensions();
            let lv = covers::mips(img);
            Some(Box::new(move |app: &mut App| {
                if app.editor.as_ref().is_some_and(|e| e.id == id) {
                    let t = app.gpu.create(w, h, &lv);
                    let e = app.editor.as_mut().unwrap();
                    e.picks.insert(0, Pick { thumb: String::new(), full: String::new(), source: "Del archivo", tex: Some(t), size: (w, h), failed: false, local: Some(m) });
                }
            }) as Apply)
        });
        app.jobs.net(0, move || {
            let v = meta::covers(&t, &a, &i);
            Some(Box::new(move |app: &mut App| {
                let jobs = app.jobs.clone();
                let Some(e) = app.editor.as_mut().filter(|e| e.id == id) else { return };
                e.picking = false;
                for (th, full, src) in v {
                    let key = th.clone();
                    e.picks.push(Pick { thumb: th, full, source: src, tex: None, size: (2, 3), failed: false, local: None });
                    jobs.net(1, move || {
                        let img = meta::fetch(&key).and_then(|b| formats::decode(&b)).filter(|i| i.width() > 50 && i.height() > 60);
                        let r = img.map(|i| {
                            let (w, h) = i.dimensions();
                            (w, h, covers::mips(i))
                        });
                        Some(Box::new(move |app: &mut App| {
                            let t = r.as_ref().filter(|_| app.editor.as_ref().is_some_and(|e| e.id == id)).map(|(w, h, m)| (app.gpu.create(*w, *h, m), *w, *h));
                            if let Some(e) = app.editor.as_mut().filter(|e| e.id == id) {
                                if let Some(p) = e.picks.iter_mut().find(|p| p.thumb == key) {
                                    match t {
                                        Some((t, w, h)) => {
                                            p.tex = Some(t);
                                            p.size = (w, h);
                                        }
                                        None => p.failed = true,
                                    }
                                }
                            }
                        }) as Apply)
                    });
                }
            }) as Apply)
        });
    }
    fn choose(&mut self, app: &mut App, m: Made) {
        if let Some((t, _)) = self.chosen_tex.take() {
            app.gpu.drop_tex(t);
        }
        if let Some(img) = formats::decode(&m.jpg) {
            let (w, h) = img.dimensions();
            let t = app.gpu.create(w, h, &covers::mips(img));
            self.chosen_tex = Some((t, w as f32 / h as f32));
        }
        self.chosen = Some(m);
        self.reset_cover = false;
        app.ui.redraw = true;
    }
    fn fetch_cover(&mut self, app: &mut App, th: String, full: String) {
        self.cover_busy = true;
        let id = self.id;
        app.jobs.net(0, move || {
            let m = meta::cover_image(&th, &full).and_then(covers::make_img);
            Some(Box::new(move |app: &mut App| {
                if let Some(mut e) = app.editor.take() {
                    if e.id == id {
                        e.cover_busy = false;
                        match m {
                            Some(m) => e.choose(app, m),
                            None => app.ui.toast_err("No se pudo descargar esa portada"),
                        }
                    }
                    app.editor = Some(e);
                }
            }) as Apply)
        });
    }
    fn pick_image(&mut self, app: &mut App) {
        if app.dialog {
            return;
        }
        app.dialog = true;
        let id = self.id;
        app.jobs.thread(move || {
            let p = rfd::FileDialog::new().set_title("Elegir portada").add_filter("Imágenes", &["jpg", "jpeg", "png", "webp", "gif", "bmp"]).pick_file();
            let m = p.and_then(|p| std::fs::read(p).ok()).and_then(|b| formats::decode(&b)).and_then(covers::make_img);
            Some(Box::new(move |app: &mut App| {
                app.dialog = false;
                if let (Some(m), Some(mut e)) = (m, app.editor.take()) {
                    if e.id == id {
                        e.choose(app, m);
                    }
                    app.editor = Some(e);
                }
            }) as Apply)
        });
    }
    /// Vuelca una ficha encontrada en los campos.
    fn apply(&mut self, app: &mut App, i: usize) {
        let c = self.cands[i].clone();
        let mut put = |k: usize, s: String| {
            if !s.is_empty() && s != "0" {
                self.f[k].set(&s);
            }
        };
        put(0, c.title.clone());
        put(1, c.subtitle.clone());
        put(2, c.authors.join("; "));
        put(5, c.publisher.clone());
        put(6, c.year.to_string());
        put(7, c.pages.to_string());
        put(8, c.language.clone());
        put(9, c.isbn.clone());
        put(10, c.genres.join("; "));
        if !c.description.is_empty() {
            self.desc.set(&c.description);
        } else if !c.work.is_empty() {
            let (w, id) = (c.work.clone(), self.id);
            app.jobs.net(0, move || {
                let t = meta::work_text(&w);
                Some(Box::new(move |app: &mut App| {
                    if let Some(e) = app.editor.as_mut().filter(|e| e.id == id) {
                        if !t.is_empty() {
                            e.desc.set(&t);
                        }
                    }
                }) as Apply)
            });
        }
        if let Some((t, f)) = c.cover.clone() {
            self.fetch_cover(app, t, f);
        }
        self.applied = Some(i);
        app.ui.toast("Datos aplicados. Revísalos y guarda.");
    }
    fn save(&mut self, app: &mut App) {
        let id = self.id;
        app.edit_book(id, |b| {
            self.store(b);
            b.meta_done = true;
        });
        if let Some(m) = self.chosen.take() {
            app.set_cover(id, &m, true);
        } else if self.reset_cover {
            app.edit_book(id, |b| {
                b.cover_ver = 0;
                b.cover_custom = false;
            });
            app.covers.forget(&mut app.gpu, id);
        } else {
            app.covers.forget(&mut app.gpu, id);
        }
        app.ui.toast("Cambios guardados");
        self.closing = true;
    }
    fn free(&mut self, app: &mut App) {
        for p in self.picks.drain(..) {
            if let Some(t) = p.tex {
                app.gpu.drop_tex(t);
            }
        }
        for c in self.cand_tex.drain(..).flatten() {
            app.gpu.drop_tex(c.0);
        }
        if let Some((t, _)) = self.chosen_tex.take() {
            app.gpu.drop_tex(t);
        }
    }
    pub fn frame(&mut self, app: &mut App) -> bool {
        let open = !self.closing;
        let t = app.ui.tween(&mut self.t, open, if open { 0.26 } else { 0.18 });
        if self.closing && t <= 0.0 {
            self.free(app);
            return false;
        }
        let Some(bi) = app.index.get(&self.id).copied() else {
            self.free(app);
            return false;
        };
        let th = app.ui.th;
        let e = ease_out(t);
        let full = app.ui.full();
        let was = app.ui.enabled;
        app.ui.enabled = was && open;
        app.ui.d.push_alpha(e);
        app.ui.d.fill(full, 0.0, Color::BLACK.a(if th.dark { 0.55 } else { 0.35 }));
        let w = (full.w - 80.0).min(1080.0);
        let h = (full.h - 90.0).min(760.0);
        let r = rect(full.cx() - w / 2.0, full.cy() - h / 2.0 + 8.0, w, h);
        app.ui.d.push_xf(0.965 + 0.035 * e, r.cx(), r.cy(), 0.0, (1.0 - e) * 14.0);
        app.ui.card(r, 26.0);
        // columna de la portada
        let side = rect(r.x, r.y, 290.0, r.h);
        app.ui.d.fill4(side, [26.0, 0.0, 0.0, 26.0], th.text.a(0.028));
        let mut l = app.look(bi, Size::Full);
        if let Some((t, a)) = self.chosen_tex {
            l.tex = Some((t, 1.0));
            l.img_aspect = a;
            l.aspect = book3d::book_aspect(a);
            if let Some(m) = &self.chosen {
                l.color = Color::hex(m.color);
            }
        }
        if self.reset_cover {
            l.tex = None;
            l.color = Color::hex(covers::gen_color(&self.f[0].text));
        }
        l.title = self.f[0].text.clone();
        let bh = 290.0;
        let (cx, cy) = (side.cx(), side.y + 62.0 + bh / 2.0);
        let sway = if app.ui.over(side) { (app.ui.input.pos.0 - cx) / 300.0 } else { 0.0 };
        let yaw = app.ui.anim(id("edyaw"), 0.42 + sway * 0.5, 8.0);
        let p = Pose { cx, cy, h: bh, yaw, pitch: 0.0, z: 0.0, open: 0.0 };
        app.ui.d.push_alpha(if th.dark { 0.8 } else { 0.4 });
        book3d::floor_shadow(&mut app.ui, &p, &l, (cx, cy), cy + bh / 2.0 + 4.0, 1.0);
        app.ui.d.pop_alpha();
        book3d::draw(&mut app.ui, &p, &l, (cx, cy));
        let mut by = cy + bh / 2.0 + 40.0;
        if self.cover_busy {
            spinner(app, cx, cy, 34.0);
        }
        let bw = side.w - 64.0;
        if app.ui.button(id("ed-cov"), rect(side.x + 32.0, by, bw, 42.0), Btn::Soft, Some(Icon::Image), "Buscar portadas") {
            self.tab = Tab::Cover;
            if !self.picked_once {
                self.find_covers(app);
            }
        }
        by += 52.0;
        if app.ui.button(id("ed-own"), rect(side.x + 32.0, by, bw, 42.0), Btn::Ghost, Some(Icon::Folder), "Usar una imagen mía") {
            self.pick_image(app);
        }
        by += 46.0;
        if app.ui.button(id("ed-auto"), rect(side.x + 32.0, by, bw, 42.0), Btn::Ghost, Some(Icon::Book), "Portada tipográfica") {
            self.reset_cover = true;
            self.chosen = None;
            if let Some((t, _)) = self.chosen_tex.take() {
                app.gpu.drop_tex(t);
            }
        }
        // pestañas
        let bx = side.r() + 30.0;
        let bw = r.r() - bx - 30.0;
        let tabs = [(Some(Icon::Pencil), "Información", ""), (Some(Icon::Wand), "Buscar datos", ""), (Some(Icon::Image), "Portadas", "")];
        let cur = self.tab as usize;
        if let Some(i) = app.ui.segmented(id("ed-tabs"), rect(bx, r.y + 24.0, 450.0f32.min(bw - 60.0), 42.0), &tabs, cur) {
            self.tab = [Tab::Info, Tab::Meta, Tab::Cover][i];
            app.ui.focus = None;
            if i == 1 && !self.searched {
                self.q_title.set(&self.f[0].text.clone());
                self.search(app);
            }
            if i == 2 && !self.picked_once {
                self.find_covers(app);
            }
        }
        if app.ui.icon_btn(id("ed-x"), rect(r.r() - 62.0, r.y + 24.0, 40.0, 40.0), Icon::Close, "Cerrar sin guardar", false) {
            self.closing = true;
        }
        let body = rect(bx, r.y + 86.0, bw, r.h - 86.0 - 84.0);
        match self.tab {
            Tab::Info => self.info(app, body),
            Tab::Meta => self.meta(app, body),
            Tab::Cover => self.gallery(app, body),
        }
        // pie
        let fy = r.b() - 66.0;
        if app.ui.button(id("ed-save"), rect(r.r() - 30.0 - 150.0, fy, 150.0, 46.0), Btn::Primary, Some(Icon::Check), "Guardar") || (open && app.ui.ctrl("s")) {
            self.save(app);
        }
        if app.ui.button(id("ed-no"), rect(r.r() - 30.0 - 150.0 - 132.0, fy, 120.0, 46.0), Btn::Ghost, None, "Cancelar") {
            self.closing = true;
        }
        app.ui.d.pop_xf();
        app.ui.d.pop_alpha();
        if open && app.ui.key(NamedKey::Escape) {
            if app.ui.focus.is_some() {
                app.ui.focus = None;
            } else {
                self.closing = true;
            }
        }
        if open && app.ui.enabled {
            self.tab_key(app);
        }
        app.ui.enabled = was;
        true
    }
    fn tab_key(&mut self, app: &mut App) {
        if self.tab != Tab::Info || !app.ui.key(NamedKey::Tab) {
            return;
        }
        let back = app.ui.input.mods.shift;
        let order: Vec<u64> = (0..F.len()).map(|i| idx("ef", i)).chain([id("ed-desc")]).collect();
        let p = order.iter().position(|i| app.ui.focus == Some(*i));
        let n = match p {
            Some(p) if back => (p + order.len() - 1) % order.len(),
            Some(p) => (p + 1) % order.len(),
            None => 0,
        };
        app.ui.focus = Some(order[n]);
        app.ui.redraw = true;
    }
    fn info(&mut self, app: &mut App, r: Rect) {
        let th = app.ui.th;
        // filas: (campo, ancho relativo)
        let rows: [&[(usize, f32)]; 6] = [&[(0, 1.0)], &[(1, 1.0)], &[(2, 1.0)], &[(3, 0.68), (4, 0.32)], &[(5, 0.46), (6, 0.18), (7, 0.18), (8, 0.18)], &[(10, 0.62), (9, 0.38)]];
        let content = rows.len() as f32 * 70.0 + 220.0;
        let sy = app.ui.scroll(id("eds"), &mut self.scroll, rect(r.x, r.y, r.w + 14.0, r.h), content);
        app.ui.clip(rect(r.x - 6.0, r.y, r.w + 20.0, r.h), 0.0);
        let mut y = r.y - sy;
        for row in rows {
            let mut x = r.x;
            let tot = r.w - 12.0 * (row.len() - 1) as f32 - 6.0;
            for (i, k) in row {
                let w = tot * k;
                app.ui.label(x + 2.0, y + 8.0, F[*i].0, th.text3);
                let rr = rect(x, y + 20.0, w, 40.0);
                let (ch, _) = app.ui.field(idx("ef", *i), rr, &mut self.f[*i], None, F[*i].1);
                if ch && *i == 0 {
                    // al escribir el título, los datos se buscan solos
                    self.q_title.set(&self.f[0].text.clone());
                    self.typing = 0.9;
                }
                x += w + 12.0;
            }
            y += 70.0;
        }
        app.ui.label(r.x + 2.0, y + 8.0, "Sinopsis", th.text3);
        let rr = rect(r.x, y + 20.0, r.w - 6.0, 180.0);
        let f = app.ui.anim(id("eddf"), (app.ui.focus == Some(id("ed-desc"))) as i32 as f32, 16.0);
        app.ui.d.boxed(rr, 16.0, th.text.a(0.045), 1.0, th.text.a(0.09).mix(th.accent.a(0.75), f));
        app.ui.edit(id("ed-desc"), rect(rr.x + 14.0, rr.y + 10.0, rr.w - 28.0, rr.h - 20.0), &mut self.desc, true, fonts().ui, 14.0, th.text, th.accent);
        app.ui.unclip();
        // sugerencia automática bajo el título
        if self.typing > 0.0 {
            self.typing -= app.ui.dt;
            app.ui.redraw = true;
            if self.typing <= 0.0 && self.f[0].text.trim().chars().count() >= 3 {
                self.q_author.set(self.f[2].text.split(';').next().unwrap_or("").trim());
                self.search(app);
            }
        }
        if self.searched && app.ui.focus == Some(idx("ef", 0)) && (!self.cands.is_empty() || self.searching) && self.applied.is_none() {
            let n = self.cands.len().min(4);
            let bh = if self.searching { 48.0 } else { n as f32 * 58.0 + 12.0 };
            let b = rect(r.x, r.y - sy + 66.0, r.w - 6.0, bh);
            app.ui.d.shadow(b.off(0.0, 10.0), 16.0, 34.0, Color::BLACK.a(0.45));
            app.ui.d.boxed(b, 16.0, th.raised, 1.0, th.line);
            if self.searching {
                spinner(app, b.x + 28.0, b.cy(), 16.0);
                app.ui.text(fonts().ui, 13.5, b.x + 50.0, b.cy(), "Buscando datos…", th.text2);
            } else {
                for i in 0..n {
                    let rr = rect(b.x + 6.0, b.y + 6.0 + i as f32 * 58.0, b.w - 12.0, 56.0);
                    let ov = app.ui.over(rr);
                    let h = app.ui.anim(idx("sug", i), ov as i32 as f32, 18.0);
                    app.ui.d.fill(rr, 12.0, th.text.a(0.07 * h));
                    if ov {
                        app.ui.cursor = CursorIcon::Pointer;
                    }
                    self.cand_thumb(app, i, rect(rr.x + 8.0, rr.y + 5.0, 31.0, 46.0));
                    let c = &self.cands[i];
                    let (a, b2) = (c.title.clone(), cand_line(c));
                    app.ui.text_fit(fonts().ui_m, 14.0, rr.x + 52.0, rr.cy() - 10.0, &a, rr.w - 64.0, th.text);
                    app.ui.text_fit(fonts().ui, 12.5, rr.x + 52.0, rr.cy() + 11.0, &b2, rr.w - 64.0, th.text3);
                    if ov && app.ui.input.pressed && app.ui.enabled {
                        self.apply(app, i);
                        app.ui.focus = None;
                        break;
                    }
                }
            }
        }
    }
    fn cand_thumb(&self, app: &mut App, i: usize, r: Rect) {
        let th = app.ui.th;
        app.ui.d.fill(r, 3.0, th.text.a(0.08));
        if let Some(Some((t, w, h))) = self.cand_tex.get(i) {
            app.ui.d.image(*t, r, 3.0, fill_uv(*w as f32 / *h as f32, r.w / r.h), Color::WHITE);
        } else {
            app.ui.icon(Icon::Book, r.cx(), r.cy(), 16.0, th.text3);
        }
    }
    fn meta(&mut self, app: &mut App, r: Rect) {
        let th = app.ui.th;
        let w1 = (r.w - 24.0 - 130.0) * 0.58;
        let w2 = (r.w - 24.0 - 130.0) * 0.42;
        let (_, e1) = app.ui.field(id("mq1"), rect(r.x, r.y, w1, 42.0), &mut self.q_title, Some(Icon::Search), "Título o ISBN");
        let (_, e2) = app.ui.field(id("mq2"), rect(r.x + w1 + 12.0, r.y, w2, 42.0), &mut self.q_author, Some(Icon::User), "Autor");
        if app.ui.button(id("mgo"), rect(r.x + w1 + w2 + 24.0, r.y, 124.0, 42.0), Btn::Primary, None, "Buscar") || e1 || e2 {
            self.search(app);
        }
        let v = rect(r.x, r.y + 60.0, r.w + 14.0, r.h - 60.0);
        if self.searching {
            spinner(app, r.cx(), v.y + 90.0, 36.0);
            app.ui.text_c(fonts().ui, 14.0, r.cx(), v.y + 140.0, "Buscando en Google Books, Open Library y Apple Books…", th.text3);
            return;
        }
        if self.cands.is_empty() {
            let s = if self.searched { "No se encontró nada. Prueba con menos palabras o sin autor." } else { "Escribe el título y pulsa Buscar." };
            app.ui.text_c(fonts().ui, 14.5, r.cx(), v.y + 90.0, s, th.text3);
            return;
        }
        let rh = 112.0;
        let sy = app.ui.scroll(id("mss"), &mut self.scroll2, v, self.cands.len() as f32 * rh + 10.0);
        app.ui.clip(rect(v.x - 6.0, v.y, v.w + 6.0, v.h), 0.0);
        for i in 0..self.cands.len() {
            let rr = rect(r.x, v.y - sy + i as f32 * rh, r.w - 6.0, rh - 10.0);
            if rr.b() < v.y || rr.y > v.b() {
                continue;
            }
            let ov = app.ui.over(rr);
            let h = app.ui.anim(idx("cand", i), ov as i32 as f32, 16.0);
            let on = self.applied == Some(i);
            app.ui.d.boxed(rr, 16.0, th.text.a(0.03 + 0.04 * h), 1.0, if on { th.accent.a(0.7) } else { th.text.a(0.06) });
            self.cand_thumb(app, i, rect(rr.x + 12.0, rr.y + 10.0, 55.0, 82.0));
            let c = self.cands[i].clone();
            let tx = rr.x + 84.0;
            let tw = rr.w - 84.0 - 124.0;
            app.ui.text_fit(fonts().ui_m, 15.0, tx, rr.y + 22.0, &c.title, tw, th.text);
            app.ui.text_fit(fonts().ui, 13.0, tx, rr.y + 43.0, &cand_line(&c), tw, th.text2);
            let d = if c.description.is_empty() { c.genres.join(" · ") } else { c.description.replace('\n', " ") };
            let ls = app.ui.t.wrap(fonts().ui, 12.5, &d, tw, 2);
            for (k, s) in ls.iter().enumerate() {
                app.ui.text(fonts().ui, 12.5, tx, rr.y + 64.0 + k as f32 * 18.0, s, th.text3);
            }
            if app.ui.button(idx("capp", i), rect(rr.r() - 112.0, rr.cy() - 20.0, 98.0, 40.0), if on { Btn::Soft } else { Btn::Primary }, None, if on { "Aplicado" } else { "Usar" }) {
                self.apply(app, i);
            }
            app.ui.text_r(fonts().ui, 11.0, rr.r() - 14.0, rr.b() - 12.0, c.source, th.text3);
        }
        app.ui.unclip();
    }
    fn gallery(&mut self, app: &mut App, r: Rect) {
        let th = app.ui.th;
        app.ui.text(fonts().ui, 13.5, r.x, r.y + 18.0, "Elige la portada que prefieras. Se aplica al guardar.", th.text3);
        if app.ui.button(id("gref"), rect(r.r() - 140.0, r.y - 2.0, 134.0, 38.0), Btn::Soft, Some(Icon::Refresh), "Buscar otra vez") {
            self.find_covers(app);
        }
        let v = rect(r.x, r.y + 50.0, r.w + 14.0, r.h - 50.0);
        self.picks.retain(|p| !p.failed);
        if self.picks.is_empty() {
            if self.picking {
                spinner(app, r.cx(), v.y + 90.0, 36.0);
                app.ui.text_c(fonts().ui, 14.0, r.cx(), v.y + 140.0, "Buscando portadas…", th.text3);
            } else {
                app.ui.text_c(fonts().ui, 14.5, r.cx(), v.y + 90.0, "No se encontraron portadas. Puedes usar una imagen tuya.", th.text3);
            }
            return;
        }
        let cols = ((r.w + 14.0) / 148.0).floor().max(2.0);
        let cw = (r.w - 6.0) / cols;
        let ih = (cw - 16.0) * 1.5;
        let ch = ih + 40.0;
        let n = self.picks.len();
        let sy = app.ui.scroll(id("gals"), &mut self.scroll3, v, (n as f32 / cols).ceil() * ch + 10.0);
        app.ui.clip(rect(v.x - 8.0, v.y, v.w + 8.0, v.h), 0.0);
        let mut want = None;
        for (i, p) in self.picks.iter().enumerate() {
            let x = r.x + (i % cols as usize) as f32 * cw;
            let y = v.y - sy + (i / cols as usize) as f32 * ch + 6.0;
            if y > v.b() || y + ch < v.y {
                continue;
            }
            let rr = rect(x, y, cw - 16.0, ih);
            let bid = idx("gal", i);
            if app.ui.click(bid, rr) && p.tex.is_some() {
                want = Some(i);
            }
            let ov = app.ui.over(rr);
            if ov {
                app.ui.cursor = CursorIcon::Pointer;
            }
            let h = app.ui.anim(bid, ov as i32 as f32, 14.0);
            let on = self.chosen_from == Some(i);
            app.ui.d.push_xf(1.0 + 0.025 * h, rr.cx(), rr.cy(), 0.0, 0.0);
            app.ui.d.shadow(rr.off(0.0, 6.0), 8.0, 18.0 + 10.0 * h, Color::BLACK.a(0.35));
            app.ui.d.fill(rr, 8.0, th.text.a(0.07));
            match p.tex {
                Some(t) => {
                    let a = app.ui.anim_from(bid ^ 5, 0.0, 1.0, 8.0);
                    app.ui.d.image(t, rr, 8.0, fill_uv(p.size.0 as f32 / p.size.1 as f32, rr.w / rr.h), Color::WHITE.a(a));
                }
                None => spinner(app, rr.cx(), rr.cy(), 20.0),
            }
            if on {
                app.ui.d.stroke(rr.grow(3.0), 11.0, 3.0, th.accent);
            }
            app.ui.d.pop_xf();
            app.ui.text_c(fonts().ui, 11.5, rr.cx(), rr.b() + 18.0, p.source, if on { th.accent } else { th.text3 });
        }
        app.ui.unclip();
        if let Some(i) = want {
            self.chosen_from = Some(i);
            match self.picks[i].local.clone() {
                Some(m) => self.choose(app, m),
                None => {
                    let (t, f) = (self.picks[i].thumb.clone(), self.picks[i].full.clone());
                    self.fetch_cover(app, t, f);
                }
            }
        }
    }
}
fn cand_line(c: &Cand) -> String {
    let mut v = vec![];
    if !c.authors.is_empty() {
        v.push(c.authors.join(", "));
    }
    if c.year > 0 {
        v.push(c.year.to_string());
    }
    if !c.publisher.is_empty() {
        v.push(c.publisher.clone());
    }
    if c.pages > 0 {
        v.push(format!("{} págs.", c.pages));
    }
    if !c.language.is_empty() {
        v.push(meta::lang_name(&c.language));
    }
    v.join("  ·  ")
}
pub fn spinner(app: &mut App, cx: f32, cy: f32, size: f32) {
    let th = app.ui.th;
    let t = app.ui.time * 1.1;
    app.ui.redraw = true;
    for i in 0..10 {
        let a = (i as f32 / 10.0 + t) * std::f32::consts::TAU;
        let r = size * 0.42;
        let k = 1.0 - (i as f32 / 10.0);
        let d = size * 0.085 * (0.5 + 0.5 * (1.0 - k));
        app.ui.d.fill(rect(cx + a.cos() * r - d, cy + a.sin() * r - d, d * 2.0, d * 2.0), d, th.accent.a(0.15 + 0.85 * (1.0 - k)));
    }
}
