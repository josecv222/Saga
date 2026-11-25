//! Biblioteca: vitrina, cuadrícula y estante, con búsqueda, filtros y grupos.
use super::panels::Panel;
use super::sheet::Sheet;
use crate::app::{App, STRIP};
use crate::book3d::{self, Pose};
use crate::covers::Size;
use crate::db::*;
use crate::gfx::*;
use crate::icons::Icon;
use crate::meta::{lang_name, norm};
use crate::text::fonts;
use crate::ui::*;
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::os::windows::process::CommandExt;
use winit::keyboard::NamedKey;
use winit::window::CursorIcon;

pub const TABS: [&str; 5] = ["Todo", "Leyendo", "Por leer", "Terminados", "Favoritos"];
pub const SORTS: [&str; 7] = ["Recientes", "Título", "Autor", "Año", "Añadidos", "Progreso", "Calificación"];
pub const GROUPS: [&str; 8] = ["Sin agrupar", "Autor", "Género", "Serie", "Formato", "Estado", "Idioma", "Década"];
const DRAWER: f32 = 272.0;

#[derive(Clone, PartialEq, Debug)]
pub enum Facet {
    Author(String),
    Genre(String),
    Series(String),
    Format(String),
    Lang(String),
    Coll(i64),
}

pub struct Group {
    pub name: String,
    pub ids: Vec<usize>,
}
pub struct Home {
    pub tab: usize,
    pub search: Edit,
    pub dirty: bool,
    /// Índices en `books`, en orden.
    pub list: Vec<usize>,
    pub groups: Vec<Group>,
    pub focus: Option<i64>,
    pos: Spring,
    target: f32,
    pub arrived: Option<i64>,
    pub facet: Vec<Facet>,
    drawer: bool,
    drawer_t: f32,
    drawer_scroll: Scroll,
    open_sec: usize,
    grid: Scroll,
    shelf: Scroll,
    pub poses: HashMap<i64, (Pose, (f32, f32))>,
    back: Option<i64>,
    back_prev: Option<i64>,
    back_t: f32,
    title_id: i64,
    title_t: f32,
    title_old: i64,
    view_t: f32,
    view_seen: usize,
    drag: Option<(f32, f32, bool)>,
    menu_book: i64,
    new_coll: Option<Edit>,
    key_nav: bool,
    was: Vec<i64>,
    intro: f32,
}
impl Home {
    pub fn new() -> Home {
        Home { tab: 0, search: Edit::default(), dirty: true, list: vec![], groups: vec![], focus: None, pos: Spring::default(), target: 0.0, arrived: None, facet: vec![], drawer: false, drawer_t: 0.0, drawer_scroll: Scroll::default(), open_sec: 0, grid: Scroll::default(), shelf: Scroll::default(), poses: HashMap::new(), back: None, back_prev: None, back_t: 1.0, title_id: -1, title_t: 1.0, title_old: -1, view_t: 1.0, view_seen: 0, drag: None, menu_book: 0, new_coll: None, key_nav: false, was: vec![], intro: 0.0 }
    }
}

fn sort_key_author(b: &Book) -> String {
    // por apellido
    let a = b.author_list().first().map(|s| s.to_string()).unwrap_or_default();
    let last = a.rsplit(' ').next().unwrap_or("").to_string();
    norm(&format!("{last} {a}"))
}
fn title_key(t: &str) -> String {
    let n = norm(t);
    for p in ["el ", "la ", "los ", "las ", "un ", "una ", "the ", "a ", "an "] {
        if let Some(r) = n.strip_prefix(p) {
            return r.to_string();
        }
    }
    n
}
fn has(app: &App, b: &Book, f: &Facet) -> bool {
    match f {
        Facet::Author(a) => b.author_list().contains(&a.as_str()),
        Facet::Genre(g) => b.genre_list().contains(&g.as_str()),
        Facet::Series(s) => &b.series == s,
        Facet::Format(x) => &b.format == x,
        Facet::Lang(l) => &b.language == l,
        Facet::Coll(c) => app.colls.iter().any(|k| k.id == *c && k.books.contains(&b.id)),
    }
}
fn in_tab(b: &Book, tab: usize) -> bool {
    match tab {
        1 => b.status == Status::Reading || b.status == Status::Paused,
        2 => b.status == Status::ToRead,
        3 => b.status == Status::Done,
        4 => b.favorite,
        _ => true,
    }
}
fn rebuild(app: &mut App) {
    app.home.dirty = false;
    let q = norm(&app.home.search.text);
    let words: Vec<&str> = q.split(' ').filter(|w| !w.is_empty()).collect();
    let mut v: Vec<usize> = (0..app.books.len())
        .filter(|i| {
            let b = &app.books[*i];
            if !in_tab(b, app.home.tab) || !app.home.facet.iter().all(|f| has(app, b, f)) {
                return false;
            }
            if words.is_empty() {
                return true;
            }
            let hay = norm(&format!("{} {} {} {} {} {}", b.title, b.subtitle, b.authors, b.series, b.genres, b.year));
            words.iter().all(|w| hay.contains(w))
        })
        .collect();
    let bk = &app.books;
    match app.set.sort {
        1 => v.sort_by_cached_key(|i| title_key(&bk[*i].title)),
        2 => v.sort_by_cached_key(|i| (sort_key_author(&bk[*i]), norm(&bk[*i].series), (bk[*i].series_ix * 10.0) as i32, bk[*i].year)),
        3 => v.sort_by_key(|i| (-bk[*i].year, bk[*i].id)),
        4 => v.sort_by_key(|i| (-bk[*i].added, -bk[*i].id)),
        5 => v.sort_by(|a, b| bk[*b].progress.total_cmp(&bk[*a].progress)),
        6 => v.sort_by_key(|i| (-bk[*i].rating, -bk[*i].opened)),
        _ => v.sort_by_key(|i| (-bk[*i].opened.max(bk[*i].added), -bk[*i].id)),
    }
    // grupos
    let mut groups: Vec<Group> = vec![];
    if app.set.group > 0 {
        let mut map: HashMap<String, usize> = HashMap::new();
        for &i in &v {
            let b = &bk[i];
            let names: Vec<String> = match app.set.group {
                1 => b.author_list().first().map(|s| vec![s.to_string()]).unwrap_or_default(),
                2 => b.genre_list().iter().take(1).map(|s| s.to_string()).collect(),
                3 => if b.series.is_empty() { vec![] } else { vec![b.series.clone()] },
                4 => vec![b.format.to_uppercase()],
                5 => vec![b.status.name().to_string()],
                6 => if b.language.is_empty() { vec![] } else { vec![lang_name(&b.language)] },
                _ => if b.year > 0 { vec![format!("{}", b.year / 10 * 10)] } else { vec![] },
            };
            let name = names.into_iter().next().unwrap_or_else(|| match app.set.group {
                1 => "Autor desconocido".into(),
                2 => "Sin género".into(),
                3 => "Fuera de serie".into(),
                6 => "Sin idioma".into(),
                _ => "Sin fecha".to_string(),
            });
            let gi = *map.entry(name.clone()).or_insert_with(|| {
                groups.push(Group { name, ids: vec![] });
                groups.len() - 1
            });
            groups[gi].ids.push(i);
        }
        let last = |n: &str| n.starts_with("Sin ") || n.starts_with("Fuera") || n.starts_with("Autor desc");
        match app.set.group {
            1 => groups.sort_by_cached_key(|g| (last(&g.name), norm(g.name.rsplit(' ').next().unwrap_or("")))),
            7 => groups.sort_by_cached_key(|g| (last(&g.name), std::cmp::Reverse(g.name.clone()))),
            5 => groups.sort_by_key(|g| Status::ALL.iter().position(|s| s.1 == g.name)),
            _ => groups.sort_by_cached_key(|g| (last(&g.name), norm(&g.name))),
        }
        if app.set.group == 3 {
            for g in &mut groups {
                g.ids.sort_by(|a, b| bk[*a].series_ix.total_cmp(&bk[*b].series_ix));
            }
        }
        v = groups.iter().flat_map(|g| g.ids.iter().copied()).collect();
    } else {
        groups.push(Group { name: String::new(), ids: v.clone() });
    }
    app.home.list = v;
    app.home.groups = groups;
    let h = &mut app.home;
    if let Some(id) = h.arrived.take() {
        if h.list.iter().any(|i| app.books[*i].id == id) {
            h.focus = Some(id);
        }
    }
    let cur = h.focus.and_then(|id| h.list.iter().position(|i| app.books[*i].id == id));
    let ids: Vec<i64> = h.list.iter().map(|i| app.books[*i].id).collect();
    match cur {
        Some(p) => {
            // el enfocado no salta aunque cambie el orden a su alrededor
            let before = h.was.iter().position(|x| Some(*x) == h.focus);
            if before.is_some_and(|b| b != p) {
                h.pos = Spring::at(p as f32);
            }
            h.target = p as f32;
        }
        None => {
            h.focus = ids.first().copied();
            h.pos = Spring::at(0.0);
            h.target = 0.0;
        }
    }
    h.was = ids;
    app.ui.redraw = true;
}

