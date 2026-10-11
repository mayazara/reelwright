//! GPU device handle, either shared with the UI or created headless.

use crate::RenderError;

/// A device and queue. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// Adapter description for logs and diagnostics.
    pub adapter_name: String,
}

impl Gpu {
    /// Wraps the UI's device so preview textures need no copies.
    pub fn shared(device: wgpu::Device, queue: wgpu::Queue, adapter_name: String) -> Self {
        Gpu {
            device,
            queue,
            adapter_name,
        }
    }

    /// Creates a device without a window, for export jobs and tests. Prefers
    /// a real GPU and falls back to a software adapter (lavapipe, WARP).
    pub fn headless() -> Result<Self, RenderError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let mut last = String::from("no adapters");
        for fallback in [false, true] {
            let options = wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: fallback,
                compatible_surface: None,
                ..Default::default()
            };
            let adapter = match pollster::block_on(instance.request_adapter(&options)) {
                Ok(a) => a,
                Err(e) => {
                    last = e.to_string();
                    continue;
                }
            };
            let info = adapter.get_info();
            let desc = wgpu::DeviceDescriptor {
                label: Some("rw-render headless"),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            };
            match pollster::block_on(adapter.request_device(&desc)) {
                Ok((device, queue)) => {
                    tracing::info!(adapter = %info.name, backend = ?info.backend, "headless GPU ready");
                    return Ok(Gpu {
                        device,
                        queue,
                        adapter_name: format!("{} ({:?})", info.name, info.backend),
                    });
                }
                Err(e) => last = e.to_string(),
            }
        }
        Err(RenderError::NoAdapter(last))
    }
}
