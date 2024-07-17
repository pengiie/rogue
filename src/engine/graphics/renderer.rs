use std::borrow::Cow;

use log::{debug, info};
use rogue_macros::Resource;
use wgpu::{
    naga::back, CommandEncoderDescriptor, ComputePassDescriptor, PipelineCompilationOptions,
    ShaderModule,
};

use crate::engine::{
    ecs::ecs_world::{self, ECSWorld},
    resource::{Res, ResMut},
};

use super::{device::DeviceResource, shaders};

#[derive(Resource)]
pub struct Renderer {
    backbuffer: wgpu::Texture,
    backbuffer_sampler: wgpu::Sampler,

    ray_bind_group_layout: wgpu::BindGroupLayout,
    ray_bind_group: wgpu::BindGroup,
    ray_pipeline: wgpu::ComputePipeline,

    blit_bind_group_layout: wgpu::BindGroupLayout,
    blit_bind_group: wgpu::BindGroup,
    blit_pipeline: wgpu::RenderPipeline,
}

impl Renderer {
    pub fn new(device: &DeviceResource) -> Self {
        let ray_shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Ray shader module"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(shaders::ray_march::SOURCE)),
        });
        let ray_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Ray bind group layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: Self::backbuffer_format(),
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                }],
            });
        let ray_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Ray pipeline layout"),
            bind_group_layouts: &[&ray_bind_group_layout],
            push_constant_ranges: &[],
        });
        let ray_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Ray pipeline"),
            layout: Some(&ray_pipeline_layout),
            module: &ray_shader_module,
            entry_point: shaders::ray_march::entry_points::main::NAME,
            compilation_options: PipelineCompilationOptions::default(),
        });

        let (backbuffer, backbuffer_view) = Self::create_backbuffer(device, 1080, 720);

        let ray_bind_group =
            Self::create_ray_bind_group(device, &ray_bind_group_layout, &backbuffer_view);

        let blit_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("blit_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let backbuffer_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("backbuffer_sampler"),
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
        let blit_bind_group = Self::create_blit_bind_group(
            device,
            &blit_bind_group_layout,
            &backbuffer_sampler,
            &backbuffer_view,
        );
        let blit_shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit_shader_module"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(shaders::blit::SOURCE)),
        });
        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit_pipeline_layout"),
            bind_group_layouts: &[&blit_bind_group_layout],
            push_constant_ranges: &[],
        });

        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit_pipeline"),
            layout: Some(&blit_pipeline_layout),
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
                    format: backbuffer.format(),
                    blend: None,
                    write_mask: wgpu::ColorWrites::all(),
                })],
            }),
            multiview: None,
        });

        Self {
            backbuffer,
            backbuffer_sampler,

            ray_bind_group_layout,
            ray_bind_group,
            ray_pipeline,

            blit_bind_group_layout,
            blit_bind_group,
            blit_pipeline,
        }
    }

    fn backbuffer_format() -> wgpu::TextureFormat {
        wgpu::TextureFormat::Rgba8Unorm
    }

    fn create_backbuffer(
        device: &DeviceResource,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let backbuffer = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Backbuffer"),
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

        let view = backbuffer.create_view(&wgpu::TextureViewDescriptor::default());

        (backbuffer, view)
    }

    fn create_ray_bind_group(
        device: &DeviceResource,
        layout: &wgpu::BindGroupLayout,
        backbuffer_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ray bind group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&backbuffer_view),
            }],
        })
    }

    fn create_blit_bind_group(
        device: &DeviceResource,
        layout: &wgpu::BindGroupLayout,
        backbuffer_sampler: &wgpu::Sampler,
        backbuffer_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit_bind_group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(backbuffer_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&backbuffer_view),
                },
            ],
        })
    }

    pub fn on_resize(&mut self, device: &DeviceResource, width: u32, height: u32) {
        // Add to app.resize and recreate bind groups with new image
        let (backbuffer, backbuffer_view) = Self::create_backbuffer(device, width, height);

        self.backbuffer = backbuffer;
    }

    pub fn write_render_data(
        renderer: ResMut<Renderer>,
        device: Res<DeviceResource>,
        ecs_world: Res<ECSWorld>,
    ) {
    }

    pub fn render(renderer: ResMut<Renderer>, device: Res<DeviceResource>) {
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
            use shaders::ray_march::entry_points::main::WORKGROUP_SIZE;
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("Ray March"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&renderer.ray_pipeline);
            compute_pass.set_bind_group(0, &renderer.ray_bind_group, &[]);
            compute_pass.dispatch_workgroups(
                (renderer.backbuffer.width() as f32 / WORKGROUP_SIZE[0] as f32).ceil() as u32,
                (renderer.backbuffer.height() as f32 / WORKGROUP_SIZE[1] as f32).ceil() as u32,
                1,
            );
        }

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

            render_pass.set_pipeline(&renderer.blit_pipeline);
            render_pass.set_bind_group(0, &renderer.blit_bind_group, &[]);

            render_pass.draw(0..6, 0..1);
        }

        device.queue().submit([encoder.finish()]);
        swapchain_texture.present();
    }
}
