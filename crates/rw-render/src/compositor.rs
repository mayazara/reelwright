//! Layer compositing.
//!
//! Each layer is a full-canvas pass that reads the canvas so far (ping-pong
//! between two `Rgba16Float` textures), so every blend mode is possible. The
//! final canvas is copied to an sRGB output texture.
//!
//! A layer with effects, or in a transition, is first drawn on its own into
//! a scratch texture (premultiplied alpha). Each effect is one or more
//! passes over that image; a transition combines the two clips' images;
//! a merge pass then blends the result onto the canvas.

use crate::{Gpu, RenderError};
use rw_media::RgbaImage;
use rw_model::{EffectKind, Fit, Look, Rgba, TransitionKind};
use std::collections::HashMap;
use std::sync::Arc;

/// What a layer shows.
#[derive(Debug, Clone)]
pub enum Source {
    Image(Arc<RgbaImage>),
    Solid(Rgba),
    /// Linear gradient; `angle` in degrees (0 = left→right, 90 = top→bottom).
    Gradient {
        from: Rgba,
        to: Rgba,
        angle: f32,
    },
}

/// One picture to draw, bottom-most first in the list passed to
/// [`Compositor::render`].
#[derive(Debug, Clone)]
pub struct Layer {
    pub source: Source,
    /// Identifies the picture's content. When a slot sees the same key as
    /// last frame, the upload is skipped (paused preview, still images).
    pub key: u64,
    /// Clockwise rotation (multiple of 90) of the stored image relative to
    /// how it should be displayed.
    pub rotation: u16,
    /// Placement, crop, colour, blend, effects. `look.opacity` already
    /// includes fades.
    pub look: Look,
    /// Seconds since the clip started, and the share of it played: animated
    /// effects (grain, shake, slow zoom) follow these.
    pub clip_time: f32,
    pub clip_progress: f32,
    /// Set when this layer is coming in through a transition from another.
    pub transition: Option<Box<LayerTransition>>,
}

/// The clip a layer is replacing, and how far along the change is.
#[derive(Debug, Clone)]
pub struct LayerTransition {
    pub from: Layer,
    pub kind: TransitionKind,
    /// `0..=1`.
    pub progress: f32,
}

impl Layer {
    /// A full-frame layer with no adjustments.
    pub fn new(source: Source) -> Self {
        Layer {
            source,
            key: 0,
            rotation: 0,
            look: Look::default(),
            clip_time: 0.0,
            clip_progress: 0.0,
            transition: None,
        }
    }

    /// A full-frame image layer with no adjustments.
    pub fn image(image: Arc<RgbaImage>, key: u64, rotation: u16) -> Self {
        Layer {
            key,
            rotation,
            ..Layer::new(Source::Image(image))
        }
    }

    /// Whether the layer must be drawn on its own before reaching the canvas.
    fn isolated(&self) -> bool {
        self.transition.is_some() || self.look.effects.iter().any(|e| e.intensity > 0.0)
    }

    /// The picture, for image layers.
    pub fn as_image(&self) -> Option<&Arc<RgbaImage>> {
        match &self.source {
            Source::Image(img) => Some(img),
            _ => None,
        }
    }

    /// Display size of the source before cropping; generated fills take the
    /// output's shape.
    fn display_size(&self, out: (u32, u32)) -> (f32, f32) {
        match &self.source {
            Source::Image(img) if self.rotation % 180 == 90 => {
                (img.height as f32, img.width as f32)
            }
            Source::Image(img) => (img.width as f32, img.height as f32),
            _ => (out.0 as f32, out.1 as f32),
        }
    }
}

const SHADER: &str = concat!(include_str!("common.wgsl"), include_str!("shader.wgsl"));
const FX_SHADER: &str = concat!(include_str!("common.wgsl"), include_str!("effects.wgsl"));
const CANVAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Output format: what the preview samples and export reads back.
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LayerUniform {
    center: [f32; 2],
    half_box: [f32; 2],
    half_content: [f32; 2],
    rot: [f32; 2],
    crop_origin: [f32; 2],
    crop_size: [f32; 2],
    flip: [f32; 2],
    out_size: [f32; 2],
    p0: [f32; 4],
    c0: [f32; 4],
    c1: [f32; 4],
    c2: [f32; 4],
    fill0: [f32; 4],
    fill1: [f32; 4],
    grad: [f32; 4],
}

