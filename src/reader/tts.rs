//! Lectura en voz alta con las voces de Windows, frase a frase.
use super::*;
use crossbeam_channel::{unbounded, Sender};
use windows::core::HSTRING;
use windows::Foundation::TypedEventHandler;
use windows::Media::Core::MediaSource;
use windows::Media::Playback::MediaPlayer;
use windows::Media::SpeechSynthesis::SpeechSynthesizer;

enum Cmd {
    Say(u64, String),
    Ended,
    Pause(bool),
    Rate(f32),
    Stop,
}
pub struct Tts {
    tx: Sender<Cmd>,
    pub paused: bool,
    /// Frases enviadas y aún sin terminar.
    queue: std::collections::VecDeque<(u64, i64, i64)>,
    /// Por dónde se sigue leyendo.
    next: i64,
    n: u64,
    rate: f32,
    voice: String,
}
impl Tts {
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}
impl Drop for Tts {
    fn drop(&mut self) {
        self.stop();
    }
}
/// Nombre e idioma de cada voz instalada.
pub fn voices() -> Vec<(String, String)> {
    static V: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let Ok(all) = SpeechSynthesizer::AllVoices() else { return vec![] };
        all.into_iter().filter_map(|v| Some((v.DisplayName().ok()?.to_string(), v.Language().ok()?.to_string()))).collect()
    })
    .clone()
}
fn run(rx: crossbeam_channel::Receiver<Cmd>, tx: Sender<Cmd>, jobs: crate::jobs::Jobs, book: i64, voice: String, lang: String, rate: f32) -> windows::core::Result<()> {
    let syn = SpeechSynthesizer::new()?;
    if let Ok(all) = SpeechSynthesizer::AllVoices() {
        let vs: Vec<_> = all.into_iter().collect();
        let by_name = vs.iter().find(|v| !voice.is_empty() && v.DisplayName().is_ok_and(|n| n.to_string() == voice));
        let l2: String = lang.chars().take(2).collect::<String>().to_lowercase();
        let by_lang = vs.iter().find(|v| l2.len() == 2 && v.Language().is_ok_and(|l| l.to_string().to_lowercase().starts_with(&l2)));
        if let Some(v) = by_name.or(by_lang) {
            let _ = syn.SetVoice(v);
        }
    }
    let _ = syn.Options().and_then(|o| o.SetSpeakingRate(rate as f64));
    let player = MediaPlayer::new()?;
    if std::env::var_os("SAGA_MUTE").is_some() {
        player.SetVolume(0.0)?;
    }
    let t2 = tx.clone();
    player.MediaEnded(&TypedEventHandler::new(move |_, _| {
        let _ = t2.send(Cmd::Ended);
        Ok(())
    }))?;
    let mut ready: std::collections::VecDeque<(u64, MediaSource)> = Default::default();
    let mut playing: Option<u64> = None;
    let make = |t: &str| -> windows::core::Result<MediaSource> {
        let s = syn.SynthesizeTextToStreamAsync(&HSTRING::from(t))?.join()?;
        MediaSource::CreateFromStream(&s, &s.ContentType()?)
    };
    let tell = |id: u64, done: bool| {
        jobs.send(Box::new(move |a: &mut App| {
            if let Some(r) = a.reader.as_mut().filter(|r| r.id == book) {
                r.tts_event(id, done);
            }
        }));
    };
    loop {
        let Ok(c) = rx.recv() else { break };
        match c {
            Cmd::Say(id, t) => match make(&t) {
                Ok(m) => ready.push_back((id, m)),
                Err(_) => tell(id, true),
            },
            Cmd::Ended => {
                if let Some(id) = playing.take() {
                    tell(id, true);
                }
            }
            Cmd::Pause(p) => {
                let _ = if p { player.Pause() } else { player.Play() };
            }
            Cmd::Rate(r) => {
                let _ = syn.Options().and_then(|o| o.SetSpeakingRate(r as f64));
            }
            Cmd::Stop => break,
        }
        if playing.is_none() {
            if let Some((id, m)) = ready.pop_front() {
                player.SetSource(&m)?;
                player.Play()?;
                playing = Some(id);
                tell(id, false);
            }
        }
    }
    let _ = player.Pause();
    let _ = player.Close();
    Ok(())
}
/// Fin de la frase que empieza en `s`.
fn sentence_end(s: &str) -> usize {
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if matches!(c, '.' | '!' | '?' | '…' | ';' | ':') {
            // cierra también comillas y paréntesis
            let mut e = i + c.len_utf8();
            while let Some((j, d)) = it.peek().copied() {
                if matches!(d, '.' | '!' | '?' | '…' | '»' | '"' | '”' | ')' | '\'' | '’') {
                    e = j + d.len_utf8();
                    it.next();
                } else {
                    break;
                }
            }
            let space = it.peek().is_none_or(|(_, d)| d.is_whitespace());
            if space && e > 24 {
                return e;
            }
        }
        if i > 420 && c == ' ' {
            return i;
        }
    }
    s.len()
}

