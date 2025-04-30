use core::panic;
use std::{
    any::Any,
    collections::{HashMap, HashSet},
    ops::Deref,
};

use log::debug;
use nalgebra::Vector2;

use crate::common::dyn_vec::TypeInfo;

use super::{
    backend::{
        Binding, Buffer, ComputePipeline, GfxComputePipelineCreateInfo, GfxComputePipelineInfo,
        GfxCullMode, GfxFrontFace, GfxImageFormat, GfxImageType,
        GfxRasterPipelineBlendStateAttachmentInfo, GfxRasterPipelineBlendStateCreateInfo,
        GfxRasterPipelineCreateInfo, GfxVertexAttribute, GfxVertexFormat, GraphicsBackendDevice,
        GraphicsBackendRecorder, Image, RasterPipeline, ResourceId, UniformSetData, Untyped,
    },
    shader::{ShaderBinding, ShaderBindingType, ShaderPath, ShaderSetBinding},
};

pub struct Baked;
pub struct Unbaked;
pub struct Pass;
pub struct FrameGraphBuilder {
    resource_infos: Vec<FrameGraphResourceInfo>,
    resource_name_map: HashMap<String, FrameGraphResource<Untyped>>,
    inputs: HashMap<FrameGraphResource<Untyped>, TypeInfo>,
    passes: HashMap<FrameGraphResource<Untyped>, FrameGraphPass>,
    pass_order: Vec<FrameGraphResource<Untyped>>,
    frame_image_infos: HashMap<FrameGraphResource<Image>, FrameGraphImageInfo>,
    // Frame images that are created lazily right when they are required, evaluated by user-defined
    // function with gpu frame context.
    frame_image_infos_delayed:
        HashMap<FrameGraphResource<Image>, Box<dyn Fn(&FrameGraphContext) -> FrameGraphImageInfo>>,
    frame_buffers: HashSet<FrameGraphResource<Buffer>>,
    compute_pipelines: HashMap<FrameGraphResource<ComputePipeline>, FrameGraphComputePipelineInfo>,
    // We can't store the graphics raster pipeline info itself since we can't resolve all our
    // image formats until we know them during execution for image inputs and delayed frame images.
    raster_pipelines: HashMap<FrameGraphResource<RasterPipeline>, FrameGraphRasterPipelineInfo>,
    swapchain_image: Option<FrameGraphResource<Image>>,
}

#[derive(Clone)]
pub struct FrameGraphResourceInfo {
    pub id: u32,
    pub name: String,
    pub type_id: std::any::TypeId,
}

pub struct FrameGraphPass {
    pub id: FrameGraphResource<Pass>,
    pub inputs: HashSet<FrameGraphResource<Untyped>>,
    pub outputs: HashSet<FrameGraphResource<Untyped>>,
    pub pass: Option<Box<dyn Fn(&mut dyn GraphicsBackendRecorder, &FrameGraphContext)>>,
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct FrameGraphImageInfo {
    pub image_type: GfxImageType,
    pub format: GfxImageFormat,
    pub extent: Vector2<u32>,
}

#[derive(PartialEq, Eq, Hash, Debug)]
pub struct FrameGraphBufferInfo {
    pub size: u64,
}

pub struct FrameGraphComputePipelineInfo {
    pub shader_path: ShaderPath,
    pub entry_point_fn: String,
}

pub struct FrameGraphRasterPipelineInfo {
    pub vertex_shader_path: ShaderPath,
    pub vertex_entry_point_fn: String,

    pub fragment_shader_path: ShaderPath,
    pub fragment_entry_point_fn: String,

    pub vertex_format: GfxVertexFormat,
    pub cull_mode: GfxCullMode,
    pub front_face: GfxFrontFace,
    pub blend_state: GfxRasterPipelineBlendStateCreateInfo,

    pub color_attachments: Vec<FrameGraphResource<Image>>,
    pub depth_attachment: Option<FrameGraphResource<Image>>,
}

impl FrameGraphImageInfo {
    pub fn new_r16float(extent: Vector2<u32>) -> Self {
        Self {
            image_type: GfxImageType::D2,
            format: GfxImageFormat::R16Float,
            extent,
        }
    }

