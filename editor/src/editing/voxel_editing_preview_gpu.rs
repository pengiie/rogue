use nalgebra::Vector3;
use rogue_engine::{
    common::{
        color::Color,
        geometry::{aabb::AABB, obb::OBB},
    },
    consts,
    debug::debug_renderer::{DebugRenderer, DebugShapeFlags},
    entity::{RenderableVoxelEntity, ecs_world::ECSWorld},
    graphics::{
        backend::{GraphicsBackendRecorder, Image},
        frame_graph::{
            FrameGraphBuilder, FrameGraphComputeInfo, FrameGraphContext, FrameGraphResource,
            IntoFrameGraphResource, Pass,
        },
        renderer::Renderer,
    },
    physics::transform::Transform,
    resource::{Res, ResMut},
    voxel::{
        voxel::VoxelModelEditRegion,
        voxel_registry::{VoxelModelId, VoxelModelRegistry},
        voxel_registry_gpu::VoxelModelRegistryGpu,
    },
    window::time::Time,
};
use rogue_macros::Resource;

use crate::{
    editing::{
        voxel_editing::{EditorVoxelEditing, EditorVoxelEditingTarget},
        voxel_editing_preview::EditorVoxelEditingPreview,
    },
    session::EditorSession,
    world,
};

struct EditorVoxelPreviewPassGraphConstants {
    pass_name: &'static str,
    compute_pipeline_name: &'static str,
    compute_pipeline_info: FrameGraphComputeInfo<'static>,
}

#[derive(Resource)]
pub struct EditorVoxelEditingPreviewGpu {
    preview_pass: Option<FrameGraphResource<Pass>>,
    graph_framebuffer: Option<FrameGraphResource<Image>>,
    graph_framebuffer_depth: Option<FrameGraphResource<Image>>,
}

impl EditorVoxelEditingPreviewGpu {
    const GRAPH: EditorVoxelPreviewPassGraphConstants = EditorVoxelPreviewPassGraphConstants {
        pass_name: "voxel_preview_edit_render_pass",
        compute_pipeline_name: "voxel_preview_edit_render_compute_pipeline",
        compute_pipeline_info: FrameGraphComputeInfo {
            shader_path: "editor_voxel_preview",
            entry_point_fn: "main",
        },
    };

    pub fn new() -> Self {
        Self {
            preview_pass: None,
            graph_framebuffer: None,
            graph_framebuffer_depth: None,
        }
    }

    pub fn set_graph_voxel_preview_pass(
        &mut self,
        fg: &mut FrameGraphBuilder,
        framebuffer: impl IntoFrameGraphResource<Image>,
        framebuffer_depth: impl IntoFrameGraphResource<Image>,
    ) -> FrameGraphResource<Pass> {
        let compute_pipeline = fg.create_compute_pipeline(
            Self::GRAPH.compute_pipeline_name,
            Self::GRAPH.compute_pipeline_info,
        );
        let framebuffer_handle = framebuffer.handle(fg);
        let framebuffer_depth_handle = framebuffer_depth.handle(fg);
        self.graph_framebuffer = Some(framebuffer_handle);
        self.graph_framebuffer_depth = Some(framebuffer_depth_handle);
        fg.create_input_pass(
            Self::GRAPH.pass_name,
            &[
                &compute_pipeline,
                &framebuffer_handle,
                &framebuffer_depth_handle,
            ],
            &[&framebuffer_handle],
        )
    }

    pub fn update_preview_model_gpu(
        mut voxel_registry_gpu: &mut VoxelModelRegistryGpu,
        model_id: VoxelModelId,
    ) {
        let is_preview_gpu_model_loaded = voxel_registry_gpu.get_model_gpu_ptr(&model_id).is_some();
        if !is_preview_gpu_model_loaded {
            voxel_registry_gpu.load_gpu_model(model_id);
        } else {
            voxel_registry_gpu.mark_gpu_model_update(&model_id);
        }
    }

    pub fn write_render_preview_pass(
        voxel_editing: Res<EditorVoxelEditing>,
        preview: Res<EditorVoxelEditingPreview>,
        preview_gpu: Res<EditorVoxelEditingPreviewGpu>,
        voxel_registry: Res<VoxelModelRegistry>,
        voxel_registry_gpu: Res<VoxelModelRegistryGpu>,
        mut renderer: ResMut<Renderer>,
    ) {
        let framebuffer_handle = preview_gpu.graph_framebuffer.as_ref().expect(
            "Should not be writing edit preview pass without setting it up in the render graph first.",
        );
        let framebuffer_depth_handle = preview_gpu.graph_framebuffer_depth.as_ref().expect(
            "Should not be writing edit preview pass without setting it up in the render graph first.",
        );

        let preview_obb = renderer.executor().supply_pass_ref(
            Self::GRAPH.pass_name,
            &mut |recorder: &mut dyn GraphicsBackendRecorder, ctx: &FrameGraphContext<'_>| {
                if !preview.show_preview {
                    return;
                }
                let Some(preview_model_id) = preview.preview_model() else {
                    return;
                };
                let Some(preview_model_gpu_ptr) =
                    voxel_registry_gpu.get_model_gpu_ptr(&preview_model_id)
                else {
                    return;
                };

                let model_side_length = voxel_registry.get_dyn_model(preview_model_id).length();
                let obb = preview
                    .preview_model_transform()
                    .as_voxel_model_obb(model_side_length);
                let rot_mat = obb.rotation.to_homogeneous();

                let framebuffer = ctx.get_image(framebuffer_handle);
                let framebuffer_depth = ctx.get_image(framebuffer_depth_handle);
                let framebuffer_size = recorder.get_image_info(&framebuffer).resolution_xy();

                let pipeline = ctx.get_compute_pipeline(Self::GRAPH.compute_pipeline_name);
                let mut compute_pass = recorder.begin_compute_pass(pipeline);
                let wg_size = compute_pass.workgroup_size();

                compute_pass.bind_uniforms(&mut |writer| {
                    writer.use_set_cache("u_frame", Renderer::SET_CACHE_SLOT_FRAME);
                    writer.write_binding("u_shader.backbuffer", framebuffer);
                    //writer.write_binding("u_shader.backbuffer_depth", framebuffer_depth);
                    writer.write_uniform::<Vector3<f32>>(
                        "u_shader.entity_info.aabb_min",
                        obb.aabb.min,
                    );
                    writer.write_uniform::<Vector3<f32>>(
                        "u_shader.entity_info.aabb_max",
                        obb.aabb.max,
                    );
                    writer.write_uniform::<Vector3<f32>>(
                        "u_shader.entity_info.rotation_1",
                        Vector3::new(rot_mat.m11, rot_mat.m21, rot_mat.m31),
                    );
                    writer.write_uniform::<Vector3<f32>>(
                        "u_shader.entity_info.rotation_2",
                        Vector3::new(rot_mat.m12, rot_mat.m22, rot_mat.m32),
                    );
                    writer.write_uniform::<Vector3<f32>>(
                        "u_shader.entity_info.rotation_3",
                        Vector3::new(rot_mat.m13, rot_mat.m23, rot_mat.m33),
                    );
                    writer.write_uniform::<u32>(
                        "u_shader.entity_info.model_info_ptr",
                        preview_model_gpu_ptr,
                    );
                });

                compute_pass.dispatch(
                    (framebuffer_size.x as f32 / wg_size.x as f32).ceil() as u32,
                    (framebuffer_size.y as f32 / wg_size.y as f32).ceil() as u32,
                    1,
                );
            },
        );
    }
}
