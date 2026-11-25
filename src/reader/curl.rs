//! La hoja que se dobla: cada tira se enrolla sobre un cilindro y cae al otro lado.
use crate::gfx::*;

pub struct Sheet {
    /// Página que se mueve (en reposo) y su lomo en el borde izquierdo (o derecho si `flip`).
    pub r: Rect,
    /// La hoja gira hacia la derecha (pasar atrás).
    pub flip: bool,
    pub front: TexId,
    /// Reverso; si no hay, se ve el papel al trasluz.
    pub back: Option<TexId>,
    pub paper: Color,
    /// 0 = en su sitio, 1 = al otro lado.
    pub t: f32,
    /// Inclinación del doblez (positivo: tira de abajo).
    pub tilt: f32,
}

#[derive(Clone, Copy)]
struct P {
    x: f32,
    y: f32,
}
/// Recorta un polígono convexo por la franja a <= d < b.
fn cut(poly: &[P], n: P, lo: f32, hi: f32) -> Vec<P> {
    let clip = |poly: &[P], f: &dyn Fn(P) -> f32| -> Vec<P> {
        let mut out = Vec::with_capacity(poly.len() + 2);
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let (da, db) = (f(a), f(b));
            if da >= 0.0 {
                out.push(a);
            }
            if (da > 0.0 && db < 0.0) || (da < 0.0 && db > 0.0) {
                let t = da / (da - db);
                out.push(P { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t });
            }
        }
        out
    };
    let p = clip(poly, &|q| q.x * n.x + q.y * n.y - lo);
    if p.len() < 3 {
        return vec![];
    }
    clip(&p, &|q| hi - (q.x * n.x + q.y * n.y))
}

