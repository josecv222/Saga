//! Interfaz inmediata: cada fotograma se describe entero y solo se guarda lo que
//! se está animando. Si nada se mueve, no se vuelve a dibujar.
use crate::gfx::*;
use crate::icons::Icon;
use crate::text::{fonts, split_keep_idx, FontId, Text};
use crate::theme::{Theme, THEMES};
use std::collections::HashMap;
use winit::keyboard::{Key, NamedKey};
use winit::window::CursorIcon;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}
#[derive(Clone, Debug)]
pub struct KeyEv {
    pub key: Key,
    pub mods: Mods,
}
#[derive(Default, Clone)]
pub struct Input {
    pub pos: (f32, f32),
    pub down: bool,
    pub pressed: bool,
    pub released: bool,
    pub rpressed: bool,
    pub back: bool,
    pub forward: bool,
    pub clicks: u8,
    pub wheel: (f32, f32),
    pub keys: Vec<KeyEv>,
    pub text: String,
    pub mods: Mods,
    pub moved: bool,
}

pub fn id(s: &str) -> u64 {
    idx(s, 0)
}
pub fn idx(s: &str, i: usize) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325 ^ (i as u64).wrapping_mul(0x9E3779B97F4A7C15);
    for b in s.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    h
}

/// Muelle con amortiguación crítica: integra exacto, no depende de los fps.
#[derive(Clone, Copy, Default, Debug)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
}
impl Spring {
    pub fn at(x: f32) -> Spring {
        Spring { x, v: 0.0 }
    }
    /// Devuelve true si sigue en movimiento.
    pub fn step(&mut self, target: f32, omega: f32, dt: f32, eps: f32) -> bool {
        let d = self.x - target;
        let c = self.v + omega * d;
        let e = (-omega * dt).exp();
        self.x = target + (d + c * dt) * e;
        self.v = (self.v - omega * c * dt) * e;
        if (self.x - target).abs() < eps && self.v.abs() < eps * 8.0 {
            self.x = target;
            self.v = 0.0;
            return false;
        }
        true
    }
}

#[derive(Default, Clone, Debug)]
pub struct Scroll {
    pub s: Spring,
    pub target: f32,
    pub max: f32,
    grab: Option<(f32, f32)>,
}
impl Scroll {
    pub fn pos(&self) -> f32 {
        self.s.x
    }
    pub fn jump(&mut self, y: f32) {
        self.s = Spring::at(y);
        self.target = y;
    }
    pub fn to(&mut self, y: f32) {
        self.target = y;
    }
}

#[derive(Clone, Default, Debug)]
pub struct Edit {
    pub text: String,
    pub caret: usize,
    pub anchor: usize,
    sx: f32,
    pub sy: f32,
    blink: f32,
}
impl Edit {
    pub fn new(s: &str) -> Edit {
        Edit { text: s.into(), caret: s.len(), anchor: s.len(), ..Default::default() }
    }
    pub fn set(&mut self, s: &str) {
        self.text = s.into();
        self.caret = s.len();
        self.anchor = s.len();
        self.sx = 0.0;
    }
    fn sel(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }
    fn replace(&mut self, s: &str) {
        let (a, b) = self.sel();
        self.text.replace_range(a..b, s);
        self.caret = a + s.len();
        self.anchor = self.caret;
    }
    fn prev(&self, i: usize) -> usize {
        self.text[..i].char_indices().next_back().map_or(0, |x| x.0)
    }
    fn next(&self, i: usize) -> usize {
        self.text[i..].chars().next().map_or(i, |c| i + c.len_utf8())
    }
    fn word(&self, i: usize, fwd: bool) -> usize {
        let mut p = i;
        if fwd {
            while p < self.text.len() && self.text[p..].starts_with(char::is_whitespace) {
                p = self.next(p);
            }
            while p < self.text.len() && !self.text[p..].starts_with(char::is_whitespace) {
                p = self.next(p);
            }
        } else {
            while p > 0 && self.text[..p].ends_with(char::is_whitespace) {
                p = self.prev(p);
            }
            while p > 0 && !self.text[..p].ends_with(char::is_whitespace) {
                p = self.prev(p);
            }
        }
        p
    }
}