/// Parameters of an effect, transition or merge pass (`Fx` in `effects.wgsl`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct FxUniform {
    a: [f32; 4],
    b: [f32; 4],
    p0: [f32; 4],
    p1: [f32; 4],
    geo: [f32; 4],
}

/// Number of passes an effect takes.
fn effect_passes(kind: EffectKind) -> u32 {
    match kind {
        EffectKind::Blur => 2,
        EffectKind::BackgroundBlurFill => 3,
        _ => 1,
    }
}

fn effect_index(kind: EffectKind) -> u32 {
    EffectKind::ALL.iter().position(|k| *k == kind).unwrap_or(0) as u32
}

fn rgba_f(c: Rgba) -> [f32; 4] {
    c.0.map(|v| f32::from(v) / 255.0)
}

/// Geometry and shading parameters for one layer in an output of `out`.
/// Size in output pixels at which a source of `display` size (before
/// cropping) is drawn with `look`. Generated fills always fill their box.
/// The UI uses this to place on-canvas handles exactly over the picture.
pub fn content_size(
    display: (f32, f32),
    look: &Look,
    out: (u32, u32),
    generated: bool,
) -> (f32, f32) {
    let t = look.transform;
    let (kx, ky) = look.crop.kept();
    let (cw, ch) = ((display.0 * kx).max(1e-3), (display.1 * ky).max(1e-3));
    let (bw, bh) = (t.size[0] * out.0 as f32, t.size[1] * out.1 as f32);
    match if generated { Fit::Stretch } else { t.fit } {
        Fit::Contain => {
            let s = (bw / cw).min(bh / ch);
            (cw * s, ch * s)
        }
        Fit::Cover => {
            let s = (bw / cw).max(bh / ch);
            (cw * s, ch * s)
        }
        Fit::Stretch => (bw, bh),
    }
}

pub(crate) fn layer_uniform(
    layer: &Layer,
    out: (u32, u32),
    lut_size: Option<usize>,
) -> LayerUniform {
    let look = &layer.look;
    let t = look.transform;
    let (ow, oh) = (out.0 as f32, out.1 as f32);
    let (kx, ky) = look.crop.kept();
    let (bw, bh) = (t.size[0] * ow, t.size[1] * oh);
    let generated = !matches!(layer.source, Source::Image(_));
    let fit = if generated { Fit::Stretch } else { t.fit };
    let content = content_size(layer.display_size(out), look, out, generated);
    let theta = t.rotation.to_radians();
    let c = look.color;
    let (kind, fill0, fill1, grad) = match &layer.source {
        Source::Image(_) => (0.0, [0.0; 4], [0.0; 4], [0.0; 4]),
        Source::Solid(col) => (1.0, rgba_f(*col), rgba_f(*col), [0.0; 4]),
        Source::Gradient { from, to, angle } => {
            let a = angle.to_radians();
            (
                2.0,
                rgba_f(*from),
                rgba_f(*to),
                [a.cos(), a.sin(), 0.0, 0.0],
            )
        }
    };
    let lut_intensity = match (&look.filter, lut_size) {
        (Some(f), Some(_)) => f.intensity,
        _ => 0.0,
    };
    LayerUniform {
        center: [t.center[0] * ow, t.center[1] * oh],
        half_box: [bw / 2.0, bh / 2.0],
        half_content: [content.0 / 2.0, content.1 / 2.0],
        rot: [theta.cos(), theta.sin()],
        crop_origin: [look.crop.left, look.crop.top],
        crop_size: [kx, ky],
        flip: [
            if t.flip_h { -1.0 } else { 1.0 },
            if t.flip_v { -1.0 } else { 1.0 },
        ],
        out_size: [ow, oh],
        p0: [
            f32::from((layer.rotation / 90) % 4),
            look.opacity.clamp(0.0, 1.0),
            look.blend.index() as f32,
            kind,
        ],
        c0: [c.exposure, c.brightness, c.contrast, c.saturation],
        c1: [c.vibrance, c.temperature, c.tint, c.highlights],
        c2: [
            c.shadows,
            lut_intensity,
            lut_size.unwrap_or(2) as f32,
            if fit == Fit::Cover { 1.0 } else { 0.0 },
        ],
        fill0,
        fill1,
        grad,
    }
}

