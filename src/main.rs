#![windows_subsystem = "windows"]
mod app;
mod book3d;
mod covers;
mod db;
mod doc;
mod formats;
mod gfx;
mod icons;
mod import;
mod jobs;
mod layout;
mod meta;
mod reader;
mod shot;
mod text;
mod theme;
mod ui;
mod views;

use app::{App, WinReq};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::*;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState};
use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows, WindowExtWindows};
use winit::window::*;

struct Runner {
    proxy: EventLoopProxy<()>,
    args: Vec<PathBuf>,
    win: Option<Arc<Window>>,
    app: Option<App>,
    last: Instant,
    /// Duración de un fotograma del monitor.
    period: f32,
    acc: f32,
    idle: bool,
    mods: ModifiersState,
    cursor: CursorIcon,
    hidden: bool,
    last_click: (Instant, (f32, f32), u8),
    ime: bool,
    shown: bool,
    occluded: bool,
    restore: Option<(i32, i32, u32, u32)>,
}

impl Runner {
    fn scale(&self) -> f32 {
        self.win.as_ref().map_or(1.0, |w| w.scale_factor() as f32)
    }
    fn refresh_period(&mut self) {
        if let Some(m) = self.win.as_ref().and_then(|w| w.current_monitor()) {
            let hz = m.refresh_rate_millihertz().unwrap_or(60_000) as f32 / 1000.0;
            self.period = 1.0 / hz.clamp(30.0, 500.0);
        }
    }
    fn redraw(&mut self) {
        let Some(win) = self.win.clone() else { return };
        let sz = win.inner_size();
        if sz.width == 0 || sz.height == 0 || self.occluded {
            return;
        }
        let now = Instant::now();
        let raw = (now - self.last).as_secs_f32();
        self.last = now;
        // tras un reposo el primer paso es de un fotograma; en marcha se ajusta al
        // ritmo del monitor para que el movimiento sea uniforme
        let dt = if self.idle {
            self.acc = 0.0;
            self.period
        } else {
            let n = ((raw + self.acc) / self.period).round().max(1.0);
            let step = n * self.period;
            self.acc = (self.acc + raw - step).clamp(-self.period, self.period);
            step.min(0.05)
        };
        let dpi = win.scale_factor() as f32;
        let Some(app) = &mut self.app else { return };
        app.gpu.resize(sz.width, sz.height);
        app.maximized = win.is_maximized();
        app.fullscreen = win.fullscreen().is_some();
        let again = app.frame(sz.width as f32 / dpi, sz.height as f32 / dpi, dpi, dt);
        self.idle = !again;
        if app.ui.cursor != self.cursor {
            self.cursor = app.ui.cursor;
            win.set_cursor(self.cursor);
        }
        if app.ui.hide_cursor != self.hidden {
            self.hidden = app.ui.hide_cursor;
            win.set_cursor_visible(!self.hidden);
        }
        let want = app.ui.ime.is_some();
        if want != self.ime {
            self.ime = want;
            win.set_ime_allowed(want);
        }
        if let Some(r) = app.ui.ime {
            win.set_ime_cursor_area(PhysicalPosition::new((r.x * dpi) as i32, (r.b() * dpi) as i32), PhysicalSize::new(2, (r.h * dpi) as u32));
        }
        if !self.shown {
            self.shown = true;
            win.set_visible(true);
        }
        if again {
            win.request_redraw();
        }
    }
    fn requests(&mut self, el: &ActiveEventLoop) {
        let (Some(win), Some(app)) = (self.win.clone(), &mut self.app) else { return };
        for r in std::mem::take(&mut app.win) {
            match r {
                WinReq::Close => {
                    self.quit(el);
                    return;
                }
                WinReq::Minimize => win.set_minimized(true),
                WinReq::Maximize => win.set_maximized(!win.is_maximized()),
                WinReq::Drag => {
                    let _ = win.drag_window();
                    app.ui.input.down = false;
                    app.ui.active = None;
                }
                WinReq::Resize(d) => {
                    let _ = win.drag_resize_window(d);
                    app.ui.input.down = false;
                    app.ui.active = None;
                }
                WinReq::Fullscreen(on) => win.set_fullscreen(on.then_some(Fullscreen::Borderless(None))),
            }
        }
    }
    fn quit(&mut self, el: &ActiveEventLoop) {
        if let (Some(win), Some(app)) = (&self.win, &mut self.app) {
            let max = win.is_maximized();
            if win.fullscreen().is_none() {
                if !max {
                    self.restore = win.outer_position().ok().map(|p| (p.x, p.y, win.inner_size().width, win.inner_size().height));
                }
                if let Some((x, y, w, h)) = self.restore.filter(|r| r.2 > 300 && r.3 > 200) {
                    app.set.win = Some((x, y, w, h, max));
                } else if let Some(p) = &mut app.set.win {
                    p.4 = max;
                }
            }
            app.shutdown();
        }
        if let Some(w) = &self.win {
            w.set_visible(false);
        }
        self.app = None;
        el.exit();
    }
}