pub fn frame(app: &mut App) {
    if app.home.dirty {
        rebuild(app);
    }
    app.home.poses.clear();
    let calm = app.set.calm;
    let dt = app.ui.dt;
    if app.home.intro < 1.0 {
        app.home.intro = (app.home.intro + dt / if calm { 0.2 } else { 0.7 }).min(1.0);
        app.ui.redraw = true;
    }
    if app.set.view != app.home.view_seen {
        app.home.view_seen = app.set.view;
        app.home.view_t = 0.0;
        focus_to_scroll(app);
    }
    let mut vt = app.home.view_t;
    app.ui.tween(&mut vt, true, 0.28);
    app.home.view_t = vt;
    backdrop(app);
    let full = app.ui.full();
    let open = app.home.drawer;
    let mut t = app.home.drawer_t;
    let dr = ease_out(app.ui.tween(&mut t, open, 0.26));
    app.home.drawer_t = t;
    let dx = dr * DRAWER;
    let area = rect(dx, STRIP + 62.0, full.w - dx, full.h - STRIP - 62.0);

    if app.books.is_empty() {
        empty(app, area);
    } else if app.home.list.is_empty() {
        nothing(app, area);
    } else {
        let e = ease_out(app.home.view_t);
        app.ui.d.push_alpha(e);
        match app.set.view {
            0 => carousel(app, area),
            1 => grid(app, area, e),
            _ => shelf(app, area, e),
        }
        app.ui.d.pop_alpha();
    }
    if dr > 0.0 {
        drawer(app, dr);
    }
    topbar(app);
    book_menu(app);
    keys(app);
}

fn focus_to_scroll(app: &mut App) {
    // al cambiar de vista, el libro enfocado sigue a la vista
    app.home.key_nav = true;
}

fn keys(app: &mut App) {
    if !app.ui.enabled {
        return;
    }
    if app.ui.ctrl("f") || app.ui.chr("/") {
        app.ui.focus = Some(id("search"));
        app.ui.input.text.clear();
    }
    if app.ui.ctrl("o") {
        app.pick_files();
    }
    if app.ui.focus == Some(id("search")) {
        if app.ui.key(NamedKey::Escape) {
            app.ui.focus = None;
            if !app.home.search.text.is_empty() {
                app.home.search.set("");
                app.home.dirty = true;
            }
        }
        if app.ui.key(NamedKey::Enter) {
            app.ui.focus = None;
        }
        return;
    }
    if app.ui.focus.is_some() || app.home.list.is_empty() {
        return;
    }
    let n = app.home.list.len();
    let mut p = app.home.target.round() as i32;
    let step = if app.set.view == 1 { grid_cols(app) as i32 } else { 8 };
    let mut moved = false;
    for (k, d) in [(NamedKey::ArrowLeft, -1), (NamedKey::ArrowRight, 1), (NamedKey::ArrowUp, -step), (NamedKey::ArrowDown, step), (NamedKey::PageUp, -10), (NamedKey::PageDown, 10), (NamedKey::Home, -100000), (NamedKey::End, 100000)] {
        if app.ui.key(k) {
            p += d;
            moved = true;
        }
    }
    if moved {
        focus_ix(app, p.clamp(0, n as i32 - 1) as usize);
        app.home.key_nav = true;
    }
    if let Some(f) = app.home.focus {
        if app.ui.key(NamedKey::Enter) {
            app.open_book(f);
        } else if app.ui.key(NamedKey::Space) || app.ui.chr("i") {
            open_sheet(app, f);
        }
    }
}
fn focus_ix(app: &mut App, i: usize) {
    app.home.target = i as f32;
    app.home.focus = app.home.list.get(i).map(|k| app.books[*k].id);
    app.ui.redraw = true;
}
pub fn open_sheet(app: &mut App, id: i64) {
    if app.home.focus != Some(id) {
        if let Some(p) = app.home.list.iter().position(|i| app.books[*i].id == id) {
            focus_ix(app, p);
        }
    }
    app.sheet = Some(Sheet::new(app, id));
    app.ui.focus = None;
    app.ui.redraw = true;
}

// ---------------------------------------------------------------- fondo
fn backdrop(app: &mut App) {
    let th = app.ui.th;
    let full = app.ui.full();
    app.ui.d.fill(full, 0.0, th.bg);
    let f = app.home.focus;
    if f != app.home.back {
        // solo se cambia cuando la nueva ya está lista
        let ok = match f.and_then(|id| app.index.get(&id).copied()) {
            Some(i) => {
                let time = app.ui.time;
                app.covers.get(&app.jobs, &app.books[i], Size::Back, time).is_some()
            }
            None => true,
        };
        if ok && app.home.back_t > 0.55 {
            app.home.back_prev = app.home.back;
            app.home.back = f;
            app.home.back_t = 0.0;
        } else if ok {
            app.ui.redraw = true;
        }
    }
    let mut t = app.home.back_t;
    app.ui.tween(&mut t, true, 0.55);
    app.home.back_t = t;
    let strength = th.glow;
    let a = full.w / full.h;
    let ta = 144.0 / 90.0;
    let uv = if a > ta { let k = ta / a; [0.0, (1.0 - k) / 2.0, 1.0, (1.0 + k) / 2.0] } else { let k = a / ta; [(1.0 - k) / 2.0, 0.0, (1.0 + k) / 2.0, 1.0] };
    for (id, al) in [(app.home.back_prev, 1.0), (app.home.back, smooth(t))] {
        if let Some(i) = id.and_then(|id| app.index.get(&id).copied()) {
            let time = app.ui.time;
            if let Some((tex, _)) = app.covers.get(&app.jobs, &app.books[i], Size::Back, time) {
                if al < 1.0 && t >= 1.0 {
                    continue;
                }
                if !(t >= 1.0 && id == app.home.back_prev && id != app.home.back) {
                    app.ui.d.backdrop(tex, full, uv, th.bg, 0.85, strength, al);
                }
            }
        }
    }
    // la sala: luz arriba, penumbra abajo
    if th.dark {
        app.ui.d.radial(rect(full.w * 0.5 - full.w * 0.45, -full.h * 0.55, full.w * 0.9, full.h * 1.25), Color::hex(0xFFE9C4).a(0.055), Color::hex(0xFFE9C4).a(0.0));
        app.ui.d.grad(rect(0.0, full.h * 0.55, full.w, full.h * 0.45), 0.0, Color::BLACK.a(0.0), Color::BLACK.a(0.30), false);
    } else {
        app.ui.d.radial(rect(full.w * 0.5 - full.w * 0.45, -full.h * 0.55, full.w * 0.9, full.h * 1.25), Color::WHITE.a(0.35), Color::WHITE.a(0.0));
    }
}

