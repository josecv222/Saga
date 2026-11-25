struct Globals { size: vec2<f32>, time: f32, pad: f32 };
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var smp: sampler;

fn sd_round(p: vec2<f32>, half: vec2<f32>, r4: vec4<f32>) -> f32 {
    // r4: tl, tr, br, bl
    let top = select(r4.wz, r4.xy, p.y < 0.0);
    let r = min(select(top.x, top.y, p.x > 0.0), min(half.x, half.y));
    let q = abs(p) - half + vec2<f32>(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
}
fn hash(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}
fn lum(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.299, 0.587, 0.114)); }

struct VIn {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) color2: vec4<f32>,
    @location(4) radius: vec4<f32>,
    @location(5) clip: vec4<f32>,
    @location(6) misc: vec4<f32>,   // kind, ancho borde, desenfoque, radio del recorte
    @location(7) border: vec4<f32>,
    @location(8) rot: vec4<f32>,    // cos, sin, dirección degradado, extra
};
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) p: vec2<f32>,
    @location(2) px: vec2<f32>,
    @location(3) @interpolate(flat) half: vec2<f32>,
    @location(4) @interpolate(flat) color: vec4<f32>,
    @location(5) @interpolate(flat) color2: vec4<f32>,
    @location(6) @interpolate(flat) radius: vec4<f32>,
    @location(7) @interpolate(flat) clip: vec4<f32>,
    @location(8) @interpolate(flat) misc: vec4<f32>,
    @location(9) @interpolate(flat) border: vec4<f32>,
    @location(10) @interpolate(flat) extra: vec2<f32>,
};
@vertex
fn vs(i: VIn, @builtin(vertex_index) vi: u32) -> VOut {
    var o: VOut;
    let c = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let kind = i.misc.x;
    var pad = 1.0;
    if (kind == 4.0) { pad = i.misc.z * 1.6 + 1.0; }
    if (kind == 2.0) { pad = 0.0; }
    let half = i.rect.zw * 0.5;
    let p = (c * 2.0 - 1.0) * (half + vec2<f32>(pad));
    let rp = vec2<f32>(p.x * i.rot.x - p.y * i.rot.y, p.x * i.rot.y + p.y * i.rot.x);
    let px = i.rect.xy + half + rp;
    o.pos = vec4<f32>(px.x / g.size.x * 2.0 - 1.0, 1.0 - px.y / g.size.y * 2.0, 0.0, 1.0);
    let t = (p + half) / max(i.rect.zw, vec2<f32>(0.001));
    o.uv = mix(i.uv.xy, i.uv.zw, t);
    o.p = p; o.px = px; o.half = half;
    o.color = i.color; o.color2 = i.color2; o.radius = i.radius; o.clip = i.clip;
    o.misc = i.misc; o.border = i.border; o.extra = i.rot.zw;
    return o;
}
fn erf(x: f32) -> f32 {
    let a = abs(x);
    let t = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
    return sign(x) * (1.0 - 1.0 / (t * t * t * t));
}
@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let kind = i32(i.misc.x + 0.5);
    let s = textureSample(tex, smp, i.uv);
    let cc = (i.clip.xy + i.clip.zw) * 0.5;
    let ch = (i.clip.zw - i.clip.xy) * 0.5;
    let cd = sd_round(i.px - cc, ch, vec4<f32>(i.misc.w));
    let clip = clamp(0.5 - cd, 0.0, 1.0);
    if (clip <= 0.0) { discard; }
    let d = sd_round(i.p, i.half, i.radius);
    var col = i.color;
    var cov = clamp(0.5 - d, 0.0, 1.0);
    if (kind == 2) {                       // glifo
        let l = lum(i.color.rgb);
        let c = pow(s.r, mix(1.0, 0.80, l));
        let a = c * i.color.a * clip;
        return vec4<f32>(i.color.rgb * a, a);
    }
    if (kind == 4) {                       // sombra
        let sg = max(i.misc.z * 0.5, 0.5);
        cov = 0.5 - 0.5 * erf(d / (sg * 1.41421));
        let a = cov * i.color.a * clip;
        return vec4<f32>(i.color.rgb * a, a);
    }
    if (kind == 6) {                       // arco de progreso
        let r = min(i.half.x, i.half.y);
        let w = i.misc.y;
        let ring = abs(length(i.p) - (r - w * 0.5)) - w * 0.5;
        var ang = atan2(i.p.x, -i.p.y) / 6.2831853;
        if (ang < 0.0) { ang += 1.0; }
        let on = select(0.0, 1.0, ang <= i.extra.y);
        let c = clamp(0.5 - ring, 0.0, 1.0);
        let a = mix(i.color2.a, i.color.a, on);
        let rgb = mix(i.color2.rgb, i.color.rgb, on);
        return vec4<f32>(rgb * a * c * clip, a * c * clip);
    }
    if (kind == 1) {                       // imagen
        col = vec4<f32>(s.rgb * i.color.rgb, s.a * i.color.a);
    } else if (kind == 3) {                // imagen con alfa premultiplicado
        let a = cov * clip * i.color.a;
        return vec4<f32>(s.rgb * i.color.rgb * a, s.a * a);
    } else if (kind == 5) {                // página recoloreada: color2 = papel, border = tinta
        let l = lum(s.rgb);
        let m = mix(i.border.rgb, i.color2.rgb, l);
        let rc = clamp(m + (s.rgb - vec3<f32>(l)) * 0.92, vec3<f32>(0.0), vec3<f32>(1.0));
        col = vec4<f32>(mix(s.rgb, rc, i.extra.y), i.color.a);
        let a = col.a * cov * clip;
        return vec4<f32>(col.rgb * a, a);
    } else if (kind == 7) {                // fondo ambiental: imagen difuminada + viñeta + grano
        let v = i.p / i.half;
        let vig = 1.0 - smoothstep(0.25, 1.5, length(v * vec2<f32>(0.85, 1.0)));
        let img = s.rgb;
        let gray = vec3<f32>(lum(img));
        var c = mix(gray, img, i.extra.x);
        c = mix(i.color2.rgb, c, i.extra.y * vig);
        c += (hash(i.px) - 0.5) / 160.0;
        let a = i.color.a * clip;
        return vec4<f32>(c * a, a);
    } else {
        var t = (i.p.y + i.half.y) / max(i.half.y * 2.0, 0.001);
        if (i.extra.x == 1.0) { t = (i.p.x + i.half.x) / max(i.half.x * 2.0, 0.001); }
        if (i.extra.x == 2.0) { t = clamp(length(i.p / i.half), 0.0, 1.0); t = t * t * (3.0 - 2.0 * t); }
        col = mix(i.color, i.color2, clamp(t, 0.0, 1.0));
        col = vec4<f32>(col.rgb + (hash(i.px) - 0.5) / 255.0, col.a);
    }
    if (i.misc.y > 0.0) {
        let inner = clamp(0.5 - (d + i.misc.y), 0.0, 1.0);
        let b = i.border;
        let edge = (1.0 - inner) * b.a;
        let a = col.a + edge * (1.0 - col.a);
        let rgb = (col.rgb * col.a * (1.0 - edge) + b.rgb * edge) / max(a, 0.0001);
        col = vec4<f32>(rgb, a);
    }
    let a = col.a * cov * clip;
    return vec4<f32>(col.rgb * a, a);
}