/// A texture a pass reads or writes.
#[derive(Debug, Clone, Copy)]
enum Tex {
    Slot(usize),
    Blank,
    Canvas(usize),
    Scratch(usize),
}

#[derive(Debug, Clone, Copy)]
enum Pipe {
    Layer,
    Effect,
    Transition,
    Merge,
}

#[derive(Debug, Clone, Copy)]
enum Uni {
    Slot(usize),
    Fx(usize),
}

/// One full-canvas draw.
#[derive(Debug, Clone, Copy)]
struct Pass {
    pipe: Pipe,
    src: Tex,
    other: Tex,
    out: Tex,
    uniform: Uni,
}

/// Every pass of a frame, with each slot's filter table and the effect
/// parameters, worked out before anything is recorded.
#[derive(Debug, Default)]
struct Plan {
    passes: Vec<Pass>,
    luts: Vec<Option<String>>,
    fx: Vec<FxUniform>,
}

struct Slot {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    key: Option<u64>,
    uniform: wgpu::Buffer,
}

struct Targets {
    size: (u32, u32),
    canvas: [wgpu::TextureView; 2],
    /// Textures for layers drawn on their own, made on first use.
    scratch: Vec<wgpu::TextureView>,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
}

/// Draws layers into an output texture of a fixed size.
pub struct Compositor {
    gpu: Gpu,
    layout: wgpu::BindGroupLayout,
    layer_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    fx_layout: wgpu::BindGroupLayout,
    effect_pipeline: wgpu::RenderPipeline,
    transition_pipeline: wgpu::RenderPipeline,
    merge_pipeline: wgpu::RenderPipeline,
    fx_uniforms: Vec<wgpu::Buffer>,
    sampler: wgpu::Sampler,
    blit_uniform: wgpu::Buffer,
    blank: wgpu::TextureView,
    identity_lut: wgpu::TextureView,
    luts: HashMap<String, (wgpu::TextureView, usize)>,
    slots: Vec<Slot>,
    targets: Targets,
}

impl std::fmt::Debug for Compositor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compositor")
            .field("size", &self.targets.size)
            .finish()
    }
}

