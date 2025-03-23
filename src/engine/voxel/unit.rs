use core::panic;

use super::{
    attachment::{Attachment, AttachmentMap},
    voxel::{VoxelData, VoxelModelImpl},
};

/// Voxel model which is a singular voxel
pub struct VoxelModelUnit {
    data: Option<VoxelData>,
    attachment_map: AttachmentMap,
}

impl VoxelModelUnit {
    pub fn new(data: Option<VoxelData>, attachment_map: AttachmentMap) -> Self {
        Self {
            data,
            attachment_map,
        }
    }

    /// Assumes the data attachments follow the default attachments defined in `voxel::attachment::Attachment`.
    pub fn with_data(data: VoxelData) -> Self {
        let mut map = AttachmentMap::new();
        for id in data.attachment_ids() {
            let attachment = match id {
                0 => &Attachment::PTMATERIAL,
                1 => &Attachment::NORMAL,
                2 => &Attachment::EMMISIVE,
                _ => panic!("Unsupported attachment id"),
            };
            map.register_attachment(attachment);
        }

        Self::new(Some(data), map)
    }

    pub fn data(&self) -> Option<&VoxelData> {
        self.data.as_ref()
    }

    pub fn attachment_map(&self) -> &AttachmentMap {
        &self.attachment_map
    }
}