    pub fn new_rgba32float(extent: Vector2<u32>) -> Self {
        Self {
            image_type: GfxImageType::D2,
            format: GfxImageFormat::Rgba32Float,
            extent,
        }
    }

    pub fn new_rgba8(extent: Vector2<u32>) -> Self {
        Self {
            image_type: GfxImageType::D2,
            format: GfxImageFormat::Rgba8Unorm,
            extent,
        }
    }

    pub fn new_depth(extent: Vector2<u32>) -> Self {
        Self {
            image_type: GfxImageType::DepthD2,
            format: GfxImageFormat::D16Unorm,
            extent,
        }
    }
}

impl FrameGraphBuilder {
    pub fn new() -> Self {
        Self {
            resource_infos: Vec::new(),
            resource_name_map: HashMap::new(),
            inputs: HashMap::new(),
            passes: HashMap::new(),
            pass_order: Vec::new(),
            frame_image_infos: HashMap::new(),
            frame_image_infos_delayed: HashMap::new(),
            frame_buffers: HashSet::new(),
            compute_pipelines: HashMap::new(),
            raster_pipelines: HashMap::new(),
            swapchain_image: None,
        }
    }

    pub fn next_id<T: 'static>(&mut self, name: String) -> FrameGraphResource<T> {
        let id = self.resource_infos.len() as u32;
        self.resource_infos.push(FrameGraphResourceInfo {
            id,
            name: name.clone(),
            type_id: std::any::TypeId::of::<T>(),
        });

        let resource = FrameGraphResource {
            id,
            _marker: std::marker::PhantomData,
        };
        self.resource_name_map.insert(name, resource.as_untyped());

        resource
    }

    pub fn create_input_pass(
        &mut self,
        name: impl ToString,
        inputs: &[&dyn IntoFrameGraphResourceUntyped],
        outputs: &[&dyn IntoFrameGraphResourceUntyped],
    ) -> FrameGraphResource<Pass> {
        let resource_handle = self.next_id(name.to_string());
        debug!("Creating input pass with name `{}`.", name.to_string());
        self.passes.insert(
            resource_handle.as_untyped(),
            FrameGraphPass {
                id: resource_handle,
                inputs: inputs
                    .into_iter()
                    .map(|resource| resource.handle_untyped(self))
                    .collect(),
                outputs: outputs
                    .into_iter()
                    .map(|resource| resource.handle_untyped(self))
                    .collect(),
                pass: None,
            },
        );
        self.pass_order.push(resource_handle.as_untyped());

        resource_handle
    }

    pub fn create_pass<F>(
        &mut self,
        name: impl ToString,
        inputs: &[&dyn IntoFrameGraphResourceUntyped],
        outputs: &[&dyn IntoFrameGraphResourceUntyped],
        pass: F,
    ) -> FrameGraphResource<Pass>
    where
        F: Fn(&mut dyn GraphicsBackendRecorder, &FrameGraphContext) + 'static,
    {
        let resource_handle = self.next_id(name.to_string());
        debug!("Creating pass with name `{}`.", name.to_string());
        self.passes.insert(
            resource_handle.as_untyped(),
            FrameGraphPass {
                id: resource_handle,
                inputs: inputs
                    .into_iter()
                    .map(|resource| resource.handle_untyped(self))
                    .collect(),
                outputs: outputs
                    .into_iter()
                    .map(|resource| resource.handle_untyped(self))
                    .collect(),
                pass: Some(Box::new(pass)),
            },
        );
        self.pass_order.push(resource_handle.as_untyped());

        resource_handle
    }

    pub fn present_image(&mut self, image: FrameGraphResource<Image>) {
        assert!(
            self.swapchain_image.is_none(),
            "`present_image` has already been called before when building this frame graph."
        );
        self.swapchain_image = Some(image);
    }

    /// Frame buffers are automatically sized on write and cached for re-use.
    pub fn create_frame_buffer(&mut self, name: &str) -> FrameGraphResource<Buffer> {
        let resource = self.next_id(name.to_string());
        self.frame_buffers.insert(resource);
        resource
    }

