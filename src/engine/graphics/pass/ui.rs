use std::{
    borrow::Cow,
    collections::HashMap,
    num::NonZero,
    ops::{DerefMut, Range},
};

use bytemuck::Zeroable;
use rogue_macros::Resource;
use wgpu::PipelineCompilationOptions;

use crate::engine::{
    ecs::ecs_world::ECSWorld,
    graphics::{
        device::DeviceResource,
        pipeline_manager::{PipelineId, RenderPipelineManager},
        renderer::Renderer,
        sampler::{FilterMode, SamplerCache, SamplerId, SamplerInfo},
        shader,
    },
    resource::{Res, ResMut},
    ui::gui::Egui,
    window::time::Time,
};

#[derive(bytemuck::Pod, Clone, Copy, Zeroable, Debug)]
#[repr(C)]
pub struct UIBufferData {
    screen_size: [f32; 2],
}

struct UIRenderPrim {
    clip_rect: egui::Rect,
    vertex_slice: Range<usize>,
    index_slice: Range<usize>,
    vertex_count: u32,
    texture_id: egui::TextureId,
}

const UI_VERTEX_BUFFER_START_COUNT: u64 = 100;
const UI_INDEX_BUFFER_START_COUNT: u64 = 100;

#[derive(Resource)]
pub struct UIPass {
    ui_vertex_buffer: wgpu::Buffer,
    ui_index_buffer: wgpu::Buffer,
    ui_uniform_buffer: wgpu::Buffer,
    ui_bind_group_layout: wgpu::BindGroupLayout,
    ui_textures: HashMap<egui::TextureId, (wgpu::Texture, wgpu::BindGroup)>,
    ui_samplers: HashMap<egui::TextureOptions, SamplerId>,
    ui_pipeline: wgpu::RenderPipeline,

    ui_render_prims: Vec<UIRenderPrim>,
    pixels_per_egui_point: f32,
}

impl UIPass {
    pub fn new(device: &DeviceResource) -> Self {
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
            size: std::mem::size_of::<UIBufferData>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
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

        return Self {
            ui_vertex_buffer,
            ui_index_buffer,
            ui_uniform_buffer,
            ui_bind_group_layout,
            ui_pipeline,
            ui_textures: HashMap::new(),
            ui_samplers: HashMap::new(),
            ui_render_prims: Vec::new(),
            pixels_per_egui_point: 0.0,
        };
    }