// ---------------------------------------------------------------- barra
fn topbar(app: &mut App) {
    let th = app.ui.th;
    let w = app.ui.w;
    let y = STRIP + 4.0;
    let cy = y + 22.0;
    app.ui.text(fonts().title_i, 22.0, 28.0, STRIP / 2.0 + 1.0, "Saga", th.text.a(0.85));
    // búsqueda
    let sw = if w > 1250.0 { 250.0 } else { 190.0 };
    let x0 = 24.0;
    let r = rect(x0, cy - 20.0, sw, 40.0);
    let (ch, _) = app.ui.field(id("search"), r, &mut app.home.search, Some(Icon::Search), "Buscar título, autor…");
    if ch {
        app.home.dirty = true;
        app.home.grid.to(0.0);
        app.home.shelf.to(0.0);
    }
    // botones
    let mut x = w - 24.0;
    let mut place = |bw: f32| {
        x -= bw;
        let r = rect(x, cy - 20.0, bw, 40.0);
        x -= 8.0;
        r
    };
    let rg = place(40.0);
    if app.ui.icon_btn(id("gear"), rg, Icon::Gear, "Ajustes", false) {
        app.panel = Some(Panel::settings());
    }
    let rs = place(40.0);
    if app.ui.icon_btn(id("sort"), rs, Icon::Sort, "Ordenar y agrupar", false) {
        let mut items = vec![MenuItem::head("Ordenar por")];
        for (i, s) in SORTS.iter().enumerate() {
            items.push(MenuItem::check(*s, app.set.sort == i));
        }
        items.push(MenuItem::head("Agrupar"));
        for (i, s) in GROUPS.iter().enumerate() {
            items.push(MenuItem::check(*s, app.set.group == i));
        }
        app.ui.open_menu(id("sortm"), rs, items);
    }
    if let Some(i) = app.ui.picked(id("sortm")) {
        let n = SORTS.len();
        if i >= 1 && i <= n {
            app.set.sort = i - 1;
        } else if i >= n + 2 {
            app.set.group = i - n - 2;
        }
        app.home.dirty = true;
    }
    let rf = place(40.0);
    let on = app.home.drawer || !app.home.facet.is_empty();
    if app.ui.icon_btn(id("filter"), rf, Icon::Filter, "Autores, géneros y colecciones", on) {
        app.home.drawer = !app.home.drawer;
    }
    let rv = place(132.0);
    if let Some(v) = app.ui.segmented(id("view"), rv, &[(Some(Icon::Carousel), "", "Vitrina"), (Some(Icon::Grid), "", "Cuadrícula"), (Some(Icon::Shelf), "", "Estante")], app.set.view.min(2)) {
        app.set.view = v;
    }
    let ra = place(40.0);
    if app.ui.icon_btn(id("add"), ra, Icon::Plus, "Añadir libros", app.ui.menu_open(id("addm"))) {
        app.ui.open_menu(id("addm"), ra, vec![MenuItem::new(Icon::File, "Elegir archivos…"), MenuItem::new(Icon::Folder, "Añadir una carpeta…")]);
    }
    match app.ui.picked(id("addm")) {
        Some(0) => app.pick_files(),
        Some(1) => app.pick_folder(),
        _ => {}
    }
    let rst = place(40.0);
    if app.ui.icon_btn(id("stats"), rst, Icon::Stats, "Resumen de lectura", false) {
        app.panel = Some(Panel::stats(app));
    }
    // pestañas
    let left = x0 + sw + 14.0;
    let right = rst.x - 14.0;
    let tw: Vec<f32> = TABS.iter().map(|t| app.ui.t.measure(fonts().ui_m, 14.0, t) + 32.0).collect();
    let total: f32 = tw.iter().sum::<f32>() + 8.0;
    if right - left >= total {
        let px = (w / 2.0 - total / 2.0).clamp(left, right - total);
        let pr = rect(px, cy - 20.0, total, 40.0);
        app.ui.d.boxed(pr, 20.0, th.text.a(0.045), 1.0, th.text.a(0.08));
        let mut xs = vec![px + 4.0];
        for t in &tw {
            xs.push(xs.last().unwrap() + t);
        }
        let kx = app.ui.anim(id("tabx"), xs[app.home.tab] - px, 18.0) + px;
        let kw = app.ui.anim(id("tabw"), tw[app.home.tab], 18.0);
        app.ui.d.fill(rect(kx, cy - 16.0, kw, 32.0), 16.0, th.text.a(0.12));
        for (i, t) in TABS.iter().enumerate() {
            let c = rect(xs[i], cy - 16.0, tw[i], 32.0);
            if app.ui.click(idx("tab", i), c) {
                set_tab(app, i);
            }
            let ov = app.ui.over(c);
            if ov {
                app.ui.cursor = CursorIcon::Pointer;
            }
            let h = app.ui.anim(idx("tab", i), ov as i32 as f32, 16.0);
            let s = app.ui.anim(idx("tabs", i), (app.home.tab == i) as i32 as f32, 16.0);
            app.ui.text_c(fonts().ui_m, 14.0, c.cx(), c.cy(), t, th.text2.mix(th.text, h.max(s)));
        }
    } else {
        let r = rect(left, cy - 20.0, (right - left).min(150.0), 40.0);
        if app.ui.button(id("tabb"), r, Btn::Soft, Some(Icon::Down), TABS[app.home.tab]) {
            let items = TABS.iter().enumerate().map(|(i, t)| MenuItem::check(*t, i == app.home.tab)).collect();
            app.ui.open_menu(id("tabm"), r, items);
        }
        if let Some(i) = app.ui.picked(id("tabm")) {
            set_tab(app, i);
        }
    }
}
fn set_tab(app: &mut App, i: usize) {
    app.home.tab = i;
    app.home.dirty = true;
    app.home.view_t = 0.35;
    app.home.grid.jump(0.0);
    app.home.shelf.jump(0.0);
}