    pub fn create_frame_image(
        &mut self,
        name: &str,
        create_info: FrameGraphImageInfo,
    ) -> FrameGraphResource<Image> {
        let resource = self.next_id(name.to_string());
        self.frame_image_infos.insert(resource, create_info);
        resource
    }

    pub fn create_frame_image_with_ctx(
        &mut self,
        name: &str,
        create_fn: impl Fn(&FrameGraphContext) -> FrameGraphImageInfo + 'static,
    ) -> FrameGraphResource<Image> {
        let resource = self.next_id(name.to_string());
        self.frame_image_infos_delayed
            .insert(resource, Box::new(create_fn));
        resource
    }

    pub fn create_compute_pipeline(
        &mut self,
        name: &str,
        create_info: FrameGraphComputeInfo<'_>,
    ) -> FrameGraphResource<ComputePipeline> {
        let resource_handle = self.next_id(name.to_string());
        let create_info = FrameGraphComputePipelineInfo {
            shader_path: ShaderPath::new(create_info.shader_path.to_owned()).expect(&format!(
                "Invalid shader path `{}`.",
                create_info.shader_path
            )),
            entry_point_fn: create_info.entry_point_fn.to_owned(),
        };
        self.compute_pipelines.insert(resource_handle, create_info);
        resource_handle
    }

    pub fn create_raster_pipeline(
        &mut self,
        name: &str,
        create_info: FrameGraphRasterInfo<'_>,
        color_attachments: &[&dyn IntoFrameGraphResourceUntyped],
        depth_attachment: Option<&dyn IntoFrameGraphResourceUntyped>,
    ) -> FrameGraphResource<RasterPipeline> {
        let resource_handle = self.next_id(name.to_string());

        let color_attachment_handles = color_attachments
            .into_iter()
            .map(|id| {
                let handle = id.handle_untyped(self);
                let info = &self.resource_infos[handle.id as usize];
                assert_eq!(
                    info.type_id,
                    std::any::TypeId::of::<Image>(),
                    "Expected color attachment input `{}`, to be an image type.",
                    info.name
                );

                handle.as_typed::<Image>()
            })
            .collect::<Vec<_>>();
        let depth_attachment_handle = depth_attachment.map(|id| {
            let handle = id.handle_untyped(self);
            let info = &self.resource_infos[handle.id as usize];
            assert_eq!(
                info.type_id,
                std::any::TypeId::of::<Image>(),
                "Expected depth attachment input `{}`, to be an image type.",
                info.name
            );

            handle.as_typed::<Image>()
        });
        let create_info = FrameGraphRasterPipelineInfo {
            vertex_shader_path: ShaderPath::new(create_info.vertex_shader_path.to_owned()).expect(
                &format!(
                    "Invalid vertex shader path `{}`.",
                    create_info.vertex_shader_path
                ),
            ),
            vertex_entry_point_fn: create_info.vertex_entry_point_fn.to_owned(),
            fragment_shader_path: ShaderPath::new(create_info.fragment_shader_path.to_owned())
                .expect(&format!(
                    "Invalid fragment shader path `{}`.",
                    create_info.vertex_shader_path
                )),
            fragment_entry_point_fn: create_info.fragment_entry_point_fn.to_owned(),
            vertex_format: create_info.vertex_format.into(),
            cull_mode: create_info.cull_mode,
            front_face: create_info.front_face,
            blend_state: create_info.blend_state.into(),
            color_attachments: color_attachment_handles,
            depth_attachment: depth_attachment_handle,
        };
        self.raster_pipelines.insert(resource_handle, create_info);
        resource_handle
    }

    pub fn create_input_image(&mut self, name: impl ToString) -> FrameGraphResource<Image> {
        self.create_input::<Image>(name)
    }

    pub fn create_input_buffer(&mut self, name: impl ToString) -> FrameGraphResource<Buffer> {
        self.create_input::<Buffer>(name)
    }

    pub fn create_input<T: 'static>(&mut self, name: impl ToString) -> FrameGraphResource<T> {
        let resource_handle = self.next_id(name.to_string());
        self.inputs
            .insert(resource_handle.as_untyped(), TypeInfo::new::<T>());