#[derive(Clone)]
pub struct MenuItem {
    pub icon: Option<Icon>,
    pub label: String,
    pub danger: bool,
    pub checked: bool,
    pub sep: bool,
    pub header: bool,
}
impl MenuItem {
    pub fn new(icon: Icon, label: impl Into<String>) -> MenuItem {
        MenuItem { icon: Some(icon), label: label.into(), danger: false, checked: false, sep: false, header: false }
    }
    pub fn plain(label: impl Into<String>) -> MenuItem {
        MenuItem { icon: None, label: label.into(), danger: false, checked: false, sep: false, header: false }
    }
    pub fn check(label: impl Into<String>, on: bool) -> MenuItem {
        MenuItem { checked: on, ..MenuItem::plain(label) }
    }
    pub fn head(label: impl Into<String>) -> MenuItem {
        MenuItem { header: true, ..MenuItem::plain(label) }
    }
    pub fn sep(mut self) -> MenuItem {
        self.sep = true;
        self
    }
    pub fn danger(mut self) -> MenuItem {
        self.danger = true;
        self
    }
}
struct Menu {
    owner: u64,
    items: Vec<MenuItem>,
    r: Rect,
    t: f32,
    closing: bool,
    scroll: Scroll,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Btn {
    Primary,
    Soft,
    Ghost,
    Danger,
}

struct Toast {
    text: String,
    t: f32,
    err: bool,
}

pub struct Ui {
    pub d: Draw,
    pub t: Text,
    pub th: Theme,
    pub input: Input,
    pub w: f32,
    pub h: f32,
    pub dt: f32,
    pub time: f32,
    pub redraw: bool,
    pub enabled: bool,
    pub active: Option<u64>,
    pub focus: Option<u64>,
    pub cursor: CursorIcon,
    /// Rectángulo del cursor de texto, para el editor de métodos de entrada.
    pub ime: Option<Rect>,
    anims: HashMap<u64, (f32, bool)>,
    menu: Option<Menu>,
    picked: Option<(u64, usize)>,
    toasts: Vec<Toast>,
    tip: Option<(u64, Rect, String)>,
    tip_t: f32,
    pub calm: bool,
    clip_stack: Vec<Rect>,
    /// Despertar dentro de N segundos aunque no haya animación.
    pub wake: f32,
    theme_from: Theme,
    theme_to: usize,
    theme_t: f32,
    pub drag_from: (f32, f32),
    /// Leyendo a pantalla limpia, el cursor también se quita.
    pub hide_cursor: bool,
}

fn mix_theme(a: &Theme, b: &Theme, t: f32) -> Theme {
    Theme { name: b.name, dark: b.dark, bg: a.bg.mix(b.bg, t), panel: a.panel.mix(b.panel, t), raised: a.raised.mix(b.raised, t), line: a.line.mix(b.line, t), text: a.text.mix(b.text, t), text2: a.text2.mix(b.text2, t), text3: a.text3.mix(b.text3, t), accent: a.accent.mix(b.accent, t), on_accent: a.on_accent.mix(b.on_accent, t), glow: lerp(a.glow, b.glow, t) }
}

impl Ui {
    pub fn new(w: f32, h: f32, dpi: f32) -> Ui {
        Ui {
            d: Draw::new(w * dpi, h * dpi, dpi),
            t: Text::new(),
            th: THEMES[0],
            input: Input::default(),
            w,
            h,
            dt: 1.0 / 60.0,
            time: 0.0,
            redraw: false,
            enabled: true,
            active: None,
            focus: None,
            cursor: CursorIcon::Default,
            ime: None,
            anims: HashMap::new(),
            menu: None,
            picked: None,
            toasts: vec![],
            tip: None,
            tip_t: 0.0,
            calm: false,
            clip_stack: vec![],
            wake: f32::MAX,
            theme_from: THEMES[0],
            theme_to: 0,
            theme_t: 1.0,
            drag_from: (0.0, 0.0),
            hide_cursor: false,
        }
    }
    pub fn set_theme(&mut self, i: usize, instant: bool) {
        let i = i.min(THEMES.len() - 1);
        if i == self.theme_to && !instant {
            return;
        }
        self.theme_from = self.th;
        self.theme_to = i;
        self.theme_t = if instant { 1.0 } else { 0.0 };
        if instant {
            self.th = THEMES[i];
        }
        self.redraw = true;
    }
    /// El ambiente de la biblioteca, aunque se esté leyendo con otro papel.
    pub fn base_th(&self) -> Theme {
        if self.theme_t < 1.0 { mix_theme(&self.theme_from, &THEMES[self.theme_to], smooth(self.theme_t)) } else { THEMES[self.theme_to] }
    }
    pub fn begin(&mut self, w: f32, h: f32, dpi: f32, dt: f32) {
        self.w = w;
        self.h = h;
        self.dt = dt;
        self.time += dt;
        self.d.reset(w * dpi, h * dpi, dpi);
        self.redraw = false;
        self.enabled = true;
        self.cursor = CursorIcon::Default;
        self.ime = None;
        self.hide_cursor = false;
        self.wake = f32::MAX;
        self.picked = None;
        self.clip_stack.clear();
        if self.theme_t < 1.0 {
            self.theme_t = (self.theme_t + dt / 0.35).min(1.0);
            self.th = mix_theme(&self.theme_from, &THEMES[self.theme_to], smooth(self.theme_t));
            self.redraw = true;
        }
        if self.input.pressed {
            self.drag_from = self.input.pos;
        }
        for (_, used) in self.anims.values_mut() {
            *used = false;
        }
        self.menu_input();
        self.tip = None;
    }
    pub fn end(&mut self) {
        self.enabled = true;
        if self.menu.is_some() {
            self.draw_menu();
        }
        self.draw_toasts();
        self.draw_tip();
        if self.input.pressed && self.active.is_none() {
            // pulsar en el vacío quita el foco
            self.focus = None;
        }
        if self.input.released || !self.input.down {
            self.active = None;
        }
        if self.anims.len() > 4000 {
            self.anims.retain(|_, v| v.1);
        }
        let i = &mut self.input;
        i.pressed = false;
        i.released = false;
        i.rpressed = false;
        i.back = false;
        i.forward = false;
        i.wheel = (0.0, 0.0);
        i.keys.clear();
        i.text.clear();
        i.moved = false;
    }
    pub fn full(&self) -> Rect {
        rect(0.0, 0.0, self.w, self.h)
    }

    // ------------------------------------------------------------ animación
    pub fn anim(&mut self, id: u64, target: f32, rate: f32) -> f32 {
        let dt = self.dt;
        let e = self.anims.entry(id).or_insert((target, true));
        e.1 = true;
        let k = 1.0 - (-rate * dt).exp();
        e.0 += (target - e.0) * k;
        if (e.0 - target).abs() < 0.002 {
            e.0 = target;
        } else {
            self.redraw = true;
        }
        e.0
    }
    /// Como `anim`, pero la primera vez arranca desde `from`.
    pub fn anim_from(&mut self, id: u64, from: f32, target: f32, rate: f32) -> f32 {
        self.anims.entry(id).or_insert((from, true));
        self.anim(id, target, rate)
    }
    pub fn peek_anim(&self, id: u64) -> f32 {
        self.anims.get(&id).map_or(0.0, |v| v.0)
    }
    pub fn set_anim(&mut self, id: u64, v: f32) {
        self.anims.insert(id, (v, true));
    }
    /// Avanza un progreso lineal 0..1 hacia abierto o cerrado.
    pub fn tween(&mut self, v: &mut f32, open: bool, secs: f32) -> f32 {
        let t = if open { 1.0 } else { 0.0 };
        if *v != t {
            let secs = if self.calm { secs * 0.35 } else { secs };
            let step = self.dt / secs;
            *v = if open { (*v + step).min(1.0) } else { (*v - step).max(0.0) };
            self.redraw = true;
        }
        *v
    }

