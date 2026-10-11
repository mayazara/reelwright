//! Choosing a graphics adapter.
//!
//! The app must open on any machine: a gaming PC, a laptop with an
//! integrated GPU, or a virtual machine with no GPU at all. We rank every
//! adapter that can draw to the window and take the best, so a software
//! adapter (Windows' WARP, Mesa's llvmpipe) is used when nothing else works.

use eframe::egui_wgpu::wgpu;

/// Lower is better: (device class, backend preference).
pub fn rank(info: &wgpu::AdapterInfo) -> (u8, u8) {
    let class = match info.device_type {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Other => 3,
        wgpu::DeviceType::Cpu => 4,
    };
    // Each platform's native API first; GL last (fewest features).
    let backend = match info.backend {
        wgpu::Backend::Dx12 if cfg!(windows) => 0,
        wgpu::Backend::Metal => 0,
        wgpu::Backend::Vulkan => 1,
        wgpu::Backend::Dx12 => 2,
        wgpu::Backend::Gl => 3,
        _ => 4,
    };
    (class, backend)
}

/// The best adapter that supports the window's surface.
pub fn select(
    adapters: &[wgpu::Adapter],
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, String> {
    let mut usable: Vec<&wgpu::Adapter> = adapters
        .iter()
        .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
        .collect();
    usable.sort_by_key(|a| rank(&a.get_info()));
    let chosen = usable.first().ok_or_else(|| {
        let names: Vec<String> = adapters.iter().map(|a| a.get_info().name).collect();
        format!(
            "no graphics adapter can draw to the window (found: {})",
            names.join(", ")
        )
    })?;
    tracing::info!(adapter = %chosen.get_info().name, backend = ?chosen.get_info().backend, "graphics adapter chosen");
    Ok((*chosen).clone())
}

/// eframe's wgpu settings with our adapter selection.
pub fn wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    let mut config = eframe::egui_wgpu::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut config.wgpu_setup {
        create.native_adapter_selector = Some(std::sync::Arc::new(select));
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(device_type: wgpu::DeviceType, backend: wgpu::Backend) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: String::new(),
            vendor: 0,
            device: 0,
            device_type,
            driver: String::new(),
            driver_info: String::new(),
            backend,
            ..Default::default()
        }
    }

    #[test]
    fn hardware_beats_software_and_gl_comes_last() {
        let gpu = rank(&info(wgpu::DeviceType::DiscreteGpu, wgpu::Backend::Vulkan));
        let igpu = rank(&info(
            wgpu::DeviceType::IntegratedGpu,
            wgpu::Backend::Vulkan,
        ));
        let warp = rank(&info(wgpu::DeviceType::Cpu, wgpu::Backend::Dx12));
        let gl = rank(&info(wgpu::DeviceType::IntegratedGpu, wgpu::Backend::Gl));
        assert!(gpu < igpu && igpu < warp);
        assert!(igpu < gl);
    }
}