        resource_handle
    }

    pub fn bake(mut self) -> anyhow::Result<FrameGraph> {
        FrameGraph::bake(self)
    }
}

pub struct FrameGraph {
    pub resource_infos: Vec<FrameGraphResourceInfo>,
    pub resource_name_map: HashMap<String, FrameGraphResourceInfo>,
    pub inputs: HashMap<FrameGraphResource<Untyped>, TypeInfo>,
    pub passes: Vec<FrameGraphPass>,

    pub compute_pipelines:
        HashMap<FrameGraphResource<ComputePipeline>, FrameGraphComputePipelineInfo>,
    pub raster_pipelines: HashMap<FrameGraphResource<RasterPipeline>, FrameGraphRasterPipelineInfo>,

    pub frame_image_infos: HashMap<FrameGraphResource<Image>, FrameGraphImageInfo>,
    pub frame_image_infos_delayed:
        HashMap<FrameGraphResource<Image>, Box<dyn Fn(&FrameGraphContext) -> FrameGraphImageInfo>>,

    pub frame_buffers: HashSet<FrameGraphResource<Buffer>>,

    pub swapchain_image: FrameGraphResource<Image>,
}

impl FrameGraph {
    pub fn builder() -> FrameGraphBuilder {
        FrameGraphBuilder::new()
    }

    fn bake(mut builder: FrameGraphBuilder) -> anyhow::Result<Self> {
        let Some(swapchain_image) = builder.swapchain_image else {
            anyhow::bail!("Swapchain image was not presented or specified.");
        };

        let mut required_resources = HashSet::new();
        required_resources.insert(swapchain_image.as_untyped());

        let mut used_passes = vec![];
        for pass_handle in builder.pass_order.iter().rev() {
            let mut pass_info = builder.passes.remove(&pass_handle).unwrap();
            if !pass_info
                .outputs
                .iter()
                .find(|output| required_resources.contains(*output))
                .is_some()
            {
                // TODO: Figure out the buffer situation.
                // continue;
            }
            // Additional implicit inputs due to any raster pipeline inputs.
            let mut raster_image_inputs = Vec::new();
            for pass_input in &pass_info.inputs {
                if let Some(raster_info) = builder
                    .raster_pipelines
                    .get(&pass_input.as_typed::<RasterPipeline>())
                {
                    for input_image in &raster_info.color_attachments {
                        raster_image_inputs.push(*input_image);
                    }
                }
                required_resources.insert(*pass_input);
            }
            for image_input in raster_image_inputs {
                pass_info.inputs.insert(image_input.as_untyped());
            }

            debug!(
                "Using pass `{}`.",
                builder.resource_infos[pass_info.id.id as usize].name
            );
            used_passes.push(pass_info);
        }

        used_passes.reverse();

        // TODO: Warn about unreachable resources in the pass graph.

        let resource_name_map = builder
            .resource_name_map
            .into_iter()
            .map(|(name, id)| (name, builder.resource_infos[id.id() as usize].clone()))
            .collect();

        Ok(Self {
            resource_infos: builder.resource_infos,
            resource_name_map,
            inputs: builder.inputs,
            passes: used_passes,
            compute_pipelines: builder.compute_pipelines,
            raster_pipelines: builder.raster_pipelines,
            frame_buffers: builder.frame_buffers,
            frame_image_infos: builder.frame_image_infos,
            frame_image_infos_delayed: builder.frame_image_infos_delayed,
            swapchain_image,
        })
    }
}

impl FrameGraphContextImpl for FrameGraph {
    fn get_handle_untyped(&self, name: &str) -> FrameGraphResource<Untyped> {
        let Some(resource_info) = self.resource_name_map.get(name) else {
            panic!(
                "Resource `{}` does not exist in the frame graph definition.",
                name
            );
        };
        FrameGraphResource::new(resource_info.id)
    }

    fn get_handle(
        &self,
        name: &str,
        expected_type: std::any::TypeId,
    ) -> FrameGraphResource<Untyped> {
        let Some(resource_info) = self.resource_name_map.get(name) else {
            panic!(
                "Resource `{}` does not exist in the frame graph definition.",
                name
            );
        };
        assert_eq!(resource_info.type_id, expected_type);
        FrameGraphResource::new(resource_info.id)
    }
}

