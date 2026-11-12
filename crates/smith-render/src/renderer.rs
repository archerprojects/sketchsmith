//! SketchSmith — GPU canvas renderer | v1.0.0 | Thursday 12 November 2026
//!
//! Renders the scene into an offscreen texture the size of the canvas area.
//! The application shows that texture inside the egui layout, which keeps the
//! 3D pass fully under our control (MSAA, depth format, later picking passes)
//! instead of drawing inside egui's own render pass.
//!
//! Passes and formats:
//! * 4× MSAA colour (`Rgba8UnormSrgb`), resolved to a single-sample texture.
//! * `Depth32Float` with **reversed-Z** (clear 0, compare `Greater`), matching
//!   [`Camera::view_proj`]: near-uniform depth precision from 0.001 mm detail to
//!   metre-scale scenes.
//! * Faces: two-sided, flat-shaded with a camera-relative light, front and back
//!   colours distinguished (Edit-mode look). Faces get a negative depth
//!   bias so coincident edges draw cleanly on top.
//! * Present-mode paint: each face vertex carries its colour, opacity,
//!   Metallic switch and texture coordinates, and names a texture slot.
//!   Textures are uploaded once per image (mipmapped RGBA, sRGB), sampled
//!   repeating with trilinear and 16× anisotropic filtering; untextured faces
//!   sample a 1×1 white texture, so one shader covers both. Faces are drawn
//!   in runs by texture. Faces under full opacity are drawn after the lines,
//!   blended, depth-tested but not written, sorted back to front by triangle
//!   centre and re-sorted only when the view direction changes. Metals take
//!   their colour into the highlight and dim their diffuse light; there are
//!   no reflections yet. Monochrome and Hidden Line ignore paint.
//! * Styled edges (Present mode): each [`StyledSegment`] is drawn as a band
//!   built in the vertex shader from six corners of an instanced quad, its
//!   width a fixed number of screen pixels whatever the zoom, smoothed by
//!   MSAA. The ISO 128 pattern is cut in the fragment shader in units of the
//!   line width (never under 1.5 px, so thin patterns stay readable): dash
//!   12, long dash 24, gap 3, dot 0.5.
//! * Lines: edges and axes. Dashed lines (negative axes, back edges) are dashed
//!   in the fragment shader with a period of a fixed number of screen pixels.
//! * Section cut ([`RenderStyle::section`]): faces and edges on the side of
//!   the plane its normal points to are discarded in the fragment shader;
//!   the axes are not cut. Section fill triangles (marked in the face
//!   buffer, see [`SECTION_FILL`]) are drawn flat in the fill colour.
//! * Styles ([`RenderStyle`]): wireframe, hidden line, shaded, monochrome and
//!   X-ray faces, optional dashed back edges, and the background colour.
//! * Pictures: the axes can be left out ([`CanvasRenderer::set_axes_shown`])
//!   and the resolved colour read back ([`CanvasRenderer::read_pixels`]),
//!   for PNG export.

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use smith_geom::DVec3;
use smith_model::TextureImage;
use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::mesh::{FaceVertex, LineVertex, SceneMesh, StyledSegment, axes};

/// A decoded image for the GPU: RGBA8 sRGB mip levels, largest first, each
/// `(width, height, pixels)`.
pub type MipLevels = Vec<(u32, u32, Vec<u8>)>;

/// MSAA sample count.
pub const SAMPLE_COUNT: u32 = 4;
/// Colour format of the canvas texture.
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Depth format.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Default canvas background: white.
pub const BACKGROUND: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Front face colour (linear): light warm grey, so faces read against white.
pub const FRONT_COLOR: [f32; 4] = [0.62, 0.60, 0.56, 1.0];
/// Back face colour in Edit mode (linear): golden yellow, sRGB (230, 180,
/// 20), so a face turned the wrong way stands out against the grey fronts.
pub const BACK_COLOR: [f32; 4] = [0.791, 0.456, 0.007, 1.0];
/// Back face colour in Present mode (linear, blue-grey), as in SketchUp.
pub const PRESENT_BACK_COLOR: [f32; 4] = [0.30, 0.36, 0.50, 1.0];
/// Monochrome face colour (linear), both sides.
const MONO_COLOR: [f32; 4] = [0.70, 0.70, 0.70, 1.0];
/// Face opacity in X-Ray style.
const XRAY_ALPHA: f32 = 0.35;
/// Section fill colour (linear): dark grey, light enough for the cut's
/// near-black outline to show against it.
const FILL_COLOR: [f32; 4] = [0.16, 0.16, 0.17, 1.0];
/// The `selected` value that marks a face vertex as section fill.
pub const SECTION_FILL: f32 = 2.0;