impl ApplicationHandler for Runner {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        let db = db::Db::open();
        let set = db.settings();
        let mut at = Window::default_attributes()
            .with_title("Saga")
            .with_decorations(false)
            .with_undecorated_shadow(true)
            .with_corner_preference(CornerPreference::Round)
            .with_min_inner_size(LogicalSize::new(900.0, 600.0))
            .with_inner_size(LogicalSize::new(1440.0, 900.0))
            .with_visible(false);
        at = at.with_window_icon(Icon::from_path_or_none());
        let mut max = true;
        if let Some((x, y, w, h, m)) = set.win {
            // solo si sigue cayendo en alguna pantalla
            let ok = el.available_monitors().any(|mo| {
                let (p, s) = (mo.position(), mo.size());
                x + 80 < p.x + s.width as i32 && x + w as i32 - 80 > p.x && y + 10 >= p.y && y + 60 < p.y + s.height as i32
            });
            if ok {
                at = at.with_position(PhysicalPosition::new(x, y)).with_inner_size(PhysicalSize::new(w, h));
            }
            max = m;
            self.restore = Some((x, y, w, h));
        } else if let Some(m) = el.primary_monitor().or(el.available_monitors().next()) {
            let k = m.scale_factor();
            let (s, p) = (m.size(), m.position());
            let (w, h) = ((1440.0 * k).min(s.width as f64 * 0.86), (900.0 * k).min(s.height as f64 * 0.86));
            at = at.with_inner_size(PhysicalSize::new(w, h)).with_position(PhysicalPosition::new(p.x + ((s.width as f64 - w) / 2.0) as i32, p.y + ((s.height as f64 - h) / 2.0) as i32));
            max = false;
        }
        let win = Arc::new(el.create_window(at).expect("ventana"));
        win.set_taskbar_icon(Icon::from_path_or_none());
        if max {
            win.set_maximized(true);
        }
        let sz = win.inner_size();
        let gpu = gfx::Gpu::new(Some(win.clone()), sz.width, sz.height, set.high_power);
        let proxy = self.proxy.clone();
        let jobs = jobs::Jobs::new(Arc::new(move || {
            let _ = proxy.send_event(());
        }));
        let dpi = win.scale_factor() as f32;
        let mut app = App::new(gpu, db, set, jobs, sz.width as f32 / dpi, sz.height as f32 / dpi, dpi);
        if !self.args.is_empty() {
            app.add_paths(std::mem::take(&mut self.args));
        }
        self.win = Some(win);
        self.app = Some(app);
        self.refresh_period();
        self.redraw();
    }
    fn user_event(&mut self, _: &ActiveEventLoop, _: ()) {
        if let Some(w) = &self.win {
            w.request_redraw();
        }
    }
    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, ev: WindowEvent) {
        let dpi = self.scale();
        let Some(app) = &mut self.app else { return };
        let i = &mut app.ui.input;
        let mut paint = true;
        match ev {
            WindowEvent::CloseRequested => {
                self.quit(el);
                return;
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                self.requests(el);
                return;
            }
            WindowEvent::Resized(_) => {
                if let Some(w) = &self.win {
                    if !w.is_maximized() && w.fullscreen().is_none() {
                        if let Ok(p) = w.outer_position() {
                            self.restore = Some((p.x, p.y, w.inner_size().width, w.inner_size().height));
                        }
                    }
                }
                // durante el redimensionado se pinta al momento
                self.redraw();
                return;
            }
            WindowEvent::Moved(p) => {
                if let Some(w) = &self.win {
                    if !w.is_maximized() && w.fullscreen().is_none() && p.x > -30000 {
                        self.restore = Some((p.x, p.y, w.inner_size().width, w.inner_size().height));
                    }
                }
                self.refresh_period();
                paint = false;
            }
            WindowEvent::ScaleFactorChanged { .. } => self.refresh_period(),
            WindowEvent::Occluded(o) => {
                self.occluded = o;
                paint = !o;
            }
            WindowEvent::Focused(f) => {
                if f {
                    // otra copia dejó un libro para abrir aquí
                    let p = db::data_dir().join("inbox.txt");
                    if let Ok(s) = std::fs::read_to_string(&p) {
                        let _ = std::fs::remove_file(&p);
                        app.add_paths(s.lines().map(PathBuf::from).collect());
                    }
                } else {
                    i.mods = ui::Mods::default();
                    self.mods = ModifiersState::empty();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x as f32 / dpi, position.y as f32 / dpi);
                paint = p != i.pos;
                i.pos = p;
                i.moved = true;
            }
            WindowEvent::CursorLeft { .. } => {
                if !i.down {
                    i.pos = (-1000.0, -1000.0);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let down = state == ElementState::Pressed;
                match button {
                    MouseButton::Left => {
                        if down {
                            let (t, p, n) = self.last_click;
                            let near = (p.0 - i.pos.0).abs() < 5.0 && (p.1 - i.pos.1).abs() < 5.0;
                            let n = if near && t.elapsed() < Duration::from_millis(400) { n % 3 + 1 } else { 1 };
                            self.last_click = (Instant::now(), i.pos, n);
                            i.clicks = n;
                            i.pressed = true;
                            i.down = true;
                        } else {
                            i.released = true;
                            i.down = false;
                        }
                    }
                    MouseButton::Right if down => i.rpressed = true,
                    MouseButton::Back if down => i.back = true,
                    MouseButton::Forward if down => i.forward = true,
                    _ => {}
                }
                // pulsar y soltar no se funden en el mismo fotograma
                self.redraw();
                self.requests(el);
                return;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x * 96.0, y * 96.0),
                    MouseScrollDelta::PixelDelta(p) => (p.x as f32 / dpi, p.y as f32 / dpi),
                };
                if i.mods.shift && x == 0.0 {
                    i.wheel.0 += y;
                } else {
                    i.wheel.0 += x;
                    i.wheel.1 += y;
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
                i.mods = ui::Mods { ctrl: self.mods.control_key(), shift: self.mods.shift_key(), alt: self.mods.alt_key() };
                paint = false;
            }
            WindowEvent::KeyboardInput { event, is_synthetic, .. } => {
                if is_synthetic || event.state != ElementState::Pressed {
                    return;
                }
                let m = i.mods;
                if !m.ctrl || m.alt {
                    if let Some(t) = &event.text {
                        if !matches!(event.logical_key, Key::Named(winit::keyboard::NamedKey::Enter | winit::keyboard::NamedKey::Tab | winit::keyboard::NamedKey::Escape | winit::keyboard::NamedKey::Backspace)) {
                            i.text.push_str(t);
                        }
                    }
                }
                i.keys.push(ui::KeyEv { key: event.logical_key, mods: m });
            }
            WindowEvent::Ime(Ime::Commit(s)) => i.text.push_str(&s),
            WindowEvent::HoveredFile(_) => app.drop_hover = true,
            WindowEvent::HoveredFileCancelled => app.drop_hover = false,
            WindowEvent::DroppedFile(p) => {
                app.drop_hover = false;
                app.add_paths(vec![p]);
            }
            _ => paint = false,
        }
        if paint {
            if let Some(w) = &self.win {
                w.request_redraw();
            }
        }
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(app) = &self.app else { return };
        if self.idle && app.ui.wake < 1000.0 {
            let at = self.last + Duration::from_secs_f32(app.ui.wake.max(0.004));
            if at <= Instant::now() {
                if let Some(w) = &self.win {
                    w.request_redraw();
                }
                el.set_control_flow(ControlFlow::Wait);
            } else {
                el.set_control_flow(ControlFlow::WaitUntil(at));
            }
        } else {
            el.set_control_flow(ControlFlow::Wait);
        }
    }
}

