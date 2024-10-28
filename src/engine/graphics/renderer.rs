use std::{
    borrow::{Borrow, Cow},
    collections::HashMap,
    num::NonZero,
    ops::{Range, Rem},
};

use bytemuck::Zeroable;
use hecs::{Query, World};
use log::{debug, info};
use nalgebra::{ComplexField, Matrix3, Matrix4};
use rogue_macros::Resource;
use wgpu::{
    CommandEncoderDescriptor, ComputePassDescriptor, PipelineCompilationOptions, ShaderModule,
};

use crate::{
    common::set::{AttributeSet, AttributeSetImpl},
    engine::{
        asset::asset::AssetPath,
        ecs::{
            self,
            ecs_world::{self, ECSWorld},
        },
        graphics::pipeline_manager::PipelineCreateInfo,
        physics::transform::Transform,
        resource::{Res, ResMut},
        ui::{gui::Egui, state::UIState},
        voxel::voxel_world::{VoxelWorld, VoxelWorldGpu},
        window::time::Time,
    },
    game::player::player::Player,
    settings::{GraphicsSettings, GraphicsSettingsAttributes, GraphicsSettingsSet, Settings},
};

use super::{
    camera::Camera,
    device::DeviceResource,
    pipeline_manager::{PipelineId, RenderPipelineManager},
    shader,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Ord, PartialOrd)]
pub enum Antialiasing {
    None,
    TAA,
}

#[derive(bytemuck::Pod, Clone, Copy, Zeroable)]
#[repr(C)]
pub struct CameraBuffer {
    transform: [f32; 16],
    rotation: [f32; 12],
    half_fov: f32,
    // Padding for struct alignment of 16
    padding: [f32; 3],
}

#[derive(bytemuck::Pod, Clone, Copy, Zeroable)]
#[repr(C)]
pub struct WorldBuffer {
    camera: CameraBuffer,
    voxel_model_entity_count: u32,
    // The frame count of the current transform of the camera.
    frame_count: u32,
    // The frame count since the launch of the application.
    total_frame_count: u32,
    // Padding for struct alignment of 16
    padding: [f32; 13],
}

#[derive(bytemuck::Pod, Clone, Copy, Zeroable, Debug)]
#[repr(C)]
pub struct UIBuffer {
    screen_size: [f32; 2],
}

pub struct RenderState {
    draw_grid: bool,
}

const MAX_ESVO_NODES: u32 = 10_000;

#[derive(Resource)]
pub struct Renderer {
    graphics_settings: GraphicsSettingsSet,

    // Elapsed frames since last camera transform update.
    frame_count: u32,
    last_camera_transform: [f32; 16],

    backbuffer: Option<(wgpu::Texture, wgpu::TextureView)>,
    radiance_total: Option<(wgpu::Texture, wgpu::TextureView)>,
    radiance_total_prev: Option<(wgpu::Texture, wgpu::TextureView)>,

    sampler_nearest: wgpu::Sampler,
    sampler_linear: wgpu::Sampler,

    /// Holds the camera data.
    world_info_buffer: wgpu::Buffer,

    ray_bind_group_layout: wgpu::BindGroupLayout,
    ray_bind_group: Option<wgpu::BindGroup>,
    ray_pipeline_id: PipelineId,

    blit_bind_group_layout: wgpu::BindGroupLayout,
    blit_bind_group: Option<wgpu::BindGroup>,
    blit_pipeline: wgpu::RenderPipeline,

    ui_vertex_buffer: wgpu::Buffer,
    ui_index_buffer: wgpu::Buffer,
    ui_uniform_buffer: wgpu::Buffer,
    ui_vertex_buffer_slices: Vec<Range<usize>>,
    ui_index_buffer_slices: Vec<Range<usize>>,
    ui_bind_group_layout: wgpu::BindGroupLayout,
    ui_textures: HashMap<egui::TextureId, (wgpu::Texture, wgpu::BindGroup)>,
    ui_samplers: HashMap<egui::TextureOptions, wgpu::Sampler>,
    ui_pipeline: wgpu::RenderPipeline,
}