// ---------------------------------------------------------------- vitrina
fn carousel(app: &mut App, area: Rect) {
    let th = app.ui.th;
    let n = app.home.list.len();
    let ch = (app.ui.h * 0.50).clamp(220.0, 640.0).min(area.h * 0.60).min((area.h * 0.97 - 262.0).max(150.0));
    let cx = area.cx();
    let cy = area.y + area.h * 0.028 + ch / 2.0;
    let floor = cy + ch / 2.0 + 3.0;
    let zone = rect(area.x, area.y, area.w, ch + area.h * 0.06);
    let d1 = ch * 0.64;
    let d2 = ch * 0.255;

    // arrastre y rueda
    if app.ui.hover(zone) && app.ui.input.wheel.1 + app.ui.input.wheel.0 != 0.0 {
        let w = app.ui.input.wheel.1 + app.ui.input.wheel.0;
        let p = (app.home.target - (w / 96.0)).clamp(0.0, n as f32 - 1.0);
        let p = if w > 0.0 { p.floor().max(app.home.target.round() - 1.0) } else { p.ceil().min(app.home.target.round() + 1.0) };
        focus_ix(app, p.max(0.0) as usize);
        app.ui.input.wheel = (0.0, 0.0);
    }
    if app.ui.hover(zone) && app.ui.input.pressed {
        app.home.drag = Some((app.ui.input.pos.0, app.home.pos.x, false));
    }
    let mut dragging = false;
    if let Some((sx, sp, moved)) = app.home.drag {
        let dx = app.ui.input.pos.0 - sx;
        let moved = moved || dx.abs() > 7.0;
        app.home.drag = Some((sx, sp, moved));
        if moved {
            dragging = true;
            app.home.pos.x = (sp - dx / (ch * 0.42)).clamp(-0.4, n as f32 - 0.6);
            app.home.pos.v = 0.0;
            let p = app.home.pos.x.round().clamp(0.0, n as f32 - 1.0) as usize;
            if p as f32 != app.home.target {
                focus_ix(app, p);
            }
            app.ui.cursor = CursorIcon::Grabbing;
            app.ui.redraw = true;
        }
        if !app.ui.input.down {
            app.home.drag = None;
        }
    }
    if !dragging {
        let (tg, dt) = (app.home.target, app.ui.dt);
        let om = if app.set.calm { 40.0 } else { 11.5 };
        if app.home.pos.step(tg, om, dt, 0.0006) {
            app.ui.redraw = true;
        }
    }
    let pos = app.home.pos.x;
    let vis = ((area.w / 2.0 - d1) / d2).ceil().max(1.0) as i32 + 2;
    let center = pos.round() as i32;
    let mut order: Vec<i32> = (center - vis..=center + vis).filter(|i| *i >= 0 && (*i as usize) < n).collect();
    order.sort_by(|a, b| (*b as f32 - pos).abs().total_cmp(&(*a as f32 - pos).abs()));
    let vp = (cx, cy + ch * 0.06);
    let intro = ease_out(app.home.intro);
    let sheet_t = app.sheet.as_ref().map_or(0.0, |s| s.t);
    let mut items = vec![];
    for i in order {
        let o = i as f32 - pos;
        let a = o.abs();
        let s = o.signum();
        let near = smooth(a.min(1.0));
        let x = s * (d1 * (1.0 - (1.0 - a.min(1.0)).powf(1.6)) + (a - 1.0).max(0.0) * d2) * (0.75 + 0.25 * intro);
        let rest = 0.40;
        let side = -s * 0.84;
        let yaw = lerp(rest, side, near);
        let z = -ch * 0.42 * near - (a - 1.0).max(0.0) * 16.0;
        let k = app.home.list[i as usize];
        let id = app.books[k].id;
        let pose = Pose { cx: cx + x, cy, h: ch, yaw, pitch: 0.0, z, open: 0.0 };
        app.home.poses.insert(id, (pose, vp));
        if app.lifted(id) {
            continue;
        }
        let edge = (1.0 - ((a - vis as f32 + 2.0) / 1.5)).clamp(0.0, 1.0);
        let mut l = app.look(k, if a < 3.5 { Size::Full } else { Size::Thumb });
        l.dim = 1.0 - (if th.dark { 0.30 } else { 0.12 }) * near - 0.035 * (a - 1.0).clamp(0.0, 5.0);
        l.spine_text = a < 4.0;
        items.push((i, id, pose, l, edge));
    }
    // suelo: reflejos y sombras primero
    for (_, _, p, l, e) in &items {
        app.ui.d.push_alpha(*e * intro);
        let fl = floor_at(p, vp);
        book3d::reflect(&mut app.ui, p, l, vp, fl, if th.dark { 0.20 } else { 0.14 });
        app.ui.d.pop_alpha();
    }
    for (_, _, p, l, e) in &items {
        app.ui.d.push_alpha(*e * intro * if th.dark { 0.9 } else { 0.45 });
        let fl = floor_at(p, vp);
        book3d::floor_shadow(&mut app.ui, p, l, vp, fl, 1.0);
        app.ui.d.pop_alpha();
    }
    let mut hit: Option<(i32, i64)> = None;
    for (i, id, p, l, e) in &items {
        app.ui.d.push_alpha(*e * intro);
        book3d::draw(&mut app.ui, p, l, vp);
        app.ui.d.pop_alpha();
        if app.ui.over(book3d::bounds(p, l, vp)) {
            hit = Some((*i, *id));
        }
    }
    if let Some((i, id)) = hit {
        if !dragging {
            app.ui.cursor = CursorIcon::Pointer;
        }
        let was_click = app.ui.input.released && app.home.drag.is_none_or(|d| !d.2);
        if was_click && app.ui.enabled && app.ui.active.is_none() && !dragging && zone.has(app.ui.drag_from) {
            if i as f32 == app.home.target.round() {
                if app.ui.input.clicks >= 2 {
                    app.open_book(id);
                } else {
                    open_sheet(app, id);
                }
            } else {
                focus_ix(app, i as usize);
            }
        }
        if app.ui.rclick(zone) {
            focus_ix(app, i as usize);
            open_book_menu(app, id);
        }
    }
    // pie: título, datos y botones
    let Some(fid) = app.home.focus else { return };
    if fid != app.home.title_id {
        app.home.title_old = app.home.title_id;
        app.home.title_id = fid;
        app.home.title_t = 0.0;
    }
    let mut t = app.home.title_t;
    app.ui.tween(&mut t, true, 0.30);
    app.home.title_t = t;
    let hide = 1.0 - smooth(sheet_t * 1.6);
    let y0 = floor + (area.b() - floor) * 0.17;
    app.ui.d.push_alpha(hide);
    for (id, a, dy) in [(app.home.title_old, 1.0 - smooth(t * 2.2), -6.0 * t), (app.home.title_id, smooth((t - 0.2) / 0.8), 8.0 * (1.0 - ease_out(t)))] {
        if a > 0.003 {
            if let Some(b) = app.book(id).cloned() {
                app.ui.d.push_alpha(a * intro);
                caption(app, &b, cx, y0 + dy, area.w);
                app.ui.d.pop_alpha();
            }
        }
    }
    if let Some(b) = app.book(fid).cloned() {
        let by = y0 + 84.0;
        let label = if b.progress >= 0.995 { "Leer de nuevo" } else if b.progress > 0.002 { "Continuar" } else { "Leer" };
        let w1 = (app.ui.btn_w(label, true) + 24.0).max(150.0);
        let w2 = 128.0;
        let tot = w1 + 12.0 + w2 + 12.0 + 46.0 + 12.0 + 46.0;
        let mut x = cx - tot / 2.0;
        app.ui.d.push_alpha(intro);
        if app.ui.button(id("c-read"), rect(x, by, w1, 46.0), Btn::Primary, Some(Icon::Book), label) {
            app.open_book(fid);
        }
        x += w1 + 12.0;
        if app.ui.button(id("c-sheet"), rect(x, by, w2, 46.0), Btn::Soft, Some(Icon::Panel), "Ficha") {
            open_sheet(app, fid);
        }
        x += w2 + 12.0;
        if app.ui.icon_btn(id("c-fav"), rect(x, by, 46.0, 46.0), if b.favorite { Icon::HeartFill } else { Icon::Heart }, if b.favorite { "Quitar de favoritos" } else { "Favorito" }, b.favorite) {
            app.edit_book(fid, |b| b.favorite = !b.favorite);
        }
        x += 58.0;
        let rm = rect(x, by, 46.0, 46.0);
        if app.ui.icon_btn(id("c-more"), rm, Icon::More, "Más opciones", false) {
            app.home.menu_book = fid;
            let items = menu_items(app, fid);
            app.ui.open_menu(id("bookm"), rm, items);
        }
        app.ui.d.pop_alpha();
    }
    // deslizador de posición
    if n > 1 {
        let sy = area.b() - 34.0;
        let sw = (area.w * 0.34).clamp(220.0, 460.0);
        let r = rect(cx - sw / 2.0, sy - 10.0, sw, 20.0);
        if app.ui.icon_btn_c(id("c-prev"), rect(r.x - 52.0, sy - 17.0, 34.0, 34.0), Icon::Left, "", false, th.text, th.accent, false) && app.home.target > 0.0 {
            focus_ix(app, app.home.target as usize - 1);
        }
        if app.ui.icon_btn_c(id("c-next"), rect(r.r() + 18.0, sy - 17.0, 34.0, 34.0), Icon::Right, "", false, th.text, th.accent, false) && (app.home.target as usize) + 1 < n {
            focus_ix(app, app.home.target as usize + 1);
        }
        let v = (pos / (n - 1) as f32).clamp(0.0, 1.0);
        if let Some(nv) = app.ui.slider_c(id("c-slider"), r, v, th.text, th.text.a(0.85)) {
            focus_ix(app, (nv * (n - 1) as f32).round() as usize);
        }
        app.ui.text_c(fonts().ui, 12.5, cx, sy - 24.0, &format!("{} / {}", app.home.target as usize + 1, n), th.text3);
    }
    app.ui.d.pop_alpha();
}
fn floor_at(p: &Pose, vp: (f32, f32)) -> f32 {
    let k = (book3d::CAM - p.z) / book3d::CAM;
    vp.1 + (p.cy + p.h / 2.0 - vp.1) / k
}
fn caption(app: &mut App, b: &Book, cx: f32, y: f32, w: f32) {
    let th = app.ui.th;
    let mut size = 34.0;
    let maxw = (w * 0.7).min(900.0);
    while size > 24.0 && app.ui.t.measure(fonts().title, size, &b.title) > maxw {
        size -= 1.0;
    }
    let t = app.ui.t.fit(fonts().title, size, &b.title, maxw);
    app.ui.text_c(fonts().title, size, cx, y, &t, th.text);
    let mut line = vec![b.author_line()];
    if !b.series.is_empty() {
        line.push(if b.series_ix > 0.0 { format!("{} {}", b.series, num(b.series_ix)) } else { b.series.clone() });
    }
    if b.year > 0 {
        line.push(b.year.to_string());
    }
    if b.progress > 0.002 && b.progress < 0.995 {
        line.push(format!("{} %", (b.progress * 100.0).round().max(1.0)));
    } else if b.progress >= 0.995 {
        line.push("Terminado".into());
    } else if b.pages > 0 {
        line.push(format!("{} páginas", b.pages));
    }
    let l = app.ui.t.fit(fonts().ui, 15.0, &line.join("   ·   "), maxw);
    app.ui.text_c(fonts().ui, 15.0, cx, y + 38.0, &l, th.text2);
}
pub fn num(f: f32) -> String {
    if f.fract() == 0.0 {
        format!("{}", f as i32)
    } else {
        format!("{f}")
    }
}