    pub fn write_render_data(
        mut ui_pass: ResMut<UIPass>,
        mut sampler_cache: ResMut<SamplerCache>,
        device: Res<DeviceResource>,
        ecs_world: Res<ECSWorld>,
        egui: Res<Egui>,
        pipeline_manager: Res<RenderPipelineManager>,
        time: Res<Time>,
    ) {
        let ui_pass = ui_pass.deref_mut();

        ui_pass.pixels_per_egui_point = egui.pixels_per_point();
        ui_pass.ui_render_prims.clear();

        // Update uniform buffer.
        let uniform_data = UIBufferData {
            screen_size: [
                device.surface_config().width as f32 / egui.pixels_per_point(),
                device.surface_config().height as f32 / egui.pixels_per_point(),
            ],
        };
        device.queue().write_buffer(
            &ui_pass.ui_uniform_buffer,
            0,
            bytemuck::bytes_of(&uniform_data),
        );
        // Update textures.
        // TODO: Clean this up since the nesting is out of hand.
        if let Some(texture_deltas) = egui.textures_delta() {
            for (id, delta) in &texture_deltas.set {
                let sampler_id = if let std::collections::hash_map::Entry::Vacant(e) =
                    ui_pass.ui_samplers.entry(delta.options)
                {
                    let options = &delta.options;
                    let sampler_id = sampler_cache.get_lazy_id(
                        SamplerInfo {
                            mag_filter: options.magnification.into(),
                            min_filter: options.minification.into(),
                            address_mode: options.wrap_mode.into(),
                            mipmap_filter: FilterMode::Nearest,
                        },
                        &device,
                    );

                    e.insert(sampler_id);
                    ui_pass.ui_samplers.get(&delta.options).unwrap()
                } else {
                    ui_pass.ui_samplers.get(&delta.options).unwrap()
                };

                if delta.is_whole() {
                    // Get or create textures.
                    let (texture, _bind_group) = {
                        if !ui_pass.ui_textures.contains_key(id) {
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

                            let sampler = sampler_cache.sampler(*sampler_id);
                            let bind_group = Self::create_ui_bind_group(
                                &device,
                                &ui_pass.ui_bind_group_layout,
                                sampler,
                                &texture_view,
                                &ui_pass.ui_uniform_buffer,
                            );
                            ui_pass.ui_textures.insert(*id, (texture, bind_group));
                        }

                        ui_pass.ui_textures.get(id).unwrap()
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

        if total_vertex_count == 0 || total_index_count == 0 {
            return;
        }

        // Resize buffers to fit vertex and index data.
        let required_vertex_size = std::mem::size_of::<epaint::Vertex>() * total_vertex_count;
        if ui_pass.ui_vertex_buffer.size() < required_vertex_size as u64 {
            let new_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ui_vertex_buffer"),
                size: required_vertex_size as u64 + (ui_pass.ui_vertex_buffer.size() / 2),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let _ = std::mem::replace(&mut ui_pass.ui_vertex_buffer, new_buffer);
        }
        let required_index_size = std::mem::size_of::<u32>() * total_index_count;
        if ui_pass.ui_index_buffer.size() < required_index_size as u64 {
            let new_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ui_index_buffer"),
                size: required_index_size as u64 + (ui_pass.ui_index_buffer.size() / 2),
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let _ = std::mem::replace(&mut ui_pass.ui_index_buffer, new_buffer);
        }

        let Some(mut writeable_vertex_buffer) = device.queue().write_buffer_with(
            &ui_pass.ui_vertex_buffer,
            0,
            NonZero::new(required_vertex_size as u64).unwrap(),
        ) else {
            return;
        };

        let Some(mut writeable_index_buffer) = device.queue().write_buffer_with(
            &ui_pass.ui_index_buffer,
            0,
            NonZero::new(required_index_size as u64).unwrap(),
        ) else {
            return;
        };

        let mut vertex_ptr = 0;
        let mut index_ptr = 0;
        for epaint::ClippedPrimitive {
            clip_rect,
            primitive,
        } in egui.primitives()
        {
            match primitive {
                epaint::Primitive::Mesh(mesh) => {
                    let byte_size = mesh.vertices.len() * std::mem::size_of::<epaint::Vertex>();
                    let vertex_slice = vertex_ptr..(vertex_ptr + byte_size);
                    writeable_vertex_buffer[vertex_slice.clone()]
                        .copy_from_slice(bytemuck::cast_slice(mesh.vertices.as_slice()));
                    vertex_ptr += byte_size;

                    let byte_size = mesh.indices.len() * std::mem::size_of::<u32>();
                    let index_slice = index_ptr..(index_ptr + byte_size);
                    writeable_index_buffer[index_slice.clone()]
                        .copy_from_slice(bytemuck::cast_slice(mesh.indices.as_slice()));
                    index_ptr += byte_size;

                    ui_pass.ui_render_prims.push(UIRenderPrim {
                        clip_rect: *clip_rect,
                        vertex_slice,
                        index_slice,
                        vertex_count: mesh.indices.len() as u32,
                        texture_id: mesh.texture_id,
                    })
                }
                epaint::Primitive::Callback(_) => todo!(),
            }
        }
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

    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        (backbuffer, backbuffer_view): (&wgpu::Texture, &wgpu::TextureView),
    ) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &backbuffer_view,
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
        render_pass.set_pipeline(&self.ui_pipeline);

        for UIRenderPrim {
            clip_rect,
            vertex_slice,
            index_slice,
            vertex_count,
            texture_id,
        } in &self.ui_render_prims
        {
            {
                let rect = clip_rect;
                if rect.width() == 0.0 || rect.height() == 0.0 {
                    continue;
                }
                let pixels_per_point = self.pixels_per_egui_point;
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
                let texture_size = backbuffer.size();
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
            let vertex_buffer_slice = self
                .ui_vertex_buffer
                .slice(vertex_slice.start as u64..vertex_slice.end as u64);
            let index_buffer_slice = self
                .ui_index_buffer
                .slice(index_slice.start as u64..index_slice.end as u64);

            if let Some((_texture, bind_group)) = self.ui_textures.get(&texture_id) {
                render_pass.set_bind_group(0, bind_group, &[]);
                render_pass.set_vertex_buffer(0, vertex_buffer_slice);
                render_pass.set_index_buffer(index_buffer_slice, wgpu::IndexFormat::Uint32);

                render_pass.draw_indexed(0..*vertex_count, 0, 0..1);
            } else {
                todo!("Couldnt find the thingy");
            }
        }
    }
}