/// How faces are drawn (display styles and Present shading).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FaceStyle {
    /// Edges only.
    Wireframe,
    /// Faces filled with the background colour: hides edges behind them.
    HiddenLine,
    /// Front and back colours with lighting.
    #[default]
    Shaded,
    /// One neutral colour for both sides, with lighting.
    Monochrome,
    /// See-through faces.
    XRay,
}

/// Everything about the look of one frame that is not geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderStyle {
    /// Face drawing style.
    pub faces: FaceStyle,
    /// Draw edges hidden behind faces as dashed lines.
    pub back_edges: bool,
    /// Background colour (linear RGBA).
    pub background: [f32; 4],
    /// The active section plane as `[nx, ny, nz, d]` (`n·p = d` on it, in
    /// world mm; `n` toward the side cut away), or `None` for no cut.
    pub section: Option<[f64; 4]>,
    /// Show paint (Present mode).
    pub paint: bool,
    /// Display scale, for line widths in pixels.
    pub pixels_per_point: f32,
}

impl Default for RenderStyle {
    fn default() -> Self {
        Self {
            faces: FaceStyle::Shaded,
            back_edges: false,
            background: BACKGROUND,
            section: None,
            paint: false,
            pixels_per_point: 1.0,
        }
    }
}
/// Dash period for dashed lines, in screen pixels.
const DASH_PIXELS: f64 = 14.0;

const SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    front: vec4<f32>,
    back: vec4<f32>,
    // x: dash period; yzw: direction toward the viewer (for highlights).
    dash: vec4<f32>,
    // x: alpha for faces; y: 1 = force every line dashed (back-edge pass);
    // z: 1 = cut by the section plane; w: 1 = ignore paint.
    params: vec4<f32>,
    // Section plane: normal and offset, relative to the render origin.
    clip: vec4<f32>,
    fill: vec4<f32>,
    // x, y: target size in pixels; z: pixels per point.
    viewport: vec4<f32>,
};

fn cut_away(p: vec3<f32>) -> bool {
    return u.params.z > 0.5 && dot(u.clip.xyz, p) > u.clip.w;
}
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct FaceOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) selected: f32,
    @location(2) rel: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) metallic: f32,
    @location(6) side: f32,
};

@vertex
fn vs_face(
    @location(0) p: vec3<f32>,
    @location(1) n: vec3<f32>,
    @location(2) selected: f32,
    @location(3) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) metallic: f32,
    @location(6) side: f32,
) -> FaceOut {
    var o: FaceOut;
    o.pos = u.view_proj * vec4<f32>(p, 1.0);
    o.normal = n;
    o.selected = selected;
    o.rel = p;
    o.uv = uv;
    o.color = color;
    o.metallic = metallic;
    o.side = side;
    return o;
}

@fragment
fn fs_face(i: FaceOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if (i.selected > 1.5) {
        return u.fill;
    }
    if (cut_away(i.rel)) {
        discard;
    }
    // A one-sided triangle (side 1 front, 2 back) shows only facing the viewer.
    if (i.side > 0.5 && !front) {
        discard;
    }
    // Which side of the model is seen: a back triangle is reversed.
    let model_front = select(front, (i.side < 1.5), (i.side > 0.5));
    let n = normalize(select(-i.normal, i.normal, front));
    let lambert = max(dot(n, u.light_dir.xyz), 0.0);
    var base = select(u.back, u.front, model_front);
    var metal = 0.0;
    if (u.params.w < 0.5 && i.color.a >= 0.0) {
        let t = textureSample(tex, samp, i.uv);
        base = vec4<f32>(i.color.rgb * t.rgb, i.color.a * t.a);
        metal = i.metallic;
    }
    // A metal's diffuse light is dim and its highlight takes its colour.
    let diffuse = base.rgb * (0.55 + 0.45 * lambert) * (1.0 - 0.6 * metal);
    let h = normalize(u.light_dir.xyz + u.dash.yzw);
    let spec = pow(max(dot(n, h), 0.0), mix(24.0, 64.0, metal));
    let highlight = mix(vec3<f32>(0.12), base.rgb * 0.9, metal) * spec;
    let shaded = diffuse + highlight * select(0.0, 1.0, (u.params.w < 0.5));
    // Selection tint: blend toward the selection blue; lightly on a model
    // back, so a reversed face still reads yellow while selected.
    let tint = select(0.12, 0.35, model_front);
    let tinted = mix(shaded, vec3<f32>(0.20, 0.35, 0.95), tint * i.selected);
    return vec4<f32>(tinted, base.a * u.params.x);
}