impl Renderer {
    pub fn new(device: &DeviceResource, pipeline_manager: &mut RenderPipelineManager) -> Self {
        let ray_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Ray bind group eayout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: Self::backbuffer_format(),
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: Self::radiance_format(),
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 6,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let ray_pipeline_id = pipeline_manager.load_pipeline(
            &device,
            PipelineCreateInfo::Compute {
                name: "ray_pipeline".to_owned(),
                shader_path: AssetPath::new(shader::voxel_trace::PATH.to_string()),
                shader_defines: {
                    let mut h = HashMap::new();
                    h.insert("GRID".to_owned(), true);
                    h
                },
            },
            &[&ray_bind_group_layout],
        );

        let world_info_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world_info_buffer"),
            size: std::mem::size_of::<WorldBuffer>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let blit_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("blit_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let sampler_nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sampler_nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });
        let sampler_linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sampler_linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });
        let blit_shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit_shader_module"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(shader::blit::SOURCE)),
        });
        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit_pipeline_layout"),
            bind_group_layouts: &[&blit_bind_group_layout],
            push_constant_ranges: &[],
        });

        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit_pipeline"),
            layout: Some(&blit_pipeline_layout),
            cache: None,
            vertex: wgpu::VertexState {
                module: &blit_shader_module,
                entry_point: "vs_main",
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader_module,
                entry_point: "fs_main",
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: device.surface_config().format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::all(),
                })],
            }),
            multiview: None,
        });

        let ui_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("ui_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::all(),
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let ui_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui_uniform_buffer"),
            size: std::mem::size_of::<UIBuffer>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        const UI_VERTEX_BUFFER_START_COUNT: u64 = 100;
        let ui_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui_vertex_buffer"),
            size: UI_VERTEX_BUFFER_START_COUNT * std::mem::size_of::<f32>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        const UI_INDEX_BUFFER_START_COUNT: u64 = 100;
        let ui_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui_vertex_buffer"),
            size: UI_INDEX_BUFFER_START_COUNT * std::mem::size_of::<u32>() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ui_shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui_shader_module"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(shader::ui::SOURCE)),
        });
        let ui_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui_pipeline_layout"),
            bind_group_layouts: &[&ui_bind_group_layout],
            push_constant_ranges: &[],
        });
        let ui_vertex_attributes = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 8,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: 16,
                shader_location: 2,
            },
        ];
        let ui_vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: 20,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ui_vertex_attributes,
        };
        let ui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui_pipeline"),
            layout: Some(&ui_pipeline_layout),
            cache: None,
            vertex: wgpu::VertexState {
                module: &ui_shader_module,
                entry_point: "vs_main",
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[ui_vertex_buffer_layout],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &ui_shader_module,
                entry_point: "fs_main",
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: device.surface_config().format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::all(),
                })],
            }),
            multiview: None,
        });

        Self {
            graphics_settings: GraphicsSettingsSet::new(),

            frame_count: 0,
            last_camera_transform: [0.0; 16],

            backbuffer: None,
            radiance_total: None,
            radiance_total_prev: None,
            sampler_nearest,
            sampler_linear,

            world_info_buffer,

            ray_bind_group_layout,
            ray_bind_group: None,
            ray_pipeline_id,

            blit_bind_group_layout,
            blit_bind_group: None,
            blit_pipeline,

            ui_samplers: HashMap::new(),
            ui_bind_group_layout,
            ui_vertex_buffer,
            ui_index_buffer,
            ui_uniform_buffer,
            ui_textures: HashMap::new(),
            ui_pipeline,
            ui_vertex_buffer_slices: Vec::new(),
            ui_index_buffer_slices: Vec::new(),
        }
    }

    fn backbuffer_format() -> wgpu::TextureFormat {
        wgpu::TextureFormat::Rgba8Unorm
    }

    fn radiance_format() -> wgpu::TextureFormat {
        wgpu::TextureFormat::Rgba32Float
    }

    pub fn sample_count(&self) -> u32 {
        self.frame_count
    }

    fn set_backbuffer_textures(&mut self, device: &DeviceResource, width: u32, height: u32) {
        let backbuffer = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("backbuffer"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::backbuffer_format(),
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let backbuffer_view = backbuffer.create_view(&wgpu::TextureViewDescriptor::default());

        let radiance_total = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("radiance_total"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::radiance_format(),
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let radiance_total_view =
            radiance_total.create_view(&wgpu::TextureViewDescriptor::default());

        let radiance_total_prev = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("radiance_total_prev"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::radiance_format(),
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let radiance_total_prev_view =
            radiance_total_prev.create_view(&wgpu::TextureViewDescriptor::default());

        self.backbuffer = Some((backbuffer, backbuffer_view));
        self.radiance_total = Some((radiance_total, radiance_total_view));
        self.radiance_total_prev = Some((radiance_total_prev, radiance_total_prev_view));
    }

    fn set_ray_bind_group(&mut self, device: &DeviceResource, voxel_world_gpu: &VoxelWorldGpu) {
        self.ray_bind_group = Some(
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ray bind group"),
                layout: &self.ray_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &self
                                .backbuffer
                                .as_ref()
                                .expect(
                                    "Shouldn't update ray bind group if backbuffer doesn't exist.",
                                )
                                .1,
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(
                            &self
                                .radiance_total
                                .as_ref()
                                .expect(
                                    "Shouldn't update ray bind group if backbuffer doesn't exist.",
                                )
                                .1,
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(
                            &self
                                .radiance_total_prev
                                .as_ref()
                                .expect(
                                    "Shouldn't update ray bind group if backbuffer doesn't exist.",
                                )
                                .1,
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.world_info_buffer,
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: voxel_world_gpu.world_acceleration_buffer(),
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: voxel_world_gpu.world_voxel_model_info_buffer(),
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: voxel_world_gpu.world_data_buffer().expect("Shouldn't update ray bind group if world data buffer doesn't exist."),
                            offset: 0,
                            size: None,
                        }),
                    },
                ],
            }),
        );
    }

    fn create_ui_bind_group(
        device: &DeviceResource,
        layout: &wgpu::BindGroupLayout,
        texture_sampler: &wgpu::Sampler,
        texture_view: &wgpu::TextureView,
        uniform_buffer: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui_bind_group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(texture_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: uniform_buffer,
                        offset: 0,
                        size: None,
                    }),
                },
            ],
        })
    }

    fn set_blit_bind_group(&mut self, device: &DeviceResource) {
        self.blit_bind_group = Some(
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("blit_bind_group"),
                layout: &self.blit_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&self.sampler_linear),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(
                            &self
                                .backbuffer
                                .as_ref()
                                .expect("Cant create bind group without required texture")
                                .1,
                        ),
                    },
                ],
            }),
        );
    }

    pub fn update_gpu_objects(
        mut renderer: ResMut<Renderer>,
        voxel_world_gpu: Res<VoxelWorldGpu>,
        device: Res<DeviceResource>,
        settings: Res<Settings>,
    ) {
        renderer
            .graphics_settings
            .refresh_updates(&settings.graphics);

        let mut updates = renderer.graphics_settings.updates().clone();

        // Ensure we initialize any non-initialized objects first.
        if renderer.backbuffer.is_none() {
            updates.insert(GraphicsSettingsAttributes::RenderSize(
                settings.graphics.render_size,
            ));
        }
        let mut update_ray_bind_group = renderer.ray_bind_group.is_none();
        let mut update_blit_bind_group = renderer.blit_bind_group.is_none();

        for update in updates {
            match update {
                GraphicsSettingsAttributes::RenderSize((width, height)) => {
                    // Resize backbuffers and recreate any bind groups that rely on them.
                    debug!("Resized backbuffers to {} x {}", width, height);
                    renderer.set_backbuffer_textures(&device, width, height);
                    update_ray_bind_group = true;
                    update_blit_bind_group = true;

                    // New total radiance texture so average must be reset.
                    renderer.frame_count = 0;
                }
                GraphicsSettingsAttributes::Antialiasing(antialiasing) => {
                    debug!("Changing renderer for antialiasing {:?}", antialiasing);
                    // TODO: Update pipeline with constructed shader based on if we want
                    // antialiasing. Aggregate all the updates at the end though, or i guess we can
                    // just implement that later.
                }
            }
        }

        if update_ray_bind_group || voxel_world_gpu.is_dirty() {
            debug!("Updating ray bind group.");
            renderer.set_ray_bind_group(&device, &voxel_world_gpu);
        }

        if update_blit_bind_group {
            debug!("Updating blit bind group.");
            renderer.set_blit_bind_group(&device);
        }
    }

    pub fn write_render_data(
        mut renderer: ResMut<Renderer>,
        device: Res<DeviceResource>,
        ecs_world: Res<ECSWorld>,
        voxel_world: Res<VoxelWorld>,
        voxel_world_gpu: Res<VoxelWorldGpu>,
        egui: Res<Egui>,
        ui_state: Res<UIState>,
        pipeline_manager: Res<RenderPipelineManager>,
        time: Res<Time>,
    ) {
        'voxel_trace: {
            let mut query = ecs_world.query::<&Transform>().with::<&Camera>();
            let Some((_, camera_transform)) = query.into_iter().next() else {
                break 'voxel_trace;
            };

            let camera_transform = camera_transform.to_view_matrix().transpose();
            let camera_transform_arr: [f32; 16] = camera_transform.as_slice().try_into().unwrap();

            // Update frame count if the camera transform changed or a pipeline was updated.
            if renderer.last_camera_transform != camera_transform_arr
                || pipeline_manager.should_reset_temporal_effects()
            {
                renderer.last_camera_transform = camera_transform_arr;
                renderer.frame_count = 0;
            }
            renderer.frame_count += 1;

            let camera_transform_arr_small: [f32; 12] = camera_transform_arr
                .into_iter()
                .take(12)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap();
            let half_fov = ui_state.player_fov.to_radians() / 2.0;
            let world_info = WorldBuffer {
                camera: CameraBuffer {
                    transform: camera_transform_arr,
                    rotation: camera_transform_arr_small,
                    half_fov,
                    padding: [0.0; 3],
                },
                voxel_model_entity_count: voxel_world_gpu.rendered_voxel_model_entity_count(),
                frame_count: renderer.frame_count,
                total_frame_count: time.frame_count(),
                padding: [0.0; 13],
            };

            device.queue().write_buffer(
                &renderer.world_info_buffer,
                0,
                bytemuck::bytes_of(&world_info),
            );
        }

        'ui: {
            // Update uniform buffer.
            let uniform_data = UIBuffer {
                screen_size: [
                    device.surface_config().width as f32 / egui.pixels_per_point(),
                    device.surface_config().height as f32 / egui.pixels_per_point(),
                ],
            };
            device.queue().write_buffer(
                &renderer.ui_uniform_buffer,
                0,
                bytemuck::bytes_of(&uniform_data),
            );
            // Update textures.
            if let Some(texture_deltas) = egui.textures_delta() {
                for (id, delta) in &texture_deltas.set {
                    let sampler = if let std::collections::hash_map::Entry::Vacant(e) =
                        renderer.ui_samplers.entry(delta.options)
                    {
                        let options = &delta.options;
                        let wrap_mode = match options.wrap_mode {
                            egui::TextureWrapMode::ClampToEdge => wgpu::AddressMode::ClampToEdge,
                            egui::TextureWrapMode::Repeat => wgpu::AddressMode::Repeat,
                            egui::TextureWrapMode::MirroredRepeat => {
                                wgpu::AddressMode::MirrorRepeat
                            }
                        };
                        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                            label: Some("ui_sampler"),
                            address_mode_u: wrap_mode,
                            address_mode_w: wrap_mode,
                            address_mode_v: wrap_mode,
                            mag_filter: match options.magnification {
                                egui::TextureFilter::Nearest => wgpu::FilterMode::Nearest,
                                egui::TextureFilter::Linear => wgpu::FilterMode::Linear,
                            },
                            min_filter: match options.minification {
                                egui::TextureFilter::Nearest => wgpu::FilterMode::Nearest,
                                egui::TextureFilter::Linear => wgpu::FilterMode::Linear,
                            },
                            mipmap_filter: wgpu::FilterMode::Nearest,
                            lod_min_clamp: 0.0,
                            lod_max_clamp: 0.0,
                            compare: None,
                            anisotropy_clamp: 1,
                            border_color: None,
                        });

                        e.insert(sampler);
                        renderer.ui_samplers.get(&delta.options).unwrap()
                    } else {
                        renderer.ui_samplers.get(&delta.options).unwrap()
                    };
                    if delta.is_whole() {
                        // Get or create textures.
                        let (texture, _bind_group) = {
                            if !renderer.ui_textures.contains_key(id) {
                                let texture = device.create_texture(&wgpu::TextureDescriptor {
                                    label: Some("ui_texture"),
                                    size: wgpu::Extent3d {
                                        width: delta.image.size()[0] as u32,
                                        height: delta.image.size()[1] as u32,
                                        depth_or_array_layers: 1,
                                    },
                                    mip_level_count: 1,
                                    sample_count: 1,
                                    dimension: wgpu::TextureDimension::D2,
                                    format: wgpu::TextureFormat::Rgba8Unorm,
                                    usage: wgpu::TextureUsages::COPY_DST
                                        | wgpu::TextureUsages::TEXTURE_BINDING,
                                    view_formats: &[],
                                });
                                let texture_view =
                                    texture.create_view(&wgpu::TextureViewDescriptor::default());

                                let bind_group = Self::create_ui_bind_group(
                                    &device,
                                    &renderer.ui_bind_group_layout,
                                    sampler,
                                    &texture_view,
                                    &renderer.ui_uniform_buffer,
                                );
                                renderer.ui_textures.insert(*id, (texture, bind_group));
                            }

                            renderer.ui_textures.get(id).unwrap()
                        };

                        if let Some(pos) = delta.pos {
                            todo!("handle pos;")
                        }
                        match &delta.image {
                            egui::ImageData::Color(image) => {
                                device.queue().write_texture(
                                    wgpu::ImageCopyTexture {
                                        texture,
                                        mip_level: 0,
                                        origin: wgpu::Origin3d::ZERO,
                                        aspect: wgpu::TextureAspect::All,
                                    },
                                    bytemuck::cast_slice(image.pixels.as_slice()),
                                    wgpu::ImageDataLayout {
                                        offset: 0,
                                        bytes_per_row: Some(4 * image.width() as u32),
                                        rows_per_image: Some(image.height() as u32),
                                    },
                                    wgpu::Extent3d {
                                        width: image.width() as u32,
                                        height: image.height() as u32,
                                        depth_or_array_layers: 1,
                                    },
                                );
                            }
                            egui::ImageData::Font(font) => {
                                let data = font.srgba_pixels(None).collect::<Vec<egui::Color32>>();
                                device.queue().write_texture(
                                    wgpu::ImageCopyTexture {
                                        texture,
                                        mip_level: 0,
                                        origin: wgpu::Origin3d::ZERO,
                                        aspect: wgpu::TextureAspect::All,
                                    },
                                    bytemuck::cast_slice(data.as_slice()),
                                    wgpu::ImageDataLayout {
                                        offset: 0,
                                        bytes_per_row: Some(4 * font.width() as u32),
                                        rows_per_image: Some(font.height() as u32),
                                    },
                                    wgpu::Extent3d {
                                        width: font.width() as u32,
                                        height: font.height() as u32,
                                        depth_or_array_layers: 1,
                                    },
                                );
                            }
                        }
                    } else {
                        todo!("implement unwhole texture updates");
                    }
                }
            }
            // Update vertex and index buffers.
            let mut total_vertex_count = 0;
            let mut total_index_count = 0;
            for epaint::ClippedPrimitive {
                clip_rect: _clip_rect,
                primitive,
            } in egui.primitives()
            {
                match primitive {
                    epaint::Primitive::Mesh(mesh) => {
                        total_vertex_count += mesh.vertices.len();
                        total_index_count += mesh.indices.len();
                    }
                    epaint::Primitive::Callback(_) => todo!(),
                }
            }
            if total_vertex_count > 0 {
                // Resize buffers to fit vertex and index data.
                let required_vertex_size =
                    std::mem::size_of::<epaint::Vertex>() * total_vertex_count;
                if renderer.ui_vertex_buffer.size() < required_vertex_size as u64 {
                    let new_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("ui_vertex_buffer"),
                        size: required_vertex_size as u64 + (renderer.ui_vertex_buffer.size() / 2),
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let _ = std::mem::replace(&mut renderer.ui_vertex_buffer, new_buffer);
                }

                let mut vertex_ptr = 0;
                let Some(mut writeable_vertex_buffer) = device.queue().write_buffer_with(
                    &renderer.ui_vertex_buffer,
                    0,
                    NonZero::new(required_vertex_size as u64).unwrap(),
                ) else {
                    break 'ui;
                };
                let mut vertex_slices = Vec::new();
                for epaint::ClippedPrimitive {
                    clip_rect: _clip_rect,
                    primitive,
                } in egui.primitives()
                {
                    match primitive {
                        epaint::Primitive::Mesh(mesh) => {
                            let size = mesh.vertices.len() * std::mem::size_of::<epaint::Vertex>();
                            let slice = vertex_ptr..(vertex_ptr + size);
                            writeable_vertex_buffer[slice.clone()]
                                .copy_from_slice(bytemuck::cast_slice(mesh.vertices.as_slice()));
                            vertex_slices.push(slice.clone());
                            vertex_ptr += size;
                        }
                        epaint::Primitive::Callback(_) => todo!(),
                    }
                }
                drop(writeable_vertex_buffer);
                renderer.ui_vertex_buffer_slices = vertex_slices;
            }
            if total_index_count > 0 {
                let required_index_size = std::mem::size_of::<u32>() * total_index_count;
                if renderer.ui_index_buffer.size() < required_index_size as u64 {
                    let new_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("ui_index_buffer"),
                        size: required_index_size as u64 + (renderer.ui_index_buffer.size() / 2),
                        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let _ = std::mem::replace(&mut renderer.ui_index_buffer, new_buffer);
                }

                let mut index_ptr = 0;
                let Some(mut writeable_index_buffer) = device.queue().write_buffer_with(
                    &renderer.ui_index_buffer,
                    0,
                    NonZero::new(required_index_size as u64).unwrap(),
                ) else {
                    break 'ui;
                };
                let mut index_slices = Vec::new();
                for epaint::ClippedPrimitive {
                    clip_rect: _clip_rect,
                    primitive,
                } in egui.primitives()
                {
                    match primitive {
                        epaint::Primitive::Mesh(mesh) => {
                            let size = mesh.indices.len() * std::mem::size_of::<u32>();
                            let slice = index_ptr..(index_ptr + size);
                            writeable_index_buffer[slice.clone()]
                                .copy_from_slice(bytemuck::cast_slice(mesh.indices.as_slice()));
                            index_slices.push(slice.clone());
                            index_ptr += size;
                        }
                        epaint::Primitive::Callback(_) => todo!(),
                    }
                }
                drop(writeable_index_buffer);
                renderer.ui_index_buffer_slices = index_slices;
            }
        }
    }

    pub fn render(
        renderer: ResMut<Renderer>,
        device: Res<DeviceResource>,
        egui: Res<Egui>,
        pipeline_manager: Res<RenderPipelineManager>,
    ) {
        let Some(backbuffer) = &renderer.backbuffer else {
            return;
        };
        let Some(radiance_total) = &renderer.radiance_total else {
            return;
        };
        let Some(radiance_total_prev) = &renderer.radiance_total_prev else {
            return;
        };
        let Some(blit_bind_group) = &renderer.blit_bind_group else {
            return;
        };
        let Some(ray_bind_group) = &renderer.ray_bind_group else {
            return;
        };
        let Some(ray_pipeline) = pipeline_manager.get_compute_pipeline(renderer.ray_pipeline_id)
        else {
            return;
        };
        let blit_pipeline = &renderer.blit_pipeline;
        let ui_pipeline = &renderer.ui_pipeline;
        // let Some(blit_pipeline) = pipeline_manager.get_render_pipeline(renderer.blit_pipeline_id)
        // else {
        //     return;
        // };
        // let Some(ui_pipeline) = pipeline_manager.get_render_pipeline(renderer.ui_pipeline_id)
        // else {
        //     return;
        // };

        let swapchain_texture = device
            .surface()
            .get_current_texture()
            .expect("Couldn't get surface texture");

        let swapchain_texture_view = swapchain_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = device
            .device()
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("renderer encoder"),
            });

        {
            use shader::voxel_trace::WORKGROUP_SIZE;
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("Ray March"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(ray_pipeline);
            compute_pass.set_bind_group(0, ray_bind_group, &[]);
            compute_pass.dispatch_workgroups(
                (backbuffer.0.width() as f32 / WORKGROUP_SIZE[0] as f32).ceil() as u32,
                (backbuffer.0.height() as f32 / WORKGROUP_SIZE[1] as f32).ceil() as u32,
                1,
            );
        }

        // Copy backbuffer to history
        {
            encoder.copy_texture_to_texture(
                wgpu::ImageCopyTexture {
                    texture: &radiance_total.0,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::ImageCopyTexture {
                    texture: &radiance_total_prev.0,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: radiance_total.0.width(),
                    height: radiance_total.0.height(),
                    depth_or_array_layers: 1,
                },
            );
        }

        // Blit backbuffer to swapchain texture
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &swapchain_texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations::<wgpu::Color> {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(blit_pipeline);
            render_pass.set_bind_group(0, blit_bind_group, &[]);

            render_pass.draw(0..6, 0..1);
        }

        // UI Pass
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &swapchain_texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations::<wgpu::Color> {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            render_pass.set_pipeline(ui_pipeline);

            let mut index_slices = renderer.ui_index_buffer_slices.iter();
            let mut vertex_slices = renderer.ui_vertex_buffer_slices.iter();
            for epaint::ClippedPrimitive {
                clip_rect,
                primitive,
            } in egui.primitives()
            {
                {
                    let rect = clip_rect;
                    if rect.width() == 0.0 || rect.height() == 0.0 {
                        continue;
                    }
                    let pixels_per_point = egui.pixels_per_point();
                    let clip_min_x = pixels_per_point * clip_rect.min.x;
                    let clip_min_y = pixels_per_point * clip_rect.min.y;
                    let clip_max_x = pixels_per_point * clip_rect.max.x;
                    let clip_max_y = pixels_per_point * clip_rect.max.y;

                    // Round to integer:
                    let clip_min_x = clip_min_x.round() as u32;
                    let clip_min_y = clip_min_y.round() as u32;
                    let clip_max_x = clip_max_x.round() as u32;
                    let clip_max_y = clip_max_y.round() as u32;

                    // Clamp:
                    let texture_size = swapchain_texture.texture.size();
                    let clip_min_x = clip_min_x.clamp(0, texture_size.width);
                    let clip_min_y = clip_min_y.clamp(0, texture_size.height);
                    let clip_max_x = clip_max_x.clamp(clip_min_x, texture_size.width);
                    let clip_max_y = clip_max_y.clamp(clip_min_y, texture_size.height);
                    render_pass.set_scissor_rect(
                        clip_min_x,
                        clip_min_y,
                        clip_max_x - clip_min_x,
                        clip_max_y - clip_min_y,
                    );
                }
                match primitive {
                    epaint::Primitive::Mesh(mesh) => {
                        let slice = vertex_slices.next().unwrap();
                        let vertex_buffer_slice = renderer
                            .ui_vertex_buffer
                            .slice(slice.start as u64..slice.end as u64);
                        let slice = index_slices.next().unwrap();
                        let index_buffer_slice = renderer
                            .ui_index_buffer
                            .slice(slice.start as u64..slice.end as u64);

                        if let Some((_texture, bind_group)) =
                            renderer.ui_textures.get(&mesh.texture_id)
                        {
                            render_pass.set_bind_group(0, bind_group, &[]);
                            render_pass.set_vertex_buffer(0, vertex_buffer_slice);
                            render_pass
                                .set_index_buffer(index_buffer_slice, wgpu::IndexFormat::Uint32);

                            render_pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
                        } else {
                            todo!("Couldnt find the thingy");
                        }
                    }
                    epaint::Primitive::Callback(_) => todo!(),
                }
            }
        }

        device.queue().submit([encoder.finish()]);
        swapchain_texture.present();
    }
}
