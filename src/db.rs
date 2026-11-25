//! Biblioteca en SQLite: libros, notas, colecciones, sesiones y ajustes.
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Status {
    ToRead,
    Reading,
    Done,
    Paused,
}
impl Status {
    pub const ALL: [(Status, &'static str); 4] = [(Status::ToRead, "Por leer"), (Status::Reading, "Leyendo"), (Status::Done, "Terminado"), (Status::Paused, "En pausa")];
    pub fn name(self) -> &'static str {
        Self::ALL[self as usize].1
    }
    fn from(i: i64) -> Status {
        Self::ALL[(i as usize).min(3)].0
    }
}

#[derive(Clone, Debug)]
pub struct Book {
    pub id: i64,
    pub path: String,
    pub format: String,
    pub title: String,
    pub subtitle: String,
    pub authors: String,
    pub series: String,
    pub series_ix: f32,
    pub publisher: String,
    pub year: i32,
    pub pages: i32,
    pub language: String,
    pub isbn: String,
    pub description: String,
    pub genres: String,
    pub rating: i32,
    pub status: Status,
    pub favorite: bool,
    pub added: i64,
    pub opened: i64,
    pub finished: i64,
    pub progress: f32,
    pub loc: i64,
    /// Sube con cada portada nueva; 0 = sin portada.
    pub cover_ver: i32,
    pub cover_custom: bool,
    pub color: u32,
    pub aspect: f32,
    pub secs: i64,
    pub meta_done: bool,
    pub missing: bool,
}
impl Default for Book {
    fn default() -> Self {
        Book { id: 0, path: String::new(), format: String::new(), title: String::new(), subtitle: String::new(), authors: String::new(), series: String::new(), series_ix: 0.0, publisher: String::new(), year: 0, pages: 0, language: String::new(), isbn: String::new(), description: String::new(), genres: String::new(), rating: 0, status: Status::ToRead, favorite: false, added: 0, opened: 0, finished: 0, progress: 0.0, loc: 0, cover_ver: 0, cover_custom: false, color: 0x33424a, aspect: 0.667, secs: 0, meta_done: false, missing: false }
    }
}
impl Book {
    pub fn author_list(&self) -> Vec<&str> {
        self.authors.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()).collect()
    }
    pub fn author_line(&self) -> String {
        let l = self.author_list();
        match l.len() {
            0 => "Autor desconocido".into(),
            1 | 2 => l.join(" y "),
            _ => format!("{} y otros", l[0]),
        }
    }
    pub fn genre_list(&self) -> Vec<&str> {
        self.genres.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()).collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Highlight = 0,
    Underline = 1,
    Postit = 2,
    Bookmark = 3,
}
#[derive(Clone, Debug)]
pub struct Note {
    pub id: i64,
    pub book: i64,
    pub kind: Kind,
    pub a: i64,
    pub b: i64,
    pub color: u8,
    /// Texto subrayado o contenido del posit.
    pub text: String,
    pub note: String,
    /// Posits: posición relativa en la página y giro.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub created: i64,
}

#[derive(Clone, Debug)]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub books: Vec<i64>,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub book: i64,
    pub start: i64,
    pub secs: i64,
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum ReadMode {
    ScrollV,
    ScrollH,
    Single,
    Double,
}
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum Turn {
    Curl,
    Slide,
    Fade,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: usize,
    pub view: usize,
    pub sort: usize,
    pub group: usize,
    pub grid_size: f32,
    pub calm: bool,
    pub auto_meta: bool,
    pub high_power: bool,
    pub win: Option<(i32, i32, u32, u32, bool)>,
    // lector
    pub paper: usize,
    pub family: usize,
    pub size: f32,
    pub leading: f32,
    pub margin: f32,
    pub para: f32,
    pub justify: bool,
    pub hyphen: bool,
    pub mode: ReadMode,
    pub fixed_mode: ReadMode,
    pub turn: Turn,
    pub brightness: f32,
    pub warmth: f32,
    /// Teñir las páginas fijas (PDF, cómic) con el papel elegido.
    pub recolor: bool,
    pub recolor_comics: bool,
    pub auto_hide: bool,
    pub footer: bool,
    pub tts_rate: f32,
    pub tts_voice: String,
    pub goal_min: i32,
    pub auto_scroll: f32,
    pub mark_color: u8,
    pub note_color: u8,
    pub ruler: bool,
    pub folders: Vec<String>,
}
impl Default for Settings {
    fn default() -> Self {
        Settings { theme: 0, view: 0, sort: 0, group: 0, grid_size: 168.0, calm: false, auto_meta: true, high_power: false, win: None, paper: 0, family: 0, size: 20.0, leading: 1.55, margin: 0.5, para: 0.25, justify: true, hyphen: true, mode: ReadMode::Double, fixed_mode: ReadMode::ScrollV, turn: Turn::Curl, brightness: 1.0, warmth: 0.0, recolor: true, recolor_comics: false, auto_hide: true, footer: true, tts_rate: 1.0, tts_voice: String::new(), goal_min: 20, auto_scroll: 0.35, mark_color: 0, note_color: 0, ruler: false, folders: vec![] }
    }
}