trait IconOpt {
    fn from_path_or_none() -> Option<Icon>;
}
impl IconOpt for Icon {
    fn from_path_or_none() -> Option<Icon> {
        use winit::platform::windows::IconExtWindows;
        Icon::from_resource(1, Some(PhysicalSize::new(48, 48))).ok()
    }
}

/// Si ya está abierta, se le pasa el archivo y se trae al frente.
fn single(args: &[PathBuf]) -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::*;
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        let m = CreateMutexW(None, true, w!("Saga.lector.unico"));
        if m.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS {
            let inbox = db::data_dir().join("inbox.txt");
            if !args.is_empty() {
                let _ = std::fs::write(&inbox, args.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>().join("\n"));
            }
            if let Ok(h) = FindWindowW(None, w!("Saga")) {
                if IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
                let _ = SetForegroundWindow(h);
            }
            return false;
        }
        std::mem::forget(m);
    }
    true
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--shot") {
        shot::run(&args[1]);
        return;
    }
    let files: Vec<PathBuf> = args.iter().map(PathBuf::from).filter(|p| p.exists()).collect();
    if !single(&files) {
        return;
    }
    std::panic::set_hook(Box::new(|i| {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(db::data_dir().join("error.log")) {
            let _ = writeln!(f, "{i}");
        }
    }));
    let el = EventLoop::with_user_event().build().expect("bucle de eventos");
    el.set_control_flow(ControlFlow::Wait);
    let proxy = el.create_proxy();
    let mut r = Runner {
        proxy,
        args: files,
        win: None,
        app: None,
        last: Instant::now(),
        period: 1.0 / 60.0,
        acc: 0.0,
        idle: true,
        mods: ModifiersState::empty(),
        cursor: CursorIcon::Default,
        hidden: false,
        last_click: (Instant::now(), (0.0, 0.0), 0),
        ime: false,
        shown: false,
        occluded: false,
        restore: None,
    };
    let _ = el.run_app(&mut r);
}
