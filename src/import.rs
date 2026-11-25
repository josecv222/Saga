//! Añadir libros y traer sus datos.
use crate::app::App;
use crate::covers::{self, Made};
use crate::db::{cover_path, thumb_path, Book};
use crate::formats::{self, Meta};
use crate::jobs::Apply;
use crate::meta::{self, Cand};
use std::path::{Path, PathBuf};

pub struct Found {
    pub path: String,
    pub format: String,
    pub meta: Meta,
    pub made: Option<Made>,
}
fn walk(p: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if p.is_dir() {
        if depth > 12 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(p) else { return };
        let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        v.sort();
        for e in v {
            // en carpetas, el texto suelto y las páginas web no son libros
            if e.is_dir() || (formats::supported(&e) && !matches!(formats::ext_of(&e).as_str(), "txt" | "md" | "markdown" | "html" | "htm" | "xhtml")) {
                walk(&e, out, depth + 1);
            }
        }
    } else if formats::supported(p) {
        out.push(p.to_path_buf());
    }
}
fn load(p: &Path) -> Found {
    let (format, mut meta, cover) = formats::probe(p);
    if meta.pages == 0 {
        // páginas de un libro de bolsillo
        if let Ok(formats::Opened::Flow(d)) = formats::open(p) {
            meta.pages = (d.len as f32 / 1650.0).ceil() as i32;
        }
    }
    Found { path: p.to_string_lossy().to_string(), format, meta, made: covers::make(&cover) }
}