// ---------------------------------------------------------------- cuadrícula
fn grid_dims(app: &App, w: f32) -> (usize, f32, f32, f32) {
    let want = app.set.grid_size.clamp(110.0, 260.0);
    let inner = w - 72.0;
    let cols = ((inner + 26.0) / (want + 26.0)).floor().max(2.0);
    let cell = inner / cols;
    let bh = want * 1.42;
    (cols as usize, cell, bh, bh + 78.0)
}
fn grid_cols(app: &App) -> usize {
    grid_dims(app, app.ui.w - app.home.drawer_t * DRAWER).0
}
fn grid(app: &mut App, area: Rect, e: f32) {
    let th = app.ui.th;
    let (cols, cell, bh, ch) = grid_dims(app, area.w);
    let x0 = area.x + 36.0;
    // alturas
    let mut y = 8.0;
    let mut heads = vec![];
    let mut cells: Vec<(usize, f32, f32, usize)> = vec![];
    let mut pos = 0;
    for (gi, g) in app.home.groups.iter().enumerate() {
        if !g.name.is_empty() {
            heads.push((gi, y));
            y += 52.0;
        }
        for (k, &b) in g.ids.iter().enumerate() {
            cells.push((b, x0 + (k % cols) as f32 * cell, y + (k / cols) as f32 * ch, pos));
            pos += 1;
        }
        y += g.ids.len().div_ceil(cols) as f32 * ch + 14.0;
    }
    let content = y + 40.0;
    if app.home.key_nav {
        app.home.key_nav = false;
        if let Some(c) = cells.get(app.home.target as usize) {
            let s = &mut app.home.grid;
            if c.2 - 30.0 < s.target {
                s.target = c.2 - 60.0;
            } else if c.2 + ch > s.target + area.h {
                s.target = c.2 + ch - area.h + 20.0;
            }
        }
    }
    let sy = app.ui.scroll(id("gscroll"), &mut app.home.grid, area, content);
    app.ui.clip(rect(area.x, area.y, area.w, area.h), 0.0);
    let top = area.y - sy + (1.0 - e) * 18.0;
    for (gi, hy) in heads {
        let yy = top + hy + 26.0;
        if yy > area.y - 40.0 && yy < area.b() + 40.0 {
            let (name, n) = { let g = &app.home.groups[gi]; (g.name.clone(), g.ids.len()) };
            let w = app.ui.text(fonts().title, 22.0, x0 + 4.0, yy, &name, th.text);
            app.ui.text(fonts().ui, 13.0, x0 + 18.0 + w, yy + 2.0, &n.to_string(), th.text3);
            app.ui.d.fill(rect(x0 + 34.0 + w + 20.0, yy, (area.w - 90.0 - w).max(0.0), 1.0), 0.0, th.line);
        }
    }
    let mut hovered = None;
    for (k, x, cy, p) in cells {
        let yy = top + cy;
        if yy > area.b() + 20.0 || yy + ch < area.y - 20.0 {
            continue;
        }
        let bid = app.books[k].id;
        let l = app.look(k, Size::Thumb);
        let big = bh > 300.0;
        let l = if big { app.look(k, Size::Full) } else { l };
        let r = rect(x, yy, cell, bh + 16.0);
        let over = app.ui.over(r);
        let h = app.ui.anim(idx("gb", bid as usize), over as i32 as f32, 13.0);
        let focus = app.home.focus == Some(bid);
        let f = app.ui.anim(idx("gf", bid as usize), focus as i32 as f32, 14.0);
        let bx = x + cell / 2.0;
        let by = yy + 12.0 + bh / 2.0;
        let pose = Pose { cx: bx, cy: by - 5.0 * h, h: bh * (1.0 + 0.035 * h), yaw: lerp(0.42, 0.08, smooth(h)), pitch: 0.0, z: 0.0, open: 0.0 };
        let vp = (bx, by);
        app.home.poses.insert(bid, (pose, vp));
        if !app.lifted(bid) {
            let fl = by + bh / 2.0 + 2.0;
            app.ui.d.push_alpha(if th.dark { 0.75 } else { 0.35 });
            book3d::floor_shadow(&mut app.ui, &pose, &l, vp, fl, 0.8 + 0.2 * h);
            app.ui.d.pop_alpha();
            book3d::draw(&mut app.ui, &pose, &l, vp);
        }
        if over {
            hovered = Some((bid, p, r));
        }
        let b = &app.books[k];
        let (title, author, prog, fav, missing) = (b.title.clone(), b.author_list().first().map(|s| s.to_string()).unwrap_or_default(), b.progress, b.favorite, b.missing);
        let ty = yy + bh + 34.0;
        let tw = cell - 22.0;
        let s = app.ui.t.fit(fonts().ui_m, 14.0, &title, tw);
        app.ui.text_c(fonts().ui_m, 14.0, bx, ty, &s, th.text.mix(th.accent, f * 0.9));
        let sub = if missing { "Archivo no encontrado".to_string() } else { author };
        let s = app.ui.t.fit(fonts().ui, 12.5, &sub, tw);
        app.ui.text_c(fonts().ui, 12.5, bx, ty + 19.0, &s, th.text3);
        if prog > 0.002 && prog < 0.995 {
            let pw = (bh * 0.5).min(tw);
            app.ui.d.fill(rect(bx - pw / 2.0, ty + 34.0, pw, 3.0), 1.5, th.text.a(0.14));
            app.ui.d.fill(rect(bx - pw / 2.0, ty + 34.0, pw * prog, 3.0), 1.5, th.accent);
        } else if prog >= 0.995 {
            app.ui.icon(Icon::Check, bx, ty + 35.0, 13.0, th.accent);
        }
        if fav {
            app.ui.icon(Icon::HeartFill, bx + bh * 0.31, yy + 22.0, 15.0, th.accent);
        }
    }
    app.ui.unclip();
    fade_edges(app, area);
    if let Some((bid, p, r)) = hovered {
        app.ui.cursor = CursorIcon::Pointer;
        if app.ui.click(idx("gc", bid as usize), r) {
            if app.home.focus == Some(bid) && app.ui.input.clicks >= 2 {
                app.open_book(bid);
            } else {
                focus_ix(app, p);
                open_sheet(app, bid);
            }
        }
        if app.ui.rclick(r) {
            focus_ix(app, p);
            open_book_menu(app, bid);
        }
    }
}
fn fade_edges(app: &mut App, area: Rect) {
    let _ = (app, area);
}