struct WideOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) @interpolate(linear) along: f32,
    @location(2) unit: f32,
    @location(3) pattern: f32,
    @location(4) rel: vec3<f32>,
};

@vertex
fn vs_wide(
    @builtin(vertex_index) vi: u32,
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) width: f32,
    @location(4) pattern: f32,
) -> WideOut {
    // Corners of the band: (t along a→b, side).
    var ts = array<f32, 6>(0.0, 1.0, 0.0, 0.0, 1.0, 1.0);
    var ss = array<f32, 6>(-1.0, -1.0, 1.0, 1.0, -1.0, 1.0);
    let t = ts[vi];
    let side = ss[vi];
    var ca = u.view_proj * vec4<f32>(a, 1.0);
    var cb = u.view_proj * vec4<f32>(b, 1.0);
    ca.w = max(ca.w, 1e-6);
    cb.w = max(cb.w, 1e-6);
    let half = 0.5 * u.viewport.xy;
    let sa = ca.xy / ca.w * half;
    let sb = cb.xy / cb.w * half;
    let d = sb - sa;
    let len = length(d);
    let dir = select(vec2<f32>(1.0, 0.0), d / len, len > 1e-6);
    let px = width * u.viewport.z;
    let c = mix(ca, cb, t);
    let offset = vec2<f32>(-dir.y, dir.x) * side * (0.5 * px + 0.5) / half;
    var o: WideOut;
    o.pos = vec4<f32>(c.xy + offset * c.w, c.zw);
    o.color = color;
    o.along = t * len;
    o.unit = max(px, 1.5);
    o.pattern = pattern;
    o.rel = mix(a, b, t);
    return o;
}

@fragment
fn fs_wide(i: WideOut) -> @location(0) vec4<f32> {
    if (cut_away(i.rel)) {
        discard;
    }
    let dot_len = max(0.5 * i.unit, 1.5);
    var on = true;
    if (i.pattern > 0.5 && i.pattern < 1.5) {
        // Dashed: dash 12, gap 3.
        on = (i.along % (15.0 * i.unit)) < 12.0 * i.unit;
    } else if (i.pattern > 1.5) {
        // Long dash 24, gap 3, then one or two dots each followed by gap 3.
        let dots = select(1.0, 2.0, (i.pattern > 2.5));
        let period = 24.0 * i.unit + 3.0 * i.unit + dots * (dot_len + 3.0 * i.unit);
        let m = i.along % period;
        let first = 27.0 * i.unit;
        let second = first + dot_len + 3.0 * i.unit;
        on = m < 24.0 * i.unit
            || (m >= first && m < first + dot_len)
            || (dots > 1.5 && m >= second && m < second + dot_len);
    }
    if (!on) {
        discard;
    }
    return i.color;
}

struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) along: f32,
    @location(2) dashed: f32,
    @location(3) rel: vec3<f32>,
};

@vertex
fn vs_line(
    @location(0) p: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) along: f32,
    @location(3) dashed: f32,
) -> LineOut {
    var o: LineOut;
    o.pos = u.view_proj * vec4<f32>(p, 1.0);
    o.color = color;
    o.along = along;
    o.dashed = dashed;
    o.rel = p;
    return o;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    if (cut_away(i.rel)) {
        discard;
    }
    if ((i.dashed > 0.5 || u.params.y > 0.5) && fract(i.along / u.dash.x) > 0.5) {
        discard;
    }
    return i.color;
}
"#;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    light_dir: [f32; 4],
    front: [f32; 4],
    back: [f32; 4],
    dash: [f32; 4],
    params: [f32; 4],
    clip: [f32; 4],
    fill: [f32; 4],
    viewport: [f32; 4],
}

