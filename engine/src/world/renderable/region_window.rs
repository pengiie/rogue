use nalgebra::Vector3;
use crate::graphics::backend::{Buffer, GfxBufferCreateInfo, GraphicsBackendDevice, ResourceId};
use crate::graphics::device::DeviceResource;
use crate::world::region_map::RegionPos;

// Flat array of chunks which acts as a sliding window as the player.
pub struct TerrainRenderableWindow {
    pub side_length: u32,
    pub gpu_region_ptrs: Vec<u32>,

    pub window_offset: Vector3<u32>,
    pub region_anchor: RegionPos,

    pub region_window_buffer: Option<ResourceId<Buffer>>,
    // Buffer updates for the window buffer.
    window_updates: Vec<(RegionPos, /*region_ptr*/ u32)>,
}

impl TerrainRenderableWindow {
    const NULL_REGION_PTR: u32 = 0xFFFF_FFFF;

    pub fn new(center_region: RegionPos, render_distance: u32) -> Self {
        let side_length = render_distance * 2 + 1;
        let volume = side_length.pow(3);
        Self {
            side_length,
            gpu_region_ptrs: vec![Self::NULL_REGION_PTR; volume as usize],
            window_offset: Vector3::zeros(),
            region_anchor: center_region.map(|x| x - render_distance as i32).into(),
            region_window_buffer: None,
            window_updates: Vec::new(),
        }
    }

    pub fn contains_region(&self, region_pos: RegionPos) -> bool {
        let rel_pos = region_pos - self.region_anchor;
        if rel_pos.x < 0
            || rel_pos.y < 0
            || rel_pos.z < 0
            || rel_pos.x >= self.side_length as i32
            || rel_pos.y >= self.side_length as i32
            || rel_pos.z >= self.side_length as i32
        {
            return false;
        }
        true
    }

    pub fn mem_pos_to_index(side_length: u32, local_pos: Vector3<u32>) -> usize {
        (local_pos.x + (local_pos.y * side_length) + (local_pos.z * side_length * side_length))
            as usize
    }

    pub fn local_pos_to_mem_pos(
        local_pos: Vector3<u32>,
        window_offset: Vector3<u32>,
        side_length: u32,
    ) -> Vector3<u32> {
        Vector3::new(
            (local_pos.x + window_offset.x) % side_length,
            (local_pos.y + window_offset.y) % side_length,
            (local_pos.z + window_offset.z) % side_length,
        )
    }

    pub fn set_region_ptr(&mut self, region_pos: RegionPos, region_ptr: u32) {
        if !self.contains_region(region_pos) {
            log::error!("Tried to set region ptr for region outside of window!");
            return;
        }
        let index = Self::mem_pos_to_index(
            self.side_length,
            (region_pos - self.region_anchor).map(|x| x as u32),
        );
        if self.gpu_region_ptrs[index] != region_ptr {
            self.gpu_region_ptrs[index] = region_ptr;
            self.window_updates.push((region_pos, region_ptr));
        }
    }

    pub fn update_gpu_objects(&mut self, device: &mut DeviceResource) {
        let req_buffer_size = (self.gpu_region_ptrs.len() * 4) as u64;
        let mut needs_resize = false;
        if let Some(region_window_buffer) = &self.region_window_buffer {
            let buffer_info = device.get_buffer_info(region_window_buffer);
            if buffer_info.size != req_buffer_size {
                needs_resize = true;
            }
        } else {
            needs_resize = true;
        }

        if needs_resize {
            if let Some(old_buffer) = &self.region_window_buffer {
                // TODO: Delete old buffer.
            }
            let new_buffer = device.create_buffer(GfxBufferCreateInfo {
                name: "terrain_region_window_buffer".to_string(),
                size: req_buffer_size,
            });
            device.write_buffer_slice(
                &new_buffer,
                0,
                bytemuck::cast_slice(self.gpu_region_ptrs.as_slice()),
            );
            self.region_window_buffer = Some(new_buffer);
        }
    }

    pub fn write_render_data(&mut self, device: &mut DeviceResource) {
        let Some(region_window_buffer) = &self.region_window_buffer else {
            return;
        };

        for (region_pos, new_ptr) in self.window_updates.drain(..) {
            let local_pos = (region_pos - self.region_anchor).map(|x| x as u32);
            let mem_pos =
                Self::local_pos_to_mem_pos(local_pos, self.window_offset, self.side_length);
            let index = Self::mem_pos_to_index(self.side_length, mem_pos) as u64;
            device.write_buffer_slice(region_window_buffer, index * 4, &new_ptr.to_le_bytes());
        }
    }
}