pub struct Db {
    c: Connection,
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

pub fn data_dir() -> PathBuf {
    let p = match std::env::var("SAGA_DATA") {
        Ok(p) => PathBuf::from(p),
        Err(_) => PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or(".".into())).join("Saga"),
    };
    let _ = std::fs::create_dir_all(p.join("covers"));
    p
}
pub fn cover_path(id: i64) -> PathBuf {
    data_dir().join("covers").join(format!("{id}.jpg"))
}
pub fn thumb_path(id: i64) -> PathBuf {
    data_dir().join("covers").join(format!("{id}.t.jpg"))
}

const COLS: &str = "id,path,format,title,subtitle,authors,series,series_ix,publisher,year,pages,language,isbn,description,genres,rating,status,favorite,added,opened,finished,progress,loc,cover_ver,cover_custom,color,aspect,secs,meta_done";

fn book_row(r: &Row) -> rusqlite::Result<Book> {
    Ok(Book {
        id: r.get(0)?,
        path: r.get(1)?,
        format: r.get(2)?,
        title: r.get(3)?,
        subtitle: r.get(4)?,
        authors: r.get(5)?,
        series: r.get(6)?,
        series_ix: r.get::<_, f64>(7)? as f32,
        publisher: r.get(8)?,
        year: r.get(9)?,
        pages: r.get(10)?,
        language: r.get(11)?,
        isbn: r.get(12)?,
        description: r.get(13)?,
        genres: r.get(14)?,
        rating: r.get(15)?,
        status: Status::from(r.get(16)?),
        favorite: r.get(17)?,
        added: r.get(18)?,
        opened: r.get(19)?,
        finished: r.get(20)?,
        progress: r.get::<_, f64>(21)? as f32,
        loc: r.get(22)?,
        cover_ver: r.get(23)?,
        cover_custom: r.get(24)?,
        color: r.get(25)?,
        aspect: r.get::<_, f64>(26)? as f32,
        secs: r.get(27)?,
        meta_done: r.get(28)?,
        missing: false,
    })
}

