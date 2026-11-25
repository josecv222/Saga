//! El librito: cubierta, lomo, tapas y canto de hojas con perspectiva real.
use crate::gfx::*;
use crate::text::fonts;
use crate::ui::Ui;

pub const CAM: f32 = 1500.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub cx: f32,
    pub cy: f32,
    pub h: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub z: f32,
    /// Apertura de la tapa, 0..1.
    pub open: f32,
}
impl Pose {
    pub fn lerp(&self, o: &Pose, t: f32) -> Pose {
        Pose { cx: lerp(self.cx, o.cx, t), cy: lerp(self.cy, o.cy, t), h: lerp(self.h, o.h, t), yaw: lerp(self.yaw, o.yaw, t), pitch: lerp(self.pitch, o.pitch, t), z: lerp(self.z, o.z, t), open: lerp(self.open, o.open, t) }
    }
}
#[derive(Clone)]
pub struct Look {
    pub tex: Option<(TexId, f32)>,
    /// Proporción de la imagen y del libro.
    pub img_aspect: f32,
    pub aspect: f32,
    /// Grosor relativo a la altura.
    pub thick: f32,
    pub color: Color,
    pub title: String,
    pub author: String,
    pub dim: f32,
    pub spine_text: bool,
    pub paper: Color,
    /// Páginas que se ven al abrirlo: izquierda y derecha.
    pub inner: [Option<TexId>; 2],
    /// Opacidad de la tapa abierta.
    pub lid: f32,
}
pub fn book_aspect(a: f32) -> f32 {
    a.clamp(0.60, 0.80)
}
pub fn thickness(pages: i32) -> f32 {
    let p = if pages <= 0 { 260.0 } else { pages as f32 };
    0.028 + 0.052 * (p / 900.0).clamp(0.05, 1.0).sqrt()
}

/// `Pose::open` con la tapa del todo tumbada.
pub const FLAT: f32 = std::f32::consts::PI / 2.75;
type V3 = [f32; 3];
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sc(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
struct Xf {
    cy: f32,
    sy: f32,
    cp: f32,
    sp: f32,
    o: V3,
    vp: (f32, f32),
}
impl Xf {
    fn rot(&self, p: V3) -> V3 {
        let x = p[0] * self.cy + p[2] * self.sy;
        let z = -p[0] * self.sy + p[2] * self.cy;
        let y = p[1] * self.cp - z * self.sp;
        let z2 = p[1] * self.sp + z * self.cp;
        [x, y, z2]
    }
    fn world(&self, p: V3) -> V3 {
        add(self.rot(p), self.o)
    }
    fn proj(&self, w: V3) -> [f32; 4] {
        let k = (CAM - w[2]).max(40.0) / CAM;
        [self.vp.0 + w[0] / k, self.vp.1 + w[1] / k, 0.0, k]
    }
}
struct Face {
    o: V3,
    u: V3,
    v: V3,
    n: V3,
    size: (f32, f32),
}
#[derive(Clone, Copy)]
struct Mat {
    kind: f32,
    color: Color,
    uv: [f32; 4],
    radius: [f32; 4],
    z: f32,
    w: f32,
    light: f32,
    /// Opacidad arriba y abajo (reflejos).
    fade: (f32, f32),
}
impl Mat {
    fn new(kind: f32, color: Color) -> Mat {
        Mat { kind, color, uv: [0.0, 0.0, 1.0, 1.0], radius: [0.0; 4], z: 0.0, w: 0.0, light: 1.0, fade: (1.0, 1.0) }
    }
}
fn facing(x: &Xf, f: &Face) -> bool {
    let c = x.world(add(f.o, add(sc(f.u, f.size.0 / 2.0), sc(f.v, f.size.1 / 2.0))));
    let n = x.rot(f.n);
    n[0] * -c[0] + n[1] * -c[1] + n[2] * (CAM - c[2]) > 0.0
}
fn emit(d: &mut Draw, x: &Xf, f: &Face, tex: TexId, m: &Mat, mirror: Option<f32>) {
    let s = 1.3 / d.scale().max(0.2);
    let (w, h) = f.size;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let mut q = [MeshV { pos: [0.0; 4], uv: [0.0; 2], local: [0.0; 2], size: [w, h], color: m.color.arr(), radius: m.radius, misc: [m.kind, m.light, m.z, m.w], clip: [0.0; 4] }; 4];
    for (i, (a, b)) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].into_iter().enumerate() {
        let lu = if a > 0.0 { w + s } else { -s };
        let lv = if b > 0.0 { h + s } else { -s };
        let p = x.world(add(f.o, add(sc(f.u, lu), sc(f.v, lv))));
        let mut pos = x.proj(p);
        if let Some(fl) = mirror {
            pos[1] = 2.0 * fl - pos[1];
        }
        q[i].pos = pos;
        q[i].local = [lu, lv];
        q[i].uv = [lerp(m.uv[0], m.uv[2], lu / w), lerp(m.uv[1], m.uv[3], lv / h)];
        q[i].color[3] *= lerp(m.fade.0, m.fade.1, b);
    }
    d.tris(tex, &[q[0], q[1], q[2], q[2], q[1], q[3]]);
}
fn light(x: &Xf, n: V3) -> f32 {
    let n = x.rot(n);
    let l = [-0.30f32, -0.28, 0.91];
    let k = n[0] * l[0] + n[1] * l[1] + n[2] * l[2];
    0.50 + 0.52 * k.max(0.0)
}
/// Recorte para que la imagen llene la cubierta.
pub fn fill_uv(img: f32, want: f32) -> [f32; 4] {
    if img > want {
        let k = want / img;
        [(1.0 - k) / 2.0, 0.0, (1.0 + k) / 2.0, 1.0]
    } else {
        let k = img / want;
        // se pierde más de abajo que de arriba: el título suele estar arriba
        [0.0, (1.0 - k) * 0.35, 1.0, (1.0 - k) * 0.35 + k]
    }
}
fn xf(p: &Pose, vp: (f32, f32)) -> Xf {
    Xf { cy: p.yaw.cos(), sy: p.yaw.sin(), cp: p.pitch.cos(), sp: p.pitch.sin(), o: [p.cx - vp.0, p.cy - vp.1, p.z], vp }
}
/// Rectángulo que ocupa en pantalla.
pub fn bounds(p: &Pose, l: &Look, vp: (f32, f32)) -> Rect {
    let x = xf(p, vp);
    let (w, h, t) = (p.h * l.aspect, p.h, p.h * l.thick);
    let (mut a, mut b, mut c, mut e) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for i in 0..8 {
        let s = |k: i32| if i & k != 0 { 0.5 } else { -0.5 };
        let q = x.proj(x.world([s(1) * w, s(2) * h, s(4) * t]));
        a = a.min(q[0]);
        b = b.min(q[1]);
        c = c.max(q[0]);
        e = e.max(q[1]);
    }
    rect(a, b, c - a, e - b)
}