pub struct FrameGraphResource<T> {
    id: u32,
    _marker: std::marker::PhantomData<T>,
}

impl<T> std::fmt::Debug for FrameGraphResource<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "Reource id: {}, type_id: {}",
            self.id,
            std::any::type_name::<T>()
        ))
    }
}

impl<T> FrameGraphResource<T> {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn as_typed<S>(&self) -> FrameGraphResource<S> {
        FrameGraphResource {
            id: self.id,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn as_untyped(&self) -> FrameGraphResource<Untyped> {
        FrameGraphResource {
            id: self.id,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }
}

impl<T> std::hash::Hash for FrameGraphResource<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> Eq for FrameGraphResource<T> {}

impl<T> PartialEq for FrameGraphResource<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Copy for FrameGraphResource<T> {}

impl<T> Clone for FrameGraphResource<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> IntoFrameGraphResourceUntyped for FrameGraphResource<T> {
    fn handle_untyped(&self, _ctx: &dyn FrameGraphContextImpl) -> FrameGraphResource<Untyped> {
        FrameGraphResource {
            id: self.id,
            _marker: std::marker::PhantomData,
        }
    }
}

pub trait IntoFrameGraphResource<T> {
    fn handle(self, ctx: &impl FrameGraphContextImpl) -> FrameGraphResource<T>;
}

pub trait IntoFrameGraphResourceUntyped {
    fn handle_untyped(&self, ctx: &dyn FrameGraphContextImpl) -> FrameGraphResource<Untyped>;
}

impl IntoFrameGraphResourceUntyped for &'static str {
    fn handle_untyped(&self, ctx: &dyn FrameGraphContextImpl) -> FrameGraphResource<Untyped> {
        ctx.get_handle_untyped(self)
    }
}

impl<T: 'static, S> IntoFrameGraphResource<T> for S
where
    S: AsRef<str>,
{
    fn handle(self, ctx: &impl FrameGraphContextImpl) -> FrameGraphResource<T> {
        ctx.get_handle(self.as_ref(), std::any::TypeId::of::<T>())
            .as_typed()
    }
}

impl<T> IntoFrameGraphResource<T> for FrameGraphResource<T> {
    fn handle(self, _ctx: &impl FrameGraphContextImpl) -> FrameGraphResource<T> {
        self
    }
}

impl<T> IntoFrameGraphResource<T> for &FrameGraphResource<T> {
    fn handle(self, _ctx: &impl FrameGraphContextImpl) -> FrameGraphResource<T> {
        self.clone()
    }
}

pub trait FrameGraphContextImpl {
    fn get_handle_untyped(&self, name: &str) -> FrameGraphResource<Untyped>;

    fn get_handle(
        &self,
        name: &str,
        expected_type: std::any::TypeId,
    ) -> FrameGraphResource<Untyped>;
}

impl FrameGraphContextImpl for FrameGraphBuilder {
    fn get_handle_untyped(&self, name: &str) -> FrameGraphResource<Untyped> {
        *self.resource_name_map.get(name).expect(&format!(
            "Resource of name `{}` has not been inserted into the frame graph builder yet.",
            name
        ))
    }

    fn get_handle(
        &self,
        name: &str,
        expected_type: std::any::TypeId,
    ) -> FrameGraphResource<Untyped> {
        let handle = self.get_handle_untyped(name);
        assert_eq!(
            self.resource_infos[handle.id() as usize].type_id(),
            expected_type
        );
        handle
    }
}

pub struct FGResourceBackendId {
    pub resource_id: ResourceId<Untyped>,
    pub expected_type: std::any::TypeId,
}

pub struct FrameGraphContext<'a> {
    pub frame_graph: &'a FrameGraph,
    pub resource_map: &'a HashMap<FrameGraphResource<Untyped>, FGResourceBackendId>,
    pub supplied_inputs: &'a HashMap<FrameGraphResource<Untyped>, Box<dyn std::any::Any>>,
}

impl<'a> FrameGraphContextImpl for FrameGraphContext<'a> {
    fn get_handle_untyped(&self, name: &str) -> FrameGraphResource<Untyped> {
        self.frame_graph.get_handle_untyped(name)
    }