#[derive(Default)]
struct GpuBuffer {
    buffer: Option<wgpu::Buffer>,
    count: u32,
}

impl GpuBuffer {
    fn upload<T: Pod>(&mut self, device: &wgpu::Device, label: &str, data: &[T]) {
        self.count = data.len() as u32;
        self.buffer = (!data.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(data),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
    }
}

struct Targets {
    size: (u32, u32),
    msaa_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    resolve: wgpu::Texture,
    resolve_view: wgpu::TextureView,
}

impl Targets {
    fn new(device: &wgpu::Device, (width, height): (u32, u32)) -> Self {
        let make = |label: &str, format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let resolve = make(
            "canvas colour",
            COLOR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        Self {
            size: (width, height),
            msaa_view: view(&make(
                "canvas msaa",
                COLOR_FORMAT,
                SAMPLE_COUNT,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            )),
            depth_view: view(&make(
                "canvas depth",
                DEPTH_FORMAT,
                SAMPLE_COUNT,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            )),
            resolve_view: view(&resolve),
            resolve,
        }
    }
}

/// The 3D canvas renderer.
pub struct CanvasRenderer {
    face_pipeline: wgpu::RenderPipeline,
    xray_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    hidden_line_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// Second uniform set for the back-edge pass (forced dashing).
    hidden_uniform_buffer: wgpu::Buffer,
    hidden_bind_group: wgpu::BindGroup,
    /// Uniforms for the axes: as the main ones, never cut.
    axes_uniform_buffer: wgpu::Buffer,
    axes_bind_group: wgpu::BindGroup,
    targets: Option<Targets>,
    faces: GpuBuffer,
    wide_pipeline: wgpu::RenderPipeline,
    styled: GpuBuffer,
    /// Runs of opaque face vertices by texture slot: `(slot, start, count)`.
    face_runs: Vec<(u32, u32, u32)>,
    /// See-through triangles, as uploaded, and their centres.
    clear: Vec<[FaceVertex; 3]>,
    clear_centres: Vec<DVec3>,
    /// The see-through triangles sorted for the last view direction.
    clear_buffer: GpuBuffer,
    clear_runs: Vec<(u32, u32, u32)>,
    clear_sorted_for: Option<DVec3>,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Bind group of the 1×1 white texture (slot 0).
    white: wgpu::BindGroup,
    /// Uploaded textures, kept by image (the `Arc` keeps its address unique).
    textures: HashMap<usize, (Arc<TextureImage>, wgpu::BindGroup)>,
    /// Bind group of each slot of the current mesh (index 0 is white).
    slots: Vec<wgpu::BindGroup>,
    edges: GpuBuffer,
    axes: GpuBuffer,
    origin: DVec3,
    /// Drawing axes: origin and basis.
    drawing_axes: (DVec3, [DVec3; 3]),
    /// The axes are drawn.
    show_axes: bool,
}

/// What [`CanvasRenderer::render`] produced.
pub struct RenderOutput<'a> {
    /// The rendered, resolved colour texture.
    pub view: &'a wgpu::TextureView,
    /// `true` if the texture was (re)created this frame, so any handle the UI
    /// holds to the previous texture must be updated.
    pub recreated: bool,
}

impl CanvasRenderer {
    /// Creates pipelines, buffers and the white texture.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("canvas shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let make_uniforms = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let uniform_buffer = make_uniforms("canvas uniforms");
        let hidden_uniform_buffer = make_uniforms("back-edge uniforms");
        let axes_uniform_buffer = make_uniforms("axes uniforms");
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("canvas uniforms layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let make_group = |buffer: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("canvas uniforms"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        };
        let bind_group = make_group(&uniform_buffer);
        let hidden_bind_group = make_group(&hidden_uniform_buffer);
        let axes_bind_group = make_group(&axes_uniform_buffer);
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("canvas pipeline layout"),
            bind_group_layouts: &[Some(&layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        // Repeating, trilinear, 16× anisotropic (all filters linear, as
        // anisotropy requires).
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });

        let face_attrs = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32, 3 => Float32x2,
            4 => Float32x4, 5 => Float32, 6 => Float32, 7 => Uint32
        ];
        let line_attrs =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4, 2 => Float32, 3 => Float32];

