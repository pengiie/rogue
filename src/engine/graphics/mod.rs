use device::DeviceResource;
use renderer::Renderer;
use shader::ShaderCompiler;

use super::resource::ResourceBank;

pub mod backend;
pub mod bvh;
pub mod camera;
pub mod device;
pub mod frame_graph;
pub mod gpu_allocator;
pub mod pass;
pub mod render_contants;
pub mod renderer;
//pub mod sampler;
pub mod shader;
pub mod vulkan;
//pub mod wgpu;

pub fn initialize_graphics_resources(app: &mut crate::app::App) {
    let mut device_ref_mut = app.get_resource_mut::<DeviceResource>();

    let shader_compiler = ShaderCompiler::new();
    let renderer = Renderer::new(&mut device_ref_mut);
    drop(device_ref_mut);

    app.insert_resource(renderer);
    app.insert_resource(shader_compiler);
}
