use core::panic;
use std::collections::{HashMap, HashSet};

use nalgebra::Vector2;

use crate::common::dyn_vec::TypeInfo;

use super::{
    backend::{
        Buffer, ComputePipeline, GfxComputePipelineCreateInfo, GfxComputePipelineInfo,
        GfxImageType, GraphicsBackendDevice, GraphicsBackendRecorder, Image, ImageFormat,
        ResourceId, Untyped,
    },
    shader::ShaderPath,
};

pub struct Baked;
pub struct Unbaked;
pub struct Pass;
pub struct FrameGraphBuilder {
    resource_infos: Vec<FrameGraphResourceInfo>,
    inputs: HashMap<FrameGraphResource<Untyped>, TypeInfo>,
    passes: HashMap<FrameGraphResource<Untyped>, FrameGraphPass>,
    frame_image_infos: HashMap<FrameGraphResource<Image>, FrameGraphImageInfo>,
    compute_pipelines: HashMap<FrameGraphResource<ComputePipeline>, GfxComputePipelineCreateInfo>,
    swapchain_image: Option<FrameGraphResource<Image>>,
}

#[derive(Clone)]
pub struct FrameGraphResourceInfo {
    pub id: u32,
    pub name: String,
    pub type_id: std::any::TypeId,
}

pub struct FrameGraphPass {
    pub inputs: Vec<FrameGraphResource<Untyped>>,
    pub outputs: Vec<FrameGraphResource<Untyped>>,
    pub pass: Box<dyn Fn(&mut dyn GraphicsBackendRecorder, &FrameGraphContext)>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct FrameGraphImageInfo {
    pub image_type: GfxImageType,
    pub format: ImageFormat,
    pub extent: Vector2<u32>,
}

impl FrameGraphImageInfo {
    pub fn new_rgba32float(extent: Vector2<u32>) -> Self {
        Self {
            image_type: GfxImageType::D2,
            format: ImageFormat::Rgba32Float,
            extent,
        }
    }
    pub fn new_depth(extent: Vector2<u32>) -> Self {
        Self {
            image_type: GfxImageType::DepthD2,
            format: ImageFormat::D16Unorm,
            extent,
        }
    }
}

impl FrameGraphBuilder {
    pub fn new() -> Self {
        Self {
            resource_infos: Vec::new(),
            inputs: HashMap::new(),
            passes: HashMap::new(),
            frame_image_infos: HashMap::new(),
            compute_pipelines: HashMap::new(),
            swapchain_image: None,
        }
    }

    pub fn next_id<T: 'static>(&mut self, name: String) -> FrameGraphResource<T> {
        let id = self.resource_infos.len() as u32;
        self.resource_infos.push(FrameGraphResourceInfo {
            id,
            name,
            type_id: std::any::TypeId::of::<T>(),
        });