        #[allow(clippy::too_many_arguments)]
        let pipeline = |label: &str,
                        vs: &str,
                        fs: &str,
                        stride: usize,
                        attrs: &[wgpu::VertexAttribute],
                        topology,
                        bias: wgpu::DepthBiasState,
                        compare: wgpu::CompareFunction,
                        write: bool,
                        blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: stride as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: attrs,
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(write),
                    depth_compare: Some(compare),
                    stencil: wgpu::StencilState::default(),
                    bias,
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLE_COUNT,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        // Reversed-Z: a negative bias pushes faces *away*, so edges lying on them win.
        let face_bias = wgpu::DepthBiasState {
            constant: -4,
            slope_scale: -1.0,
            clamp: 0.0,
        };
        use wgpu::CompareFunction::{Greater, Less};
        let face_size = std::mem::size_of::<FaceVertex>();
        let line_size = std::mem::size_of::<LineVertex>();
        let tris = wgpu::PrimitiveTopology::TriangleList;
        let lines = wgpu::PrimitiveTopology::LineList;
        let no_bias = wgpu::DepthBiasState::default();
        let face_pipeline = pipeline(
            "faces",
            "vs_face",
            "fs_face",
            face_size,
            &face_attrs,
            tris,
            face_bias,
            Greater,
            true,
            None,
        );
        // X-Ray: blended, and not written to depth so edges behind stay visible.
        let xray_pipeline = pipeline(
            "faces x-ray",
            "vs_face",
            "fs_face",
            face_size,
            &face_attrs,
            tris,
            face_bias,
            Greater,
            false,
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        let line_pipeline = pipeline(
            "lines",
            "vs_line",
            "fs_line",
            line_size,
            &line_attrs,
            lines,
            no_bias,
            Greater,
            true,
            None,
        );
        // Back edges: only where something nearer is already drawn (reversed-Z: Less).
        let hidden_line_pipeline = pipeline(
            "back edges",
            "vs_line",
            "fs_line",
            line_size,
            &line_attrs,
            lines,
            no_bias,
            Less,
            false,
            None,
        );

        let white = texture_group(
            device,
            queue,
            &texture_layout,
            &sampler,
            "white texture",
            &[(1, 1, vec![255, 255, 255, 255])],
        );
        // Styled edges: one instance per segment, six corners each.
        let wide_attrs = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32, 4 => Float32
        ];
        let wide_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("styled edges"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_wide"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<StyledSegment>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wide_attrs,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: tris,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(Greater),
                stencil: wgpu::StencilState::default(),
                bias: no_bias,
            }),
            multisample: wgpu::MultisampleState {
                count: SAMPLE_COUNT,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_wide"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let mut renderer = Self {
            wide_pipeline,
            styled: GpuBuffer::default(),
            face_pipeline,
            xray_pipeline,
            line_pipeline,
            hidden_line_pipeline,
            uniform_buffer,
            bind_group,
            hidden_uniform_buffer,
            hidden_bind_group,
            axes_uniform_buffer,
            axes_bind_group,
            targets: None,
            faces: GpuBuffer::default(),
            face_runs: Vec::new(),
            clear: Vec::new(),
            clear_centres: Vec::new(),
            clear_buffer: GpuBuffer::default(),
            clear_runs: Vec::new(),
            clear_sorted_for: None,
            texture_layout,
            sampler,
            white,
            textures: HashMap::new(),
            slots: Vec::new(),
            edges: GpuBuffer::default(),
            axes: GpuBuffer::default(),
            origin: DVec3::ZERO,
            drawing_axes: (DVec3::ZERO, [DVec3::X, DVec3::Y, DVec3::Z]),
            show_axes: true,
        };
        renderer.upload(device, queue, &SceneMesh::default(), &|_| None);
        renderer
    }

    /// Replaces the scene geometry. Textures the mesh uses that are not on
    /// the GPU yet are decoded with `decode` and uploaded (one `decode` can
    /// not read is shown white); textures no longer used are dropped.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mesh: &SceneMesh,
        decode: &dyn Fn(&TextureImage) -> Option<MipLevels>,
    ) {
        self.origin = mesh.origin;
        // Textures: keep those still used, add new ones.
        let used: Vec<usize> = mesh
            .textures
            .iter()
            .map(|t| Arc::as_ptr(t) as usize)
            .collect();
        self.textures.retain(|k, _| used.contains(k));
        for image in &mesh.textures {
            let key = Arc::as_ptr(image) as usize;
            if self.textures.contains_key(&key) {
                continue;
            }
            if let Some(levels) = decode(image) {
                let group = texture_group(
                    device,
                    queue,
                    &self.texture_layout,
                    &self.sampler,
                    &image.name,
                    &levels,
                );
                self.textures.insert(key, (image.clone(), group));
            }
        }
        self.slots = std::iter::once(self.white.clone())
            .chain(used.iter().map(|k| {
                self.textures
                    .get(k)
                    .map_or_else(|| self.white.clone(), |(_, g)| g.clone())
            }))
            .collect();
        // Faces: opaque ones in runs by texture; see-through ones apart.
        let mut opaque: Vec<[FaceVertex; 3]> = Vec::new();
        self.clear.clear();
        for t in mesh.faces.as_chunks::<3>().0 {
            if t[0].is_clear() {
                self.clear.push(*t);
            } else {
                opaque.push(*t);
            }
        }
        opaque.sort_by_key(|t| t[0].texture);
        let flat: Vec<FaceVertex> = opaque.iter().flatten().copied().collect();
        self.face_runs = runs(&flat);
        self.faces.upload(device, "faces", &flat);
        let origin = self.origin;
        self.clear_centres = self
            .clear
            .iter()
            .map(|t| {
                t.iter()
                    .map(|v| DVec3::from_array(v.position.map(f64::from)))
                    .sum::<DVec3>()
                    / 3.0
                    + origin
            })
            .collect();
        self.clear_sorted_for = None;
        self.edges.upload(device, "edges", &mesh.edges);
        self.styled.upload(device, "styled edges", &mesh.styled);
        self.upload_axes(device);
    }

    /// Orders the see-through triangles back to front for a view along
    /// `forward`, when it has changed since the last sort.
    fn sort_clear(&mut self, device: &wgpu::Device, forward: DVec3) {
        if self.clear_sorted_for == Some(forward) {
            return;
        }
        self.clear_sorted_for = Some(forward);
        let mut order: Vec<usize> = (0..self.clear.len()).collect();
        // Farthest along the view direction first.
        order.sort_by(|a, b| {
            forward
                .dot(self.clear_centres[*b])
                .total_cmp(&forward.dot(self.clear_centres[*a]))
        });
        let flat: Vec<FaceVertex> = order.iter().flat_map(|i| self.clear[*i]).collect();
        self.clear_runs = runs(&flat);
        self.clear_buffer.upload(device, "see-through faces", &flat);
    }

    /// Sets the drawing axes shown in the canvas.
    pub fn set_axes(&mut self, device: &wgpu::Device, origin: DVec3, basis: [DVec3; 3]) {
        if self.drawing_axes != (origin, basis) {
            self.drawing_axes = (origin, basis);
            self.upload_axes(device);
        }
    }

    /// Whether the axes are drawn (`false` for pictures).
    pub fn set_axes_shown(&mut self, shown: bool) {
        self.show_axes = shown;
    }

    /// The last render's colour, read back: `(width, height, pixels)`, the
    /// pixels RGBA, sRGB, rows top first and tightly packed. `None` before
    /// any render or if the GPU fails to map it.
    pub fn read_pixels(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Option<(u32, u32, Vec<u8>)> {
        let targets = self.targets.as_ref()?;
        let (width, height) = targets.size;
        let padded = padded_row(width);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("picture readback"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("picture readback"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &targets.resolve,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;
        let pixels = {
            let view = buffer.get_mapped_range(..).ok()?;
            unpad_rows(&view, width, height, padded)
        };
        buffer.unmap();
        Some((width, height, pixels))
    }

    fn upload_axes(&mut self, device: &wgpu::Device) {
        let (o, b) = self.drawing_axes;
        self.axes.upload(device, "axes", &axes(self.origin, o, b));
    }

    /// Renders the scene for `camera` into a `width × height` texture.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        style: &RenderStyle,
        width: u32,
        height: u32,
    ) -> RenderOutput<'_> {
        let size = (width.max(1), height.max(1));
        let recreated = self.targets.as_ref().is_none_or(|t| t.size != size);
        if recreated {
            self.targets = Some(Targets::new(device, size));
        }

        let aspect = f64::from(size.0) / f64::from(size.1);
        let view_proj = camera
            .view_proj(aspect, self.origin)
            .as_mat4()
            .to_cols_array_2d();
        let light = (-camera.forward() + camera.up() * 0.6 + camera.right() * 0.3).normalize();
        let toward_viewer = (-camera.forward()).as_vec3();
        self.sort_clear(device, camera.forward());
        let dash = (camera.world_per_pixel(f64::from(size.1)) * DASH_PIXELS) as f32;
        // Edit mode marks backs yellow; Present mode keeps its blue-grey.
        let back_color = if style.paint {
            PRESENT_BACK_COLOR
        } else {
            BACK_COLOR
        };
        let (front, back, alpha) = match style.faces {
            FaceStyle::HiddenLine => (style.background, style.background, 1.0),
            FaceStyle::Monochrome => (MONO_COLOR, MONO_COLOR, 1.0),
            FaceStyle::XRay => (FRONT_COLOR, back_color, XRAY_ALPHA),
            FaceStyle::Shaded | FaceStyle::Wireframe => (FRONT_COLOR, back_color, 1.0),
        };
        let mut uniforms = Uniforms {
            view_proj,
            light_dir: light.as_vec3().extend(0.0).to_array(),
            front,
            back,
            dash: [dash, toward_viewer.x, toward_viewer.y, toward_viewer.z],
            params: [
                alpha,
                0.0,
                0.0,
                if style.paint && matches!(style.faces, FaceStyle::Shaded | FaceStyle::XRay) {
                    0.0
                } else {
                    1.0
                },
            ],
            clip: [0.0; 4],
            fill: FILL_COLOR,
            viewport: [size.0 as f32, size.1 as f32, style.pixels_per_point, 0.0],
        };
        queue.write_buffer(&self.axes_uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        if let Some([nx, ny, nz, d]) = style.section {
            let n = DVec3::new(nx, ny, nz);
            // Relative to the render origin, as the vertex positions are.
            // A face lying on the plane is kept: the cut starts a hair past
            // it, so f32 rounding cannot make it flicker.
            let tolerance = 1e-3 + camera.distance * 1e-6;
            let d_rel = d - n.dot(self.origin) + tolerance;
            uniforms.clip = [nx as f32, ny as f32, nz as f32, d_rel as f32];
            uniforms.params[2] = 1.0;
        }
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        uniforms.params[1] = 1.0;
        queue.write_buffer(
            &self.hidden_uniform_buffer,
            0,
            bytemuck::bytes_of(&uniforms),
        );
        let [r, g, b, a] = style.background.map(f64::from);
        let clear = wgpu::Color { r, g, b, a };

        let Some(targets) = self.targets.as_ref() else {
            unreachable!("targets were created above");
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("canvas"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canvas pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.msaa_view,
                    depth_slice: None,
                    resolve_target: Some(&targets.resolve_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            // Opaque faces first (they fill depth), then lines, then see-through faces.
            let opaque = matches!(
                style.faces,
                FaceStyle::Shaded | FaceStyle::Monochrome | FaceStyle::HiddenLine
            );
            pass.set_bind_group(1, &self.white, &[]);
            if opaque && let Some(buf) = &self.faces.buffer {
                pass.set_pipeline(&self.face_pipeline);
                pass.set_vertex_buffer(0, buf.slice(..));
                for (slot, start, count) in &self.face_runs {
                    pass.set_bind_group(1, &self.slots[*slot as usize], &[]);
                    pass.draw(*start..*start + *count, 0..1);
                }
            }
            pass.set_pipeline(&self.line_pipeline);
            if self.show_axes
                && let Some(buf) = &self.axes.buffer
            {
                pass.set_bind_group(0, &self.axes_bind_group, &[]);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..self.axes.count, 0..1);
                pass.set_bind_group(0, &self.bind_group, &[]);
            }
            if let Some(buf) = &self.edges.buffer {
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..self.edges.count, 0..1);
            }
            if let Some(buf) = &self.styled.buffer {
                pass.set_pipeline(&self.wide_pipeline);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..6, 0..self.styled.count);
                pass.set_pipeline(&self.line_pipeline);
            }
            if style.back_edges
                && opaque
                && let Some(buf) = &self.edges.buffer
            {
                pass.set_pipeline(&self.hidden_line_pipeline);
                pass.set_bind_group(0, &self.hidden_bind_group, &[]);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..self.edges.count, 0..1);
                pass.set_bind_group(0, &self.bind_group, &[]);
            }
            // See-through paint (and in X-Ray every face): blended last.
            pass.set_pipeline(&self.xray_pipeline);
            if style.faces == FaceStyle::XRay
                && let Some(buf) = &self.faces.buffer
            {
                pass.set_vertex_buffer(0, buf.slice(..));
                for (slot, start, count) in &self.face_runs {
                    pass.set_bind_group(1, &self.slots[*slot as usize], &[]);
                    pass.draw(*start..*start + *count, 0..1);
                }
            }
            if matches!(style.faces, FaceStyle::Shaded | FaceStyle::XRay)
                && let Some(buf) = &self.clear_buffer.buffer
            {
                pass.set_vertex_buffer(0, buf.slice(..));
                for (slot, start, count) in &self.clear_runs {
                    pass.set_bind_group(1, &self.slots[*slot as usize], &[]);
                    pass.draw(*start..*start + *count, 0..1);
                }
            }
        }
        queue.submit([encoder.finish()]);

        RenderOutput {
            view: &targets.resolve_view,
            recreated,
        }
    }
}