    fn get_handle(
        &self,
        name: &str,
        expected_type: std::any::TypeId,
    ) -> FrameGraphResource<Untyped> {
        self.frame_graph.get_handle(name, expected_type)
    }
}

impl<'a> FrameGraphContext<'a> {
    fn get_resource_id<T: 'static>(
        &self,
        resource: impl IntoFrameGraphResource<T>,
    ) -> ResourceId<T> {
        let fg_resource = resource.handle(self.frame_graph);
        let Some(resource) = self.resource_map.get(&fg_resource.as_untyped()) else {
            panic!(
                "Frame graph resource `{}` has not been supplied to the executor, or has not been defined as an input to this frame pass.",
                self.frame_graph
                    .resource_infos
                    .get(fg_resource.id as usize)
                    .unwrap()
                    .name
            );
        };
        assert_eq!(resource.expected_type, std::any::TypeId::of::<T>());
        ResourceId::new(resource.resource_id.id())
    }

    pub fn get_image(&self, resource: impl IntoFrameGraphResource<Image>) -> ResourceId<Image> {
        self.get_resource_id(resource)
    }

    pub fn get_buffer(&self, resource: impl IntoFrameGraphResource<Buffer>) -> ResourceId<Buffer> {
        self.get_resource_id(resource)
    }

    pub fn get_compute_pipeline(
        &self,
        resource: impl IntoFrameGraphResource<ComputePipeline>,
    ) -> ResourceId<ComputePipeline> {
        self.get_resource_id(resource)
    }

    pub fn get_raster_pipeline(
        &self,
        resource: impl IntoFrameGraphResource<RasterPipeline>,
    ) -> ResourceId<RasterPipeline> {
        self.get_resource_id(resource)
    }

    pub fn get_vec2<T: Clone + 'static>(
        &self,
        resource: impl IntoFrameGraphResource<Vector2<T>>,
    ) -> Vector2<T> {
        let handle = resource.handle(self.frame_graph);
        let val = self
            .supplied_inputs
            .get(&handle.as_untyped())
            .expect("Input hasn't been supplied.");
        val.downcast_ref::<Vector2<T>>()
            .expect(&format!(
                "Expected input {} to be vec2 but it wasn't.",
                self.frame_graph.resource_infos[handle.id() as usize].name,
            ))
            .clone()
    }
}

pub struct FrameGraphComputeInfo<'a> {
    /// Shader path starting from the assets/shader directory, with the separator being :: and
    /// excluding the file extension since Slang is expected.
    /// So a valid path would be `pass::blit`.
    pub shader_path: &'a str,
    pub entry_point_fn: &'a str,
}

pub struct FrameGraphRasterInfo<'a> {
    pub vertex_shader_path: &'a str,
    pub vertex_entry_point_fn: &'a str,
    pub fragment_shader_path: &'a str,
    pub fragment_entry_point_fn: &'a str,
    pub blend_state: FrameGraphRasterBlendInfo<'a>,
    pub vertex_format: FrameGraphVertexFormat<'a>,
    pub cull_mode: GfxCullMode,
    pub front_face: GfxFrontFace,
}

pub struct FrameGraphVertexFormat<'a> {
    pub attributes: &'a [GfxVertexAttribute],
}

impl From<FrameGraphVertexFormat<'_>> for GfxVertexFormat {
    fn from(value: FrameGraphVertexFormat<'_>) -> Self {
        GfxVertexFormat {
            attributes: value.attributes.to_vec(),
        }
    }
}

pub struct FrameGraphRasterBlendInfo<'a> {
    pub attachments: &'a [GfxRasterPipelineBlendStateAttachmentInfo],
}

impl From<FrameGraphRasterBlendInfo<'_>> for GfxRasterPipelineBlendStateCreateInfo {
    fn from(value: FrameGraphRasterBlendInfo<'_>) -> Self {
        GfxRasterPipelineBlendStateCreateInfo {
            attachments: value.attachments.to_vec(),
        }
    }
}
