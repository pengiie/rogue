use std::collections::{HashMap, HashSet};

use nalgebra::Vector3;

use crate::engine::{
    graphics::{
        backend::{Buffer, GfxBufferCreateInfo, ResourceId},
        device::DeviceResource,
    },
    voxel::{voxel_registry::VoxelModelId, voxel_world_gpu::VoxelWorldModelGpuInfo},
};

pub struct RenderableChunks {
    pub side_length: u32,
    pub chunk_model_pointers: Vec<VoxelModelId>,

    pub window_offset: Vector3<u32>,
    pub chunk_anchor: Vector3<i32>,
    pub is_dirty: bool,

    pub to_update_chunk_normals: HashSet<Vector3<i32>>,
    pub to_unload_models: Vec<VoxelModelId>,
}

impl RenderableChunks {
    pub fn new(render_distance: u32) -> Self {
        let side_length = render_distance * 2;
        Self {
            side_length,
            chunk_model_pointers: vec![VoxelModelId::null(); side_length.pow(3) as usize],
            window_offset: Vector3::new(0, 0, 0),
            chunk_anchor: Vector3::new(0, 0, 0),
            is_dirty: false,
            to_update_chunk_normals: HashSet::new(),
            to_unload_models: Vec::new(),
        }
    }

    pub fn in_bounds(&self, world_chunk_pos: &Vector3<i32>) -> bool {
        let local_chunk_pos = world_chunk_pos - self.chunk_anchor;
        !(local_chunk_pos.x < 0
            || local_chunk_pos.y < 0
            || local_chunk_pos.z < 0
            || local_chunk_pos.x >= self.side_length as i32
            || local_chunk_pos.y >= self.side_length as i32
            || local_chunk_pos.z >= self.side_length as i32)
    }

    pub fn clear(&mut self) {
        self.to_update_chunk_normals.clear();
        self.chunk_model_pointers.fill(VoxelModelId::null());
        self.is_dirty = true;
    }

    pub fn resize(&mut self, chunk_render_distance: u32) {
        self.clear();
        self.side_length = chunk_render_distance * 2;
        self.chunk_model_pointers = vec![VoxelModelId::null(); self.side_length.pow(3) as usize];
        self.window_offset = self
            .chunk_anchor
            .map(|x| x.rem_euclid(self.side_length as i32) as u32);
    }

    pub fn try_load_chunk(
        &mut self,
        world_chunk_pos: &Vector3<i32>,
        model_id: VoxelModelId,
    ) -> bool {
        if !self.in_bounds(world_chunk_pos) {
            return false;
        }

        let local_chunk_pos = (world_chunk_pos - self.chunk_anchor).map(|x| x as u32);
        let window_chunk_pos =
            local_chunk_pos.zip_map(&self.window_offset, |x, y| (x + y) % self.side_length);
        let index = self.get_chunk_index(window_chunk_pos);

        if self.chunk_model_pointers[index as usize] != model_id {
            self.is_dirty = true;
            self.chunk_model_pointers[index as usize] = model_id;
            return true;
        }
        return false;
    }

    pub fn update_player_position(&mut self, player_chunk_position: Vector3<i32>) {
        let new_anchor = player_chunk_position.map(|x| x - (self.side_length as i32 / 2));
        if self.chunk_anchor == new_anchor {
            return;
        }
        let new_window_offset = new_anchor.map(|x| x.rem_euclid(self.side_length as i32) as u32);

        // TODO: Don't unload chunks if we are first initializing the player position.
        let translation = new_anchor - self.chunk_anchor;
        let ranges = translation.zip_zip_map(
            &self.window_offset.cast::<i32>(),
            &new_window_offset.cast::<i32>(),
            |translation, old_window_offset, new_window_offset| {
                if translation.is_positive() {
                    (new_window_offset - translation)..new_window_offset
                } else {
                    (old_window_offset + translation)..old_window_offset
                }
            },
        );

        for x in ranges.x.clone() {
            let x = x.rem_euclid(self.side_length as i32) as u32;
            for y in 0..self.side_length {
                for z in 0..self.side_length {
                    self.unload_chunk(Vector3::new(x, y, z));
                }
            }
        }
        for y in ranges.y.clone() {
            let y = y.rem_euclid(self.side_length as i32) as u32;
            for x in 0..self.side_length {
                for z in 0..self.side_length {
                    self.unload_chunk(Vector3::new(x, y, z));
                }
            }
        }
        for z in ranges.z.clone() {
            let z = z.rem_euclid(self.side_length as i32) as u32;
            for x in 0..self.side_length {
                for y in 0..self.side_length {
                    self.unload_chunk(Vector3::new(x, y, z));
                }
            }
        }

        if !ranges.x.is_empty() || !ranges.y.is_empty() || !ranges.z.is_empty() {
            self.is_dirty = true;
        }

        self.chunk_anchor = new_anchor;
        self.window_offset = new_window_offset;
    }