// ---------------------------------------------------------------- estante
fn shelf(app: &mut App, area: Rect, e: f32) {
    let th = app.ui.th;
    let bh = (app.set.grid_size * 1.30).clamp(150.0, 320.0);
    let gap_top = 46.0;
    let board = 16.0;
    let rowh = bh + gap_top + board + 26.0;
    let x0 = area.x + 56.0;
    let width = area.w - 112.0;
    struct Row {
        label: Option<usize>,
        ids: Vec<(usize, f32, usize)>,
    }
    // reparto por baldas
    let mut rows: Vec<Row> = vec![];
    let mut pos = 0;
    let reserve = bh * 0.72;
    for (gi, g) in app.home.groups.iter().enumerate() {
        let mut first = true;
        let mut x = 0.0;
        for &k in &g.ids {
            let b = &app.books[k];
            let t = spine_w(b, bh);
            if first || x + t > width - reserve {
                rows.push(Row { label: (first && !g.name.is_empty()).then_some(gi), ids: vec![] });
                x = 0.0;
                first = false;
            }
            rows.last_mut().unwrap().ids.push((k, t, pos));
            pos += 1;
            x += t + 1.5;
        }
    }
    let content = rows.len() as f32 * rowh + 60.0;
    if app.home.key_nav {
        app.home.key_nav = false;
        let tg = app.home.target as usize;
        if let Some(ri) = rows.iter().position(|r| r.ids.iter().any(|i| i.2 == tg)) {
            let y = ri as f32 * rowh;
            let s = &mut app.home.shelf;
            if y < s.target {
                s.target = y;
            } else if y + rowh > s.target + area.h {
                s.target = y + rowh - area.h + 10.0;
            }
        }
    }
    let sy = app.ui.scroll(id("sscroll"), &mut app.home.shelf, area, content);
    app.ui.clip(area, 0.0);
    let mut hovered: Option<(i64, usize, Rect)> = None;
    let mut front: Vec<(Pose, crate::book3d::Look, (f32, f32))> = vec![];
    let wood = if th.dark { th.raised.mix(Color::hex(0x5A4632), 0.45) } else { th.raised.mix(Color::hex(0xB08E63), 0.55) };
    // baldas vacías hasta abajo: sitio para lo que vendrá
    while rows.len() as f32 * rowh < area.h {
        rows.push(Row { label: None, ids: vec![] });
    }
    for (ri, row) in rows.iter().enumerate() {
        let y = area.y - sy + 10.0 + ri as f32 * rowh + (1.0 - e) * 24.0 * (1.0 + (ri % 4) as f32 * 0.4);
        if y > area.b() || y + rowh < area.y {
            continue;
        }
        let base = y + gap_top + bh;
        // fondo del hueco y balda
        app.ui.d.grad(rect(x0 - 26.0, y + 6.0, width + 52.0, gap_top + bh - 6.0), 6.0, Color::BLACK.a(if th.dark { 0.16 } else { 0.03 }), Color::BLACK.a(if th.dark { 0.30 } else { 0.09 }), false);
        let bd = rect(x0 - 34.0, base, width + 68.0, board);
        app.ui.d.shadow(bd.off(0.0, 10.0), 4.0, 22.0, Color::BLACK.a(if th.dark { 0.5 } else { 0.22 }));
        app.ui.d.grad(rect(bd.x, bd.y, bd.w, 5.0), 2.0, wood.mul(1.28), wood.mul(1.12), false);
        app.ui.d.grad(rect(bd.x, bd.y + 4.0, bd.w, board - 4.0), 2.0, wood.mul(0.95), wood.mul(0.72), false);
        if let Some(g) = row.label {
            let (name, n) = { let g = &app.home.groups[g]; (g.name.clone(), g.ids.len()) };
            let s = format!("{name}  ·  {n}");
            let w = app.ui.t.measure(fonts().ui_m, 12.0, &s) + 24.0;
            let pl = rect(x0 - 10.0, base + 1.0, w, board + 8.0);
            app.ui.d.shadow(pl.off(0.0, 2.0), 5.0, 8.0, Color::BLACK.a(0.4));
            app.ui.d.grad(pl, 5.0, Color::hex(0xE0C48A), Color::hex(0xB08A4E), false);
            app.ui.text(fonts().ui_m, 12.0, pl.x + 12.0, pl.cy(), &s, Color::hex(0x33240F));
        }
        let mut x = x0;
        // qué libro toca el ratón: según las posiciones ya animadas
        let mut placed = vec![];
        for (k, t, p) in &row.ids {
            let bid = app.books[*k].id;
            let a = app.ui.peek_anim(idx("sb", bid as usize));
            let asp = book3d::book_aspect(if app.books[*k].cover_ver > 0 { app.books[*k].aspect } else { 2.0 / 3.0 });
            let open_w = bh * asp * 0.90 + t * 0.5 + 16.0;
            let occ = lerp(*t, open_w, smooth(a));
            placed.push((*k, *t, *p, bid, x, occ, a, asp));
            x += occ + 1.5;
        }
        for (k, t, p, bid, x, occ, a, _asp) in placed {
            let r = rect(x, base - bh - 8.0, occ, bh + 8.0);
            let over = app.ui.over(r);
            if over {
                hovered = Some((bid, p, r));
            }
            let key = app.home.focus == Some(bid) && app.ui.input.pos.0 < 0.0;
            // el muelle real: se escribe a mano para leerlo antes de colocar
            let tg = (over || key) as i32 as f32;
            let na = a + (tg - a) * (1.0 - (-11.0 * app.ui.dt).exp());
            let na = if (na - tg).abs() < 0.003 { tg } else { app.ui.redraw = true; na };
            app.ui.set_anim(idx("sb", bid as usize), na);
            let s = smooth(a);
            let mut l = app.look(k, if a > 0.02 { Size::Full } else { Size::Thumb });
            l.thick = t / bh;
            let h = bh * height_var(bid);
            let cx = lerp(x + t / 2.0, x + occ / 2.0, s);
            let pose = Pose { cx, cy: base - h / 2.0 - 4.0 * s, h, yaw: lerp(FRAC_PI_2, 0.36, s), pitch: 0.0, z: lerp(-h * l.aspect / 2.0, 30.0, s), open: 0.0 };
            let vp = (cx, base - bh / 2.0);
            app.home.poses.insert(bid, (pose, vp));
            if app.lifted(bid) {
                continue;
            }
            if a > 0.01 {
                front.push((pose, l, vp));
            } else {
                book3d::draw(&mut app.ui, &pose, &l, vp);
            }
        }
    }
    for (p, l, vp) in &front {
        app.ui.d.push_alpha(if th.dark { 0.8 } else { 0.4 });
        book3d::floor_shadow(&mut app.ui, p, l, *vp, p.cy + p.h / 2.0 + 2.0, 0.8);
        app.ui.d.pop_alpha();
        book3d::draw(&mut app.ui, p, l, *vp);
    }
    app.ui.unclip();
    if let Some((bid, p, r)) = hovered {
        app.ui.cursor = CursorIcon::Pointer;
        if app.ui.click(idx("sc", bid as usize), r) {
            if app.home.focus == Some(bid) && app.ui.input.clicks >= 2 {
                app.open_book(bid);
            } else {
                focus_ix(app, p);
                open_sheet(app, bid);
            }
        }
        if app.ui.rclick(r) {
            focus_ix(app, p);
            open_book_menu(app, bid);
        }
        if let Some(b) = app.book(bid).cloned() {
            let s = app.ui.t.fit(fonts().ui_m, 13.5, &format!("{}   ·   {}", b.title, b.author_line()), area.w * 0.7);
            let w = app.ui.t.measure(fonts().ui_m, 13.5, &s) + 36.0;
            let pr = rect(area.cx() - w / 2.0, area.b() - 56.0, w, 38.0);
            app.ui.d.shadow(pr.off(0.0, 6.0), 19.0, 22.0, Color::BLACK.a(0.4));
            app.ui.d.boxed(pr, 19.0, th.raised, 1.0, th.line);
            app.ui.text_c(fonts().ui_m, 13.5, pr.cx(), pr.cy(), &s, th.text);
        }
    }
}
fn spine_w(b: &Book, h: f32) -> f32 {
    (book3d::thickness(b.pages) * 2.0 * h).clamp(22.0, 46.0)
}
/// Los libros de un estante no miden todos lo mismo.
fn height_var(id: i64) -> f32 {
    0.90 + ((id.wrapping_mul(7919) % 11) as f32 / 11.0) * 0.10
}

