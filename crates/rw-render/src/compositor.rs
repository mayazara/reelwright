//! Layer compositing.
//!
//! Each layer is a full-canvas pass that reads the canvas so far (ping-pong
//! between two `Rgba16Float` textures), so every blend mode is possible. The
//! final canvas is copied to an sRGB output texture.

use crate::{Gpu, RenderError};
use rw_media::RgbaImage;
use rw_model::{Fit, Look, Rgba};
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
    /// Placement, crop, colour, blend. `look.opacity` already includes fades.
    pub look: Look,
}

impl Layer {
    /// A full-frame image layer with no adjustments.
    pub fn image(image: Arc<RgbaImage>, key: u64, rotation: u16) -> Self {
        Layer {
            source: Source::Image(image),
            key,
            rotation,
            look: Look::default(),
        }
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

const SHADER: &str = include_str!("shader.wgsl");
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

fn rgba_f(c: Rgba) -> [f32; 4] {
    c.0.map(|v| f32::from(v) / 255.0)
}

/// Geometry and shading parameters for one layer in an output of `out`.
pub(crate) fn layer_uniform(
    layer: &Layer,
    out: (u32, u32),
    lut_size: Option<usize>,
) -> LayerUniform {
    let look = &layer.look;
    let t = look.transform;
    let (ow, oh) = (out.0 as f32, out.1 as f32);
    let (dw, dh) = layer.display_size(out);
    let (kx, ky) = look.crop.kept();
    let (cw, ch) = ((dw * kx).max(1e-3), (dh * ky).max(1e-3));
    let (bw, bh) = (t.size[0] * ow, t.size[1] * oh);
    let generated = !matches!(layer.source, Source::Image(_));
    let fit = if generated { Fit::Stretch } else { t.fit };
    let content = match fit {
        Fit::Contain => {
            let s = (bw / cw).min(bh / ch);
            (cw * s, ch * s)
        }
        Fit::Cover => {
            let s = (bw / cw).max(bh / ch);
            (cw * s, ch * s)
        }
        Fit::Stretch => (bw, bh),
    };
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
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
}

/// Draws layers into an output texture of a fixed size.
pub struct Compositor {
    gpu: Gpu,
    layout: wgpu::BindGroupLayout,
    layer_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
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

    /// Draws `layers` (bottom first) over `background`.
    pub fn render(&mut self, background: Rgba, layers: &[Layer]) {
        let out = self.targets.size;
        let queue = self.gpu.queue.clone();
        // Upload textures and uniforms; remember each layer's LUT.
        let mut lut_ids: Vec<Option<String>> = Vec::with_capacity(layers.len());
        for (i, layer) in layers.iter().enumerate() {
            let lut_id = layer
                .look
                .filter
                .as_ref()
                .filter(|f| f.intensity > 0.0)
                .map(|f| f.id.clone());
            let lut_size = lut_id.as_deref().and_then(|id| self.lut(id));
            let uniform = layer_uniform(layer, out, lut_size);
            let size = match &layer.source {
                Source::Image(img) => (img.width, img.height),
                _ => (1, 1),
            };
            let slot = self.slot(i, size);
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
            lut_ids.push(lut_size.and(lut_id));
        }
        let blit = LayerUniform {
            out_size: [out.0 as f32, out.1 as f32],
            ..LayerUniform::default()
        };
        queue.write_buffer(&self.blit_uniform, 0, bytemuck::bytes_of(&blit));

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
        let mut current = 0;
        for (i, layer) in layers.iter().enumerate() {
            let src = match layer.source {
                Source::Image(_) => &self.slots[i].view,
                _ => &self.blank,
            };
            let lut = match &lut_ids[i] {
                Some(id) => &self.luts[id].0,
                None => &self.identity_lut,
            };
            let group = self.bind_group(
                src,
                &self.slots[i].uniform,
                &self.targets.canvas[current],
                lut,
            );
            let mut p = pass(
                &mut encoder,
                &self.targets.canvas[1 - current],
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
            p.set_pipeline(&self.layer_pipeline);
            p.set_bind_group(0, &group, &[]);
            p.draw(0..3, 0..1);
            drop(p);
            current = 1 - current;
        }
        let group = self.bind_group(
            &self.blank,
            &self.blit_uniform,
            &self.targets.canvas[current],
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