    // ------------------------------------------------------------ ratón
    pub fn hover(&self, r: Rect) -> bool {
        self.enabled && r.has(self.input.pos) && self.clip_stack.last().is_none_or(|c| c.has(self.input.pos)) && self.active.is_none()
    }
    /// Igual que hover, pero también vale mientras se mantiene pulsado.
    pub fn over(&self, r: Rect) -> bool {
        self.enabled && r.has(self.input.pos) && self.clip_stack.last().is_none_or(|c| c.has(self.input.pos))
    }
    pub fn click(&mut self, id: u64, r: Rect) -> bool {
        if self.hover(r) && self.input.pressed {
            self.active = Some(id);
            self.redraw = true;
        }
        self.active == Some(id) && self.input.released && r.has(self.input.pos)
    }
    pub fn rclick(&mut self, r: Rect) -> bool {
        self.hover(r) && self.input.rpressed
    }
    /// Arrastre: devuelve true mientras este elemento tiene el ratón.
    pub fn drag(&mut self, id: u64, r: Rect) -> bool {
        if self.hover(r) && self.input.pressed {
            self.active = Some(id);
        }
        if self.active == Some(id) {
            self.redraw = true;
            return true;
        }
        false
    }
    pub fn clip(&mut self, r: Rect, rad: f32) {
        self.d.push_clip(r, rad);
        let c = match self.clip_stack.last() {
            Some(c) => {
                let x = c.x.max(r.x);
                let y = c.y.max(r.y);
                rect(x, y, (c.r().min(r.r()) - x).max(0.0), (c.b().min(r.b()) - y).max(0.0))
            }
            None => r,
        };
        self.clip_stack.push(c);
    }
    pub fn unclip(&mut self) {
        self.d.pop_clip();
        self.clip_stack.pop();
    }
    pub fn key(&mut self, k: NamedKey) -> bool {
        if !self.enabled {
            return false;
        }
        let n = self.input.keys.len();
        self.input.keys.retain(|e| !(e.key == Key::Named(k) && !e.mods.ctrl && !e.mods.alt));
        n != self.input.keys.len()
    }
    /// Tecla de carácter sin estar escribiendo.
    pub fn chr(&mut self, c: &str) -> bool {
        if !self.enabled || self.focus.is_some() {
            return false;
        }
        let n = self.input.keys.len();
        self.input.keys.retain(|e| !(matches!(&e.key, Key::Character(s) if s.eq_ignore_ascii_case(c)) && !e.mods.ctrl && !e.mods.alt));
        n != self.input.keys.len()
    }
    pub fn ctrl(&mut self, c: &str) -> bool {
        if !self.enabled {
            return false;
        }
        let n = self.input.keys.len();
        self.input.keys.retain(|e| !(matches!(&e.key, Key::Character(s) if s.eq_ignore_ascii_case(c)) && e.mods.ctrl));
        n != self.input.keys.len()
    }

    // ------------------------------------------------------------ texto
    pub fn text(&mut self, f: FontId, size: f32, x: f32, cy: f32, s: &str, c: Color) -> f32 {
        self.t.draw(&mut self.d, f, size, x, cy, s, c)
    }
    pub fn text_c(&mut self, f: FontId, size: f32, cx: f32, cy: f32, s: &str, c: Color) -> f32 {
        let w = self.t.measure(f, size, s);
        self.t.draw(&mut self.d, f, size, cx - w / 2.0, cy, s, c);
        w
    }
    pub fn text_r(&mut self, f: FontId, size: f32, r: f32, cy: f32, s: &str, c: Color) -> f32 {
        let w = self.t.measure(f, size, s);
        self.t.draw(&mut self.d, f, size, r - w, cy, s, c);
        w
    }
    #[allow(clippy::too_many_arguments)]
    pub fn text_fit(&mut self, f: FontId, size: f32, x: f32, cy: f32, s: &str, w: f32, c: Color) -> f32 {
        let s = self.t.fit(f, size, s, w);
        self.t.draw(&mut self.d, f, size, x, cy, &s, c)
    }
    /// Rótulo pequeño espaciado.
    pub fn label(&mut self, x: f32, cy: f32, s: &str, c: Color) -> f32 {
        self.t.draw_sp(&mut self.d, fonts().ui_m, 11.0, x, cy, &s.to_uppercase(), c, 1.6)
    }
    pub fn label_w(&mut self, s: &str) -> f32 {
        let u = s.to_uppercase();
        self.t.measure(fonts().ui_m, 11.0, &u) + 1.6 * u.chars().count() as f32
    }
    pub fn label_c(&mut self, cx: f32, cy: f32, s: &str, c: Color) {
        let w = self.label_w(s) - 1.6;
        self.label(cx - w / 2.0, cy, s, c);
    }
    pub fn icon(&mut self, ic: Icon, cx: f32, cy: f32, size: f32, c: Color) {
        self.t.icon(&mut self.d, ic, cx, cy, size, c);
    }