// ---------------------------------------------------------------- vacío
fn empty(app: &mut App, area: Rect) {
    let th = app.ui.th;
    let b = Book { title: "Tu próxima historia".into(), authors: "Saga".into(), id: -1, pages: 300, ..Default::default() };
    let time = app.ui.time;
    let tex = app.covers.best(&app.jobs, &b, time);
    let h = (area.h * 0.42).min(380.0);
    let (cx, cy) = (area.cx(), area.y + area.h * 0.06 + h / 2.0);
    let sway = (app.ui.input.pos.0 - cx).clamp(-600.0, 600.0) / 600.0;
    let yaw = app.ui.anim(id("emptyyaw"), 0.42 + sway * 0.25, 6.0);
    let pose = Pose { cx, cy, h, yaw, pitch: 0.0, z: 0.0, open: 0.0 };
    let l = crate::book3d::Look { tex, img_aspect: 2.0 / 3.0, aspect: 2.0 / 3.0, thick: 0.06, color: Color::hex(crate::covers::gen_color(&b.title)), title: b.title.clone(), author: "Saga".into(), dim: 1.0, spine_text: true, paper: Color::hex(0xE2D8C4), inner: [None, None], lid: 1.0 };
    let vp = (cx, cy);
    book3d::reflect(&mut app.ui, &pose, &l, vp, cy + h / 2.0 + 2.0, 0.2);
    book3d::floor_shadow(&mut app.ui, &pose, &l, vp, cy + h / 2.0 + 2.0, if th.dark { 0.9 } else { 0.4 });
    book3d::draw(&mut app.ui, &pose, &l, vp);
    let y = cy + h / 2.0 + 74.0;
    app.ui.text_c(fonts().title, 34.0, cx, y, "Un rincón tranquilo para leer", th.text);
    app.ui.text_c(fonts().ui, 15.0, cx, y + 40.0, "Arrastra aquí tus libros o elige una carpeta. EPUB, PDF, MOBI, AZW3, FB2, CBZ, CBR, DJVU, DOCX y más.", th.text2);
    let bw = 190.0;
    if app.ui.button(id("e-files"), rect(cx - bw - 7.0, y + 76.0, bw, 48.0), Btn::Primary, Some(Icon::Plus), "Añadir libros") {
        app.pick_files();
    }
    if app.ui.button(id("e-dir"), rect(cx + 7.0, y + 76.0, bw, 48.0), Btn::Soft, Some(Icon::Folder), "Elegir carpeta") {
        app.pick_folder();
    }
}
fn nothing(app: &mut App, area: Rect) {
    let th = app.ui.th;
    let (cx, cy) = (area.cx(), area.cy() - 40.0);
    app.ui.icon(Icon::Book, cx, cy - 46.0, 54.0, th.text3);
    let msg = if !app.home.search.text.is_empty() { "Nada coincide con la búsqueda" } else if !app.home.facet.is_empty() { "Ningún libro con esos filtros" } else { ["Aún no hay libros", "No tienes lecturas empezadas", "No queda nada por leer", "Todavía no has terminado ninguno", "Marca un libro con el corazón y aparecerá aquí"][app.home.tab] };
    app.ui.text_c(fonts().title, 26.0, cx, cy + 16.0, msg, th.text2);
    if !app.home.search.text.is_empty() || !app.home.facet.is_empty() {
        if app.ui.button(id("n-clear"), rect(cx - 90.0, cy + 50.0, 180.0, 42.0), Btn::Soft, None, "Quitar filtros") {
            app.home.search.set("");
            app.home.facet.clear();
            app.home.dirty = true;
        }
    }
}

