use std::{
    borrow::Cow,
    collections::HashMap,
    num::NonZero,
    ops::{DerefMut, Range},
};

use bytemuck::Zeroable;
use fixedbitset::IndexRange;
use log::debug;
use nalgebra::Vector2;
use rogue_macros::Resource;
use wgpu::PipelineCompilationOptions;

use crate::engine::{
    ecs::ecs_world::ECSWorld,
    graphics::{
        backend::{
            Buffer, GfxAddressMode, GfxFilterMode, GfxImageCreateInfo, GfxImageFormat, GfxImageType, GfxImageWrite, GfxPassOnceImpl, GfxRenderPassAttachment, GfxSamplerCreateInfo, GraphicsBackendDevice, GraphicsBackendRecorder, Image, ResourceId, Sampler
        },
        device::DeviceResource,
        frame_graph::FrameGraphContext,
        render_contants,
        renderer::Renderer,
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
    ui_textures: HashMap<egui::TextureId, (ResourceId<Image>, ResourceId<Sampler>)>,
    ui_samplers: HashMap<egui::TextureOptions, ResourceId<Sampler>>,

    ui_render_prims: Vec<UIRenderPrim>,
    pixels_per_egui_point: f32,
}

impl UIPass {
    pub fn new() -> Self {
        // let ui_vertex_attributes = [
        //     wgpu::VertexAttribute {
        //         format: wgpu::VertexFormat::Float32x2,
        //         offset: 0,
        //         shader_location: 0,
        //     },
        //     wgpu::VertexAttribute {
        //         format: wgpu::VertexFormat::Float32x2,
        //         offset: 8,
        //         shader_location: 1,
        //     },
        //     wgpu::VertexAttribute {
        //         format: wgpu::VertexFormat::Uint32,
        //         offset: 16,
        //         shader_location: 2,
        //     },
        // ];
        // let ui_vertex_buffer_layout = wgpu::VertexBufferLayout {
        //     array_stride: 20,
        //     step_mode: wgpu::VertexStepMode::Vertex,
        //     attributes: &ui_vertex_attributes,
        // };
        //let ui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        //    label: Some("ui_pipeline"),
        //    layout: Some(&ui_pipeline_layout),
        //    cache: None,
        //    vertex: wgpu::VertexState {
        //        module: &ui_shader_module,
        //        entry_point: "vs_main",
        //        compilation_options: PipelineCompilationOptions::default(),
        //        buffers: &[ui_vertex_buffer_layout],
        //    },
        //    primitive: wgpu::PrimitiveState {
        //        topology: wgpu::PrimitiveTopology::TriangleList,
        //        strip_index_format: None,
        //        front_face: wgpu::FrontFace::Ccw,
        //        cull_mode: None,
        //        unclipped_depth: false,
        //        polygon_mode: wgpu::PolygonMode::Fill,
        //        conservative: false,
        //    },
        //    depth_stencil: None,
        //    multisample: wgpu::MultisampleState::default(),
        //    fragment: Some(wgpu::FragmentState {
        //        module: &ui_shader_module,
        //        entry_point: "fs_main",
        //        compilation_options: PipelineCompilationOptions::default(),
        //        targets: &[Some(wgpu::ColorTargetState {
        //            format: device.surface_config().format,
        //            blend: Some(wgpu::BlendState {
        //                color: wgpu::BlendComponent {
        //                    src_factor: wgpu::BlendFactor::One,
        //                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        //                    operation: wgpu::BlendOperation::Add,
        //                },
        //                alpha: wgpu::BlendComponent {
        //                    src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
        //                    dst_factor: wgpu::BlendFactor::One,
        //                    operation: wgpu::BlendOperation::Add,
        //                },
        //            }),
        //            write_mask: wgpu::ColorWrites::all(),
        //        })],
        //    }),
        //    multiview: None,
        //});

        return Self {
            ui_textures: HashMap::new(),
            ui_samplers: HashMap::new(),
            ui_render_prims: Vec::new(),
            pixels_per_egui_point: 0.0,
        };
    }

    pub fn write_debug_ui_render_data(
        mut ui_pass: ResMut<UIPass>,
        mut renderer: ResMut<Renderer>,
        mut device: ResMut<DeviceResource>,
        ecs_world: Res<ECSWorld>,
        egui: Res<Egui>,
        time: Res<Time>,
    ) {
        let ui_pass = ui_pass.deref_mut();

        ui_pass.pixels_per_egui_point = egui.pixels_per_point();
        ui_pass.ui_render_prims.clear();

        // Update textures.
        // TODO: Clean this up since the nesting is out of hand.
        if let Some(texture_deltas) = egui.textures_delta() {
            for (id, delta) in &texture_deltas.set {
                let sampler_id = if let std::collections::hash_map::Entry::Vacant(e) =
                    ui_pass.ui_samplers.entry(delta.options)
                {
                    let options = &delta.options;
                    let sampler_id = device.create_sampler(GfxSamplerCreateInfo {
                        mag_filter: options.magnification.into(),
                        min_filter: options.minification.into(),
                        address_mode: options.wrap_mode.into(),
                        mipmap_filter: GfxFilterMode::Nearest,
                    });

                    e.insert(sampler_id);
                    ui_pass.ui_samplers.get(&delta.options).unwrap()
                } else {
                    ui_pass.ui_samplers.get(&delta.options).unwrap()
                };

                if delta.is_whole() {
                    // Get or create textures.
                    let texture = {
                        if !ui_pass.ui_textures.contains_key(id) {
                            let texture = device.create_image(GfxImageCreateInfo {
                                name: "ui_texture".to_owned(),
                                image_type: GfxImageType::D2,
                                extent: Vector2::new(
                                    delta.image.size()[0] as u32,
                                    delta.image.size()[1] as u32,
                                ),
                                format: GfxImageFormat::Rgba8Srgb,
                            });
                            ui_pass.ui_textures.insert(*id, (texture, *sampler_id));
                        }

                        ui_pass.ui_textures.get(id).unwrap().0
                    };

                    if let Some(pos) = delta.pos {
                        todo!("handle pos;")
                    }
                    match &delta.image {
                        egui::ImageData::Color(image) => {
                            device.write_image(GfxImageWrite {
                                image: texture,
                                data: bytemuck::cast_slice(image.pixels.as_slice()),
                                offset: Vector2::zeros(),
                                extent: Vector2::new(image.width() as u32, image.height() as u32),
                            });
                        }
                        egui::ImageData::Font(font) => {
                            let data = font.srgba_pixels(None).collect::<Vec<egui::Color32>>();
                            device.write_image(GfxImageWrite {
                                image: texture,
                                data: bytemuck::cast_slice(data.as_slice()),
                                offset: Vector2::zeros(),
                                extent: Vector2::new(font.width() as u32, font.height() as u32),
                            });
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
            // We maybe don't have to do this.
            // renderer
            //     .frame_graph_executor
            //     .mark_buffer_empty(Renderer::GRAPH.debug_ui.buffer_vertex_buffer);
            // renderer
            //     .frame_graph_executor
            //     .mark_buffer_empty(Renderer::GRAPH.debug_ui.buffer_index_buffer);
            return;
        }

        // TODO: Make a `write_buffers` that can take a n-sized tuple of (buffer, size) and the function
        // will have a n-sized tuple with the corresponding write pointers in the same order.
        let required_vertex_size = std::mem::size_of::<epaint::Vertex>() * total_vertex_count;
        let required_index_size = std::mem::size_of::<u32>() * total_index_count;
        let writeable_vertex_buffer = renderer
            .frame_graph_executor
            .write_buffer(
                Renderer::GRAPH.debug_ui.buffer_vertex_buffer,
                required_vertex_size as u64,
            )
            .as_mut_ptr();
        let writeable_index_buffer = renderer
            .frame_graph_executor
            .write_buffer(
                Renderer::GRAPH.debug_ui.buffer_index_buffer,
                required_index_size as u64,
            )
            .as_mut_ptr();
        // Safety: I just know these pointers are valid since they're mapped to a staging buffer, trust me bro.
        let (writeable_vertex_buffer, writeable_index_buffer) = unsafe {
            (
                std::slice::from_raw_parts_mut(writeable_vertex_buffer, required_vertex_size),
                std::slice::from_raw_parts_mut(writeable_index_buffer, required_index_size),
            )
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

    pub fn write_ui_pass(mut ui_pass: ResMut<UIPass>, mut renderer: ResMut<Renderer>) {
        let ui_pass: &mut UIPass = &mut ui_pass;
        renderer.frame_graph_executor.supply_pass_ref(
            Renderer::GRAPH.debug_ui.pass_ui,
            
                &mut |recorder: &mut dyn GraphicsBackendRecorder, ctx: &FrameGraphContext<'_>| {
                    if ui_pass.ui_render_prims.is_empty() {
                        return;
                    }

                    let backbuffer = ctx.get_image(Renderer::GRAPH.image_backbuffer);
                    let backbuffer_info = recorder.get_image_info(&backbuffer);
                    let vertex_buffer =
                        ctx.get_buffer(Renderer::GRAPH.debug_ui.buffer_vertex_buffer);
                    let index_buffer = ctx.get_buffer(Renderer::GRAPH.debug_ui.buffer_index_buffer);

                    let raster_pipeline =
                        ctx.get_raster_pipeline(Renderer::GRAPH.debug_ui.pipeline_raster_ui);
                    let mut render_pass = recorder.begin_render_pass(raster_pipeline, &[GfxRenderPassAttachment::new_load(backbuffer)], None);

                    let pixels_per_point = ui_pass.pixels_per_egui_point;
                    let logical_screen_size = Vector2::new(backbuffer_info.resolution.x as f32 / pixels_per_point, backbuffer_info.resolution.y as f32 / pixels_per_point);
                    for UIRenderPrim {
                        clip_rect,
                        vertex_slice,
                        index_slice,
                        vertex_count,
                        texture_id,
                    } in ui_pass.ui_render_prims.drain(..)
                    {
                        // Set scissor.
                        {
                            let rect = clip_rect;
                            if rect.width() == 0.0 || rect.height() == 0.0 {
                                continue;
                            }

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
                            let texture_size = backbuffer_info.resolution_xy();
                            let clip_min_x = clip_min_x.clamp(0, texture_size.x);
                            let clip_min_y = clip_min_y.clamp(0, texture_size.y);
                            let clip_max_x = clip_max_x.clamp(clip_min_x, texture_size.x);
                            let clip_max_y = clip_max_y.clamp(clip_min_y, texture_size.y);
                            render_pass.set_scissor(
                                clip_min_x,
                                clip_min_y,
                                clip_max_x - clip_min_x,
                                clip_max_y - clip_min_y,
                            );
                        }

                        let Some((texture, sampler)) = ui_pass.ui_textures.get(&texture_id) else {
                            panic!("Debug ui render primitive has a texture ID that hasn't been populated in `ui_pass.ui_textures` yet.");
                        };
                        render_pass.bind_uniforms(&mut |writer| {
                            writer.write_uniform("u_shader.screen_size", logical_screen_size);
                            writer.write_binding("u_shader.texture", *texture);
                            writer.write_binding("u_shader.sampler", *sampler);
                        });

                        render_pass.bind_vertex_buffer(
                            vertex_buffer,
                            vertex_slice.start().unwrap() as u64,
                        );
                        render_pass.bind_index_buffer(
                            index_buffer,
                            index_slice.start().unwrap() as u64,
                        );

                        render_pass.draw_indexed(vertex_count);
                    }
                },
        );
    }
}

impl From<egui::TextureFilter> for GfxFilterMode {
    fn from(value: egui::TextureFilter) -> Self {
        match value {
            egui::TextureFilter::Nearest => Self::Nearest,
            egui::TextureFilter::Linear => Self::Linear,
        }
    }
}

impl From<egui::TextureWrapMode> for GfxAddressMode {
    fn from(value: egui::TextureWrapMode) -> Self {
        match value {
            egui::TextureWrapMode::ClampToEdge => Self::ClampToEdge,
            egui::TextureWrapMode::Repeat => Self::Repeat,
            egui::TextureWrapMode::MirroredRepeat => Self::MirroredRepeat,
        }
    }
}