    // ------------------------------------------------------------ botones
    pub fn tooltip(&mut self, r: Rect, text: &str) {
        if self.hover(r) && !text.is_empty() {
            self.tip = Some((idx(text, r.x as usize), r, text.into()));
        }
    }
    pub fn btn_w(&mut self, label: &str, icon: bool) -> f32 {
        self.t.measure(fonts().ui_m, 14.0, label) + if icon { 26.0 } else { 0.0 } + 40.0
    }
    pub fn button(&mut self, idv: u64, r: Rect, kind: Btn, icon: Option<Icon>, label: &str) -> bool {
        let clicked = self.click(idv, r);
        let over = self.over(r) && (self.active.is_none() || self.active == Some(idv));
        let h = self.anim(idv, over as i32 as f32, 16.0);
        let p = self.anim(idv ^ 0x51, (self.active == Some(idv)) as i32 as f32, 28.0);
        if over {
            self.cursor = CursorIcon::Pointer;
        }
        let th = self.th;
        let rad = r.h / 2.0;
        self.d.push_xf(1.0 - 0.035 * p, r.cx(), r.cy(), 0.0, 0.0);
        let fg = match kind {
            Btn::Primary => {
                self.d.shadow(r.off(0.0, 5.0).inset(4.0), rad, 22.0, th.accent.a(0.10 + 0.22 * h));
                let top = th.accent.mix(Color::WHITE, 0.14 + 0.16 * h);
                self.d.grad(r, rad, top, th.accent.mix(Color::WHITE, 0.10 * h), false);
                th.on_accent
            }
            Btn::Soft => {
                self.d.boxed(r, rad, th.text.a(0.055 + 0.065 * h), 1.0, th.text.a(0.10 + 0.10 * h));
                th.text
            }
            Btn::Ghost => {
                self.d.fill(r, rad, th.text.a(0.09 * h));
                th.text2.mix(th.text, h)
            }
            Btn::Danger => {
                let red = if th.dark { Color::hex(0xE87A70) } else { Color::hex(0xB3362B) };
                self.d.boxed(r, rad, red.a(0.08 + 0.12 * h), 1.0, red.a(0.35));
                red
            }
        };
        let tw = self.t.measure(fonts().ui_m, 14.0, label);
        let iw = if icon.is_some() { if label.is_empty() { 18.0 } else { 26.0 } } else { 0.0 };
        let avail = r.w - 16.0 - iw;
        let label = if tw > avail { self.t.fit(fonts().ui_m, 14.0, label, avail) } else { label.to_string() };
        let tw = self.t.measure(fonts().ui_m, 14.0, &label);
        let x = r.cx() - (tw + iw) / 2.0;
        if let Some(ic) = icon {
            self.icon(ic, x + 9.0, r.cy(), 18.0, fg);
        }
        self.text(fonts().ui_m, 14.0, x + iw, r.cy(), &label, fg);
        self.d.pop_xf();
        clicked
    }
    pub fn icon_btn(&mut self, idv: u64, r: Rect, ic: Icon, tip: &str, on: bool) -> bool {
        let th = self.th;
        self.icon_btn_c(idv, r, ic, tip, on, th.text, th.accent, true)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn icon_btn_c(&mut self, idv: u64, r: Rect, ic: Icon, tip: &str, on: bool, fg: Color, accent: Color, frame: bool) -> bool {
        let clicked = self.click(idv, r);
        let over = self.over(r) && (self.active.is_none() || self.active == Some(idv));
        let h = self.anim(idv, over as i32 as f32, 16.0);
        let p = self.anim(idv ^ 0x51, (self.active == Some(idv)) as i32 as f32, 28.0);
        let o = self.anim(idv ^ 0x77, on as i32 as f32, 16.0);
        if over {
            self.cursor = CursorIcon::Pointer;
        }
        self.d.push_xf(1.0 - 0.08 * p, r.cx(), r.cy(), 0.0, 0.0);
        let rad = r.h.min(r.w) / 2.0;
        if frame {
            self.d.boxed(r, rad, fg.a(0.04 + 0.08 * h).mix(accent.a(0.18), o), 1.0, fg.a(0.09 + 0.08 * h).mix(accent.a(0.45), o));
        } else {
            self.d.fill(r, rad, fg.a(0.10 * h).mix(accent.a(0.18), o));
        }
        let c = fg.a(0.72 + 0.28 * h).mix(accent, o);
        self.icon(ic, r.cx(), r.cy(), 19.0, c);
        self.d.pop_xf();
        self.tooltip(r, tip);
        clicked
    }
    /// Selector en píldora con la marca que se desliza. Devuelve la opción pulsada.
    pub fn segmented(&mut self, idv: u64, r: Rect, items: &[(Option<Icon>, &str, &str)], sel: usize) -> Option<usize> {
        let th = self.th;
        self.d.boxed(r, r.h / 2.0, th.text.a(0.045), 1.0, th.text.a(0.09));
        let n = items.len() as f32;
        let iw = (r.w - 6.0) / n;
        let p = self.anim(idv ^ 0x9191, sel as f32, 18.0);
        let knob = rect(r.x + 3.0 + p * iw, r.y + 3.0, iw, r.h - 6.0);
        self.d.shadow(knob.off(0.0, 2.0), knob.h / 2.0, 10.0, Color::BLACK.a(0.25));
        self.d.fill(knob, knob.h / 2.0, th.text.a(0.94));
        let mut out = None;
        for (i, (ic, label, tip)) in items.iter().enumerate() {
            let c = rect(r.x + 3.0 + i as f32 * iw, r.y + 3.0, iw, r.h - 6.0);
            let bid = idv ^ idx("seg", i + 1);
            if self.click(bid, c) && i != sel {
                out = Some(i);
            }
            let over = self.over(c);
            if over {
                self.cursor = CursorIcon::Pointer;
            }
            let h = self.anim(bid, over as i32 as f32, 16.0);
            let near = (1.0 - (p - i as f32).abs()).clamp(0.0, 1.0);
            let col = th.text2.mix(th.text, h).mix(th.bg, near);
            if label.is_empty() {
                if let Some(ic) = ic {
                    self.icon(*ic, c.cx(), c.cy(), 18.0, col);
                }
            } else {
                let tw = self.t.measure(fonts().ui_m, 13.0, label);
                let w = tw + if ic.is_some() { 24.0 } else { 0.0 };
                let mut x = c.cx() - w / 2.0;
                if let Some(ic) = ic {
                    self.icon(*ic, x + 8.0, c.cy(), 17.0, col);
                    x += 24.0;
                }
                self.text(fonts().ui_m, 13.0, x, c.cy(), label, col);
            }
            self.tooltip(c, tip);
        }
        out
    }
    pub fn chip(&mut self, idv: u64, x: f32, y: f32, label: &str, on: bool) -> (f32, bool) {
        let w = self.t.measure(fonts().ui, 13.0, label) + 26.0;
        let r = rect(x, y, w, 30.0);
        let c = self.click(idv, r);
        let over = self.over(r);
        if over {
            self.cursor = CursorIcon::Pointer;
        }
        let h = self.anim(idv, over as i32 as f32, 16.0);
        let o = self.anim(idv ^ 3, on as i32 as f32, 16.0);
        let th = self.th;
        self.d.boxed(r, 15.0, th.text.a(0.05 + 0.06 * h).mix(th.accent.a(0.20), o), 1.0, th.text.a(0.08).mix(th.accent.a(0.5), o));
        self.text(fonts().ui, 13.0, x + 13.0, r.cy(), label, th.text2.mix(th.text, h).mix(th.accent, o));
        (w, c)
    }
    pub fn toggle(&mut self, idv: u64, x: f32, cy: f32, on: bool) -> bool {
        let r = rect(x, cy - 12.0, 44.0, 24.0);
        let c = self.click(idv, r.grow(4.0));
        if self.over(r) {
            self.cursor = CursorIcon::Pointer;
        }
        let o = self.anim(idv, on as i32 as f32, 18.0);
        let th = self.th;
        self.d.fill(r, 12.0, th.text.a(0.16).mix(th.accent, o));
        let k = rect(r.x + 3.0 + o * 20.0, r.y + 3.0, 18.0, 18.0);
        self.d.shadow(k.off(0.0, 1.0), 9.0, 5.0, Color::BLACK.a(0.35));
        self.d.fill(k, 9.0, th.text2.mix(th.on_accent, o).mix(Color::WHITE, 0.75));
        c
    }
    /// Deslizador 0..1. Devuelve el valor nuevo mientras se arrastra.
    pub fn slider(&mut self, idv: u64, r: Rect, v: f32) -> Option<f32> {
        let th = self.th;
        self.slider_c(idv, r, v, th.text, th.accent)
    }
    pub fn slider_c(&mut self, idv: u64, r: Rect, v: f32, fg: Color, accent: Color) -> Option<f32> {
        let act = self.drag(idv, r.grow(6.0));
        let over = self.over(r.grow(6.0)) || act;
        if over {
            self.cursor = CursorIcon::Pointer;
        }
        let h = self.anim(idv, over as i32 as f32, 18.0);
        let mut v = v.clamp(0.0, 1.0);
        let mut out = None;
        if act {
            let n = ((self.input.pos.0 - r.x) / r.w).clamp(0.0, 1.0);
            if (n - v).abs() > 0.0001 {
                v = n;
                out = Some(n);
            }
        }
        let th = lerp(4.0, 5.0, h);
        let line = rect(r.x, r.cy() - th / 2.0, r.w, th);
        self.d.fill(line, th / 2.0, fg.a(0.16));
        self.d.fill(rect(line.x, line.y, (line.w * v).max(th), line.h), th / 2.0, accent);
        let kr = lerp(6.5, 8.5, h);
        let k = rect(r.x + r.w * v - kr, r.cy() - kr, kr * 2.0, kr * 2.0);
        self.d.shadow(k.off(0.0, 1.5), kr, 7.0, Color::BLACK.a(0.35));
        self.d.fill(k, kr, accent.mix(Color::WHITE, 0.65));
        out
    }
    /// Tarjeta flotante con sombra.
    pub fn card(&mut self, r: Rect, rad: f32) {
        let th = self.th;
        self.d.shadow(r.off(0.0, 18.0), rad, 60.0, Color::BLACK.a(if th.dark { 0.55 } else { 0.22 }));
        self.d.shadow(r.off(0.0, 3.0), rad, 10.0, Color::BLACK.a(if th.dark { 0.30 } else { 0.10 }));
        self.d.boxed(r, rad, th.panel, 1.0, th.line);
    }

    // ------------------------------------------------------------ desplazamiento
    pub fn scroll(&mut self, idv: u64, s: &mut Scroll, view: Rect, content: f32) -> f32 {
        s.max = (content - view.h).max(0.0);
        if self.over(view) && self.input.wheel.1 != 0.0 && self.active.is_none() {
            s.target -= self.input.wheel.1;
            self.input.wheel.1 = 0.0;
        }
        s.target = s.target.clamp(0.0, s.max);
        // barra
        if s.max > 0.5 {
            let th = self.th;
            let kh = (view.h * view.h / content).max(36.0);
            let track = view.h - 8.0 - kh;
            let ky = view.y + 4.0 + track * (s.s.x / s.max).clamp(0.0, 1.0);
            let zone = rect(view.r() - 14.0, view.y, 14.0, view.h);
            if self.hover(zone) && self.input.pressed {
                self.active = Some(idv);
                s.grab = Some((self.input.pos.1, s.target));
            }
            if self.active == Some(idv) {
                if let Some((gy, st)) = s.grab {
                    s.target = (st + (self.input.pos.1 - gy) / track.max(1.0) * s.max).clamp(0.0, s.max);
                    s.s = Spring::at(s.target);
                }
                self.redraw = true;
            }
            let hot = self.over(zone) || self.active == Some(idv);
            let near = self.over(view) as i32 as f32;
            let a = self.anim(idv, if hot { 1.0 } else { 0.35 * near }, 12.0);
            let kw = 4.0 + 3.0 * (a - 0.35).max(0.0);
            self.d.fill(rect(view.r() - 5.0 - kw, ky, kw, kh), kw / 2.0, th.text.a(0.30 * a));
        }
        if s.s.step(s.target, 22.0, self.dt, 0.15) {
            self.redraw = true;
        }
        s.s.x
    }

    // ------------------------------------------------------------ edición
    fn lines(&mut self, f: FontId, size: f32, s: &str, width: f32) -> Vec<(usize, usize)> {
        let mut out = vec![];
        let mut start = 0;
        for para in s.split('\n') {
            let mut ls = start;
            let mut w = 0.0;
            for (o, word) in split_keep_idx(para) {
                let ww = self.t.measure(f, size, word);
                let tw = self.t.measure(f, size, word.trim_end());
                if w + tw > width && start + o > ls {
                    out.push((ls, start + o));
                    ls = start + o;
                    w = 0.0;
                }
                if tw > width {
                    // palabra más larga que la línea
                    let mut cw = 0.0;
                    for (ci, ch) in word.char_indices() {
                        let chw = self.t.measure(f, size, ch.encode_utf8(&mut [0; 4]));
                        if cw + chw > width && ci > 0 {
                            out.push((ls, start + o + ci));
                            ls = start + o + ci;
                            cw = 0.0;
                        }
                        cw += chw;
                    }
                    w = cw;
                } else {
                    w += ww;
                }
            }
            out.push((ls, start + para.len()));
            start += para.len() + 1;
        }
        out
    }
    /// Texto editable dentro de `r`. Devuelve (cambió, Intro).
    #[allow(clippy::too_many_arguments)]
    pub fn edit(&mut self, idv: u64, r: Rect, e: &mut Edit, multi: bool, f: FontId, size: f32, col: Color, sel: Color) -> (bool, bool) {
        let focused = self.focus == Some(idv);
        let lh = (size * 1.45).round();
        let hit = r.grow(4.0);
        if self.hover(hit) && self.input.pressed {
            self.active = Some(idv);
            self.focus = Some(idv);
            self.redraw = true;
        }
        if self.over(hit) {
            self.cursor = CursorIcon::Text;
        }
        let width = if multi { r.w } else { f32::MAX };
        let mut lines = self.lines(f, size, &e.text, width);
        let mut changed = false;
        let mut enter = false;
        e.caret = e.caret.min(e.text.len());
        e.anchor = e.anchor.min(e.text.len());
        let find_line = |lines: &[(usize, usize)], c: usize| lines.iter().rposition(|l| l.0 <= c).unwrap_or(0);
        if self.active == Some(idv) {
            let y = self.input.pos.1 - r.y + e.sy;
            let li = if multi { ((y / lh) as isize).clamp(0, lines.len() as isize - 1) as usize } else { 0 };
            let (a, b) = lines[li];
            let x = self.input.pos.0 - r.x + e.sx;
            let mut best = a;
            let mut bd = f32::MAX;
            let seg = e.text[a..b].trim_end_matches('\n').to_string();
            for i in seg.char_indices().map(|c| c.0).chain(std::iter::once(seg.len())) {
                let w = self.t.measure(f, size, &seg[..i]);
                if (w - x).abs() < bd {
                    bd = (w - x).abs();
                    best = a + i;
                }
            }
            if self.input.pressed && self.input.clicks == 2 {
                e.anchor = e.word(e.next(best), false);
                e.caret = e.word(best, true);
            } else if self.input.pressed && self.input.clicks >= 3 {
                e.anchor = 0;
                e.caret = e.text.len();
            } else if self.input.clicks < 2 {
                e.caret = best;
                if self.input.pressed && !self.input.mods.shift {
                    e.anchor = best;
                }
            }
            e.blink = 0.0;
        }
        if focused {
            let keys = std::mem::take(&mut self.input.keys);
            let mut rest = vec![];
            for k in keys {
                let m = k.mods;
                let mv = |e: &mut Edit, p: usize| {
                    e.caret = p;
                    if !m.shift {
                        e.anchor = p;
                    }
                    e.blink = 0.0;
                };
                match &k.key {
                    Key::Named(NamedKey::ArrowLeft) => {
                        let p = if m.ctrl {
                            e.word(e.caret, false)
                        } else if !m.shift && e.caret != e.anchor {
                            e.sel().0
                        } else {
                            e.prev(e.caret)
                        };
                        mv(e, p);
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        let p = if m.ctrl {
                            e.word(e.caret, true)
                        } else if !m.shift && e.caret != e.anchor {
                            e.sel().1
                        } else {
                            e.next(e.caret)
                        };
                        mv(e, p);
                    }
                    Key::Named(n @ (NamedKey::ArrowUp | NamedKey::ArrowDown)) if multi => {
                        let li = find_line(&lines, e.caret);
                        let x = { let s = e.text[lines[li].0..e.caret].to_string(); self.t.measure(f, size, &s) };
                        let ti = if *n == NamedKey::ArrowUp { li.checked_sub(1) } else { (li + 1 < lines.len()).then_some(li + 1) };
                        if let Some(ti) = ti {
                            let seg = e.text[lines[ti].0..lines[ti].1].to_string();
                            let mut best = (f32::MAX, 0);
                            for i in seg.char_indices().map(|c| c.0).chain(std::iter::once(seg.len())) {
                                let w = self.t.measure(f, size, &seg[..i]);
                                if (w - x).abs() < best.0 {
                                    best = ((w - x).abs(), i);
                                }
                            }
                            mv(e, lines[ti].0 + best.1);
                        }
                    }
                    Key::Named(NamedKey::Home) => {
                        let p = if multi && !m.ctrl { lines[find_line(&lines, e.caret)].0 } else { 0 };
                        mv(e, p);
                    }
                    Key::Named(NamedKey::End) => {
                        let p = if multi && !m.ctrl { lines[find_line(&lines, e.caret)].1 } else { e.text.len() };
                        mv(e, p);
                    }
                    Key::Named(NamedKey::Backspace) => {
                        if e.caret == e.anchor {
                            e.anchor = if m.ctrl { e.word(e.caret, false) } else { e.prev(e.caret) };
                        }
                        e.replace("");
                        changed = true;
                    }
                    Key::Named(NamedKey::Delete) => {
                        if e.caret == e.anchor {
                            e.anchor = if m.ctrl { e.word(e.caret, true) } else { e.next(e.caret) };
                        }
                        e.replace("");
                        changed = true;
                    }
                    Key::Named(NamedKey::Enter) => {
                        if multi {
                            e.replace("\n");
                            changed = true;
                        } else {
                            enter = true;
                        }
                    }
                    Key::Character(c) if m.ctrl && !m.alt => match c.to_lowercase().as_str() {
                        "a" => {
                            e.anchor = 0;
                            e.caret = e.text.len();
                        }
                        "c" | "x" => {
                            let (a, b) = e.sel();
                            if a != b {
                                set_clipboard(&e.text[a..b]);
                                if c.eq_ignore_ascii_case("x") {
                                    e.replace("");
                                    changed = true;
                                }
                            }
                        }
                        "v" => {
                            if let Some(s) = get_clipboard() {
                                let s = s.replace("\r\n", "\n");
                                let s = if multi { s } else { s.replace('\n', " ") };
                                e.replace(&s);
                                changed = true;
                            }
                        }
                        _ => rest.push(k.clone()),
                    },
                    Key::Character(_) | Key::Named(NamedKey::Space) => {}
                    _ => rest.push(k.clone()),
                }
            }
            self.input.keys = rest;
            let s: String = self.input.text.chars().filter(|c| !c.is_control()).collect();
            if !s.is_empty() {
                e.replace(&s);
                changed = true;
                e.blink = 0.0;
            }
            self.input.text.clear();
            if changed {
                e.blink = 0.0;
                lines = self.lines(f, size, &e.text, width);
            }
        }
        // mantener el cursor a la vista
        let li = find_line(&lines, e.caret);
        let seg = e.text[lines[li].0..e.caret].to_string();
        let cx = self.t.measure(f, size, &seg);
        if focused {
            if !multi {
                if cx - e.sx > r.w - 2.0 {
                    e.sx = cx - r.w + 2.0;
                }
                if cx < e.sx {
                    e.sx = cx;
                }
            } else {
                let cy = li as f32 * lh;
                if cy + lh - e.sy > r.h {
                    e.sy = cy + lh - r.h;
                }
                if cy < e.sy {
                    e.sy = cy;
                }
            }
        } else if !multi {
            e.sx = 0.0;
        }
        if multi {
            let max = (lines.len() as f32 * lh - r.h).max(0.0);
            if self.over(r) && self.input.wheel.1 != 0.0 && max > 0.0 {
                e.sy -= self.input.wheel.1 * 0.5;
                self.input.wheel.1 = 0.0;
            }
            e.sy = e.sy.clamp(0.0, max);
        }
        self.clip(rect(r.x - 2.0, r.y, r.w + 4.0, r.h), 0.0);
        let (sa, sb) = e.sel();
        for (i, &(a, b)) in lines.iter().enumerate() {
            let top = if multi { r.y + i as f32 * lh - e.sy } else { r.cy() - lh / 2.0 };
            if top > r.b() || top + lh < r.y {
                continue;
            }
            if focused && sa != sb && sb > a && sa <= b {
                let x0 = { let s = e.text[a..sa.clamp(a, b)].to_string(); self.t.measure(f, size, &s) };
                let x1 = { let s = e.text[a..sb.clamp(a, b)].to_string(); self.t.measure(f, size, &s) };
                self.d.fill(rect(r.x + x0 - e.sx, top + 1.0, (x1 - x0).max(3.0), lh - 2.0), 3.0, sel.a(0.35));
            }
            let s = e.text[a..b].to_string();
            self.t.draw(&mut self.d, f, size, r.x - e.sx, top + lh / 2.0, &s, col);
        }
        if focused {
            let top = if multi { r.y + li as f32 * lh - e.sy } else { r.cy() - lh / 2.0 };
            e.blink += self.dt;
            let on = (e.blink % 1.06) < 0.53;
            let c = rect(r.x + cx - e.sx, top + 2.0, 1.6, lh - 4.0);
            if on {
                self.d.fill(c, 0.8, sel);
            }
            self.ime = Some(c);
            self.wake = self.wake.min(0.53 - (e.blink % 0.53));
        }
        self.unclip();
        (changed, enter)
    }
    /// Campo de una línea con marco.
    pub fn field(&mut self, idv: u64, r: Rect, e: &mut Edit, icon: Option<Icon>, hint: &str) -> (bool, bool) {
        let th = self.th;
        let f = self.anim(idv ^ 0xF0, (self.focus == Some(idv)) as i32 as f32, 16.0);
        let h = self.anim(idv ^ 0xF1, self.over(r) as i32 as f32, 16.0);
        let rad = (r.h / 2.0).min(18.0);
        self.d.boxed(r, rad, th.text.a(0.045 + 0.02 * h), 1.0, th.text.a(0.09 + 0.05 * h).mix(th.accent.a(0.75), f));
        let mut x = r.x + 14.0;
        if let Some(ic) = icon {
            self.icon(ic, r.x + 20.0, r.cy(), 17.0, th.text3.mix(th.text, f));
            x = r.x + 38.0;
        }
        let inner = rect(x, r.y, r.r() - 12.0 - x, r.h);
        if e.text.is_empty() {
            self.text_fit(fonts().ui, 14.0, x, r.cy(), hint, inner.w, th.text3);
        }
        // pulsar en cualquier parte del marco enfoca
        if self.hover(r) && self.input.pressed {
            self.focus = Some(idv);
            self.active = Some(idv);
        }
        self.edit(idv, inner, e, false, fonts().ui, 14.0, th.text, th.accent)
    }

    // ------------------------------------------------------------ menús
    pub fn open_menu(&mut self, owner: u64, anchor: Rect, items: Vec<MenuItem>) {
        let mut w: f32 = 180.0;
        for i in &items {
            let tw = self.t.measure(fonts().ui, 14.0, &i.label) + 74.0;
            w = w.max(tw.min(380.0));
        }
        let h: f32 = items.iter().map(menu_h).sum::<f32>() + 14.0;
        let h = h.min(self.h - 24.0);
        let mut x = anchor.x;
        if x + w > self.w - 10.0 {
            x = (anchor.r() - w).max(10.0).min(self.w - w - 10.0);
        }
        let mut y = anchor.b() + 8.0;
        if y + h > self.h - 10.0 {
            y = if anchor.y - h - 8.0 > 10.0 { anchor.y - h - 8.0 } else { self.h - h - 10.0 };
        }
        self.menu = Some(Menu { owner, items, r: rect(x, y, w, h), t: 0.0, closing: false, scroll: Scroll::default() });
        self.redraw = true;
    }
    pub fn menu_open(&self, owner: u64) -> bool {
        self.menu.as_ref().is_some_and(|m| m.owner == owner && !m.closing)
    }
    pub fn any_menu(&self) -> bool {
        self.menu.as_ref().is_some_and(|m| !m.closing)
    }
    pub fn picked(&mut self, owner: u64) -> Option<usize> {
        match self.picked {
            Some((o, i)) if o == owner => {
                self.picked = None;
                Some(i)
            }
            _ => None,
        }
    }
    fn menu_input(&mut self) {
        let Some(m) = &mut self.menu else { return };
        if m.closing {
            return;
        }
        let p = self.input.pos;
        if self.input.pressed || self.input.rpressed {
            if !m.r.has(p) {
                m.closing = true;
                self.input.pressed = false;
                self.input.rpressed = false;
            } else {
                let mut y = m.r.y + 7.0 - m.scroll.pos();
                for (i, it) in m.items.iter().enumerate() {
                    let h = menu_h(it);
                    if p.1 >= y && p.1 < y + h && !it.header && self.input.pressed {
                        self.picked = Some((m.owner, i));
                        m.closing = true;
                        break;
                    }
                    y += h;
                }
                self.input.pressed = false;
                self.input.rpressed = false;
            }
        }
        if self.input.keys.iter().any(|k| k.key == Key::Named(NamedKey::Escape)) {
            m.closing = true;
            self.input.keys.clear();
        }
        // la rueda es del menú
        if self.input.wheel.1 != 0.0 {
            let total: f32 = m.items.iter().map(menu_h).sum::<f32>() + 14.0;
            m.scroll.target = (m.scroll.target - self.input.wheel.1).clamp(0.0, (total - m.r.h).max(0.0));
            self.input.wheel.1 = 0.0;
        }
        if m.r.has(p) {
            // lo de debajo no reacciona
            self.enabled = false;
        }
    }
    /// Un menú abierto se queda con el ratón: las vistas preguntan antes de reaccionar.
    pub fn menu_blocks(&self) -> bool {
        self.menu.as_ref().is_some_and(|m| !m.closing && m.r.has(self.input.pos))
    }
    fn draw_menu(&mut self) {
        let mut m = self.menu.take().unwrap();
        let open = !m.closing;
        let t = self.tween(&mut m.t, open, if open { 0.16 } else { 0.10 });
        if m.closing && t <= 0.0 {
            return;
        }
        if m.scroll.s.step(m.scroll.target, 22.0, self.dt, 0.2) {
            self.redraw = true;
        }
        let th = self.th;
        let e = ease_out(t);
        self.d.push_alpha(e);
        self.d.push_xf(0.96 + 0.04 * e, m.r.cx(), m.r.y, 0.0, 0.0);
        self.d.shadow(m.r.off(0.0, 12.0), 16.0, 40.0, Color::BLACK.a(if th.dark { 0.55 } else { 0.22 }));
        self.d.boxed(m.r, 16.0, th.raised, 1.0, th.text.a(0.10));
        self.d.push_clip(m.r.inset(1.0), 15.0);
        let mut y = m.r.y + 7.0 - m.scroll.pos();
        for (i, it) in m.items.iter().enumerate() {
            let h = menu_h(it);
            if it.sep {
                self.d.fill(rect(m.r.x + 16.0, y + h - 4.5, m.r.w - 32.0, 1.0), 0.0, th.line);
            }
            let row = rect(m.r.x + 7.0, y, m.r.w - 14.0, if it.sep { h - 9.0 } else { h });
            if it.header {
                self.label(row.x + 12.0, row.cy() + 2.0, &it.label, th.text3);
            } else {
                let over = open && row.has(self.input.pos);
                if over {
                    self.cursor = CursorIcon::Pointer;
                }
                let a = self.anim(idx("menu", i) ^ m.owner, over as i32 as f32, 24.0);
                self.d.fill(row, 10.0, th.text.a(0.09 * a));
                let red = if th.dark { Color::hex(0xF0897F) } else { Color::hex(0xB3362B) };
                let c = if it.danger { red } else { th.text };
                let mut x = row.x + 12.0;
                if let Some(ic) = it.icon {
                    self.icon(ic, row.x + 20.0, row.cy(), 17.0, c.fade(0.85));
                    x = row.x + 38.0;
                }
                self.text_fit(fonts().ui, 14.0, x, row.cy(), &it.label, row.r() - x - 30.0, c);
                if it.checked {
                    self.icon(Icon::Check, row.r() - 18.0, row.cy(), 16.0, th.accent);
                }
            }
            y += h;
        }
        self.d.pop_clip();
        self.d.pop_xf();
        self.d.pop_alpha();
        self.menu = Some(m);
    }

    // ------------------------------------------------------------ avisos
    pub fn toast(&mut self, s: impl Into<String>) {
        self.toasts.push(Toast { text: s.into(), t: 0.0, err: false });
        self.redraw = true;
    }
    pub fn toast_err(&mut self, s: impl Into<String>) {
        self.toasts.push(Toast { text: s.into(), t: 0.0, err: true });
        self.redraw = true;
    }
    fn draw_toasts(&mut self) {
        let mut ts = std::mem::take(&mut self.toasts);
        let th = self.th;
        let mut y = self.h - 34.0;
        let n = ts.len();
        for (i, t) in ts.iter_mut().enumerate().rev() {
            if n - i > 4 {
                t.t += 10.0;
            }
            t.t += self.dt;
            let life = if t.err { 6.0 } else { 3.4 };
            let a = ease_out((t.t / 0.25).min(1.0)) * (1.0 - ((t.t - life) / 0.3).clamp(0.0, 1.0));
            if t.t < 0.3 || t.t > life {
                self.redraw = true;
            } else {
                self.wake = self.wake.min(life - t.t + 0.01);
            }
            let s = self.t.fit(fonts().ui, 14.0, &t.text, (self.w - 120.0).min(760.0));
            let w = self.t.measure(fonts().ui, 14.0, &s) + 62.0;
            let r = rect(self.w / 2.0 - w / 2.0, y - 42.0 * a, w, 42.0);
            self.d.push_alpha(a);
            self.d.shadow(r.off(0.0, 8.0), 21.0, 30.0, Color::BLACK.a(0.45));
            self.d.boxed(r, 21.0, th.raised, 1.0, th.text.a(0.10));
            let red = if th.dark { Color::hex(0xF0897F) } else { Color::hex(0xB3362B) };
            self.icon(if t.err { Icon::Info } else { Icon::Check }, r.x + 24.0, r.cy(), 17.0, if t.err { red } else { th.accent });
            self.text(fonts().ui, 14.0, r.x + 42.0, r.cy(), &s, th.text);
            self.d.pop_alpha();
            y -= 52.0 * a;
        }
        ts.retain(|t| t.t < if t.err { 6.3 } else { 3.7 });
        self.toasts = ts;
    }
    fn draw_tip(&mut self) {
        let Some((i, r, s)) = self.tip.clone() else {
            self.tip_t = (self.tip_t - self.dt * 4.0).max(0.0).min(0.4);
            return;
        };
        let _ = i;
        self.tip_t += self.dt;
        let delay = 0.45;
        if self.tip_t < delay {
            self.wake = self.wake.min(delay - self.tip_t + 0.005);
            return;
        }
        let a = ((self.tip_t - delay) / 0.12).min(1.0);
        if a < 1.0 {
            self.redraw = true;
        }
        let th = self.th;
        let w = self.t.measure(fonts().ui, 12.5, &s) + 22.0;
        let mut b = rect(r.cx() - w / 2.0, r.b() + 8.0, w, 28.0);
        if b.b() > self.h - 6.0 {
            b.y = r.y - 36.0;
        }
        b.x = b.x.clamp(6.0, (self.w - w - 6.0).max(6.0));
        self.d.push_alpha(a);
        self.d.shadow(b.off(0.0, 4.0), 9.0, 16.0, Color::BLACK.a(0.4));
        self.d.boxed(b, 9.0, th.raised, 1.0, th.text.a(0.10));
        self.text(fonts().ui, 12.5, b.x + 11.0, b.cy(), &s, th.text);
        self.d.pop_alpha();
    }
    /// Estrellas 0..5. Devuelve la nota pulsada.
    pub fn stars(&mut self, idv: u64, x: f32, cy: f32, size: f32, val: i32) -> Option<i32> {
        let th = self.th;
        let mut out = None;
        let gap = size + 5.0;
        let zone = rect(x, cy - size, gap * 5.0, size * 2.0);
        let hov = if self.over(zone) { ((self.input.pos.0 - x) / gap) as i32 + 1 } else { 0 };
        for i in 1..=5 {
            let r = rect(x + (i - 1) as f32 * gap, cy - size * 0.7, gap, size * 1.4);
            if self.click(idv ^ idx("star", i as usize), r) {
                out = Some(if val == i { 0 } else { i });
            }
            let on = if hov > 0 { i <= hov } else { i <= val };
            let a = self.anim(idv ^ idx("stara", i as usize), on as i32 as f32, 20.0);
            self.icon(Icon::StarFill, r.x + size / 2.0, cy, size * (0.9 + 0.1 * a), th.text.a(0.16).mix(th.accent, a));
        }
        if hov > 0 {
            self.cursor = CursorIcon::Pointer;
        }
        out
    }
}
fn menu_h(i: &MenuItem) -> f32 {
    (if i.header { 30.0 } else { 36.0 }) + if i.sep { 9.0 } else { 0.0 }
}

pub fn set_clipboard(s: &str) {
    if let Ok(mut c) = arboard::Clipboard::new() {
        let _ = c.set_text(s.to_string());
    }
}
pub fn get_clipboard() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}
