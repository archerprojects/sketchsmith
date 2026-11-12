//! SketchSmith — GPU setup | v1.0.0 | Thursday 12 November 2026
//!
//! Builds the [`egui_wgpu::WgpuConfiguration`] used by the egui-wgpu painter.
//!
//! Backend policy: Vulkan preferred, OpenGL ES fallback. wgpu is given
//! both backends (`PRIMARY | GL`, overridable with `WGPU_BACKEND`), and a custom
//! adapter selector ranks the enumerated adapters: real hardware always beats a
//! software renderer (llvmpipe), then Vulkan beats OpenGL ES, then discrete beats
//! integrated. Every adapter found is logged, so a missing GPU driver is obvious
//! from the start-up log.
//!
//! The display handle is supplied at instance creation because Wayland with the GL
//! backend requires it.

use std::sync::Arc;

use egui_wgpu::{WgpuConfiguration, WgpuSetupCreateNew};
use winit::event_loop::OwnedDisplayHandle;

/// Returns the painter configuration for the given display.
pub fn painter_configuration(display: OwnedDisplayHandle) -> WgpuConfiguration {
    let mut setup = WgpuSetupCreateNew::from_display_handle(display);
    setup.native_adapter_selector = Some(Arc::new(select_adapter));
    WgpuConfiguration {
        wgpu_setup: setup.into(),
        ..Default::default()
    }
}

/// Picks the best adapter: hardware before software, then Vulkan before GL, then
/// discrete before integrated before virtual. Adapters that cannot present to
/// `surface` are skipped.
fn select_adapter(
    adapters: &[wgpu::Adapter],
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, String> {
    for a in adapters {
        let info = a.get_info();
        log::info!(
            "GPU found: {} ({:?}, {:?}){}",
            info.name,
            info.backend,
            info.device_type,
            if surface.is_none_or(|s| a.is_surface_supported(s)) {
                ""
            } else {
                " — cannot present to this window"
            }
        );
    }
    let chosen = adapters
        .iter()
        .filter(|adapter| surface.is_none_or(|s| adapter.is_surface_supported(s)))
        .min_by_key(|adapter| {
            let info = adapter.get_info();
            (
                info.device_type == wgpu::DeviceType::Cpu,
                backend_rank(info.backend),
                device_rank(info.device_type),
            )
        })
        .cloned()
        .ok_or_else(|| {
            format!(
                "no GPU adapter can present to the window ({} enumerated); \
                 a Vulkan or OpenGL ES driver is required",
                adapters.len()
            )
        })?;

    let info = chosen.get_info();
    log::info!(
        "GPU adapter: {} ({:?}, {:?}, driver: {} {})",
        info.name,
        info.backend,
        info.device_type,
        info.driver,
        info.driver_info
    );
    Ok(chosen)
}

/// Lower is better. Vulkan is the primary backend; GL ES is the fallback.
fn backend_rank(backend: wgpu::Backend) -> u8 {
    match backend {
        wgpu::Backend::Vulkan => 0,
        wgpu::Backend::Gl => 1,
        _ => 2,
    }
}

/// Lower is better.
fn device_rank(device_type: wgpu::DeviceType) -> u8 {
    match device_type {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Cpu => 3,
        wgpu::DeviceType::Other => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vulkan_outranks_gl() {
        assert!(backend_rank(wgpu::Backend::Vulkan) < backend_rank(wgpu::Backend::Gl));
        assert!(backend_rank(wgpu::Backend::Gl) < backend_rank(wgpu::Backend::Noop));
    }

    #[test]
    fn software_vulkan_never_beats_hardware_gl() {
        // The ranking key: (is_software, backend, device type). Lower wins.
        let soft_vulkan = (
            true,
            backend_rank(wgpu::Backend::Vulkan),
            device_rank(wgpu::DeviceType::Cpu),
        );
        let hard_gl = (
            false,
            backend_rank(wgpu::Backend::Gl),
            device_rank(wgpu::DeviceType::DiscreteGpu),
        );
        assert!(hard_gl < soft_vulkan);
    }

    #[test]
    fn hardware_outranks_software() {
        assert!(
            device_rank(wgpu::DeviceType::DiscreteGpu)
                < device_rank(wgpu::DeviceType::IntegratedGpu)
        );
        assert!(device_rank(wgpu::DeviceType::IntegratedGpu) < device_rank(wgpu::DeviceType::Cpu));
    }
}