impl App {
    pub fn pick_files(&mut self) {
        if self.dialog {
            return;
        }
        self.dialog = true;
        self.jobs.thread(|| {
            let r = rfd::FileDialog::new().set_title("Añadir libros").add_filter("Libros", formats::EXTS).add_filter("Todos los archivos", &["*"]).pick_files();
            Some(Box::new(move |a: &mut App| {
                a.dialog = false;
                a.add_paths(r.unwrap_or_default());
            }) as Apply)
        });
    }
    pub fn pick_folder(&mut self) {
        if self.dialog {
            return;
        }
        self.dialog = true;
        self.jobs.thread(|| {
            let r = rfd::FileDialog::new().set_title("Añadir una carpeta de libros").pick_folder();
            Some(Box::new(move |a: &mut App| {
                a.dialog = false;
                a.add_paths(r.into_iter().collect());
            }) as Apply)
        });
    }
    pub fn add_paths(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        self.jobs.run(0, move || {
            let mut v = vec![];
            for p in &paths {
                walk(p, &mut v, 0);
            }
            Some(Box::new(move |a: &mut App| a.import_files(v)) as Apply)
        });
    }
    fn import_files(&mut self, v: Vec<PathBuf>) {
        let known: std::collections::HashSet<String> = self.books.iter().map(|b| b.path.to_lowercase()).collect();
        let n = v.len();
        let fresh: Vec<PathBuf> = v.into_iter().filter(|p| !known.contains(&p.to_string_lossy().to_lowercase())).collect();
        if fresh.is_empty() {
            self.ui.toast(match n {
                0 => "No se encontraron libros",
                1 => "Ese libro ya está en tu biblioteca",
                _ => "Esos libros ya están en tu biblioteca",
            });
            return;
        }
        self.import_total += fresh.len();
        for p in fresh {
            self.jobs.run(2, move || {
                let f = std::panic::catch_unwind(|| load(&p)).unwrap_or_else(|_| {
                    let (t, a) = formats::from_filename(&p);
                    Found { path: p.to_string_lossy().to_string(), format: formats::ext_of(&p), meta: Meta { title: t, authors: a.into_iter().collect(), ..Default::default() }, made: None }
                });
                Some(Box::new(move |a: &mut App| a.imported(f)) as Apply)
            });
        }
    }
    fn imported(&mut self, f: Found) {
        self.import_done += 1;
        let m = f.meta;
        let title = if m.title.trim().is_empty() { "Sin título".to_string() } else { m.title };
        let known = meta::genres(&m.genres);
        let mut b = Book {
            path: f.path,
            format: f.format,
            authors: m.authors.join("; "),
            series: m.series,
            series_ix: m.series_ix,
            publisher: m.publisher,
            year: m.year,
            pages: m.pages,
            language: m.language,
            isbn: m.isbn,
            description: m.description,
            genres: if known.is_empty() { m.genres } else { known }.join("; "),
            color: covers::gen_color(&title),
            title,
            ..Default::default()
        };
        if self.db.insert(&mut b).is_ok() {
            if let Some(md) = &f.made {
                write_cover(&mut b, md);
                self.db.save(&b);
            }
            let id = b.id;
            self.books.push(b);
            self.reindex();
            self.home.dirty = true;
            self.home.arrived = Some(id);
            if self.set.auto_meta {
                self.auto_meta(id);
            }
        }
        if self.import_done >= self.import_total {
            let n = self.import_total;
            self.import_done = 0;
            self.import_total = 0;
            self.ui.toast(if n == 1 { "Libro añadido".to_string() } else { format!("{n} libros añadidos") });
        }
    }
    /// Completa lo que falte sin pisar lo que ya hay.
    pub fn auto_meta(&mut self, id: i64) {
        let Some(b) = self.book(id) else { return };
        if b.title.chars().count() < 3 {
            return;
        }
        let (t, a, i, has_cover) = (b.title.clone(), b.authors.clone(), b.isbn.clone(), b.cover_ver > 0);
        self.jobs.net(2, move || {
            let mut all: Vec<Cand> = meta::search(&t, &a, &i).into_iter().filter(|c| c.score >= 0.79).collect();
            if all.is_empty() {
                return Some(Box::new(move |app: &mut App| app.meta_tried(id)) as Apply);
            }
            let mut best = all.remove(0);
            // el año de la obra, no el de la reedición
            let y = meta::first_year(&best.title, best.authors.first().map_or(a.as_str(), |s| s));
            let y = if y > 1000 { y } else { all.iter().filter(|c| c.source == "Open Library" && c.year > 1400).map(|c| c.year).next().unwrap_or(0) };
            if y > 0 && (best.year == 0 || y < best.year) {
                best.year = y;
            }
            // lo que le falte lo ponen las demás fichas del mismo libro
            for c in &all {
                if best.year == 0 {
                    best.year = c.year;
                }
                if best.publisher.is_empty() {
                    best.publisher = c.publisher.clone();
                }
                if best.isbn.is_empty() && c.language == best.language {
                    best.isbn = c.isbn.clone();
                }
                if best.genres.len() < 2 {
                    for g in &c.genres {
                        if !best.genres.contains(g) && best.genres.len() < 4 {
                            best.genres.push(g.clone());
                        }
                    }
                }
                if best.cover.is_none() {
                    best.cover = c.cover.clone();
                }
                if best.work.is_empty() {
                    best.work = c.work.clone();
                }
                if best.description.len() < 60 {
                    best.description = c.description.clone();
                }
            }
            if best.description.is_empty() {
                best.description = meta::work_text(&best.work);
            }
            let made = match &best.cover {
                Some((th, full)) if !has_cover => meta::cover_image(th, full).and_then(covers::make_img),
                _ => None,
            };
            Some(Box::new(move |app: &mut App| app.fill_meta(id, best, made)) as Apply)
        });
    }
    fn meta_tried(&mut self, id: i64) {
        if let Some(b) = self.book_mut(id) {
            b.meta_done = true;
            let b = b.clone();
            self.db.save(&b);
        }
    }
    fn fill_meta(&mut self, id: i64, c: Cand, made: Option<Made>) {
        let Some(b) = self.book_mut(id) else { return };
        let set = |d: &mut String, s: &str| {
            if d.trim().is_empty() && !s.is_empty() {
                *d = s.to_string();
            }
        };
        set(&mut b.subtitle, &c.subtitle);
        set(&mut b.authors, &c.authors.join("; "));
        set(&mut b.publisher, &c.publisher);
        set(&mut b.language, &c.language);
        set(&mut b.isbn, &c.isbn);
        set(&mut b.genres, &c.genres.join("; "));
        if b.description.len() < 60 && c.description.len() > b.description.len() {
            b.description = c.description.clone();
        }
        if b.year == 0 {
            b.year = c.year;
        }
        if b.pages == 0 {
            b.pages = c.pages;
        }
        b.meta_done = true;
        if b.cover_ver == 0 {
            if let Some(m) = &made {
                write_cover(b, m);
            }
        }
        let b = b.clone();
        self.db.save(&b);
        self.home.dirty = true;
    }
    pub fn set_cover(&mut self, id: i64, m: &Made, custom: bool) {
        if let Some(b) = self.book_mut(id) {
            write_cover(b, m);
            b.cover_custom = custom;
            let b = b.clone();
            self.db.save(&b);
            self.covers.forget(&mut self.gpu, id);
            self.ui.redraw = true;
        }
    }
}
pub fn write_cover(b: &mut Book, m: &Made) {
    if std::fs::write(cover_path(b.id), &m.jpg).is_ok() {
        let _ = std::fs::write(thumb_path(b.id), &m.thumb);
        b.cover_ver += 1;
        b.color = m.color;
        b.aspect = m.aspect;
    }
}