// ---------------------------------------------------------------- cajón de clasificación
fn drawer(app: &mut App, t: f32) {
    let th = app.ui.th;
    let r = rect(-DRAWER + t * DRAWER, STRIP + 62.0, DRAWER, app.ui.h - STRIP - 62.0);
    let body = rect(r.x + 24.0, r.y + 4.0, r.w - 34.0, r.h - 20.0);
    app.ui.d.push_alpha(t);
    let count = |app: &App, f: &Facet| app.books.iter().filter(|b| in_tab(b, app.home.tab) && has(app, b, f)).count();
    let mut secs: Vec<(&str, Icon, Vec<(Facet, String, usize)>)> = vec![];
    let colls: Vec<(Facet, String, usize)> = app.colls.iter().map(|c| (Facet::Coll(c.id), c.name.clone(), count(app, &Facet::Coll(c.id)))).collect();
    secs.push(("Colecciones", Icon::Layers, colls));
    let tally = |app: &App, f: &dyn Fn(&Book) -> Vec<String>| {
        let mut m: HashMap<String, usize> = HashMap::new();
        for b in app.books.iter().filter(|b| in_tab(b, app.home.tab)) {
            for k in f(b) {
                *m.entry(k).or_default() += 1;
            }
        }
        let mut v: Vec<(String, usize)> = m.into_iter().collect();
        v.sort_by_cached_key(|x| (std::cmp::Reverse(x.1), norm(&x.0)));
        v
    };
    let authors = tally(app, &|b| b.author_list().iter().map(|s| s.to_string()).collect());
    secs.push(("Autores", Icon::User, authors.into_iter().map(|(n, c)| (Facet::Author(n.clone()), n, c)).collect()));
    let genres = tally(app, &|b| b.genre_list().iter().map(|s| s.to_string()).collect());
    secs.push(("Géneros", Icon::Tag, genres.into_iter().map(|(n, c)| (Facet::Genre(n.clone()), n, c)).collect()));
    let series = tally(app, &|b| if b.series.is_empty() { vec![] } else { vec![b.series.clone()] });
    secs.push(("Series", Icon::Book, series.into_iter().map(|(n, c)| (Facet::Series(n.clone()), n, c)).collect()));
    let formats = tally(app, &|b| vec![b.format.clone()]);
    secs.push(("Formatos", Icon::File, formats.into_iter().map(|(n, c)| (Facet::Format(n.clone()), n.to_uppercase(), c)).collect()));
    let langs = tally(app, &|b| if b.language.is_empty() { vec![] } else { vec![b.language.clone()] });
    secs.push(("Idiomas", Icon::Globe, langs.into_iter().map(|(n, c)| (Facet::Lang(n.clone()), lang_name(&n), c)).collect()));

    let mut content = 0.0;
    for (i, s) in secs.iter().enumerate() {
        content += 42.0;
        if i == app.home.open_sec {
            content += s.2.len() as f32 * 34.0 + if i == 0 { 44.0 } else { 8.0 };
        }
    }
    let sy = app.ui.scroll(id("drs"), &mut app.home.drawer_scroll, body, content + 20.0);
    app.ui.clip(body, 0.0);
    let mut y = body.y - sy;
    let mut toggle = None;
    for (i, (name, ic, items)) in secs.iter().enumerate() {
        let hr = rect(body.x, y, body.w - 14.0, 38.0);
        if app.ui.click(idx("sec", i), hr) {
            app.home.open_sec = if app.home.open_sec == i { usize::MAX } else { i };
        }
        let ov = app.ui.over(hr);
        if ov {
            app.ui.cursor = CursorIcon::Pointer;
        }
        let a = app.ui.anim(idx("sec", i), ov as i32 as f32, 16.0);
        let o = app.ui.anim(idx("seco", i), (app.home.open_sec == i) as i32 as f32, 16.0);
        app.ui.icon(*ic, hr.x + 12.0, hr.cy(), 17.0, th.text2.mix(th.text, a.max(o)));
        app.ui.text(fonts().ui_m, 14.5, hr.x + 32.0, hr.cy(), name, th.text2.mix(th.text, a.max(o)));
        app.ui.text_r(fonts().ui, 12.5, hr.r() - 6.0, hr.cy(), &items.len().to_string(), th.text3);
        y += 42.0;
        if app.home.open_sec == i {
            for (k, (f, label, n)) in items.iter().enumerate() {
                let rr = rect(body.x + 4.0, y, body.w - 18.0, 32.0);
                if rr.b() > body.y && rr.y < body.b() {
                    let on = app.home.facet.contains(f);
                    let bid = idx(label, k + i * 1000);
                    if app.ui.click(bid, rr) {
                        toggle = Some(f.clone());
                    }
                    if app.ui.rclick(rr) {
                        if let Facet::Coll(c) = f {
                            app.home.menu_book = *c;
                            app.ui.open_menu(id("collm"), rect(app.ui.input.pos.0, app.ui.input.pos.1, 1.0, 1.0), vec![MenuItem::new(Icon::Trash, "Eliminar colección").danger()]);
                        }
                    }
                    let ov = app.ui.over(rr);
                    if ov {
                        app.ui.cursor = CursorIcon::Pointer;
                    }
                    let h = app.ui.anim(bid, ov as i32 as f32, 18.0);
                    let o = app.ui.anim(bid ^ 7, on as i32 as f32, 18.0);
                    app.ui.d.fill(rr, 10.0, th.text.a(0.06 * h).mix(th.accent.a(0.18), o));
                    app.ui.text_fit(fonts().ui, 13.5, rr.x + 28.0, rr.cy(), label, rr.w - 66.0, th.text2.mix(th.text, h).mix(th.accent, o));
                    app.ui.text_r(fonts().ui, 12.0, rr.r() - 10.0, rr.cy(), &n.to_string(), th.text3);
                }
                y += 34.0;
            }
            if i == 0 {
                if let Some(mut ed) = app.home.new_coll.take() {
                    let rr = rect(body.x + 4.0, y + 2.0, body.w - 18.0, 36.0);
                    let (_, enter) = app.ui.field(id("newcoll"), rr, &mut ed, Some(Icon::Layers), "Nombre de la colección");
                    let esc = app.ui.key(NamedKey::Escape);
                    if enter && !ed.text.trim().is_empty() {
                        app.db.add_collection(ed.text.trim());
                        app.colls = app.db.collections();
                        app.ui.focus = None;
                    } else if esc || (app.ui.focus != Some(id("newcoll")) && ed.text.is_empty() && !app.ui.input.pressed) {
                        app.ui.focus = None;
                    } else {
                        app.home.new_coll = Some(ed);
                    }
                } else {
                    let rr = rect(body.x + 4.0, y, body.w - 18.0, 36.0);
                    if app.ui.button(id("addcoll"), rr, Btn::Ghost, Some(Icon::Plus), "Nueva colección") {
                        app.home.new_coll = Some(Edit::default());
                        app.ui.focus = Some(id("newcoll"));
                    }
                }
                y += 44.0;
            } else {
                y += 8.0;
            }
        }
    }
    app.ui.unclip();
    if let Some(f) = toggle {
        let h = &mut app.home;
        match h.facet.iter().position(|x| *x == f) {
            Some(p) => {
                h.facet.remove(p);
            }
            None => h.facet.push(f),
        }
        h.dirty = true;
        h.view_t = 0.4;
    }
    if app.ui.picked(id("collm")) == Some(0) {
        let c = app.home.menu_book;
        app.ask("¿Eliminar la colección?", "Los libros siguen en tu biblioteca; solo desaparece la agrupación.", "Eliminar", move |a| {
            a.db.delete_collection(c);
            a.colls = a.db.collections();
            a.home.facet.retain(|f| *f != Facet::Coll(c));
            a.home.dirty = true;
        });
    }
    app.ui.d.pop_alpha();
}

// ---------------------------------------------------------------- menú de un libro
fn open_book_menu(app: &mut App, bid: i64) {
    app.home.menu_book = bid;
    let items = menu_items(app, bid);
    let p = app.ui.input.pos;
    app.ui.open_menu(id("bookm"), rect(p.0, p.1, 1.0, 1.0), items);
}
fn menu_items(app: &App, bid: i64) -> Vec<MenuItem> {
    let Some(b) = app.book(bid) else { return vec![] };
    let mut v = vec![
        MenuItem::new(Icon::Book, if b.progress > 0.002 { "Continuar leyendo" } else { "Leer" }),
        MenuItem::new(Icon::Panel, "Ver ficha"),
        MenuItem::new(Icon::Pencil, "Editar información…"),
        MenuItem::new(Icon::Wand, "Buscar datos y portada…").sep(),
        MenuItem::new(if b.favorite { Icon::HeartFill } else { Icon::Heart }, if b.favorite { "Quitar de favoritos" } else { "Añadir a favoritos" }).sep(),
        MenuItem::head("Estado"),
    ];
    for (s, n) in Status::ALL {
        v.push(MenuItem::check(n, b.status == s));
    }
    v.push(MenuItem::head("Colecciones"));
    for c in &app.colls {
        v.push(MenuItem::check(c.name.clone(), c.books.contains(&bid)));
    }
    v.push(MenuItem::new(Icon::Plus, "Nueva colección…").sep());
    v.push(MenuItem::new(Icon::Folder, "Abrir su carpeta"));
    v.push(MenuItem::new(Icon::Trash, "Quitar de la biblioteca").danger());
    v
}
fn book_menu(app: &mut App) {
    let Some(i) = app.ui.picked(id("bookm")) else { return };
    let bid = app.home.menu_book;
    let nc = app.colls.len();
    match i {
        0 => app.open_book(bid),
        1 => open_sheet(app, bid),
        2 => app.editor = crate::views::editor::Editor::new(app, bid, false),
        3 => app.editor = crate::views::editor::Editor::new(app, bid, true),
        4 => app.edit_book(bid, |b| b.favorite = !b.favorite),
        6..=9 => app.set_status(bid, Status::ALL[i - 6].0),
        _ if i >= 11 && i < 11 + nc => {
            let c = &mut app.colls[i - 11];
            let on = !c.books.contains(&bid);
            if on {
                c.books.push(bid);
            } else {
                c.books.retain(|b| *b != bid);
            }
            let cid = c.id;
            app.db.set_in_collection(cid, bid, on);
            app.home.dirty = true;
        }
        _ if i == 11 + nc => {
            app.home.drawer = true;
            app.home.open_sec = 0;
            app.home.new_coll = Some(Edit::default());
            app.ui.focus = Some(id("newcoll"));
        }
        _ if i == 12 + nc => {
            if let Some(b) = app.book(bid) {
                let _ = std::process::Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", b.path)).spawn();
            }
        }
        _ if i == 13 + nc => app.ask_remove(bid),
        _ => {}
    }
}