fn srgb_to_linear(v: u8) -> f64 {
    let s = f64::from(v) / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

fn texture_2d(
    device: &wgpu::Device,
    label: &str,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn lut_texture(gpu: &Gpu, lut: &rw_fx::Lut) -> wgpu::TextureView {
    let n = lut.size as u32;
    let extent = wgpu::Extent3d {
        width: n,
        height: n,
        depth_or_array_layers: n,
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lut"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &lut.to_rgba8(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(n * 4),
            rows_per_image: Some(n),
        },
        extent,
    );
    texture.create_view(&Default::default())
}

impl Compositor {
    pub fn new(gpu: &Gpu, size: (u32, u32)) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compositor"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let tex_entry = |binding, dim, filterable| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: dim,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("layer"),
            entries: &[
                tex_entry(0, wgpu::TextureViewDimension::D2, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(3, wgpu::TextureViewDimension::D2, false),
                tex_entry(4, wgpu::TextureViewDimension::D3, true),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("compositor"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label: &str, fs: &str, format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let layer_pipeline = pipeline("layers", "fs_layer", CANVAS_FORMAT);
        let blit_pipeline = pipeline("blit", "fs_blit", OUTPUT_FORMAT);

        let fx_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects"),
            source: wgpu::ShaderSource::Wgsl(FX_SHADER.into()),
        });
        let fx_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effect"),
            entries: &[
                tex_entry(0, wgpu::TextureViewDimension::D2, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(3, wgpu::TextureViewDimension::D2, true),
            ],
        });
        let fx_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects"),
            bind_group_layouts: &[Some(&fx_layout)],
            immediate_size: 0,
        });
        let fx_pipeline = |label: &str, fs: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&fx_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &fx_module,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &fx_module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: CANVAS_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let effect_pipeline = fx_pipeline("effect", "fs_effect");
        let transition_pipeline = fx_pipeline("transition", "fs_transition");
        let merge_pipeline = fx_pipeline("merge", "fs_merge");
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("compositor"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let blit_uniform = Self::uniform_buffer(device);
        let blank = texture_2d(
            device,
            "blank",
            (1, 1),
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureUsages::TEXTURE_BINDING,
        )
        .create_view(&Default::default());
        let identity_lut = lut_texture(gpu, &rw_fx::Lut::identity(2));
        let targets = Self::make_targets(device, size);
        Compositor {
            gpu: gpu.clone(),
            layout,
            layer_pipeline,
            blit_pipeline,
            fx_layout,
            effect_pipeline,
            transition_pipeline,
            merge_pipeline,
            fx_uniforms: Vec::new(),
            sampler,
            blit_uniform,
            blank,
            identity_lut,
            luts: HashMap::new(),
            slots: Vec::new(),
            targets,
        }
    }

    fn uniform_buffer(device: &wgpu::Device) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("layer uniform"),
            size: std::mem::size_of::<LayerUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn make_targets(device: &wgpu::Device, size: (u32, u32)) -> Targets {
        let size = (size.0.max(1), size.1.max(1));
        let canvas_usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let canvas = [
            texture_2d(device, "canvas a", size, CANVAS_FORMAT, canvas_usage)
                .create_view(&Default::default()),
            texture_2d(device, "canvas b", size, CANVAS_FORMAT, canvas_usage)
                .create_view(&Default::default()),
        ];
        let output = texture_2d(
            device,
            "output",
            size,
            OUTPUT_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        let output_view = output.create_view(&Default::default());
        Targets {
            size,
            canvas,
            scratch: Vec::new(),
            output,
            output_view,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.targets.size
    }

    /// Changes the output size. The output texture is replaced, so a UI that
    /// registered it must register the new view.
    pub fn resize(&mut self, size: (u32, u32)) {
        let size = (size.0.max(1), size.1.max(1));
        if size != self.targets.size {
            self.targets = Self::make_targets(&self.gpu.device, size);
        }
    }

    /// The rendered frame, for the preview to display.
    pub fn output_view(&self) -> &wgpu::TextureView {
        &self.targets.output_view
    }

    fn slot(&mut self, index: usize, size: (u32, u32)) -> &mut Slot {
        let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        while self.slots.len() <= index {
            let texture = texture_2d(
                &self.gpu.device,
                "layer",
                (1, 1),
                wgpu::TextureFormat::Rgba8UnormSrgb,
                usage,
            );
            let view = texture.create_view(&Default::default());
            self.slots.push(Slot {
                texture,
                view,
                size: (1, 1),
                key: None,
                uniform: Self::uniform_buffer(&self.gpu.device),
            });
        }
        let slot = &mut self.slots[index];
        if slot.size != size {
            slot.texture = texture_2d(
                &self.gpu.device,
                "layer",
                size,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                usage,
            );
            slot.view = slot.texture.create_view(&Default::default());
            slot.size = size;
            slot.key = None;
        }
        slot
    }

    /// The GPU table for a filter id, loaded on first use.
    fn lut(&mut self, id: &str) -> Option<usize> {
        if !self.luts.contains_key(id) {
            let info = rw_fx::filter(id)?;
            self.luts.insert(
                id.to_owned(),
                (lut_texture(&self.gpu, &info.lut), info.lut.size),
            );
        }
        self.luts.get(id).map(|(_, n)| *n)
    }

    fn bind_group(
        &self,
        src: &wgpu::TextureView,
        uniform: &wgpu::Buffer,
        dst: &wgpu::TextureView,
        lut: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        self.gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("layer"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(src),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(dst),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(lut),
                    },
                ],
            })
    }

    /// Uploads a layer's picture and parameters to slot `index`; returns
    /// the filter table it uses, if any.
    fn prepare_slot(&mut self, index: usize, layer: &Layer, isolated: bool) -> Option<String> {
        let out = self.targets.size;
        let lut_id = layer
            .look
            .filter
            .as_ref()
            .filter(|f| f.intensity > 0.0)
            .map(|f| f.id.clone());
        let lut_size = lut_id.as_deref().and_then(|id| self.lut(id));
        let mut uniform = layer_uniform(layer, out, lut_size);
        if isolated {
            uniform.grad[2] = 1.0;
        }
        let size = match &layer.source {
            Source::Image(img) => (img.width, img.height),
            _ => (1, 1),
        };
        let queue = self.gpu.queue.clone();
        let slot = self.slot(index, size);
        if let Source::Image(img) = &layer.source
            && slot.key != Some(layer.key)
        {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &slot.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &img.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(img.width * 4),
                    rows_per_image: Some(img.height),
                },
                wgpu::Extent3d {
                    width: img.width,
                    height: img.height,
                    depth_or_array_layers: 1,
                },
            );
            slot.key = Some(layer.key);
        }
        queue.write_buffer(&slot.uniform, 0, bytemuck::bytes_of(&uniform));
        lut_size.and(lut_id)
    }

    /// Plans drawing `layer` on its own into scratch textures
    /// `base..base + 3`, then its effects; returns the scratch holding the
    /// result.
    fn plan_isolated(&mut self, plan: &mut Plan, layer: &Layer, base: usize) -> usize {
        let slot = plan.luts.len();
        let lut = self.prepare_slot(slot, layer, true);
        plan.luts.push(lut);
        plan.passes.push(Pass {
            pipe: Pipe::Layer,
            src: Tex::Slot(slot),
            other: Tex::Blank,
            out: Tex::Scratch(base),
            uniform: Uni::Slot(slot),
        });
        let out = self.targets.size;
        let geo = layer_uniform(layer, out, None);
        // Half size of what shows: a filled picture is cut to its box.
        let visible = if geo.c2[3] > 0.5 {
            [
                geo.half_box[0].min(geo.half_content[0]),
                geo.half_box[1].min(geo.half_content[1]),
            ]
        } else {
            geo.half_content
        };
        let mut current = base;
        for effect in layer.look.effects.iter().filter(|e| e.intensity > 0.0) {
            let input = current;
            let params = rw_fx::effects::resolved_params(effect);
            for m in 0..effect_passes(effect.kind) {
                // Write somewhere that isn't being read.
                let next = (base..base + 3)
                    .find(|s| *s != current && *s != input)
                    .unwrap_or(base);
                let index = plan.fx.len();
                plan.fx.push(FxUniform {
                    a: [
                        out.0 as f32,
                        out.1 as f32,
                        effect_index(effect.kind) as f32,
                        effect.intensity.clamp(0.0, 1.0),
                    ],
                    b: [layer.clip_time, layer.clip_progress, 0.0, m as f32],
                    p0: [params[0], params[1], params[2], params[3]],
                    p1: [params[4], params[5], params[6], params[7]],
                    geo: [geo.center[0], geo.center[1], visible[0], visible[1]],
                });
                plan.passes.push(Pass {
                    pipe: Pipe::Effect,
                    src: Tex::Scratch(current),
                    other: Tex::Scratch(input),
                    out: Tex::Scratch(next),
                    uniform: Uni::Fx(index),
                });
                current = next;
            }
        }
        current
    }

    /// Draws `layers` (bottom first) over `background`.
    pub fn render(&mut self, background: Rgba, layers: &[Layer]) {
        let out = self.targets.size;
        let mut plan = Plan::default();
        let mut canvas = 0;
        for layer in layers {
            if !layer.isolated() {
                let slot = plan.luts.len();
                let lut = self.prepare_slot(slot, layer, false);
                plan.luts.push(lut);
                plan.passes.push(Pass {
                    pipe: Pipe::Layer,
                    src: Tex::Slot(slot),
                    other: Tex::Canvas(canvas),
                    out: Tex::Canvas(1 - canvas),
                    uniform: Uni::Slot(slot),
                });
                canvas = 1 - canvas;
                continue;
            }
            let mut result = self.plan_isolated(&mut plan, layer, 0);
            if let Some(tr) = &layer.transition {
                let from = self.plan_isolated(&mut plan, &tr.from, 3);
                let target = (0..3).find(|s| *s != result).unwrap_or(0);
                let index = plan.fx.len();
                plan.fx.push(FxUniform {
                    a: [out.0 as f32, out.1 as f32, tr.kind.index() as f32, 1.0],
                    b: [0.0, tr.progress.clamp(0.0, 1.0), 0.0, 0.0],
                    ..FxUniform::default()
                });
                plan.passes.push(Pass {
                    pipe: Pipe::Transition,
                    src: Tex::Scratch(from),
                    other: Tex::Scratch(result),
                    out: Tex::Scratch(target),
                    uniform: Uni::Fx(index),
                });
                result = target;
            }
            let index = plan.fx.len();
            plan.fx.push(FxUniform {
                a: [out.0 as f32, out.1 as f32, 0.0, 0.0],
                b: [0.0, 0.0, 0.0, layer.look.blend.index() as f32],
                ..FxUniform::default()
            });
            plan.passes.push(Pass {
                pipe: Pipe::Merge,
                src: Tex::Scratch(result),
                other: Tex::Canvas(canvas),
                out: Tex::Canvas(1 - canvas),
                uniform: Uni::Fx(index),
            });
            canvas = 1 - canvas;
        }
        self.upload_plan(&plan);

        let blit = LayerUniform {
            out_size: [out.0 as f32, out.1 as f32],
            ..LayerUniform::default()
        };
        self.gpu
            .queue
            .write_buffer(&self.blit_uniform, 0, bytemuck::bytes_of(&blit));

        let [r, g, b, _] = background.0;
        let clear = wgpu::Color {
            r: srgb_to_linear(r),
            g: srgb_to_linear(g),
            b: srgb_to_linear(b),
            a: 1.0,
        };
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("composite"),
            });
        fn pass<'e>(
            encoder: &'e mut wgpu::CommandEncoder,
            target: &wgpu::TextureView,
            load: wgpu::LoadOp<wgpu::Color>,
        ) -> wgpu::RenderPass<'e> {
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compositor"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
        }
        drop(pass(
            &mut encoder,
            &self.targets.canvas[0],
            wgpu::LoadOp::Clear(clear),
        ));
        for step in &plan.passes {
            let (group, pipeline) = match (step.pipe, step.uniform) {
                (Pipe::Layer, Uni::Slot(i)) => {
                    let lut = match &plan.luts[i] {
                        Some(id) => &self.luts[id].0,
                        None => &self.identity_lut,
                    };
                    let group = self.bind_group(
                        self.view(step.src),
                        &self.slots[i].uniform,
                        self.view(step.other),
                        lut,
                    );
                    (group, &self.layer_pipeline)
                }
                (pipe, Uni::Fx(i)) => {
                    let group = self.fx_bind_group(
                        self.view(step.src),
                        &self.fx_uniforms[i],
                        self.view(step.other),
                    );
                    let pipeline = match pipe {
                        Pipe::Transition => &self.transition_pipeline,
                        Pipe::Merge => &self.merge_pipeline,
                        _ => &self.effect_pipeline,
                    };
                    (group, pipeline)
                }
                (_, Uni::Slot(_)) => unreachable!("only layer passes use slot uniforms"),
            };
            let mut p = pass(
                &mut encoder,
                self.view(step.out),
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            );
            p.set_pipeline(pipeline);
            p.set_bind_group(0, &group, &[]);
            p.draw(0..3, 0..1);
        }
        let group = self.bind_group(
            &self.blank,
            &self.blit_uniform,
            &self.targets.canvas[canvas],
            &self.identity_lut,
        );
        let mut p = pass(
            &mut encoder,
            &self.targets.output_view,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        p.set_pipeline(&self.blit_pipeline);
        p.set_bind_group(0, &group, &[]);
        p.draw(0..3, 0..1);
        drop(p);
        self.gpu.queue.submit([encoder.finish()]);
    }

    /// Makes the scratch textures and effect uniforms a plan needs and
    /// uploads the effect parameters.
    fn upload_plan(&mut self, plan: &Plan) {
        let device = &self.gpu.device;
        let scratch_needed = plan
            .passes
            .iter()
            .flat_map(|p| [p.src, p.other, p.out])
            .filter_map(|t| match t {
                Tex::Scratch(i) => Some(i + 1),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        while self.targets.scratch.len() < scratch_needed {
            let view = texture_2d(
                device,
                "scratch",
                self.targets.size,
                CANVAS_FORMAT,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            )
            .create_view(&Default::default());
            self.targets.scratch.push(view);
        }
        while self.fx_uniforms.len() < plan.fx.len() {
            self.fx_uniforms
                .push(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("effect uniform"),
                    size: std::mem::size_of::<FxUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
        }
        for (buffer, fx) in self.fx_uniforms.iter().zip(&plan.fx) {
            self.gpu
                .queue
                .write_buffer(buffer, 0, bytemuck::bytes_of(fx));
        }
    }

    fn view(&self, tex: Tex) -> &wgpu::TextureView {
        match tex {
            Tex::Slot(i) => &self.slots[i].view,
            Tex::Blank => &self.blank,
            Tex::Canvas(i) => &self.targets.canvas[i],
            Tex::Scratch(i) => &self.targets.scratch[i],
        }
    }

    fn fx_bind_group(
        &self,
        src: &wgpu::TextureView,
        uniform: &wgpu::Buffer,
        other: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        self.gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("effect"),
                layout: &self.fx_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(src),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(other),
                    },
                ],
            })
    }

    /// Copies the last rendered frame to the CPU (sRGB RGBA8).
    pub fn read_back(&self) -> Result<RgbaImage, RenderError> {
        let (w, h) = self.targets.size;
        let unpadded = w * 4;
        let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let device = &self.gpu.device;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.targets.output,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let index = self.gpu.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: None,
            })
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        rx.recv()
            .map_err(|e| RenderError::Readback(e.to_string()))?
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        let data = slice
            .get_mapped_range()
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        let mut pixels = Vec::with_capacity((unpadded * h) as usize);
        for row in 0..h as usize {
            let start = row * padded as usize;
            pixels.extend_from_slice(&data[start..start + unpadded as usize]);
        }
        drop(data);
        buffer.unmap();
        Ok(RgbaImage::new(w, h, pixels))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rw_model::Transform;

    fn near(a: [f32; 2], b: [f32; 2]) {
        assert!(
            (a[0] - b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01,
            "{a:?} != {b:?}"
        );
    }

    fn img(w: u32, h: u32) -> Layer {
        Layer::image(
            Arc::new(RgbaImage::new(w, h, vec![0; (w * h * 4) as usize])),
            1,
            0,
        )
    }

    #[test]
    fn contain_cover_stretch_geometry() {
        let out = (1920, 1080);
        // 4:3 picture full frame: letterboxed to 1440 wide.
        let u = layer_uniform(&img(400, 300), out, None);
        near(u.half_content, [720.0, 540.0]);
        near(u.center, [960.0, 540.0]);
        let mut l = img(400, 300);
        l.look.transform.fit = Fit::Cover;
        near(layer_uniform(&l, out, None).half_content, [960.0, 720.0]);
        l.look.transform.fit = Fit::Stretch;
        near(layer_uniform(&l, out, None).half_content, [960.0, 540.0]);
    }

    #[test]
    fn crop_and_rotation_metadata_change_the_shape() {
        let out = (1000, 1000);
        // 200x100 stored, displayed rotated 90°: a portrait 100x200.
        let mut l = img(200, 100);
        l.rotation = 90;
        near(layer_uniform(&l, out, None).half_content, [250.0, 500.0]);
        // Cropping half the width away leaves 50x200.
        l.look.crop.right = 0.5;
        near(layer_uniform(&l, out, None).half_content, [125.0, 500.0]);
    }

    #[test]
    fn box_and_generated_fill() {
        let mut l = Layer {
            source: Source::Solid(Rgba([255, 0, 0, 255])),
            key: 0,
            rotation: 0,
            look: Look::default(),
            clip_time: 0.0,
            clip_progress: 0.0,
            transition: None,
        };
        l.look.transform = Transform {
            center: [0.25, 0.5],
            size: [0.5, 1.0],
            ..Transform::default()
        };
        let u = layer_uniform(&l, (800, 400), None);
        near(u.center, [200.0, 200.0]);
        near(u.half_content, [200.0, 200.0]);
        assert_eq!(u.p0[3], 1.0);
    }
}