// ---------------------------------------------------------------- mallas 3D
struct MIn {
    @location(0) pos: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) local: vec2<f32>,
    @location(3) size: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) radius: vec4<f32>,
    @location(6) misc: vec4<f32>,    // kind, luz, extra, extra2
    @location(7) clip: vec4<f32>,
};
struct MOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) local: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) @interpolate(flat) radius: vec4<f32>,
    @location(5) misc: vec4<f32>,
    @location(6) @interpolate(flat) clip: vec4<f32>,
};
@vertex
fn mvs(i: MIn) -> MOut {
    var o: MOut;
    o.pos = i.pos; o.uv = i.uv; o.local = i.local; o.size = i.size; o.color = i.color;
    o.radius = i.radius; o.misc = i.misc; o.clip = i.clip;
    return o;
}
@fragment
fn mfs(i: MOut) -> @location(0) vec4<f32> {
    let kind = i32(i.misc.x + 0.5);
    let s = textureSample(tex, smp, i.uv);
    let half = i.size * 0.5;
    let p = i.local - half;
    let d = sd_round(p, half, i.radius);
    let fw = max(fwidth(d), 0.0001);
    let fl = fwidth(i.local);
    var cov = clamp(0.5 - d / fw, 0.0, 1.0);
    let inside = i.pos.x >= i.clip.x && i.pos.y >= i.clip.y && i.pos.x <= i.clip.z && i.pos.y <= i.clip.w;
    if (!inside) { discard; }
    let u = i.local / i.size;
    var rgb = i.color.rgb;
    var a = i.color.a;
    if (kind == 0) {                       // cubierta con imagen
        rgb = s.rgb * i.color.rgb;
        let hx = u.x;
        let groove = exp(-pow((hx - 0.045) / 0.010, 2.0));
        let ridge = exp(-pow((hx - 0.062) / 0.012, 2.0));
        let roll = smoothstep(0.035, 0.0, hx);
        rgb *= 1.0 - 0.30 * groove - 0.22 * roll;
        rgb += vec3<f32>(0.10) * ridge * (0.35 + lum(rgb));
        let sh = u.x * 0.8 + (1.0 - u.y) * 0.55 - i.misc.z;
        let gl = exp(-sh * sh * 9.0) * 0.085;
        rgb += vec3<f32>(gl);
        let rim = clamp(1.0 + d / max(fw * 1.2, 0.8), 0.0, 1.0);
        rgb += vec3<f32>(0.09) * rim * rim;
    } else if (kind == 1) {                // tela: lomo y contracubierta
        let w = hash(floor(i.local * 1.5)) * 0.03;
        rgb = rgb * (0.985 + w);
        let x = u.x * 2.0 - 1.0;
        rgb *= mix(1.0, 1.0 - 0.42 * x * x, i.misc.z);
        rgb += vec3<f32>(0.055) * exp(-pow((x + 0.25) / 0.35, 2.0)) * i.misc.z;
    } else if (kind == 2) {                // canto de las hojas
        let n = i.misc.z;
        let vert = i.misc.w > 0.5;
        let coord = select(i.local.y, i.local.x, vert);
        let fc = select(fl.y, fl.x, vert) * n;
        let fade = clamp(1.0 - fc * 0.9, 0.0, 1.0);
        let line = 0.5 + 0.5 * sin(coord * n * 6.28318);
        let big = hash(vec2<f32>(floor(coord * 0.9), 3.0));
        rgb *= 1.0 - 0.13 * line * fade - 0.07 * big;
    } else if (kind == 3) {                // glifo en perspectiva
        let c = pow(s.r, mix(1.0, 0.8, lum(i.color.rgb)));
        let al = c * a;
        return vec4<f32>(rgb * i.misc.y * al, al);
    } else if (kind == 4) {                // hoja (textura)
        rgb = s.rgb * i.color.rgb;
    } else if (kind == 5) {                // sombra (degradado en vértices)
        return vec4<f32>(rgb * a, a);
    } else if (kind == 7) {                // reverso: papel con lo impreso al trasluz
        rgb = mix(i.color.rgb, s.rgb, 0.10);
    }
    rgb = rgb * i.misc.y;
    a = a * cov;
    if (a <= 0.001) { discard; }
    return vec4<f32>(rgb * a, a);
}