pub fn draw(ui: &mut Ui, p: &Pose, l: &Look, vp: (f32, f32)) {
    draw_in(ui, p, l, vp, None, 1.0);
}
/// Reflejo en el suelo, desvaneciéndose.
pub fn reflect(ui: &mut Ui, p: &Pose, l: &Look, vp: (f32, f32), floor: f32, strength: f32) {
    if strength > 0.01 {
        draw_in(ui, p, l, vp, Some(floor), strength);
    }
}
fn draw_in(ui: &mut Ui, p: &Pose, l: &Look, vp: (f32, f32), mirror: Option<f32>, strength: f32) {
    let x = xf(p, vp);
    let h = p.h;
    let w = h * l.aspect;
    let t = h * l.thick;
    let (hw, hh, ht) = (w / 2.0, h / 2.0, t / 2.0);
    let bt = (t * 0.10).clamp(0.6, 3.0);
    let ov = h * 0.0075;
    let r = (h * 0.008).max(0.8);
    let dim = l.dim;
    let is_mirror = mirror.is_some();
    let fade = if is_mirror { (-0.75 * strength, strength) } else { (1.0, 1.0) };
    let d = &mut ui.d;
    let cloth = l.color;
    let paper = l.paper;

    let front = Face { o: [-hw, -hh, ht], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], n: [0.0, 0.0, 1.0], size: (w, h) };
    let back = Face { o: [hw, -hh, -ht], u: [-1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], n: [0.0, 0.0, -1.0], size: (w, h) };
    let spine = Face { o: [-hw, -hh, -ht], u: [0.0, 0.0, 1.0], v: [0.0, 1.0, 0.0], n: [-1.0, 0.0, 0.0], size: (t, h) };
    // tapa trasera vista por dentro (asoma por los cantos)
    let binn = Face { o: [-hw, -hh, -ht + bt], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], n: [0.0, 0.0, 1.0], size: (w, h) };
    let pz0 = -ht + bt;
    let pt = t - 2.0 * bt;
    let pr = Face { o: [hw - ov, -hh + ov, ht - bt], u: [0.0, 0.0, -1.0], v: [0.0, 1.0, 0.0], n: [1.0, 0.0, 0.0], size: (pt, h - 2.0 * ov) };
    let pt_f = Face { o: [-hw + bt, -hh + ov, pz0], u: [1.0, 0.0, 0.0], v: [0.0, 0.0, 1.0], n: [0.0, -1.0, 0.0], size: (w - ov - bt, pt) };
    let pb_f = Face { o: [-hw + bt, hh - ov, ht - bt], u: [1.0, 0.0, 0.0], v: [0.0, 0.0, -1.0], n: [0.0, 1.0, 0.0], size: (w - ov - bt, pt) };

    let solid = |c: Color, li: f32| {
        let mut m = Mat::new(6.0, c);
        m.light = li * dim;
        m.fade = fade;
        m
    };
    let cover_mat = |li: f32| {
        let mut m = Mat::new(1.0, cloth);
        m.light = li * dim;
        m.radius = [1.0, r, r, 1.0];
        m.fade = fade;
        m
    };
    let front_seen = facing(&x, &front);
    let open = p.open > 0.001;
    let board_edges = |d: &mut Draw, z1: f32| {
        // cantos de una tapa: derecha, arriba y abajo
        let c = cloth.mul(0.92);
        let e = [
            Face { o: [hw, -hh, z1], u: [0.0, 0.0, -1.0], v: [0.0, 1.0, 0.0], n: [1.0, 0.0, 0.0], size: (bt, h) },
            Face { o: [-hw, -hh, z1 - bt], u: [1.0, 0.0, 0.0], v: [0.0, 0.0, 1.0], n: [0.0, -1.0, 0.0], size: (w, bt) },
            Face { o: [-hw, hh, z1], u: [1.0, 0.0, 0.0], v: [0.0, 0.0, -1.0], n: [0.0, 1.0, 0.0], size: (w, bt) },
        ];
        for f in &e {
            if facing(&x, f) {
                emit(d, &x, f, TEX_WHITE, &solid(c, light(&x, f.n).max(0.45)), mirror);
            }
        }
    };
    let pages = |d: &mut Draw| {
        for (f, vert) in [(&pr, 1.0), (&pt_f, 0.0), (&pb_f, 0.0)] {
            if facing(&x, f) {
                let mut m = Mat::new(2.0, paper);
                m.light = (light(&x, f.n) * 0.55 + 0.42) * dim;
                m.z = 0.55;
                m.w = vert;
                m.fade = fade;
                emit(d, &x, f, TEX_WHITE, &m, mirror);
            }
        }
    };
    if front_seen {
        if !is_mirror && facing(&x, &binn) {
            emit(d, &x, &binn, TEX_WHITE, &solid(cloth.mul(0.55), 0.8), mirror);
        }
        board_edges(d, -ht + bt);
        pages(d);
        if open && !is_mirror {
            let top = Face { o: [-hw + bt, -hh + ov, ht - bt], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], n: [0.0, 0.0, 1.0], size: (w - ov - bt, h - 2.0 * ov) };
            emit(d, &x, &top, TEX_WHITE, &solid(paper, 1.0), mirror);
            if let Some(tex) = l.inner[1] {
                let k = ov * (1.0 - p.open / FLAT).max(0.0);
                let top = Face { o: [-hw, -hh + k, ht - bt], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], n: [0.0, 0.0, 1.0], size: (w - k, h - 2.0 * k) };
                emit(d, &x, &top, tex, &Mat::new(4.0, Color::WHITE), mirror);
            }
        }
    } else {
        board_edges(d, ht);
        pages(d);
        board_edges(d, -ht + bt);
        if facing(&x, &back) {
            let mut m = cover_mat(light(&x, back.n));
            m.radius = [r, 1.0, 1.0, r];
            emit(d, &x, &back, TEX_WHITE, &m, mirror);
        }
    }
    let spine_seen = facing(&x, &spine);
    if spine_seen {
        let mut m = Mat::new(1.0, cloth.mul(0.97));
        m.light = (light(&x, spine.n) * 0.75 + 0.30) * dim;
        m.z = 0.9;
        m.fade = fade;
        emit(d, &x, &spine, TEX_WHITE, &m, mirror);
    }
    if front_seen || open {
        let ang = p.open * 2.75;
        if !open {
            board_edges(d, ht);
        }
        let hinge = |q: V3| -> V3 {
            let (dx, dz) = (q[0] + hw, q[2] - ht);
            [-hw + dx * ang.cos() - dz * ang.sin(), q[1], ht + dx * ang.sin() + dz * ang.cos()]
        };
        let hn = [-(ang.sin()), 0.0, ang.cos()];
        let f = Face { o: hinge(front.o), u: [ang.cos(), 0.0, ang.sin()], v: front.v, n: hn, size: (w, h) };
        if facing(&x, &f) {
            let mut m = cover_mat(light(&x, hn));
            match l.tex {
                Some((tex, a)) => {
                    if a < 1.0 {
                        emit(d, &x, &f, TEX_WHITE, &m, mirror);
                    }
                    m.kind = 0.0;
                    m.color = Color::WHITE.a(a);
                    m.uv = fill_uv(l.img_aspect, l.aspect);
                    m.z = 0.55 + p.yaw * 0.9 + ang * 0.4;
                    emit(d, &x, &f, tex, &m, mirror);
                }
                None => emit(d, &x, &f, TEX_WHITE, &m, mirror),
            }
        } else if open {
            // guarda interior
            let inn = Face { o: add(f.o, sc(f.u, w)), u: sc(f.u, -1.0), v: f.v, n: sc(hn, -1.0), size: (w, h) };
            emit(d, &x, &inn, TEX_WHITE, &solid(cloth.mix(paper, 0.25).mul(0.8).a(l.lid), 0.9), mirror);
            let pad = Face { o: add(add(f.o, sc(f.u, w - ov)), sc(f.v, ov)), u: sc(f.u, -1.0), v: f.v, n: sc(hn, -1.0), size: (w - ov, h - 2.0 * ov) };
            emit(d, &x, &pad, TEX_WHITE, &solid(paper.mul(0.96).a(l.lid), 1.0), mirror);
            if let Some(tex) = l.inner[0] {
                let k = ov * (1.0 - p.open / FLAT).max(0.0);
                let pad = Face { o: add(add(f.o, sc(f.u, w - k)), sc(f.v, k)), u: sc(f.u, -1.0), v: f.v, n: sc(hn, -1.0), size: (w - k, h - 2.0 * k) };
                let mut m = Mat::new(4.0, Color::WHITE.a(l.lid));
                m.light = 0.74 - 0.26 * ang.cos();
                emit(d, &x, &pad, tex, &m, mirror);
            }
        }
    }
    if spine_seen && l.spine_text && !is_mirror {
        let seen = t * p.yaw.sin().abs();
        if seen > 9.0 {
            spine_text(ui, &x, l, w, h, t, ((seen - 9.0) / 4.0).min(1.0));
        }
    }
}
fn spine_text(ui: &mut Ui, x: &Xf, l: &Look, w: f32, h: f32, t: f32, alpha: f32) {
    let size = (t * 0.44).clamp(5.0, 17.0);
    let fg = if l.color.lum() > 0.55 { Color::hex(0x1C1A17) } else { Color::hex(0xF4EEDF) };
    let room = h * 0.80;
    let asz = size * 0.72;
    let author = ui.t.fit(fonts().ui, asz, &l.author, room * 0.34);
    let aw = if l.author.is_empty() { 0.0 } else { ui.t.measure(fonts().ui, asz, &author) };
    let title = ui.t.fit(fonts().ui_m, size, &l.title, room - aw - 14.0);
    let dpi = ui.d.dpi;
    for (s, f, sz, at, a) in [(title, fonts().ui_m, size, -h / 2.0 + h * 0.09, 0.95), (author, fonts().ui, asz, h / 2.0 - h * 0.09 - aw, 0.75)] {
        let (qs, _) = ui.t.quads(f, sz, &s, 0.0, dpi);
        let mut v = Vec::with_capacity(qs.len() * 6);
        for (r, uv) in qs {
            // la línea base queda hacia la contracubierta
            let mk = |px: f32, py: f32, u: f32, vv: f32| MeshV {
                pos: x.proj(x.world([-w / 2.0 - 0.15, at + px, -sz * 0.34 - py])),
                uv: [u, vv],
                local: [1.0; 2],
                size: [2.0; 2],
                color: fg.a(a * alpha).arr(),
                radius: [0.0; 4],
                misc: [3.0, l.dim, 0.0, 0.0],
                clip: [0.0; 4],
            };
            let (a0, b0, a1, b1) = (mk(r.x, r.y, uv[0], uv[1]), mk(r.r(), r.y, uv[2], uv[1]), mk(r.x, r.b(), uv[0], uv[3]), mk(r.r(), r.b(), uv[2], uv[3]));
            v.extend([a0, b0, a1, a1, b0, b1]);
        }
        ui.d.tris(TEX_ATLAS, &v);
    }
}
/// Sombra suave sobre el suelo.
pub fn floor_shadow(ui: &mut Ui, p: &Pose, l: &Look, vp: (f32, f32), floor: f32, a: f32) {
    let b = bounds(p, l, vp);
    ui.d.shadow(rect(b.x - b.w * 0.02, floor - 5.0, b.w * 1.04, 10.0), 5.0, 26.0, Color::BLACK.a(0.55 * a));
    ui.d.shadow(rect(b.x + b.w * 0.03, floor - 2.0, b.w * 0.94, 4.0), 2.0, 7.0, Color::BLACK.a(0.45 * a));
}
