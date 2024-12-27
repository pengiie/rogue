use core::panic;
use std::collections::{HashMap, HashSet};

use nalgebra::Vector2;

use crate::common::dyn_vec::TypeInfo;

use super::backend::{
    Buffer, ComputePipeline, GraphicsBackendRecorder, Image, ResourceId, Untyped,
};

pub struct Baked;
pub struct Unbaked;
pub struct Pass;
pub struct FrameGraphBuilder {
    resource_infos: Vec<FrameGraphResourceInfo>,
    inputs: HashMap<FrameGraphResource<Untyped>, TypeInfo>,
    passes: HashMap<FrameGraphResource<Untyped>, FrameGraphPass>,
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

impl FrameGraphBuilder {
    pub fn new() -> Self {
        Self {
            resource_infos: Vec::new(),
            inputs: HashMap::new(),
            passes: HashMap::new(),
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

    pub fn create_frame_image(&mut self, name: &str) -> FrameGraphResource<Buffer> {
        todo!()
    }

    pub fn create_compute_pipeline(
        &mut self,
        name: &str,
        create_info: FGComputeInfo<'_>,
    ) -> FrameGraphResource<ComputePipeline> {
        todo!()
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
                "Frame graph resource `{}` has not been supplied to the executor.",
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

pub struct FGComputeInfo<'a> {
    /// Shader path starting from the assets/shader directory, with the separator being :: and
    /// excluding the file extension since Slang is expected.
    /// So a valid path would be `pass::blit`.
    pub shader_path: &'a str,
    pub entry_point_fn: &'a str,
}

// TODO: Isn't a graph much right now, but that's ok since we are not using Vulkan right now.
// pub struct FrameGraph {}
//
// pub struct ComputePipeline;
// pub struct Image;
// pub struct Buffer;
// pub struct FrameResource<T> {
//     _marker: std::marker::PhantomData<T>,
// }
//
// pub struct FrameResourceRef<T> {
//     _marker: std::marker::PhantomData<T>,
// }
//
// impl<T> FrameResource<T> {
//     pub fn new() -> Self {
//         Self {
//             _marker: std::marker::PhantomData,
//         }
//     }
// }
//
// impl FrameResource<ComputePipeline> {}
//
// impl FrameGraph {
//     fn new() -> Self {
//         Self {}
//     }
//
//     pub fn run_tasks(&mut self) {}
//
//     pub fn cached_builder(self) -> FrameGraphBuilder {
//         FrameGraphBuilder::new(Some(self))
//     }
//
//     pub fn builder() -> FrameGraphBuilder {
//         FrameGraphBuilder::new(None)
//     }
// }
//
// pub struct FrameContext {}
//
// pub struct WgpuFrameRecorderImpl<'a> {
//     encoder: wgpu::CommandEncoder,
//
//     last_compute_pass: Option<wgpu::ComputePass<'a>>,
// }
//
// impl<'a> FrameRecorderImpl for WgpuFrameRecorderImpl<'a> {
//     fn begin_compute_pass(&mut self) {
//         self.last_compute_pass = Some(self.encoder.begin_compute_pass(
//             &wgpu::ComputePassDescriptor {
//                 label: Some("compute_pass"),
//                 timestamp_writes: todo!(),
//             },
//         ));
//     }
//
//     fn end_compute_pass(&mut self) {
//         let _dropped = self.last_compute_pass.take();
//     }
//
//     fn set_compute_pipeline(&mut self, compute_pipeline: &wgpu::ComputePipeline) {
//         let Some(compute_pass) = &mut self.last_compute_pass else {
//             panic!("Dispatch must be called from within a compute pass");
//         };
//         compute_pass.set_pipeline(compute_pipeline);
//     }
//
//     fn dispatch(&mut self, x: u32, y: u32, z: u32) {
//         let Some(compute_pass) = &mut self.last_compute_pass else {
//             panic!("Dispatch must be called from within a compute pass");
//         };
//         compute_pass.dispatch_workgroups(x, y, z);
//     }
// }
//
// trait FrameRecorderImpl {
//     fn begin_compute_pass(&mut self);
//     fn end_compute_pass(&mut self);
//     fn set_compute_pipeline(&mut self, compute_pipeline: &wgpu::ComputePipeline);
//     fn dispatch(&mut self, x: u32, y: u32, z: u32);
// }
//
// pub struct FrameRecorder {
//     recorder: Box<dyn FrameRecorderImpl>,
// }
//
// impl FrameRecorder {
//     pub fn begin_compute_pass(&self) {}
//     pub fn begin_render_pass(&self) {}
//     pub fn blit(&self) {}
// }
//
// pub struct RecordComputePass<'a> {
//     recorder: &'a mut FrameRecorder,
// }
//
// impl RecordComputePass<'_> {
//     pub fn dispatch(&mut self, x: u32, y: u32, z: u32) {
//         self.recorder.recorder.dispatch(x, y, z);
//     }
// }
//
// impl Drop for RecordComputePass<'_> {
//     fn drop(&mut self) {
//         self.recorder.recorder.end_compute_pass();
//     }
// }
//
// pub struct FrameComputePipelineInfo {}
//
// impl FrameComputePipelineInfo {
//     pub fn input(binding: u32, )
// }
//
// pub struct FrameGraphBuilder {
//     cached_graph: Option<FrameGraph>,
// }
//
// impl FrameGraphBuilder {
//     fn new(cached_graph: Option<FrameGraph>) -> Self {
//         Self { cached_graph }
//     }
//
//     pub fn create_image(&mut self, name: &str) -> FrameResource<Image> {}
//     pub fn create_texture(&mut self, name: &str) -> FrameResourceRef<Image> {}
//     pub fn create_buffer(&mut self, name: &str) {}
//     pub fn create_typed_buffer<T>(&mut self, name: &str) {}
//     pub fn create_buffer_ref(&mut self, name: &str) -> FrameResourceRef<Buffer> {}
//
//     pub fn create_raster_pipeline(&mut self, name: &str) {}
//     pub fn create_compute_pipeline(&mut self, name: &str, info: &FrameComputePipelineInfo) {}
//
//     pub fn add_task<F>(&mut self, task_fn: impl Into<FrameTask>) {}
//
//     pub fn build(self) -> FrameGraph {
//         if let Some(cached_graph) = self.cached_graph {
//             return cached_graph;
//         } else {
//             let new_graph = FrameGraph::new();
//             return new_graph;
//         }
//     }
// }
//
// impl<F1, F2> Into<FrameTask> for (F1, F2)
// where
//     F1: FnOnce() -> Vec<FrameTaskDependency>,
//     F2: FnMut(&mut FrameRecorder, &FrameContext) + 'static,
// {
//     fn into(self) -> FrameTask {
//         FrameTask {
//             dependencies: self.0(),
//             boxed_run: Box::new(self.1),
//         }
//     }
// }
//
// pub struct FrameTask {
//     pub dependencies: Vec<FrameTaskDependency>,
//     pub boxed_run: Box<dyn FnMut(&mut FrameRecorder, &FrameContext)>,
// }
//
// pub struct FrameTaskDependency {}
