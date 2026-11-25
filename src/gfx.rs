//! Motor de dibujo: una lista de órdenes por fotograma que la GPU pinta de una vez.
//! Rectángulos, imágenes, sombras y letras van por la misma tubería instanciada;
//! los libros y las hojas que se curvan son mallas con perspectiva real.
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
pub const fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}
impl Rect {
    pub fn has(&self, p: (f32, f32)) -> bool {
        p.0 >= self.x && p.1 >= self.y && p.0 < self.x + self.w && p.1 < self.y + self.h
    }
    pub fn r(&self) -> f32 {
        self.x + self.w
    }
    pub fn b(&self) -> f32 {
        self.y + self.h
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
    pub fn inset(&self, d: f32) -> Rect {
        rect(self.x + d, self.y + d, self.w - 2.0 * d, self.h - 2.0 * d)
    }
    pub fn grow(&self, d: f32) -> Rect {
        self.inset(-d)
    }
    pub fn off(&self, dx: f32, dy: f32) -> Rect {
        rect(self.x + dx, self.y + dy, self.w, self.h)
    }
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.r() && o.x < self.r() && self.y < o.b() && o.y < self.b()
    }
    pub fn lerp(&self, o: &Rect, t: f32) -> Rect {
        rect(lerp(self.x, o.x, t), lerp(self.y, o.y, t), lerp(self.w, o.w, t), lerp(self.h, o.h, t))
    }
}
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
pub fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
pub fn ease_out(t: f32) -> f32 {
    let t = 1.0 - t.clamp(0.0, 1.0);
    1.0 - t * t * t
}
pub fn ease_io(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Color(pub f32, pub f32, pub f32, pub f32);
impl Color {
    pub const fn hex(h: u32) -> Color {
        Color(((h >> 16) & 255) as f32 / 255.0, ((h >> 8) & 255) as f32 / 255.0, (h & 255) as f32 / 255.0, 1.0)
    }
    pub const WHITE: Color = Color(1.0, 1.0, 1.0, 1.0);
    pub const BLACK: Color = Color(0.0, 0.0, 0.0, 1.0);
    pub const fn a(self, a: f32) -> Color {
        Color(self.0, self.1, self.2, a)
    }
    pub fn fade(self, a: f32) -> Color {
        Color(self.0, self.1, self.2, self.3 * a)
    }
    pub fn mix(self, o: Color, t: f32) -> Color {
        Color(lerp(self.0, o.0, t), lerp(self.1, o.1, t), lerp(self.2, o.2, t), lerp(self.3, o.3, t))
    }
    pub fn mul(self, k: f32) -> Color {
        Color(self.0 * k, self.1 * k, self.2 * k, self.3)
    }
    pub fn lum(self) -> f32 {
        0.299 * self.0 + 0.587 * self.1 + 0.114 * self.2
    }
    pub fn arr(self) -> [f32; 4] {
        [self.0, self.1, self.2, self.3]
    }
    pub fn to_u32(self) -> u32 {
        let f = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
        f(self.0) << 16 | f(self.1) << 8 | f(self.2)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TexId(pub u32);
pub const TEX_WHITE: TexId = TexId(0);
pub const TEX_ATLAS: TexId = TexId(1);
pub const ATLAS: u32 = 2048;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Inst {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub color2: [f32; 4],
    pub radius: [f32; 4],
    pub clip: [f32; 4],
    pub misc: [f32; 4],
    pub border: [f32; 4],
    pub rot: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct MeshV {
    pub pos: [f32; 4],
    pub uv: [f32; 2],
    pub local: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub radius: [f32; 4],
    pub misc: [f32; 4],
    pub clip: [f32; 4],
}

enum Cmd {
    Quads { tex: TexId, start: u32, end: u32 },
    Mesh { tex: TexId, start: u32, end: u32 },
}

#[derive(Clone, Copy)]
struct Clip {
    r: [f32; 4],
    rad: f32,
}

/// Lista de dibujo con pilas de recorte, opacidad y transformación.
pub struct Draw {
    insts: Vec<Inst>,
    mesh: Vec<MeshV>,
    cmds: Vec<Cmd>,
    clips: Vec<Clip>,
    alphas: Vec<f32>,
    xf: Vec<(f32, f32, f32)>,
    /// Tamaño físico del destino.
    pub size: (f32, f32),
    pub dpi: f32,
}
impl Draw {
    pub fn new(w: f32, h: f32, dpi: f32) -> Draw {
        Draw { insts: vec![], mesh: vec![], cmds: vec![], clips: vec![Clip { r: [-1e5, -1e5, 1e5, 1e5], rad: 0.0 }], alphas: vec![1.0], xf: vec![(dpi, 0.0, 0.0)], size: (w, h), dpi }
    }
    pub fn xf(&self) -> (f32, f32, f32) {
        *self.xf.last().unwrap()
    }
    pub fn reset(&mut self, w: f32, h: f32, dpi: f32) {
        self.insts.clear();
        self.mesh.clear();
        self.cmds.clear();
        self.clips.truncate(1);
        self.alphas.truncate(1);
        self.xf.clear();
        self.xf.push((dpi, 0.0, 0.0));
        self.size = (w, h);
        self.dpi = dpi;
    }
    pub fn alpha(&self) -> f32 {
        *self.alphas.last().unwrap()
    }
    pub fn push_alpha(&mut self, a: f32) {
        let a = self.alpha() * a.clamp(0.0, 1.0);
        self.alphas.push(a);
    }
    pub fn pop_alpha(&mut self) {
        self.alphas.pop();
    }
    /// Escala alrededor de un punto y desplaza.
    pub fn push_xf(&mut self, s: f32, cx: f32, cy: f32, dx: f32, dy: f32) {
        let (ps, px, py) = *self.xf.last().unwrap();
        let lx = cx - cx * s + dx;
        let ly = cy - cy * s + dy;
        self.xf.push((ps * s, px + ps * lx, py + ps * ly));
    }
    pub fn pop_xf(&mut self) {
        self.xf.pop();
    }
    pub fn scale(&self) -> f32 {
        self.xf.last().unwrap().0
    }
    fn tr(&self, r: Rect) -> Rect {
        let (s, x, y) = *self.xf.last().unwrap();
        rect(r.x * s + x, r.y * s + y, r.w * s, r.h * s)
    }
    pub fn push_clip(&mut self, r: Rect, rad: f32) {
        let r = self.tr(r);
        let c = *self.clips.last().unwrap();
        let n = [r.x.max(c.r[0]), r.y.max(c.r[1]), r.r().min(c.r[2]), r.b().min(c.r[3])];
        let s = self.scale();
        self.clips.push(Clip { r: [n[0], n[1], n[2].max(n[0]), n[3].max(n[1])], rad: rad * s });
    }
    pub fn pop_clip(&mut self) {
        self.clips.pop();
    }
    pub fn clip_rect(&self) -> Rect {
        let c = self.clips.last().unwrap().r;
        rect(c[0], c[1], c[2] - c[0], c[3] - c[1])
    }
    fn push(&mut self, tex: TexId, mut i: Inst) {
        let c = *self.clips.last().unwrap();
        let k = i.misc[2] * 1.6 + 2.0;
        // fuera del recorte: ni se envía
        let ext = if i.rot[1] != 0.0 { i.rect[2].max(i.rect[3]) } else { 0.0 } + k;
        if i.rect[0] - ext > c.r[2] || i.rect[1] - ext > c.r[3] || i.rect[0] + i.rect[2] + ext < c.r[0] || i.rect[1] + i.rect[3] + ext < c.r[1] {
            return;
        }
        i.clip = c.r;
        i.misc[3] = c.rad;
        let n = self.insts.len() as u32;
        match self.cmds.last_mut() {
            Some(Cmd::Quads { tex: t, end, .. }) if *t == tex || tex == TEX_WHITE || *t == TEX_WHITE => {
                if *t == TEX_WHITE {
                    *t = tex;
                }
                *end = n + 1;
            }
            _ => self.cmds.push(Cmd::Quads { tex, start: n, end: n + 1 }),
        }
        self.insts.push(i);
    }
    fn base(&self, r: Rect) -> Inst {
        let r = self.tr(r);
        Inst { rect: [r.x, r.y, r.w, r.h], uv: [0.0, 0.0, 1.0, 1.0], color: [0.0; 4], color2: [0.0; 4], radius: [0.0; 4], clip: [0.0; 4], misc: [0.0; 4], border: [0.0; 4], rot: [1.0, 0.0, 0.0, 0.0] }
    }
    fn col(&self, c: Color) -> [f32; 4] {
        [c.0, c.1, c.2, c.3 * self.alpha()]
    }
    pub fn fill(&mut self, r: Rect, rad: f32, c: Color) {
        self.grad(r, rad, c, c, false);
    }
    pub fn fill4(&mut self, r: Rect, rad: [f32; 4], c: Color) {
        let mut i = self.base(r);
        let s = self.scale();
        i.color = self.col(c);
        i.color2 = i.color;
        i.radius = rad.map(|v| v * s);
        self.push(TEX_WHITE, i);
    }
    pub fn grad(&mut self, r: Rect, rad: f32, a: Color, b: Color, horizontal: bool) {
        if a.3 <= 0.0 && b.3 <= 0.0 {
            return;
        }
        let mut i = self.base(r);
        i.color = self.col(a);
        i.color2 = self.col(b);
        i.radius = [rad * self.scale(); 4];
        i.rot[2] = horizontal as i32 as f32;
        self.push(TEX_WHITE, i);
    }
    /// Luz radial: del centro (a) al borde (b).
    pub fn radial(&mut self, r: Rect, a: Color, b: Color) {
        let mut i = self.base(r);
        i.color = self.col(a);
        i.color2 = self.col(b);
        i.rot[2] = 2.0;
        self.push(TEX_WHITE, i);
    }
    pub fn boxed(&mut self, r: Rect, rad: f32, fill: Color, bw: f32, bc: Color) {
        let mut i = self.base(r);
        i.color = self.col(fill);
        i.color2 = i.color;
        i.radius = [rad * self.scale(); 4];
        i.misc[1] = (bw * self.scale()).max(1.0);
        i.border = self.col(bc);
        self.push(TEX_WHITE, i);
    }
    pub fn stroke(&mut self, r: Rect, rad: f32, bw: f32, bc: Color) {
        self.boxed(r, rad, bc.a(0.0), bw, bc);
    }
    pub fn shadow(&mut self, r: Rect, rad: f32, blur: f32, c: Color) {
        let mut i = self.base(r);
        let s = self.scale();
        i.color = self.col(c);
        i.radius = [rad * s; 4];
        i.misc[0] = 4.0;
        i.misc[2] = blur * s;
        self.push(TEX_WHITE, i);
    }
    pub fn shadow_rot(&mut self, r: Rect, rad: f32, blur: f32, ang: f32, c: Color) {
        let mut i = self.base(r);
        let s = self.scale();
        i.color = self.col(c);
        i.radius = [rad * s; 4];
        i.misc[0] = 4.0;
        i.misc[2] = blur * s;
        i.rot[0] = ang.cos();
        i.rot[1] = ang.sin();
        self.push(TEX_WHITE, i);
    }
    pub fn image(&mut self, tex: TexId, r: Rect, rad: f32, uv: [f32; 4], tint: Color) {
        let mut i = self.base(r);
        i.uv = uv;
        i.color = self.col(tint);
        i.radius = [rad * self.scale(); 4];
        i.misc[0] = 1.0;
        self.push(tex, i);
    }
    pub fn image_premul(&mut self, tex: TexId, r: Rect, alpha: f32) {
        let mut i = self.base(r);
        i.color = self.col(Color::WHITE.a(alpha));
        i.misc[0] = 3.0;
        self.push(tex, i);
    }
    pub fn page(&mut self, tex: TexId, r: Rect, uv: [f32; 4], paper: Color, ink: Color, strength: f32) {
        let mut i = self.base(r);
        i.uv = uv;
        i.color = self.col(Color::WHITE);
        i.color2 = paper.arr();
        i.border = ink.arr();
        i.rot[3] = strength;
        i.misc[0] = 5.0;
        self.push(tex, i);
    }
    #[allow(clippy::too_many_arguments)]
    pub fn backdrop(&mut self, tex: TexId, r: Rect, uv: [f32; 4], base: Color, sat: f32, strength: f32, alpha: f32) {
        let mut i = self.base(r);
        i.uv = uv;
        i.color = self.col(Color::WHITE.a(alpha));
        i.color2 = base.arr();
        i.rot[2] = sat;
        i.rot[3] = strength;
        i.misc[0] = 7.0;
        self.push(tex, i);
    }
    pub fn arc(&mut self, r: Rect, w: f32, t: f32, on: Color, off: Color) {
        let mut i = self.base(r);
        i.color = self.col(on);
        i.color2 = self.col(off);
        i.misc[0] = 6.0;
        i.misc[1] = w * self.scale();
        i.rot[3] = t;
        self.push(TEX_WHITE, i);
    }
    /// Rectángulo girado alrededor de su centro.
    pub fn rotated(&mut self, r: Rect, rad: f32, ang: f32, a: Color, b: Color) {
        let mut i = self.base(r);
        i.color = self.col(a);
        i.color2 = self.col(b);
        i.radius = [rad * self.scale(); 4];
        i.rot[0] = ang.cos();
        i.rot[1] = ang.sin();
        self.push(TEX_WHITE, i);
    }
    /// Glifo ya colocado en píxeles físicos.
    pub fn glyph(&mut self, r: Rect, uv: [f32; 4], c: Color, rot: Option<(f32, f32)>) {
        let mut i = self.base(r);
        i.rect = [r.x, r.y, r.w, r.h];
        i.uv = uv;
        i.color = self.col(c);
        i.misc[0] = 2.0;
        if let Some((co, si)) = rot {
            i.rot[0] = co;
            i.rot[1] = si;
        }
        self.push(TEX_ATLAS, i);
    }
    /// Triángulos ya proyectados; `pos` llega en píxeles (x, y) con su w.
    pub fn tris(&mut self, tex: TexId, v: &[MeshV]) {
        if v.is_empty() {
            return;
        }
        let a = self.alpha();
        let c = self.clips.last().unwrap().r;
        let (s, tx, ty) = *self.xf.last().unwrap();
        let n = self.mesh.len() as u32;
        for m in v {
            let mut m = *m;
            let w = m.pos[3];
            let px = m.pos[0] * s + tx;
            let py = m.pos[1] * s + ty;
            m.pos = [(px / self.size.0 * 2.0 - 1.0) * w, (1.0 - py / self.size.1 * 2.0) * w, 0.0, w];
            m.color[3] *= a;
            m.clip = c;
            self.mesh.push(m);
        }
        let e = self.mesh.len() as u32;
        match self.cmds.last_mut() {
            Some(Cmd::Mesh { tex: t, end, .. }) if *t == tex => *end = e,
            _ => self.cmds.push(Cmd::Mesh { tex, start: n, end: e }),
        }
    }
}

struct Tex {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
    w: u32,
    h: u32,
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    surface: Option<wgpu::Surface<'static>>,
    config: wgpu::SurfaceConfiguration,
    quad: wgpu::RenderPipeline,
    meshp: wgpu::RenderPipeline,
    tex_layout: wgpu::BindGroupLayout,
    glob_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    texs: HashMap<u32, Tex>,
    next: u32,
    ibuf: (wgpu::Buffer, u64),
    mbuf: (wgpu::Buffer, u64),
    globs: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    pub size: (u32, u32),
    shot: Option<TexId>,
    pub adapter: String,
}

pub const FMT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

impl Gpu {
    pub fn new(window: Option<Arc<winit::window::Window>>, w: u32, h: u32, high_power: bool) -> Gpu {
        // lo lee la copia de wgpu-hal de `vendor`: solo se despierta la gráfica elegida
        if high_power {
            std::env::set_var("SAGA_HIGH_POWER", "1");
        }
        #[cfg(windows)]
        if let Some(m) = window.as_ref().and_then(|w| w.current_monitor()) {
            use winit::platform::windows::MonitorHandleExtWindows;
            std::env::set_var("SAGA_MONITOR", (m.hmonitor() as isize).to_string());
        }
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wgpu::Backends::DX12, ..Default::default() });
        let surface = window.map(|win| instance.create_surface(win).expect("superficie"));
        let opts = |power, fallback| wgpu::RequestAdapterOptions { power_preference: power, compatible_surface: surface.as_ref(), force_fallback_adapter: fallback };
        let pref = if high_power { wgpu::PowerPreference::HighPerformance } else { wgpu::PowerPreference::LowPower };
        let adapter = pollster::block_on(instance.request_adapter(&opts(pref, false))).or_else(|_| pollster::block_on(instance.request_adapter(&opts(pref, true)))).expect("sin adaptador gráfico");
        let name = adapter.get_info().name;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits { max_texture_dimension_2d: 8192, ..wgpu::Limits::downlevel_defaults() }.using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .expect("dispositivo gráfico");
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: FMT,
            width: w.max(1),
            height: h.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 1,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
        };
        if let Some(s) = &surface {
            s.configure(&device, &config);
        }
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()) });
        let glob_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&glob_layout, &tex_layout], push_constant_ranges: &[] });
        let blend = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
        let ia = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4];
        let ma = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x2, 2 => Float32x2, 3 => Float32x2, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4];
        let mk = |vs: &str, fs: &str, buf: wgpu::VertexBufferLayout, topo| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), compilation_options: Default::default(), buffers: &[buf] },
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(fs), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: FMT, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })] }),
                primitive: wgpu::PrimitiveState { topology: topo, ..Default::default() },
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let quad = mk("vs", "fs", wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Inst>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &ia }, wgpu::PrimitiveTopology::TriangleStrip);
        let meshp = mk("mvs", "mfs", wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<MeshV>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &ma }, wgpu::PrimitiveTopology::TriangleList);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::FilterMode::Linear, anisotropy_clamp: 8, ..Default::default() });
        let mkbuf = |n: u64| device.create_buffer(&wgpu::BufferDescriptor { label: None, size: n, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let (ib, mb) = (mkbuf(1 << 20), mkbuf(1 << 19));
        let mut g = Gpu { device, queue, surface, config, quad, meshp, tex_layout, glob_layout, sampler, texs: HashMap::new(), next: 0, ibuf: (ib, 1 << 20), mbuf: (mb, 1 << 19), globs: vec![], size: (w, h), shot: None, adapter: name };
        g.create(1, 1, &[vec![255, 255, 255, 255]]); // TEX_WHITE
        g.create_fmt(ATLAS, ATLAS, 1, wgpu::TextureFormat::R8Unorm, false); // TEX_ATLAS
        if g.surface.is_none() {
            g.shot = Some(g.create_target(w, h));
        }
        g
    }
    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 || (w, h) == self.size {
            return;
        }
        self.size = (w, h);
        self.config.width = w;
        self.config.height = h;
        if let Some(s) = &self.surface {
            s.configure(&self.device, &self.config);
        } else if let Some(t) = self.shot {
            self.drop_tex(t);
            self.shot = Some(self.create_target(w, h));
        }
    }
    fn create_fmt(&mut self, w: u32, h: u32, mips: u32, format: wgpu::TextureFormat, target: bool) -> TexId {
        let mut usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        if target {
            usage |= wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
        }
        let tex = self.device.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 }, mip_level_count: mips, sample_count: 1, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] });
        let view = tex.create_view(&Default::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.tex_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) }],
        });
        let id = self.next;
        self.next += 1;
        self.texs.insert(id, Tex { tex, view, bind, w, h });
        TexId(id)
    }
    /// Textura RGBA con sus niveles ya reducidos en otro hilo.
    pub fn create(&mut self, w: u32, h: u32, levels: &[Vec<u8>]) -> TexId {
        let id = self.create_fmt(w, h, levels.len() as u32, wgpu::TextureFormat::Rgba8Unorm, false);
        let t = &self.texs[&id.0];
        for (l, data) in levels.iter().enumerate() {
            let (lw, lh) = ((w >> l).max(1), (h >> l).max(1));
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &t.tex, mip_level: l as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(lw * 4), rows_per_image: Some(lh) },
                wgpu::Extent3d { width: lw, height: lh, depth_or_array_layers: 1 },
            );
        }
        id
    }
    pub fn create_target(&mut self, w: u32, h: u32) -> TexId {
        self.create_fmt(w, h, 1, FMT, true)
    }
    pub fn tex_size(&self, t: TexId) -> Option<(u32, u32)> {
        self.texs.get(&t.0).map(|t| (t.w, t.h))
    }
    pub fn drop_tex(&mut self, t: TexId) {
        if t.0 > 1 {
            self.texs.remove(&t.0);
        }
    }
    pub fn upload_atlas(&self, x: u32, y: u32, w: u32, h: u32, data: &[u8]) {
        let t = &self.texs[&TEX_ATLAS.0];
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &t.tex, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Pinta primero las listas fuera de pantalla (hojas) y luego la ventana.
    pub fn render(&mut self, off: &[(TexId, Draw, Color)], main: &Draw, clear: Color, time: f32) -> bool {
        let lists: Vec<&Draw> = off.iter().map(|o| &o.1).chain(std::iter::once(main)).collect();
        let (mut ni, mut nm) = (0u64, 0u64);
        for l in &lists {
            ni += (l.insts.len() * std::mem::size_of::<Inst>()) as u64;
            nm += (l.mesh.len() * std::mem::size_of::<MeshV>()) as u64;
        }
        for (buf, need) in [(&mut self.ibuf, ni), (&mut self.mbuf, nm)] {
            if need > buf.1 {
                let n = need.next_power_of_two();
                *buf = (self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: n, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }), n);
            }
        }
        let (mut oi, mut om) = (0u64, 0u64);
        let mut bases = vec![];
        for (k, l) in lists.iter().enumerate() {
            bases.push((oi, om));
            if !l.insts.is_empty() {
                let b = bytemuck::cast_slice(&l.insts);
                self.queue.write_buffer(&self.ibuf.0, oi, b);
                oi += b.len() as u64;
            }
            if !l.mesh.is_empty() {
                let b = bytemuck::cast_slice(&l.mesh);
                self.queue.write_buffer(&self.mbuf.0, om, b);
                om += b.len() as u64;
            }
            if self.globs.len() <= k {
                let buf = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
                let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &self.glob_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }] });
                self.globs.push((buf, bind));
            }
            self.queue.write_buffer(&self.globs[k].0, 0, bytemuck::cast_slice(&[l.size.0, l.size.1, time, 0.0f32]));
        }
        let frame = match &self.surface {
            Some(s) => match s.get_current_texture() {
                Ok(f) => Some(f),
                Err(wgpu::SurfaceError::Timeout) => return false,
                Err(_) => {
                    s.configure(&self.device, &self.config);
                    match s.get_current_texture() {
                        Ok(f) => Some(f),
                        Err(_) => return false,
                    }
                }
            },
            None => None,
        };
        let mut enc = self.device.create_command_encoder(&Default::default());
        for (k, l) in lists.iter().enumerate() {
            let (view, col) = if k < off.len() {
                match self.texs.get(&off[k].0 .0) {
                    Some(t) => (t.view.clone(), off[k].2),
                    None => continue,
                }
            } else if let Some(f) = &frame {
                (f.texture.create_view(&Default::default()), clear)
            } else {
                (self.texs[&self.shot.unwrap().0].view.clone(), clear)
            };
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: col.0 as f64, g: col.1 as f64, b: col.2 as f64, a: col.3 as f64 }), store: wgpu::StoreOp::Store } })],
                ..Default::default()
            });
            p.set_bind_group(0, &self.globs[k].1, &[]);
            for c in &l.cmds {
                match c {
                    Cmd::Quads { tex, start, end } => {
                        let Some(t) = self.texs.get(&tex.0) else { continue };
                        p.set_pipeline(&self.quad);
                        p.set_bind_group(1, &t.bind, &[]);
                        p.set_vertex_buffer(0, self.ibuf.0.slice(bases[k].0..));
                        p.draw(0..4, *start..*end);
                    }
                    Cmd::Mesh { tex, start, end } => {
                        let Some(t) = self.texs.get(&tex.0) else { continue };
                        p.set_pipeline(&self.meshp);
                        p.set_bind_group(1, &t.bind, &[]);
                        p.set_vertex_buffer(0, self.mbuf.0.slice(bases[k].1..));
                        p.draw(*start..*end, 0..1);
                    }
                }
            }
        }
        self.queue.submit([enc.finish()]);
        if let Some(f) = frame {
            f.present();
        }
        true
    }
    /// Solo para las pruebas sin ventana: devuelve la imagen RGBA.
    pub fn read_shot(&self) -> Vec<u8> {
        let t = &self.texs[&self.shot.unwrap().0];
        let bpr = (t.w * 4 + 255) / 256 * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (bpr * t.h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(t.tex.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: None } }, wgpu::Extent3d { width: t.w, height: t.h, depth_or_array_layers: 1 });
        self.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.wait();
        let d = buf.slice(..).get_mapped_range();
        let mut out = Vec::with_capacity((t.w * t.h * 4) as usize);
        for y in 0..t.h {
            let row = &d[(y * bpr) as usize..(y * bpr + t.w * 4) as usize];
            for p in row.chunks(4) {
                out.extend([p[2], p[1], p[0], 255]);
            }
        }
        out
    }
    pub fn wait(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}