impl Reader {
    pub(super) fn tts_from(&mut self, app: &mut App, loc: i64) {
        self.tts = None;
        let (tx, rx) = unbounded();
        let lang = match &self.content {
            Content::Flow(v) if !v.doc.lang.is_empty() => v.doc.lang.clone(),
            _ => app.book(self.id).map(|b| b.language.clone()).filter(|l| !l.is_empty()).unwrap_or("es".into()),
        };
        let (t2, j, id, rate, voice) = (tx.clone(), app.jobs.clone(), self.id, app.set.tts_rate.clamp(0.5, 3.0), app.set.tts_voice.clone());
        let v2 = voice.clone();
        app.jobs.thread(move || {
            let r = run(rx, t2, j, id, v2, lang, rate);
            r.err().map(|e| {
                Box::new(move |a: &mut App| {
                    a.ui.toast_err(format!("No se pudo leer en voz alta: {}", e.message()));
                    if let Some(r) = &mut a.reader {
                        r.tts = None;
                        r.speak = None;
                    }
                }) as Apply
            })
        });
        self.tts = Some(Tts { tx, paused: false, queue: Default::default(), next: loc, n: 0, rate, voice });
        self.speak = None;
        self.tts_fill();
        app.ui.redraw = true;
    }
    pub(super) fn tts_stop(&mut self) {
        self.tts = None;
        self.speak = None;
    }
    pub(super) fn tts_pause(&mut self, app: &mut App) {
        if let Some(t) = &mut self.tts {
            t.paused = !t.paused;
            let _ = t.tx.send(Cmd::Pause(t.paused));
            app.ui.redraw = true;
        }
    }
    /// Salta a la frase anterior o siguiente.
    pub(super) fn tts_skip(&mut self, app: &mut App, dir: i32) {
        let Some((a, b)) = self.speak else { return };
        if dir > 0 {
            self.tts_from(app, b);
        } else {
            let mut s = (a - 400).max(0);
            if !self.content.is_flow() {
                s = fixed_loc(loc_page(a), loc_char(a).saturating_sub(400));
            }
            let mut last = s;
            while let Some((x, y, _)) = self.sentence(s) {
                if y >= a {
                    break;
                }
                last = x;
                s = y;
            }
            let last = if self.sentence(last).is_some_and(|z| z.1 < a) { s } else { last };
            let last = if last >= a { (a - 1).max(0) } else { last };
            self.tts_from(app, last);
        }
    }
    /// La frase siguiente a una posición.
    fn sentence(&mut self, from: i64) -> Option<(i64, i64, String)> {
        match &self.content {
            Content::Flow(v) => {
                let mut l = from.max(0) as usize;
                for _ in 0..4000 {
                    if l >= v.doc.len {
                        return None;
                    }
                    let (c, b) = v.doc.block_at(l)?;
                    let ch = &v.doc.chapters[c];
                    let Some(bl) = ch.blocks.get(b) else {
                        l = v.doc.chapters.get(c + 1)?.start;
                        continue;
                    };
                    let o = floor(&bl.text, l.saturating_sub(bl.start));
                    let rest = &bl.text[o..];
                    let trim = rest.len() - rest.trim_start().len();
                    let rest = rest.trim_start();
                    if rest.is_empty() {
                        l = match ch.blocks.get(b + 1) {
                            Some(n) => n.start.max(l + 1),
                            None => v.doc.chapters.get(c + 1)?.start.max(l + 1),
                        };
                        continue;
                    }
                    let e = sentence_end(rest);
                    let a = bl.start + o + trim;
                    return Some((a as i64, (a + e) as i64, rest[..e].replace('\u{ad}', "").replace('\n', " ")));
                }
                None
            }
            Content::Fixed(v) => {
                let (mut p, mut k) = (loc_page(from), loc_char(from));
                while p < v.sizes.len() {
                    let cs = match v.chars.get(&p) {
                        Some(c) => c.clone(),
                        None => std::sync::Arc::new(v.doc.text(p)),
                    };
                    while k < cs.len() && cs[k].c.is_whitespace() {
                        k += 1;
                    }
                    if k < cs.len() {
                        let s: String = cs[k..].iter().map(|c| c.c).collect();
                        let e = sentence_end(&s);
                        let n = s[..e].chars().count().max(1);
                        return Some((fixed_loc(p, k), fixed_loc(p, k + n), join(&cs[k..k + n])));
                    }
                    p += 1;
                    k = 0;
                }
                None
            }
        }
    }
    fn tts_fill(&mut self) {
        loop {
            let Some(t) = &self.tts else { return };
            if t.queue.len() >= 2 {
                return;
            }
            let from = t.next;
            let s = self.sentence(from);
            let Some(t) = &mut self.tts else { return };
            match s {
                Some((a, b, text)) => {
                    t.n += 1;
                    t.next = b.max(a + 1);
                    if text.chars().any(|c| c.is_alphanumeric()) {
                        t.queue.push_back((t.n, a, b));
                        let _ = t.tx.send(Cmd::Say(t.n, text));
                    }
                }
                None => return,
            }
        }
    }
    pub fn tts_event(&mut self, n: u64, done: bool) {
        let Some(t) = &mut self.tts else { return };
        if done {
            t.queue.retain(|q| q.0 != n);
            self.tts_fill();
            if self.tts.as_ref().is_some_and(|t| t.queue.is_empty()) {
                self.tts_stop();
            }
        } else if let Some(q) = t.queue.iter().find(|q| q.0 == n) {
            self.speak = Some((q.1, q.2));
        }
    }
    /// La página sigue a la voz.
    pub(super) fn tts_frame(&mut self, app: &mut App) {
        let Some(t) = &mut self.tts else { return };
        if (t.rate - app.set.tts_rate).abs() > 0.02 && !app.ui.input.down {
            t.rate = app.set.tts_rate;
            let _ = t.tx.send(Cmd::Rate(t.rate.clamp(0.5, 3.0)));
        }
        if t.voice != app.set.tts_voice {
            let a = self.speak.map_or(self.loc, |s| s.0);
            self.tts_from(app, a);
            return;
        }
        let Some((a, _)) = self.speak else { return };
        if self.turning.is_some() || self.press.is_some() {
            return;
        }
        match self.mode_seen {
            Some(ReadMode::ScrollV) => {
                let mut at: Option<Rect> = None;
                for p in &self.placed {
                    if let Some(r) = content::rects(p, &self.content, a, a + 1).first() {
                        at = Some(*r);
                    }
                }
                match at {
                    Some(r) if r.y > app.ui.h * 0.66 || r.y < 60.0 => {
                        if (self.sy.x - self.sy_t).abs() < 1.0 {
                            self.sy_t += r.y - app.ui.h * 0.30;
                            app.ui.redraw = true;
                        }
                    }
                    None if !self.placed.is_empty() && !self.on_screen(a) && (self.sy.x - self.sy_t).abs() < 1.0 => self.seek(a),
                    _ => {}
                }
            }
            _ => {
                if !self.placed.is_empty() && !self.on_screen(a) {
                    let (s, e) = self.visible();
                    if a >= e && a < e + 4000 {
                        self.turn(app, 1);
                    } else if a < s || a >= e {
                        self.seek(a);
                        app.ui.redraw = true;
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::sentence_end;
    #[test]
    fn sentences() {
        let s = "Había una vez un faro muy alto. Luego se apagó.";
        assert_eq!(&s[..sentence_end(s)], "Había una vez un faro muy alto.");
        assert_eq!(sentence_end("sin punto"), 9);
    }
}
