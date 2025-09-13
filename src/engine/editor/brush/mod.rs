use nalgebra::Vector3;

use crate::{
    common::color::Color,
    consts,
    engine::{
        debug::{DebugFlags, DebugLine, DebugRenderer},
        editor::editor::Editor,
        entity::{ecs_world::ECSWorld, RenderableVoxelEntity},
        input::{mouse, Input},
        physics::transform::Transform,
        voxel::{
            attachment::{
                Attachment, AttachmentInfoMap, AttachmentMap, BuiltInMaterial, PTMaterial,
            },
            cursor::{VoxelEditEntityInfo, VoxelEditInfo},
            voxel_world::{VoxelEdit, VoxelTraceInfo, VoxelWorld},
        },
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditorEditingTool {
    Pencil,
    Eraser,
    Brush,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditorEditingMaterial {
    PTMat,
    BMat,
}

pub struct EditorWorldEditing {
    pub entity_enabled: bool,
    pub terrain_enabled: bool,
    pub size: u32,
    pub color: Color,
    pub bmat_index: u16,
    pub tool: EditorEditingTool,
    pub material: EditorEditingMaterial,
}

impl EditorWorldEditing {
    pub fn new() -> Self {
        Self {
            terrain_enabled: true,
            entity_enabled: false,
            size: 2,
            bmat_index: 0,
            color: Color::new_srgb(0.5, 0.5, 0.5),
            tool: EditorEditingTool::Pencil,
            material: EditorEditingMaterial::BMat,
        }
    }

    pub fn pencil_edit(&self, voxel_world: &mut VoxelWorld, voxel_trace: &Option<VoxelTraceInfo>) {
        match self.material {
            EditorEditingMaterial::PTMat => {
                let mut attachment_map = AttachmentMap::new();
                attachment_map.register_attachment(Attachment::PTMATERIAL);
                self.apply_edit(
                    voxel_world,
                    voxel_trace,
                    self.size,
                    attachment_map,
                    |center| {
                        let size = self.size;
                        let color = self.color;
                        return Box::new(move |mut voxel, world_voxel_pos, local_voxel_pos| {
                            let distance = center
                                .cast::<f32>()
                                .metric_distance(&world_voxel_pos.cast::<f32>());
                            if distance <= size as f32 {
                                voxel.set_attachment(
                                    Attachment::PTMATERIAL_ID,
                                    &[PTMaterial::diffuse(color.clone()).encode()],
                                );
                            }
                        });
                    },
                );
            }
            EditorEditingMaterial::BMat => {
                let mut attachment_map = AttachmentMap::new();
                attachment_map.register_attachment(Attachment::BMAT);
                self.apply_edit(
                    voxel_world,
                    voxel_trace,
                    self.size,
                    attachment_map,
                    |center| {
                        let size = self.size;
                        let bmat_index = self.bmat_index;
                        return Box::new(move |mut voxel, world_voxel_pos, local_voxel_pos| {
                            let distance = center
                                .cast::<f32>()
                                .metric_distance(&world_voxel_pos.cast::<f32>());
                            if distance <= size as f32 {
                                voxel.set_attachment(
                                    Attachment::BMAT_ID,
                                    &[BuiltInMaterial::new(bmat_index).encode()],
                                );
                            }
                        });
                    },
                );
            }
        }
    }

    pub fn render_preview(
        &self,
        debug_renderer: &mut DebugRenderer,
        hovered_trace: &Option<VoxelTraceInfo>,
        ecs_world: &ECSWorld,
        voxel_world: &VoxelWorld,
    ) {
        let center = match hovered_trace {
            Some(VoxelTraceInfo::Terrain { world_voxel_pos }) => {
                if self.terrain_enabled {
                    let center = (world_voxel_pos.cast::<f32>()
                        * consts::voxel::VOXEL_METER_LENGTH)
                        .add_scalar(consts::voxel::VOXEL_METER_LENGTH * 0.5);
                    Some(center)
                } else {
                    None
                }
            }
            Some(VoxelTraceInfo::Entity {
                entity_id,
                voxel_model_id,
                local_voxel_pos,
            }) => {
                if self.entity_enabled {
                    let Ok(mut preview_entity_query) =
                        ecs_world.query_one::<(&Transform, &RenderableVoxelEntity)>(*entity_id)
                    else {
                        return;
                    };
                    let Some((model_local_transform, renderable)) = preview_entity_query.get()
                    else {
                        return;
                    };
                    let model_world_transform =
                        ecs_world.get_world_transform(*entity_id, &model_local_transform);

                    let side_length = voxel_world
                        .get_dyn_model(renderable.voxel_model_id().unwrap())
                        .length();
                    let model_obb = model_world_transform.as_voxel_model_obb(side_length);
                    let forward = model_world_transform.forward()
                        * consts::voxel::VOXEL_METER_LENGTH
                        * model_world_transform.scale.z;
                    let right = model_world_transform.right()
                        * consts::voxel::VOXEL_METER_LENGTH
                        * model_world_transform.scale.x;
                    let up = model_world_transform.up()
                        * consts::voxel::VOXEL_METER_LENGTH
                        * model_world_transform.scale.y;

                    let center = model_obb.aabb.min
                        + forward * (local_voxel_pos.z as f32 + 0.5)
                        + right * (local_voxel_pos.x as f32 + 0.5)
                        + up * (local_voxel_pos.y as f32 + 0.5);
                    Some(center)
                } else {
                    None
                }
            }
            None => None,
        };

        if let Some(center) = center {
            debug_renderer.draw_line(DebugLine {
                start: center,
                end: center,
                thickness: (self.size as f32 + 1.0) * consts::voxel::VOXEL_METER_LENGTH * 0.5,
                color: self.color.clone(),
                alpha: 0.4,
                flags: DebugFlags::NONE,
            });
        }
    }

    pub fn update_brush(
        &mut self,
        input: &Input,
        voxel_world: &mut VoxelWorld,
        hovered_trace: &Option<VoxelTraceInfo>,
    ) {
        // Editing things.
        if input.is_mouse_button_pressed(mouse::Button::Left) {
            let size = self.size;

            match self.tool {
                EditorEditingTool::Pencil => self.pencil_edit(voxel_world, &hovered_trace),
                EditorEditingTool::Brush => {}
                EditorEditingTool::Eraser => {
                    let mut attachment_map = AttachmentMap::new();
                    self.apply_edit(
                        voxel_world,
                        &hovered_trace,
                        size,
                        attachment_map,
                        |center| {
                            Box::new(move |mut voxel, world_voxel_pos, local_voxel_pos| {
                                let distance = center
                                    .cast::<f32>()
                                    .metric_distance(&world_voxel_pos.cast::<f32>());
                                if distance <= size as f32 {
                                    voxel.set_removed();
                                }
                            })
                        },
                    );
                }
            }
        }
    }

    // Applies an edit to either the terrain or an entity using the voxel
    // trace information and the current brush settings.
    fn apply_edit(
        &self,
        voxel_world: &mut VoxelWorld,
        trace: &Option<VoxelTraceInfo>,
        size: u32,
        attachment_map: AttachmentInfoMap,
        f: impl Fn(
            /*world_model_voxel_center=*/ Vector3<i32>,
        ) -> Box<
            dyn Fn(
                VoxelEdit,
                /*world/model_voxel_pos*/ Vector3<i32>,
                /*local/model_voxel_pos=*/ Vector3<u32>,
            ),
        >,
    ) {
        let size_vec = Vector3::new(size, size, size);
        if let Some(VoxelTraceInfo::Terrain {
            world_voxel_pos: world_voxel_hit,
        }) = trace
        {
            if self.terrain_enabled {
                let anchor = world_voxel_hit - size_vec.cast::<i32>();
                voxel_world.apply_voxel_edit(
                    VoxelEditInfo {
                        world_voxel_position: anchor,
                        world_voxel_length: (size_vec * 2).add_scalar(1),
                        attachment_map,
                    },
                    f(*world_voxel_hit),
                );
            }
        } else if let Some(VoxelTraceInfo::Entity {
            entity_id,
            voxel_model_id,
            local_voxel_pos,
        }) = &trace
        {
            if self.entity_enabled {
                let local_voxel_pos = local_voxel_pos.cast::<i32>();
                let anchor = local_voxel_pos - size_vec.cast::<i32>();
                voxel_world.apply_voxel_edit_entity(
                    VoxelEditEntityInfo {
                        model_id: *voxel_model_id,
                        local_voxel_pos: anchor,
                        voxel_length: (size_vec * 2).add_scalar(1),
                        attachment_map,
                    },
                    f(local_voxel_pos),
                );
            }
        }
    }
}
