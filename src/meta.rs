//! Metadatos y portadas en línea: Google Books, Open Library y Apple Books.
//! Solo se envía el título, el autor o el ISBN que se busca.
use crate::formats::html::plain;
use crate::formats::{flip_author, isbn_of, year_of};
use serde_json::Value;
use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct Cand {
    pub source: &'static str,
    pub title: String,
    pub subtitle: String,
    pub authors: Vec<String>,
    pub publisher: String,
    pub year: i32,
    pub pages: i32,
    pub language: String,
    pub isbn: String,
    pub description: String,
    pub genres: Vec<String>,
    /// Miniatura y versión grande.
    pub cover: Option<(String, String)>,
    /// Para pedir la sinopsis más tarde (Open Library).
    pub work: String,
    pub score: f32,
}

fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(14))).user_agent("Saga/1.0 (desktop book reader)").http_status_as_error(true).build().into())
}
fn json(url: &str) -> Option<Value> {
    let mut r = agent().get(url).call().ok()?;
    let mut v = vec![];
    r.body_mut().as_reader().take(8 << 20).read_to_end(&mut v).ok()?;
    serde_json::from_slice(&v).ok()
}
pub fn fetch(url: &str) -> Option<Vec<u8>> {
    let mut r = agent().get(url).call().ok()?;
    let mut v = vec![];
    r.body_mut().as_reader().take(24 << 20).read_to_end(&mut v).ok()?;
    Some(v)
}
fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}
fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").trim().to_string()
}
fn list(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(s).filter(|x| !x.is_empty()).collect()).unwrap_or_default()
}
/// Minúsculas, sin tildes ni signos.
pub fn norm(t: &str) -> String {
    let mut o = String::new();
    for c in t.to_lowercase().chars() {
        let c = match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            c => c,
        };
        if c.is_alphanumeric() {
            o.push(c);
        } else if !o.ends_with(' ') {
            o.push(' ');
        }
    }
    o.trim().to_string()
}
fn sim(a: &str, b: &str) -> f32 {
    let (a, b) = (norm(a), norm(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let wa: Vec<&str> = a.split(' ').collect();
    let wb: Vec<&str> = b.split(' ').collect();
    let hit = wa.iter().filter(|w| wb.contains(w)).count() as f32;
    2.0 * hit / (wa.len() + wb.len()) as f32
}
fn score(c: &Cand, title: &str, author: &str, isbn: &str) -> f32 {
    if !isbn.is_empty() && c.isbn == isbn {
        return 2.0;
    }
    let full = format!("{} {}", c.title, c.subtitle);
    let short = title.split([':', '.', '(']).next().unwrap_or(title);
    let t = sim(&c.title, title).max(sim(&full, title)).max(sim(&c.title, short) * 0.95);
    let a = if author.is_empty() { 0.5 } else { c.authors.iter().map(|x| sim(x, author)).fold(0.0, f32::max) };
    t * 0.65 + a * 0.35 + if c.cover.is_some() { 0.04 } else { 0.0 } + if c.description.len() > 80 { 0.03 } else { 0.0 } + if c.language == "es" { 0.02 } else { 0.0 }
}

fn google(q: &str) -> Vec<Cand> {
    let Some(j) = json(&format!("https://www.googleapis.com/books/v1/volumes?q={q}&maxResults=14&printType=books")) else { return vec![] };
    let mut out = vec![];
    for it in j["items"].as_array().into_iter().flatten() {
        let v = &it["volumeInfo"];
        let vid = s(&it["id"]);
        let mut isbn = String::new();
        for i in v["industryIdentifiers"].as_array().into_iter().flatten() {
            if s(&i["type"]) == "ISBN_13" || isbn.is_empty() {
                isbn = isbn_of(&s(&i["identifier"]));
            }
        }
        let thumb = s(&v["imageLinks"]["thumbnail"]).replace("http://", "https://").replace("&edge=curl", "");
        let cover = (!thumb.is_empty()).then(|| (thumb.clone(), format!("https://books.google.com/books/publisher/content/images/frontcover/{vid}?fife=w900-h1400&source=gbs_api")));
        out.push(Cand {
            source: "Google Books",
            title: s(&v["title"]),
            subtitle: s(&v["subtitle"]),
            authors: list(&v["authors"]),
            publisher: s(&v["publisher"]),
            year: year_of(&s(&v["publishedDate"])),
            pages: v["pageCount"].as_i64().unwrap_or(0) as i32,
            language: s(&v["language"]).chars().take(2).collect(),
            isbn,
            description: plain(&s(&v["description"])),
            genres: genres(&list(&v["categories"])),
            cover,
            ..Default::default()
        });
    }
    out
}
/// Año de la obra original: la ficha con más ediciones que casa con título y autor.
pub fn first_year(title: &str, author: &str) -> i32 {
    if author.trim().is_empty() {
        return 0;
    }
    let Some(j) = json(&format!("https://openlibrary.org/search.json?q={}&sort=editions&limit=5&fields=first_publish_year,edition_count,author_name", enc(&format!("{title} {author}")))) else { return 0 };
    // el apellido basta para casar «Fiódor Dostoyevski» con «Fyodor Dostoyevsky»;
    // sin él, una obra más reeditada de otro autor se colaría (Drácula → 1818)
    let key = |a: &str| norm(a).rsplit(' ').next().unwrap_or("").chars().take(5).collect::<String>();
    let mine = key(author);
    let Some(d) = j["docs"].as_array().into_iter().flatten().find(|d| list(&d["author_name"]).iter().any(|a| key(a) == mine)) else { return 0 };
    if mine.is_empty() || d["edition_count"].as_i64().unwrap_or(0) < 8 {
        return 0;
    }
    d["first_publish_year"].as_i64().unwrap_or(0) as i32
}
fn openlib(q: &str) -> Vec<Cand> {
    let Some(j) = json(&format!("https://openlibrary.org/search.json?{q}&limit=14&fields=key,title,subtitle,author_name,first_publish_year,number_of_pages_median,publisher,language,isbn,cover_i,subject")) else { return vec![] };
    let mut out = vec![];
    for d in j["docs"].as_array().into_iter().flatten() {
        let langs = list(&d["language"]);
        let lang = if langs.iter().any(|l| l == "spa") { "es".to_string() } else { langs.first().map(|l| iso2(l)).unwrap_or_default() };
        let cover = d["cover_i"].as_i64().filter(|c| *c > 0).map(|c| (format!("https://covers.openlibrary.org/b/id/{c}-M.jpg"), format!("https://covers.openlibrary.org/b/id/{c}-L.jpg")));
        out.push(Cand {
            source: "Open Library",
            title: s(&d["title"]),
            subtitle: s(&d["subtitle"]),
            authors: list(&d["author_name"]).into_iter().take(3).map(|a| flip_author(&a)).collect(),
            publisher: d["publisher"][0].as_str().unwrap_or("").into(),
            year: d["first_publish_year"].as_i64().unwrap_or(0) as i32,
            pages: d["number_of_pages_median"].as_i64().unwrap_or(0) as i32,
            language: lang,
            isbn: list(&d["isbn"]).iter().map(|i| isbn_of(i)).find(|i| i.len() == 13).unwrap_or_default(),
            genres: genres(&list(&d["subject"])),
            cover,
            work: s(&d["key"]),
            ..Default::default()
        });
    }
    out
}
fn iso2(l: &str) -> String {
    match l {
        "spa" => "es",
        "eng" => "en",
        "fre" | "fra" => "fr",
        "ger" | "deu" => "de",
        "ita" => "it",
        "por" => "pt",
        "cat" => "ca",
        "jpn" => "ja",
        "rus" => "ru",
        "lat" => "la",
        x => return x.chars().take(2).collect(),
    }
    .into()
}
fn apple(term: &str) -> Vec<Cand> {
    let Some(j) = json(&format!("https://itunes.apple.com/search?term={term}&media=ebook&entity=ebook&country=es&limit=12")) else { return vec![] };
    let mut out = vec![];
    for d in j["results"].as_array().into_iter().flatten() {
        let art = s(&d["artworkUrl100"]);
        let cover = art.rsplit_once('/').map(|(b, _)| (format!("{b}/400x400bb.jpg"), format!("{b}/1200x1800bb.jpg")));
        out.push(Cand { source: "Apple Books", title: s(&d["trackName"]), authors: s(&d["artistName"]).split(" & ").map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect(), year: year_of(&s(&d["releaseDate"])), description: plain(&s(&d["description"])), genres: genres(&list(&d["genres"])), cover, ..Default::default() });
    }
    out
}
/// Sinopsis de Open Library, que va aparte.
pub fn work_text(key: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    let Some(j) = json(&format!("https://openlibrary.org{key}.json")) else { return String::new() };
    let d = &j["description"];
    let t = if d.is_string() { s(d) } else { s(&d["value"]) };
    t.split("----").next().unwrap_or("").split("([source]").next().unwrap_or("").trim().to_string()
}

pub fn search(title: &str, author: &str, isbn: &str) -> Vec<Cand> {
    let isbn = isbn_of(isbn);
    let a1 = author.split(';').next().unwrap_or("").trim().to_string();
    let t = title.trim().to_string();
    let (gq, oq) = if t.is_empty() && !isbn.is_empty() {
        (format!("isbn:{isbn}"), format!("isbn={isbn}"))
    } else {
        let g = if a1.is_empty() { enc(&t) } else { format!("intitle:{}+inauthor:{}", enc(&t), enc(&a1)) };
        let o = if a1.is_empty() { format!("q={}", enc(&t)) } else { format!("title={}&author={}", enc(&t), enc(&a1)) };
        (g, o)
    };
    let term = enc(&format!("{t} {a1}"));
    let by_isbn = (!isbn.is_empty() && !t.is_empty()).then(|| format!("isbn:{isbn}"));
    let mut all: Vec<Cand> = std::thread::scope(|sc| {
        let h1 = sc.spawn(|| google(&gq));
        let h2 = sc.spawn(|| openlib(&oq));
        let h3 = sc.spawn(|| if t.is_empty() { vec![] } else { apple(&term) });
        let h4 = sc.spawn(|| by_isbn.as_ref().map(|q| google(q)).unwrap_or_default());
        [h1, h2, h3, h4].into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });
    if all.is_empty() && !a1.is_empty() {
        // sin el autor, por si está mal escrito
        all = std::thread::scope(|sc| {
            let h1 = sc.spawn(|| google(&enc(&t)));
            let h2 = sc.spawn(|| openlib(&format!("q={}", enc(&t))));
            [h1, h2].into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        });
    }
    for c in &mut all {
        c.score = score(c, title, &a1, &isbn);
    }
    all.retain(|c| !c.title.is_empty() && (c.score > 0.28 || t.is_empty()));
    all.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut seen: Vec<(String, &str)> = vec![];
    all.retain(|c| {
        let k = (format!("{}|{}|{}", norm(&c.title), c.authors.first().map(|a| norm(a)).unwrap_or_default(), c.year), c.source);
        if seen.contains(&k) {
            return false;
        }
        seen.push(k);
        true
    });
    all.truncate(24);
    all
}
/// Todas las portadas que se encuentren, sin repetir.
pub fn covers(title: &str, author: &str, isbn: &str) -> Vec<(String, String, &'static str)> {
    let mut out: Vec<(String, String, &'static str)> = vec![];
    for c in search(title, author, isbn) {
        if let Some((t, f)) = c.cover {
            if !out.iter().any(|o| o.0 == t) {
                out.push((t, f, c.source));
            }
        }
    }
    let i = isbn_of(isbn);
    if !i.is_empty() {
        out.push((format!("https://covers.openlibrary.org/b/isbn/{i}-M.jpg?default=false"), format!("https://covers.openlibrary.org/b/isbn/{i}-L.jpg?default=false"), "Open Library"));
    }
    out.truncate(30);
    out
}
/// Descarga la mejor versión de una portada.
pub fn cover_image(thumb: &str, full: &str) -> Option<image::RgbaImage> {
    let ok = |i: &image::RgbaImage| {
        let (w, h) = i.dimensions();
        // el «imagen no disponible» de Google es gris y apaisado o diminuto
        w > 90 && h > 120 && (h as f32 / w as f32) > 0.9
    };
    let g = |u: &str| fetch(u).and_then(|b| crate::formats::decode(&b)).filter(ok);
    g(full).or_else(|| g(thumb))
}

/// Los géneros más comunes, en español y sin repetir.
pub fn genres(v: &[String]) -> Vec<String> {
    const T: &[(&str, &str)] = &[
        ("science fiction", "Ciencia ficción"),
        ("ciencia ficcion", "Ciencia ficción"),
        ("sci-fi", "Ciencia ficción"),
        ("fantasy", "Fantasía"),
        ("fantasia", "Fantasía"),
        ("horror", "Terror"),
        ("terror", "Terror"),
        ("thriller", "Suspenso"),
        ("suspense", "Suspenso"),
        ("suspenso", "Suspenso"),
        ("mystery", "Misterio"),
        ("misterio", "Misterio"),
        ("detective", "Misterio"),
        ("policiac", "Policíaca"),
        ("crime", "Policíaca"),
        ("romance", "Romance"),
        ("love stories", "Romance"),
        ("romantica", "Romance"),
        ("historical fiction", "Novela histórica"),
        ("novela historica", "Novela histórica"),
        ("young adult", "Juvenil"),
        ("juvenile", "Juvenil"),
        ("juvenil", "Juvenil"),
        ("children", "Infantil"),
        ("infantil", "Infantil"),
        ("poetry", "Poesía"),
        ("poesia", "Poesía"),
        ("drama", "Teatro"),
        ("teatro", "Teatro"),
        ("biograph", "Biografía"),
        ("biografia", "Biografía"),
        ("memoir", "Biografía"),
        ("philosophy", "Filosofía"),
        ("filosofia", "Filosofía"),
        ("psycholog", "Psicología"),
        ("psicolog", "Psicología"),
        ("self-help", "Autoayuda"),
        ("autoayuda", "Autoayuda"),
        ("business", "Negocios"),
        ("negocios", "Negocios"),
        ("econom", "Economía"),
        ("computer", "Informática"),
        ("informatica", "Informática"),
        ("programming", "Informática"),
        ("technology", "Tecnología"),
        ("engineering", "Ingeniería"),
        ("ingenieria", "Ingeniería"),
        ("mathematic", "Matemáticas"),
        ("matematic", "Matemáticas"),
        ("physics", "Física"),
        ("fisica", "Física"),
        ("chemistry", "Química"),
        ("medic", "Medicina"),
        ("health", "Salud"),
        ("salud", "Salud"),
        ("religio", "Religión"),
        ("history", "Historia"),
        ("historia", "Historia"),
        ("politic", "Política"),
        ("law", "Derecho"),
        ("derecho", "Derecho"),
        ("art ", "Arte"),
        ("arte", "Arte"),
        ("music", "Música"),
        ("cook", "Cocina"),
        ("cocina", "Cocina"),
        ("travel", "Viajes"),
        ("viajes", "Viajes"),
        ("comics", "Cómic"),
        ("comic", "Cómic"),
        ("graphic novel", "Cómic"),
        ("manga", "Manga"),
        ("adventure", "Aventuras"),
        ("aventura", "Aventuras"),
        ("education", "Educación"),
        ("educacion", "Educación"),
        ("science", "Ciencia"),
        ("ciencia", "Ciencia"),
        ("classic", "Clásicos"),
        ("clasico", "Clásicos"),
        ("essay", "Ensayo"),
        ("ensayo", "Ensayo"),
        ("short stories", "Cuentos"),
        ("cuento", "Cuentos"),
        ("fiction", "Ficción"),
        ("ficcion", "Ficción"),
        ("novel", "Novela"),
    ];
    let mut out: Vec<String> = vec![];
    for g in v {
        for part in g.split(['/', ',', ';']) {
            let n = format!("{} ", norm(part));
            if n.trim().is_empty() || n.contains("libros") && !n.contains("comic") {
                continue;
            }
            if let Some((_, es)) = T.iter().find(|(k, _)| n.contains(k)) {
                if !out.iter().any(|o| o == es) {
                    out.push(es.to_string());
                }
            }
        }
        if out.len() >= 4 {
            break;
        }
    }
    // «Ficción» sobra si ya hay algo más concreto
    if out.len() > 1 {
        out.retain(|g| g != "Ficción");
    }
    out.truncate(4);
    out
}

pub fn lang_name(code: &str) -> String {
    match code {
        "es" => "Español",
        "en" => "Inglés",
        "fr" => "Francés",
        "de" => "Alemán",
        "it" => "Italiano",
        "pt" => "Portugués",
        "ca" => "Catalán",
        "gl" => "Gallego",
        "eu" => "Euskera",
        "ja" => "Japonés",
        "zh" => "Chino",
        "ko" => "Coreano",
        "ru" => "Ruso",
        "la" => "Latín",
        "" => "",
        x => return x.to_uppercase(),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoring() {
        let c = Cand { title: "Cien años de soledad".into(), authors: vec!["Gabriel García Márquez".into()], ..Default::default() };
        assert!(score(&c, "cien anos de soledad", "Gabriel Garcia Marquez", "") > 0.95);
        assert!(score(&c, "El amor en los tiempos del cólera", "Otra persona", "") < 0.4);
        assert_eq!(genres(&["Fiction / Science Fiction / General".into()]), vec!["Ciencia ficción"]);
    }
}
