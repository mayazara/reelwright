//! Layer compositing.

use crate::{Gpu, RenderError, fit_contain};
use rw_media::RgbaImage;
use rw_model::Rgba;
use std::sync::Arc;

/// One picture to draw, bottom-most first in the list passed to
/// [`Compositor::render`].
#[derive(Debug, Clone)]
pub struct Layer {
    pub image: Arc<RgbaImage>,
    /// Identifies the picture's content. When a slot sees the same key as
    /// last frame, the upload is skipped (paused preview, still images).
    pub key: u64,
    /// Clockwise rotation in degrees (multiple of 90).
    pub rotation: u16,
    pub opacity: f32,
}

impl Layer {
    /// Size as displayed after rotation.
    fn display_size(&self) -> (u32, u32) {
        if self.rotation % 180 == 90 {
            (self.image.height, self.image.width)
        } else {
            (self.image.width, self.image.height)
        }
    }
}

const SHADER: &str = r#"
struct LayerUniform {
    // Destination rectangle in clip space: x0, y0, x1, y1.
    rect: vec4<f32>,
    // Quarter turns clockwise, opacity, unused, unused.
    params: vec4<f32>,
};

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> layer: LayerUniform;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_layer(@builtin(vertex_index) i: u32) -> VsOut {
    // Two triangles covering the unit square.
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let c = corners[i];
    var out: VsOut;
    out.pos = vec4(mix(layer.rect.x, layer.rect.z, c.x), mix(layer.rect.y, layer.rect.w, c.y), 0.0, 1.0);
    // `c` is the display UV (0,0 top-left); map back to the source texture.
    let turns = u32(layer.params.x + 0.5) % 4u;
    var uv = c;
    if turns == 1u { uv = vec2(c.y, 1.0 - c.x); }
    if turns == 2u { uv = vec2(1.0 - c.x, 1.0 - c.y); }
    if turns == 3u { uv = vec2(1.0 - c.y, c.x); }
    out.uv = uv;
    return out;
}

@fragment
fn fs_layer(in: VsOut) -> @location(0) vec4<f32> {
    // The texture is sRGB, so sampling yields linear light; alpha is straight.
    let c = textureSample(tex, samp, in.uv);
    let a = c.a * layer.params.y;
    return vec4(c.rgb * a, a);
}

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> VsOut {
    let p = vec2(f32((i << 1u) & 2u), f32(i & 2u));
    var out: VsOut;
    out.pos = vec4(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2(p.x, 1.0 - p.y);
    return out;
}

@fragment
fn fs_blit(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, in.uv);
    return vec4(c.rgb, 1.0);
}
"#;

const CANVAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Output format: what the preview samples and export reads back.
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LayerUniform {
    rect: [f32; 4],
    params: [f32; 4],
}

struct Slot {
    texture: wgpu::Texture,
    size: (u32, u32),
    key: Option<u64>,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

struct Targets {
    size: (u32, u32),
    canvas_view: wgpu::TextureView,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    blit_bind_group: wgpu::BindGroup,
}

/// Draws layers into an output texture of a fixed size.
pub struct Compositor {
    gpu: Gpu,
    layout: wgpu::BindGroupLayout,
    layer_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    blit_uniform: wgpu::Buffer,
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

impl Compositor {
    pub fn new(gpu: &Gpu, size: (u32, u32)) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compositor"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("layer"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("compositor"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label: &str, vs: &str, fs: &str, format, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
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
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let layer_pipeline = pipeline(
            "layers",
            "vs_layer",
            "fs_layer",
            CANVAS_FORMAT,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let blit_pipeline = pipeline("blit", "vs_full", "fs_blit", OUTPUT_FORMAT, None);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("compositor"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let blit_uniform = Self::uniform_buffer(device);
        let targets = Self::make_targets(device, &layout, &sampler, &blit_uniform, size);
        Compositor {
            gpu: gpu.clone(),
            layout,
            layer_pipeline,
            blit_pipeline,
            sampler,
            blit_uniform,
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

    fn make_targets(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        uniform: &wgpu::Buffer,
        size: (u32, u32),
    ) -> Targets {
        let size = (size.0.max(1), size.1.max(1));
        let extent = wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        };
        let canvas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("canvas"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CANVAS_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("output"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let canvas_view = canvas.create_view(&Default::default());
        let output_view = output.create_view(&Default::default());
        let blit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&canvas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        Targets {
            size,
            canvas_view,
            output,
            output_view,
            blit_bind_group,
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
            self.targets = Self::make_targets(
                &self.gpu.device,
                &self.layout,
                &self.sampler,
                &self.blit_uniform,
                size,
            );
        }
    }

    /// The rendered frame, for the preview to display.
    pub fn output_view(&self) -> &wgpu::TextureView {
        &self.targets.output_view
    }

    fn slot(&mut self, index: usize, size: (u32, u32)) -> &mut Slot {
        let needs_new = self.slots.get(index).is_none_or(|s| s.size != size);
        if needs_new {
            let device = &self.gpu.device;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("layer"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let uniform = Self::uniform_buffer(device);
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("layer"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            });
            let slot = Slot {
                texture,
                size,
                key: None,
                uniform,
                bind_group,
            };
            if index < self.slots.len() {
                self.slots[index] = slot;
            } else {
                self.slots.push(slot);
            }
        }
        &mut self.slots[index]
    }

    /// Draws `layers` (bottom first) over `background`, letterboxing each to
    /// fit the output.
    pub fn render(&mut self, background: Rgba, layers: &[Layer]) {
        let out = self.targets.size;
        for (i, layer) in layers.iter().enumerate() {
            let img = &layer.image;
            let (x, y, w, h) = fit_contain(layer.display_size(), out);
            let to_clip_x = |px: f32| px / out.0 as f32 * 2.0 - 1.0;
            let to_clip_y = |py: f32| 1.0 - py / out.1 as f32 * 2.0;
            let uniform = LayerUniform {
                rect: [
                    to_clip_x(x),
                    to_clip_y(y),
                    to_clip_x(x + w),
                    to_clip_y(y + h),
                ],
                params: [
                    f32::from(layer.rotation / 90),
                    layer.opacity.clamp(0.0, 1.0),
                    0.0,
                    0.0,
                ],
            };
            let queue = self.gpu.queue.clone();
            let slot = self.slot(i, (img.width, img.height));
            if slot.key != Some(layer.key) {
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
        }

        let [r, g, b, a] = background.0;
        let clear = wgpu::Color {
            r: srgb_to_linear(r),
            g: srgb_to_linear(g),
            b: srgb_to_linear(b),
            a: f64::from(a) / 255.0,
        };
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("composite"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layers"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets.canvas_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.layer_pipeline);
            for slot in &self.slots[..layers.len()] {
                pass.set_bind_group(0, &slot.bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("output"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets.output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &self.targets.blit_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
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