impl Db {
    pub fn open() -> Db {
        Self::open_at(&data_dir().join("saga.db"))
    }
    pub fn open_at(p: &Path) -> Db {
        let c = Connection::open(p).expect("no se pudo abrir la biblioteca");
        let _ = c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;");
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS books(id INTEGER PRIMARY KEY AUTOINCREMENT, path TEXT UNIQUE NOT NULL, format TEXT NOT NULL, title TEXT NOT NULL, subtitle TEXT NOT NULL DEFAULT '', authors TEXT NOT NULL DEFAULT '', series TEXT NOT NULL DEFAULT '', series_ix REAL NOT NULL DEFAULT 0, publisher TEXT NOT NULL DEFAULT '', year INTEGER NOT NULL DEFAULT 0, pages INTEGER NOT NULL DEFAULT 0, language TEXT NOT NULL DEFAULT '', isbn TEXT NOT NULL DEFAULT '', description TEXT NOT NULL DEFAULT '', genres TEXT NOT NULL DEFAULT '', rating INTEGER NOT NULL DEFAULT 0, status INTEGER NOT NULL DEFAULT 0, favorite INTEGER NOT NULL DEFAULT 0, added INTEGER NOT NULL DEFAULT 0, opened INTEGER NOT NULL DEFAULT 0, finished INTEGER NOT NULL DEFAULT 0, progress REAL NOT NULL DEFAULT 0, loc INTEGER NOT NULL DEFAULT 0, cover_ver INTEGER NOT NULL DEFAULT 0, cover_custom INTEGER NOT NULL DEFAULT 0, color INTEGER NOT NULL DEFAULT 3359306, aspect REAL NOT NULL DEFAULT 0.667, secs INTEGER NOT NULL DEFAULT 0, meta_done INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS notes(id INTEGER PRIMARY KEY AUTOINCREMENT, book INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE, kind INTEGER NOT NULL, a INTEGER NOT NULL, b INTEGER NOT NULL, color INTEGER NOT NULL, text TEXT NOT NULL, note TEXT NOT NULL, x REAL NOT NULL, y REAL NOT NULL, w REAL NOT NULL, h REAL NOT NULL, created INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS notes_book ON notes(book);
             CREATE TABLE IF NOT EXISTS collections(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS coll_books(coll INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE, book INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE, PRIMARY KEY(coll, book));
             CREATE TABLE IF NOT EXISTS sessions(book INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE, start INTEGER NOT NULL, secs INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS kv(k TEXT PRIMARY KEY, v TEXT NOT NULL);",
        )
        .expect("esquema");
        Db { c }
    }
    pub fn books(&self) -> Vec<Book> {
        let mut st = self.c.prepare(&format!("SELECT {COLS} FROM books")).unwrap();
        let mut v: Vec<Book> = st.query_map([], book_row).unwrap().flatten().collect();
        for b in &mut v {
            b.missing = !Path::new(&b.path).exists();
        }
        v
    }
    pub fn has_path(&self, p: &str) -> Option<i64> {
        self.c.query_row("SELECT id FROM books WHERE path=?1", [p], |r| r.get(0)).ok()
    }
    pub fn insert(&self, b: &mut Book) -> rusqlite::Result<i64> {
        b.added = now();
        self.c.execute("INSERT INTO books(path,format,title,added) VALUES(?1,?2,?3,?4)", params![b.path, b.format, b.title, b.added])?;
        b.id = self.c.last_insert_rowid();
        self.save(b);
        Ok(b.id)
    }
    pub fn save(&self, b: &Book) {
        let r = self.c.execute(
            "UPDATE books SET path=?2,format=?3,title=?4,subtitle=?5,authors=?6,series=?7,series_ix=?8,publisher=?9,year=?10,pages=?11,language=?12,isbn=?13,description=?14,genres=?15,rating=?16,status=?17,favorite=?18,added=?19,opened=?20,finished=?21,progress=?22,loc=?23,cover_ver=?24,cover_custom=?25,color=?26,aspect=?27,secs=?28,meta_done=?29 WHERE id=?1",
            params![b.id, b.path, b.format, b.title, b.subtitle, b.authors, b.series, b.series_ix as f64, b.publisher, b.year, b.pages, b.language, b.isbn, b.description, b.genres, b.rating, b.status as i64, b.favorite, b.added, b.opened, b.finished, b.progress as f64, b.loc, b.cover_ver, b.cover_custom, b.color, b.aspect as f64, b.secs, b.meta_done],
        );
        if let Err(e) = r {
            eprintln!("guardar libro: {e}");
        }
    }
    pub fn delete(&self, id: i64) {
        let _ = self.c.execute("DELETE FROM books WHERE id=?1", [id]);
        let _ = std::fs::remove_file(cover_path(id));
        let _ = std::fs::remove_file(thumb_path(id));
    }
    pub fn notes(&self, book: i64) -> Vec<Note> {
        let mut st = self.c.prepare("SELECT id,book,kind,a,b,color,text,note,x,y,w,h,created FROM notes WHERE book=?1 ORDER BY a,id").unwrap();
        let v = st
            .query_map([book], |r| {
                Ok(Note {
                    id: r.get(0)?,
                    book: r.get(1)?,
                    kind: match r.get::<_, i64>(2)? {
                        0 => Kind::Highlight,
                        1 => Kind::Underline,
                        2 => Kind::Postit,
                        _ => Kind::Bookmark,
                    },
                    a: r.get(3)?,
                    b: r.get(4)?,
                    color: r.get(5)?,
                    text: r.get(6)?,
                    note: r.get(7)?,
                    x: r.get::<_, f64>(8)? as f32,
                    y: r.get::<_, f64>(9)? as f32,
                    w: r.get::<_, f64>(10)? as f32,
                    h: r.get::<_, f64>(11)? as f32,
                    created: r.get(12)?,
                })
            })
            .unwrap()
            .flatten()
            .collect();
        v
    }
    pub fn note_counts(&self) -> std::collections::HashMap<i64, usize> {
        let mut st = self.c.prepare("SELECT book,COUNT(*) FROM notes GROUP BY book").unwrap();
        let v = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as usize))).unwrap().flatten().collect();
        v
    }
    pub fn save_note(&self, n: &mut Note) {
        if n.id == 0 {
            n.created = now();
            if self.c.execute("INSERT INTO notes(book,kind,a,b,color,text,note,x,y,w,h,created) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)", params![n.book, n.kind as i64, n.a, n.b, n.color, n.text, n.note, n.x as f64, n.y as f64, n.w as f64, n.h as f64, n.created]).is_ok() {
                n.id = self.c.last_insert_rowid();
            }
        } else {
            let _ = self.c.execute("UPDATE notes SET a=?2,b=?3,color=?4,text=?5,note=?6,x=?7,y=?8,w=?9,h=?10,kind=?11 WHERE id=?1", params![n.id, n.a, n.b, n.color, n.text, n.note, n.x as f64, n.y as f64, n.w as f64, n.h as f64, n.kind as i64]);
        }
    }
    pub fn delete_note(&self, id: i64) {
        let _ = self.c.execute("DELETE FROM notes WHERE id=?1", [id]);
    }
    pub fn collections(&self) -> Vec<Collection> {
        let mut st = self.c.prepare("SELECT id,name FROM collections ORDER BY name COLLATE NOCASE").unwrap();
        let mut v: Vec<Collection> = st.query_map([], |r| Ok(Collection { id: r.get(0)?, name: r.get(1)?, books: vec![] })).unwrap().flatten().collect();
        let mut st = self.c.prepare("SELECT coll,book FROM coll_books").unwrap();
        let links: Vec<(i64, i64)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().flatten().collect();
        for (c, b) in links {
            if let Some(x) = v.iter_mut().find(|x| x.id == c) {
                x.books.push(b);
            }
        }
        v
    }
    pub fn add_collection(&self, name: &str) -> i64 {
        let _ = self.c.execute("INSERT INTO collections(name) VALUES(?1)", [name]);
        self.c.last_insert_rowid()
    }
    pub fn delete_collection(&self, id: i64) {
        let _ = self.c.execute("DELETE FROM collections WHERE id=?1", [id]);
    }
    pub fn set_in_collection(&self, coll: i64, book: i64, on: bool) {
        let q = if on { "INSERT OR IGNORE INTO coll_books VALUES(?1,?2)" } else { "DELETE FROM coll_books WHERE coll=?1 AND book=?2" };
        let _ = self.c.execute(q, [coll, book]);
    }
    pub fn add_session(&self, book: i64, start: i64, secs: i64) {
        let _ = self.c.execute("INSERT INTO sessions VALUES(?1,?2,?3)", [book, start, secs]);
    }
    pub fn sessions(&self) -> Vec<Session> {
        let mut st = self.c.prepare("SELECT book,start,secs FROM sessions ORDER BY start").unwrap();
        let v = st.query_map([], |r| Ok(Session { book: r.get(0)?, start: r.get(1)?, secs: r.get(2)? })).unwrap().flatten().collect();
        v
    }
    pub fn settings(&self) -> Settings {
        self.c.query_row("SELECT v FROM kv WHERE k='settings'", [], |r| r.get::<_, String>(0)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save_settings(&self, s: &Settings) {
        let _ = self.c.execute("INSERT INTO kv VALUES('settings',?1) ON CONFLICT(k) DO UPDATE SET v=?1", [serde_json::to_string(s).unwrap_or_default()]);
    }
    pub fn close(self) {
        let _ = self.c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }
}
