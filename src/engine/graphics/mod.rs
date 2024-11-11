use device::DeviceResource;
use pass::ui::UIPass;
use pipeline_manager::RenderPipelineManager;
use renderer::Renderer;
use sampler::SamplerCache;

use super::resource::ResourceBank;

pub mod bvh;
pub mod camera;
pub mod device;
pub mod pass;
pub mod pipeline_manager;
pub mod renderer;
pub mod sampler;
pub mod shader;

pub fn initialize_graphics_resources(rb: &mut ResourceBank) {
    let device_ref = rb.get_resource::<DeviceResource>();

    let mut render_pipeline_manager = RenderPipelineManager::new();
    let renderer = Renderer::new(&device_ref, &mut render_pipeline_manager);
    let sampler_cache = SamplerCache::new();
    let ui_pass = UIPass::new(&device_ref);

    drop(device_ref);
    rb.insert(sampler_cache);
    rb.insert(render_pipeline_manager);
    rb.insert(renderer);
    rb.insert(ui_pass);
}