        FrameGraphResource {
            id,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn create_pass<F>(
        &mut self,
        name: impl ToString,
        inputs: &[&dyn FrameGraphResourceImpl],
        outputs: &[&dyn FrameGraphResourceImpl],
        pass: F,
    ) -> FrameGraphResource<Pass>
    where
        F: Fn(&mut dyn GraphicsBackendRecorder, &FrameGraphContext) + 'static,
    {
        let resource_handle = self.next_id(name.to_string());
        self.passes.insert(
            resource_handle.as_untyped(),
            FrameGraphPass {
                inputs: inputs
                    .into_iter()
                    .map(|resource| resource.as_untyped())
                    .collect(),
                outputs: outputs
                    .into_iter()
                    .map(|resource| resource.as_untyped())
                    .collect(),
                pass: Box::new(pass),
            },
        );

        resource_handle
    }

    pub fn create_pass_ref(
        &mut self,
        name: impl ToString,
        inputs: &[&dyn FrameGraphResourceImpl],
        outputs: &[&dyn FrameGraphResourceImpl],
    ) -> FrameGraphResource<Pass> {
        let resource_handle = self.next_id(name.to_string());
        todo!("register");

        resource_handle
    }

    pub fn present_image(&mut self, image: FrameGraphResource<Image>) {
        assert!(
            self.swapchain_image.is_none(),
            "`present_image` has already been called before when building this frame graph."
        );
        self.swapchain_image = Some(image);
    }

    pub fn create_frame_buffer(&mut self, name: &str) -> FrameGraphResource<Buffer> {
        todo!()
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

    pub fn create_compute_pipeline(
        &mut self,
        name: &str,
        create_info: FrameGraphComputeInfo<'_>,
    ) -> FrameGraphResource<ComputePipeline> {
        let resource_handle = self.next_id(name.to_string());
        let create_info = GfxComputePipelineCreateInfo {
            shader_path: ShaderPath::new(create_info.shader_path.to_owned()).expect(&format!(
                "Invalid shader path `{}`.",
                create_info.shader_path
            )),
            entry_point_fn: create_info.entry_point_fn.to_owned(),
        };
        self.compute_pipelines.insert(resource_handle, create_info);
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
        HashMap<FrameGraphResource<ComputePipeline>, GfxComputePipelineCreateInfo>,
    pub frame_image_infos: HashMap<FrameGraphResource<Image>, FrameGraphImageInfo>,

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

        let resource_name_map = builder
            .resource_infos
            .iter()
            .map(|info| (info.name.clone(), info.clone()))
            .collect::<HashMap<_, _>>();

        let mut required_resources = HashSet::new();
        required_resources.insert(swapchain_image.as_untyped());

        let mut used_passes = vec![];
        for (pass_handle, pass_info) in builder.passes {
            if pass_info
                .outputs
                .iter()
                .find(|output| required_resources.contains(*output))
                .is_some()
            {
                for pass_input in &pass_info.inputs {
                    required_resources.insert(*pass_input);
                }
                used_passes.push(pass_info);
            }
        }

        used_passes.reverse();

        // TODO: Warn about unreachable resources in the pass graph.

        Ok(Self {
            resource_infos: builder.resource_infos,
            resource_name_map,
            inputs: builder.inputs,
            passes: used_passes,
            compute_pipelines: builder.compute_pipelines,
            frame_image_infos: builder.frame_image_infos,
            swapchain_image,
        })
    }
}

impl FrameGraphContextImpl for FrameGraph {
    fn get_handle<T: 'static>(&self, name: impl AsRef<str>) -> FrameGraphResource<T> {
        let Some(resource_info) = self.resource_name_map.get(name.as_ref()) else {
            panic!("Resource does not exist in frame graph.")
        };
        assert_eq!(resource_info.type_id, std::any::TypeId::of::<T>());
        FrameGraphResource::new(resource_info.id)
    }
}

#[derive(Debug)]
pub struct FrameGraphResource<T> {
    id: u32,
    _marker: std::marker::PhantomData<T>,
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

pub trait FrameGraphResourceImpl {
    fn as_untyped(&self) -> FrameGraphResource<Untyped>;
}

impl<T> FrameGraphResourceImpl for FrameGraphResource<T> {
    fn as_untyped(&self) -> FrameGraphResource<Untyped> {
        FrameGraphResource {
            id: self.id,
            _marker: std::marker::PhantomData,
        }
    }
}

pub trait IntoFrameGraphResource<T> {
    fn handle(self, ctx: &impl FrameGraphContextImpl) -> FrameGraphResource<T>;
}

impl<T: 'static, S> IntoFrameGraphResource<T> for S
where
    S: AsRef<str>,
{
    fn handle(self, ctx: &impl FrameGraphContextImpl) -> FrameGraphResource<T> {
        ctx.get_handle::<T>(self)
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
    fn get_handle<T: 'static>(&self, name: impl AsRef<str>) -> FrameGraphResource<T>;
}

pub struct FGResourceBackendId {
    pub resource_id: ResourceId<Untyped>,
    pub expected_type: std::any::TypeId,
}

pub struct FrameGraphContext<'a> {
    pub frame_graph: &'a FrameGraph,
    pub resource_map: &'a HashMap<FrameGraphResource<Untyped>, FGResourceBackendId>,
}

impl<'a> FrameGraphContextImpl for FrameGraphContext<'a> {
    fn get_handle<T: 'static>(&self, name: impl AsRef<str>) -> FrameGraphResource<T> {
        self.frame_graph.get_handle(name)
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

    pub fn get_compute_pipeline(
        &self,
        resource: impl IntoFrameGraphResource<ComputePipeline>,
    ) -> ResourceId<ComputePipeline> {
        self.get_resource_id(resource)
    }

    pub fn get_vec2<T>(&self, resource: impl IntoFrameGraphResource<Vector2<T>>) -> Vector2<T> {
        todo!()
    }
}

pub struct FrameGraphComputeInfo<'a> {
    /// Shader path starting from the assets/shader directory, with the separator being :: and
    /// excluding the file extension since Slang is expected.
    /// So a valid path would be `pass::blit`.
    pub shader_path: &'a str,
    pub entry_point_fn: &'a str,
}
