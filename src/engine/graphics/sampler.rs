use std::collections::HashMap;

use rogue_macros::Resource;

use crate::common::id::create_id_type;

use super::device::DeviceResource;

create_id_type!(SamplerId);

#[derive(Resource)]
pub struct SamplerCache {
    samplers: Vec<wgpu::Sampler>,
    info_map: HashMap<SamplerInfo, SamplerId>,
}

impl SamplerCache {
    pub fn new() -> Self {
        Self {
            samplers: Vec::new(),
            info_map: HashMap::new(),
        }
    }

    pub fn get_lazy_id(&mut self, info: SamplerInfo, device: &DeviceResource) -> SamplerId {
        if let Some(id) = self.info_map.get(&info) {
            return *id;
        };

        let id = SamplerId(self.samplers.len() as u64);
        let sampler = device
            .device()
            .create_sampler(&info.into_wgpu_descriptor(&format!("sampler_{}", id)));
        self.samplers.push(sampler);
        self.info_map.insert(info, id);

        return id;
    }

    pub fn sampler(&self, id: SamplerId) -> &wgpu::Sampler {
        self.samplers
            .get(*id as usize)
            .expect(&format!("Sampler with id {} doesn't exist.", id))
    }
}

#[derive(Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct SamplerInfo {
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
    pub mipmap_filter: FilterMode,
    pub address_mode: AddressMode,
}

impl SamplerInfo {
    pub fn into_wgpu_descriptor<'a>(&self, label: &'a str) -> wgpu::SamplerDescriptor<'a> {
        let address_mode = self.address_mode.into();
        wgpu::SamplerDescriptor {
            label: Some(label),
            address_mode_u: address_mode,
            address_mode_v: address_mode,
            address_mode_w: address_mode,
            mag_filter: self.mag_filter.into(),
            min_filter: self.min_filter.into(),
            mipmap_filter: self.mipmap_filter.into(),
            // TODO: Support mipmapping.
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        }
    }
}

#[derive(Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum FilterMode {
    Nearest,
    Linear,
}

impl From<FilterMode> for wgpu::FilterMode {
    fn from(value: FilterMode) -> Self {
        match value {
            FilterMode::Nearest => Self::Nearest,
            FilterMode::Linear => Self::Linear,
        }
    }
}

impl From<egui::TextureFilter> for FilterMode {
    fn from(value: egui::TextureFilter) -> Self {
        match value {
            egui::TextureFilter::Nearest => Self::Nearest,
            egui::TextureFilter::Linear => Self::Linear,
        }
    }
}

#[derive(Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum AddressMode {
    ClampToEdge,
    Repeat,
    MirroredRepeat,
}

impl From<egui::TextureWrapMode> for AddressMode {
    fn from(value: egui::TextureWrapMode) -> Self {
        match value {
            egui::TextureWrapMode::ClampToEdge => Self::ClampToEdge,
            egui::TextureWrapMode::Repeat => Self::Repeat,
            egui::TextureWrapMode::MirroredRepeat => Self::MirroredRepeat,
        }
    }
}

impl From<AddressMode> for wgpu::AddressMode {
    fn from(value: AddressMode) -> Self {
        match value {
            AddressMode::ClampToEdge => wgpu::AddressMode::ClampToEdge,
            AddressMode::Repeat => wgpu::AddressMode::Repeat,
            AddressMode::MirroredRepeat => wgpu::AddressMode::MirrorRepeat,
        }
    }
}