pub fn draw(d: &mut Draw, s: &Sheet, clip_single: Option<Rect>) {
    let (w, h) = (s.r.w, s.r.h);
    let t = s.t.clamp(0.0, 1.0);
    let pi = std::f32::consts::PI;
    // el doblez recorre la página y el cilindro se cierra en los extremos
    let rad = (w * 0.13) * (pi * t).sin().max(0.0).powf(0.7) + 0.5;
    let ang = s.tilt * (1.0 - t).min(1.0) * (t * 8.0).min(1.0);
    let n = P { x: ang.cos(), y: ang.sin() };
    // borde libre: de w a -w
    let edge = w * (1.0 - 2.0 * t);
    let fold = (edge + w - pi * rad * 0.5 * (pi * t).sin()) / 2.0;
    let fold = fold.clamp(0.0, w);
    // distancia al doblez: se mide desde el punto (fold, h/2)
    let c = fold * n.x + h * 0.5 * n.y;
    let quad = [P { x: 0.0, y: 0.0 }, P { x: w, y: 0.0 }, P { x: w, y: h }, P { x: 0.0, y: h }];
    let sign = if s.flip { -1.0 } else { 1.0 };
    let x0 = if s.flip { s.r.r() } else { s.r.x };
    let mut far: f32 = 0.0;
    for q in quad {
        far = far.max(q.x * n.x + q.y * n.y);
    }
    let near = quad.iter().map(|q| q.x * n.x + q.y * n.y).fold(f32::MAX, f32::min);
    let arc = pi * rad;
    let cam = 2600.0;
    let cen = (s.r.cx(), s.r.cy());

    // sombra que la hoja arroja sobre lo de abajo
    let lift = (pi * t).sin().max(0.0);
    if lift > 0.01 {
        let shade = |d: &mut Draw, xa: f32, xb: f32, a0: f32, a1: f32| {
            let mut v = vec![];
            let mk = |x: f32, y: f32, a: f32| MeshV { pos: [x0 + sign * x, s.r.y + y, 0.0, 1.0], uv: [0.0; 2], local: [1.0; 2], size: [2.0; 2], color: [0.0, 0.0, 0.0, a], radius: [0.0; 4], misc: [5.0, 1.0, 0.0, 0.0], clip: [0.0; 4] };
            let sk = |y: f32| (h * 0.5 - y) * n.y / n.x.max(0.2);
            let (p0, p1, p2, p3) = (mk(xa + sk(0.0), 0.0, a0), mk(xb + sk(0.0), 0.0, a1), mk(xa + sk(h), h, a0), mk(xb + sk(h), h, a1));
            v.extend([p0, p1, p2, p2, p1, p3]);
            d.tris(TEX_WHITE, &v);
        };
        d.push_clip(match clip_single {
            Some(c) => c,
            None => rect(s.r.x - w, s.r.y, w * 3.0, h),
        }, 0.0);
        // delante del rollo
        shade(d, fold + rad * 0.2, fold + rad + w * 0.16 * lift, 0.34 * lift, 0.0);
        // bajo el borde que cae al otro lado
        let e = fold - (w - fold - arc).max(0.0);
        if w - fold > arc {
            shade(d, e - w * 0.10 * lift, e, 0.0, 0.26 * lift);
            shade(d, e, fold, 0.26 * lift, 0.16 * lift);
        }
        d.pop_clip();
    }

    if let Some(c) = clip_single {
        d.push_clip(c, 0.0);
    }
    // tiras: plana, rollo y reverso
    let mut stops = vec![near - 1.0, c];
    let steps = 26;
    for k in 1..=steps {
        stops.push(c + arc * k as f32 / steps as f32);
    }
    stops.push(far.max(c + arc) + 1.0);
    for i in 0..stops.len() - 1 {
        let (lo, hi) = (stops[i], stops[i + 1]);
        if hi <= near || lo >= far {
            continue;
        }
        let poly = cut(&quad, n, lo, hi);
        if poly.len() < 3 {
            continue;
        }
        let mid = ((lo + hi) / 2.0 - c).max(0.0);
        let th = (mid / rad).min(pi);
        let front = th < pi / 2.0;
        let tex = if front { s.front } else { s.back.unwrap_or(s.front) };
        let mut vs: Vec<MeshV> = Vec::with_capacity(poly.len());
        for q in &poly {
            let dist = q.x * n.x + q.y * n.y - c;
            let (shift, z, light) = if dist <= 0.0 {
                // ligera sombra al acercarse al doblez
                let k = (1.0 + dist / (rad * 1.6)).clamp(0.0, 1.0);
                (0.0, 0.0, 1.0 - 0.10 * k * k * lift)
            } else if dist < arc {
                let a = dist / rad;
                let nd = rad * a.sin();
                let z = rad * (1.0 - a.cos());
                let dark = s.paper.lum() < 0.25;
                let l = if dark && a > pi * 0.3 { let b = (a - pi * 0.3) / (pi * 0.7); 0.62 + 0.38 * b + 0.62 * (pi * b).sin() * (1.0 - b * 0.4) } else if a < pi / 2.0 { 1.0 - 0.10 * lift - 0.40 * (a / (pi / 2.0)).powf(1.4) * lift } else { let b = (a - pi / 2.0) / (pi / 2.0); lerp(0.62, 1.0, b.powf(0.6)) };
                (nd - dist, z, lerp(1.0, l, lift.min(1.0).max(0.2)))
            } else {
                (-(dist - arc) - dist, 2.0 * rad, 1.0)
            };
            let x = q.x + n.x * shift;
            let y = q.y + n.y * shift;
            let (px, py) = (x0 + sign * x, s.r.y + y);
            let k = (cam - z) / cam;
            let (sx, sy) = (cen.0 + (px - cen.0) / k, cen.1 + (py - cen.1) / k);
            let u0 = q.x / w;
            // frente: si la hoja pasa hacia atrás el lomo está a la derecha
            let u = match (front || s.back.is_none(), s.flip) {
                (true, false) => u0,
                (true, true) => 1.0 - u0,
                (false, false) => 1.0 - u0,
                (false, true) => u0,
            };
            let kind = if !front && s.back.is_none() { 7.0 } else { 4.0 };
            let col = if kind == 7.0 { s.paper.arr() } else { [1.0; 4] };
            vs.push(MeshV { pos: [sx, sy, 0.0, k], uv: [u, q.y / h], local: [q.x, q.y], size: [w, h], color: col, radius: [0.0; 4], misc: [kind, light, 0.0, 0.0], clip: [0.0; 4] });
        }
        let mut tri = Vec::with_capacity((vs.len() - 2) * 3);
        for k in 1..vs.len() - 1 {
            tri.extend([vs[0], vs[k], vs[k + 1]]);
        }
        d.tris(tex, &tri);
    }
    if clip_single.is_some() {
        d.pop_clip();
    }
}