/// Runs of consecutive vertices sharing a texture slot: `(slot, start, count)`.
fn runs(vertices: &[FaceVertex]) -> Vec<(u32, u32, u32)> {
    let mut out: Vec<(u32, u32, u32)> = Vec::new();
    for (i, v) in vertices.iter().enumerate() {
        let i = i as u32;
        match out.last_mut() {
            Some((slot, _, count)) if *slot == v.texture => *count += 1,
            _ => out.push((v.texture, i, 1)),
        }
    }
    out
}

/// A texture of `levels` (RGBA8 sRGB, largest first) and its bind group.
fn texture_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    label: &str,
    levels: &[(u32, u32, Vec<u8>)],
) -> wgpu::BindGroup {
    let (w, h, _) = levels.first().map_or((1, 1, Vec::new()), |l| l.clone());
    let descriptor = wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len().max(1) as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    };
    let texture = device.create_texture(&descriptor);
    for (level, (lw, lh, px)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            px,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(lw * 4),
                rows_per_image: Some(*lh),
            },
            wgpu::Extent3d {
                width: *lw,
                height: *lh,
                depth_or_array_layers: 1,
            },
        );
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// A row of `width` RGBA pixels padded to wgpu's copy alignment, bytes.
fn padded_row(width: u32) -> u32 {
    (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
}

/// `height` rows of `padded` bytes with their padding dropped.
fn unpad_rows(data: &[u8], width: u32, height: u32, padded: u32) -> Vec<u8> {
    let row = (width * 4) as usize;
    (0..height as usize)
        .flat_map(|y| {
            let start = y * padded as usize;
            &data[start..start + row]
        })
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_pad_to_256_bytes_and_back() {
        assert_eq!(padded_row(1), 256);
        assert_eq!(padded_row(64), 256);
        assert_eq!(padded_row(65), 512);
        // Two 3-pixel rows, each padded to 256 bytes.
        let mut data = vec![0u8; 512];
        data[..12].copy_from_slice(&[1; 12]);
        data[256..268].copy_from_slice(&[2; 12]);
        let px = unpad_rows(&data, 3, 2, 256);
        assert_eq!(px.len(), 24);
        assert!(px[..12].iter().all(|b| *b == 1) && px[12..].iter().all(|b| *b == 2));
    }
}