    pub fn update_render_distance(&mut self, new_render_distance: u32) {
        todo!()
    }

    fn unload_chunk(&mut self, local_chunk_pos: Vector3<u32>) {
        let index = self.get_chunk_index(local_chunk_pos) as usize;
        let chunk_model = self.chunk_model_pointers[index];
        self.chunk_model_pointers[index] = VoxelModelId::null();
        if chunk_model != VoxelModelId::null() {
            self.to_unload_models.push(chunk_model);
        }
    }

    pub fn chunk_exists(&self, world_chunk_pos: Vector3<i32>) -> bool {
        let local_pos = world_chunk_pos - self.chunk_anchor;
        return self.get_chunk_model(local_pos.map(|x| x as u32)).is_some();
    }

    /// local_chunk_pos is local to self.chunk_anchor, with sliding window offset not taken into
    /// account.
    pub fn get_chunk_model(&self, local_chunk_pos: Vector3<u32>) -> Option<VoxelModelId> {
        let window_adjusted_pos = local_chunk_pos.zip_map(&self.window_offset, |x, y| {
            (x as u32 + y) % self.side_length
        });
        let index = self.get_chunk_index(window_adjusted_pos);
        let chunk_model_id = &self.chunk_model_pointers[index as usize];
        (!chunk_model_id.is_null() && !chunk_model_id.is_air()).then_some(*chunk_model_id)
    }

    pub fn get_chunk_index(&self, local_chunk_pos: Vector3<u32>) -> u32 {
        local_chunk_pos.x
            + local_chunk_pos.y * self.side_length
            + local_chunk_pos.z * self.side_length.pow(2)
    }
}

pub struct RenderableChunksGpu {
    pub terrain_acceleration_buffer: Option<ResourceId<Buffer>>,
    pub terrain_side_length: u32,
    pub terrain_anchor: Vector3<i32>,
    pub terrain_window_offset: Vector3<u32>,
}

impl RenderableChunksGpu {
    pub fn new() -> Self {
        Self {
            terrain_acceleration_buffer: None,
            terrain_side_length: 0,
            terrain_anchor: Vector3::new(0, 0, 0),
            terrain_window_offset: Vector3::new(0, 0, 0),
        }
    }

    pub fn update_gpu_objects(
        &mut self,
        device: &mut DeviceResource,
        renderable_chunks: &RenderableChunks,
    ) {
        let req_size = 4 * (renderable_chunks.side_length as u64).pow(3);
        if let Some(buffer) = self.terrain_acceleration_buffer {
            let buffer_info = device.get_buffer_info(&buffer);
            if buffer_info.size < req_size {
                //TODO delete previous buffer.
                self.terrain_acceleration_buffer =
                    Some(device.create_buffer(GfxBufferCreateInfo {
                        name: "world_terrain_acceleration_buffer".to_owned(),
                        size: req_size,
                    }));
            }
        } else {
            self.terrain_acceleration_buffer = Some(device.create_buffer(GfxBufferCreateInfo {
                name: "world_terrain_acceleration_buffer".to_owned(),
                size: req_size,
            }));
        }
    }

    pub fn write_render_data(
        &mut self,
        device: &mut DeviceResource,
        renderable_chunks: &RenderableChunks,
        voxel_model_info_map: &HashMap<VoxelModelId, VoxelWorldModelGpuInfo>,
        mut should_update: bool,
    ) {
        self.terrain_side_length = renderable_chunks.side_length;
        self.terrain_anchor = renderable_chunks.chunk_anchor;
        self.terrain_window_offset = renderable_chunks.window_offset;

        if renderable_chunks.is_dirty || should_update {
            // TODO: Copy incrementally with updates.
            let volume = renderable_chunks.side_length.pow(3) as usize;
            let mut buf = vec![0xFFFF_FFFFu32; volume];
            for i in 0..volume {
                let id = &renderable_chunks.chunk_model_pointers[i];
                if id.is_null() {
                    continue;
                }

                let Some(model_info) = voxel_model_info_map.get(id) else {
                    continue;
                };
                buf[i] = model_info.info_allocation.start_index_stride_dword() as u32;
            }

            device.write_buffer_slice(
                self.terrain_acceleration_buffer.as_ref().unwrap(),
                0,
                bytemuck::cast_slice::<u32, u8>(buf.as_slice()),
            );
        }
    }
}
