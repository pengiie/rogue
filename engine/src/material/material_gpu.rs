use std::collections::{HashMap, HashSet};

use nalgebra::Vector2;
use rogue_macros::Resource;

use crate::common::freelist::{FreeList, FreeListHandle};
use crate::asset::{
    asset::{AssetHandle, AssetPath, AssetStatus, Assets, GameAssetPath},
    repr::image::ImageAsset,
};
use crate::event::{EventReader, Events};
use crate::graphics::{
    backend::{
        Buffer, GfxAddressMode, GfxBufferCreateInfo, GfxFilterMode, GfxImageCreateInfo,
        GfxImageFormat, GfxImageType, GfxImageWrite, GfxSamplerCreateInfo, Image,
        ResourceId, Sampler,
    },
    device::DeviceResource,
};
use crate::material::{
    MaterialBank, MaterialCreateEvent, MaterialId, MaterialSamplerOptions,
    MaterialTextureType, MaterialUpdateEvent,
};
use crate::resource::{Res, ResMut};

struct MaterialDescriptor {
    material_id: MaterialId,
    color_texture_index: Option<MaterialGpuTextureId>,
    color_sampler_index: Option<MaterialGpuSamplerId>,
}

type MaterialGpuTextureId = FreeListHandle<Option<ResourceId<Image>>>;
type MaterialGpuSamplerId = FreeListHandle<ResourceId<Sampler>>;
type MaterialDescriptorId = FreeListHandle<MaterialDescriptor>;

struct LoadingTexture {
    asset_handle: AssetHandle,
    texture_index: MaterialGpuTextureId,
}

#[derive(Resource)]
pub struct MaterialBankGpu {
    material_textures: FreeList<Option<ResourceId<Image>>>,
    asset_to_texture_map: HashMap<GameAssetPath, MaterialGpuTextureId>,
    loading_textures: HashMap<GameAssetPath, LoadingTexture>,

    material_samplers: FreeList<ResourceId<Sampler>>,
    options_to_sampler_map: HashMap<MaterialSamplerOptions, MaterialGpuSamplerId>,

    // Buffer and free list match in terms of indices.
    material_descriptors: FreeList<MaterialDescriptor>,
    material_descriptor_buffer: Option<ResourceId<Buffer>>,
    /// Materials who should have their descriptor written in the gpu buffer.
    /// Happens when a material is first registered or its texture/sampler changes.
    loading_materials: HashSet<MaterialId>,
    material_map: HashMap<MaterialId, FreeListHandle<MaterialDescriptor>>,

    material_create_event_reader: EventReader<MaterialCreateEvent>,
    material_update_event_reader: EventReader<MaterialUpdateEvent>,
}

impl MaterialBankGpu {
    pub fn new() -> Self {
        Self {
            material_textures: FreeList::new(),
            asset_to_texture_map: HashMap::new(),
            loading_textures: HashMap::new(),

            material_samplers: FreeList::new(),
            options_to_sampler_map: HashMap::new(),

            material_descriptors: FreeList::new(),
            material_descriptor_buffer: None,
            loading_materials: HashSet::new(),

            material_map: HashMap::new(),

            material_create_event_reader: EventReader::new(),
            material_update_event_reader: EventReader::new(),
        }
    }

    fn get_or_create_sampler_static(
        options_to_sampler_map: &mut HashMap<MaterialSamplerOptions, MaterialGpuSamplerId>,
        material_samplers: &mut FreeList<ResourceId<Sampler>>,
        device: &mut DeviceResource,
        options: &MaterialSamplerOptions,
    ) -> MaterialGpuSamplerId {
        if let Some(sampler_id) = options_to_sampler_map.get(options) {
            return *sampler_id;
        }

        // Create new sampler.
        let sampler = device.create_sampler(GfxSamplerCreateInfo {
            mag_filter: GfxFilterMode::Linear,
            min_filter: GfxFilterMode::Linear,
            mipmap_filter: GfxFilterMode::Linear,
            address_mode: GfxAddressMode::Repeat,
        });
        let sampler_index = material_samplers.push(sampler);
        options_to_sampler_map.insert(options.clone(), sampler_index);
        sampler_index
    }

    pub fn get_or_load_texture_static(
        asset_to_texture_map: &mut HashMap<GameAssetPath, MaterialGpuTextureId>,
        loading_textures: &mut HashMap<GameAssetPath, LoadingTexture>,
        material_textures: &mut FreeList<Option<ResourceId<Image>>>,
        material_bank: &MaterialBank,
        device: &mut DeviceResource,
        assets: &mut Assets,
        asset_path: &GameAssetPath,
        material_desc_id: MaterialDescriptorId,
    ) -> MaterialGpuTextureId {
        if let Some(texture_id) = asset_to_texture_map.get(asset_path) {
            // Queue up this material to be registered when the texture finishes loading.
            return *texture_id;
        }

        let texture_index = material_textures.push(None);
        asset_to_texture_map.insert(asset_path.clone(), texture_index);

        // Not loaded yet, start loading.
        let image_asset_path =
            asset_path.as_file_asset_path(assets.project_assets_dir().as_ref().unwrap());
        let image_asset_handle = assets.load_asset::<ImageAsset>(image_asset_path);
        let loading_texture = LoadingTexture {
            asset_handle: image_asset_handle,
            texture_index,
        };
        loading_textures.insert(asset_path.clone(), loading_texture);

        texture_index
    }

    pub fn write_render_data(
        material_bank: Res<MaterialBank>,
        mut material_bank_gpu: ResMut<MaterialBankGpu>,
        mut device: ResMut<DeviceResource>,
        mut assets: ResMut<Assets>,
        events: Res<Events>,
    ) {
        let material_bank_gpu = &mut material_bank_gpu as &mut MaterialBankGpu;

        // Update the status of any currently loading textures.
        let mut finished_textures = Vec::new();
        for (loading_path, loading_texture) in &material_bank_gpu.loading_textures {
            match assets.get_asset_status(&loading_texture.asset_handle) {
                AssetStatus::InProgress => {}
                AssetStatus::Saved => unreachable!(),
                AssetStatus::Loaded => {
                    let image_asset = assets
                        .get_asset::<ImageAsset>(&loading_texture.asset_handle)
                        .expect("Texture asset should be loaded by now.");
                    let image_data = image_asset.convert_to_rgba();
                    let gpu_image = device.create_image(GfxImageCreateInfo {
                        name: format!("texture_{}", &loading_path.asset_path),
                        image_type: GfxImageType::D2,
                        format: GfxImageFormat::Rgba8Unorm,
                        extent: image_asset.size,
                    });
                    device.write_image(GfxImageWrite {
                        image: gpu_image,
                        data: &image_data,
                        offset: Vector2::new(0, 0),
                        extent: image_asset.size,
                    });
                    log::info!("Loaded material texture {:?}", loading_path);

                    // Update texture in gpu bank.
                    let texture_index = loading_texture.texture_index;
                    let mut texture = material_bank_gpu
                        .material_textures
                        .get_mut(texture_index)
                        .unwrap();
                    *texture = Some(gpu_image.clone());
                    log::info!(
                        "Registered texture for material gpu at index {:?}",
                        texture_index
                    );
                    log::info!(
                        "textures is now {:?}",
                        material_bank_gpu
                            .material_textures
                            .iter_all()
                            .collect::<Vec<_>>()
                    );

                    finished_textures.push(loading_path.clone());
                }
                AssetStatus::NotFound => {
                    log::error!("Failed to find texture asset at path {:?}", loading_path,);
                    finished_textures.push(loading_path.clone());
                }
                AssetStatus::Error(error) => {
                    log::error!(
                        "Failed while loading texture at path {:?}. Error: {}",
                        loading_path,
                        error
                    );
                    finished_textures.push(loading_path.clone());
                }
            }
        }
        for finished_path in finished_textures {
            material_bank_gpu.loading_textures.remove(&finished_path);
        }

        // Load and register any new project materials for the gpu representation
        for event in material_bank_gpu.material_create_event_reader.read(&events) {
            let Some(material) = material_bank.materials.get(event.material_id) else {
                // Material no longer exists in the material bank.
                continue;
            };
            if material_bank_gpu
                .material_map
                .contains_key(&event.material_id)
            {
                panic!(
                    "Got creation event for material id {:?} twice which shouldn't happen.",
                    event.material_id
                );
            }

            // Register the gpu material.
            let material_descriptor_id = material_bank_gpu.material_descriptors.next_free_handle();
            let color_texture_id = material.color_texture.as_ref().map(|color_texture_path| {
                Self::get_or_load_texture_static(
                    &mut material_bank_gpu.asset_to_texture_map,
                    &mut material_bank_gpu.loading_textures,
                    &mut material_bank_gpu.material_textures,
                    &material_bank,
                    &mut device,
                    &mut assets,
                    color_texture_path,
                    material_descriptor_id,
                )
            });
            let sampler_id = (material.color_texture.is_some()).then(|| {
                Self::get_or_create_sampler_static(
                    &mut material_bank_gpu.options_to_sampler_map,
                    &mut material_bank_gpu.material_samplers,
                    &mut device,
                    &MaterialSamplerOptions {},
                )
            });

            // Create descriptor.
            let descriptor = MaterialDescriptor {
                material_id: event.material_id,
                color_texture_index: color_texture_id,
                color_sampler_index: sampler_id,
            };
            assert_eq!(
                material_descriptor_id,
                material_bank_gpu.material_descriptors.push(descriptor)
            );
            material_bank_gpu
                .material_map
                .insert(event.material_id, material_descriptor_id);
            // Mark for gpu update when textures are ready.
            material_bank_gpu
                .loading_materials
                .insert(event.material_id);
        }

        // Update any existing material with changed textures.
        for event in material_bank_gpu.material_update_event_reader.read(&events) {
            let Some(material) = material_bank.materials.get(event.material_id) else {
                // Material no longer exists in the material bank.
                continue;
            };
            let Some(material_descriptor_id) =
                material_bank_gpu.material_map.get(&event.material_id)
            else {
                panic!("GPU side material descriptor should exist by this materials registration event.");
            };

            let material_descriptor = material_bank_gpu
                .material_descriptors
                .get_mut(*material_descriptor_id)
                .expect(
                    "Material descriptor should exist if it is referenced in the material map.",
                );

            match event.updated_texture_type {
                MaterialTextureType::Color => {
                    let new_color_texture_path = material.color_texture.as_ref();
                    log::info!(
                        "Material id {:?} color texture updated to {:?}",
                        event.material_id,
                        new_color_texture_path
                    );

                    // Get or load new texture.
                    let color_texture_id =
                        new_color_texture_path.as_ref().map(|color_texture_path| {
                            Self::get_or_load_texture_static(
                                &mut material_bank_gpu.asset_to_texture_map,
                                &mut material_bank_gpu.loading_textures,
                                &mut material_bank_gpu.material_textures,
                                &material_bank,
                                &mut device,
                                &mut assets,
                                color_texture_path,
                                *material_descriptor_id,
                            )
                        });
                    let sampler_id = (new_color_texture_path.is_some()).then(|| {
                        Self::get_or_create_sampler_static(
                            &mut material_bank_gpu.options_to_sampler_map,
                            &mut material_bank_gpu.material_samplers,
                            &mut device,
                            &MaterialSamplerOptions {},
                        )
                    });

                    if material_descriptor.color_texture_index != color_texture_id
                        || material_descriptor.color_sampler_index != sampler_id
                    {
                        // TODO: Worry about deleting possible old textures or marking
                        // for garbage collection.
                        material_descriptor.color_texture_index = color_texture_id;
                        material_descriptor.color_sampler_index = sampler_id;

                        // Mark for gpu update when textures are ready.
                        material_bank_gpu
                            .loading_materials
                            .insert(event.material_id);
                    }
                }
            }
        }

        // Check all the loading gpu materials to see if they are done.
        let mut finished_materials = Vec::new();
        for material in material_bank_gpu.loading_materials.iter() {
            let Some(material_descriptor_id) = material_bank_gpu.material_map.get(material) else {
                // Material no longer exists in the material bank gpu.
                finished_materials.push(*material);
                continue;
            };

            let Some(material_descriptor) = material_bank_gpu
                .material_descriptors
                .get(*material_descriptor_id)
            else {
                // Material no longer exists in the material bank gpu.
                finished_materials.push(*material);
                panic!(
                    "Reference to material descriptor shouldn't exist if the descriptor doesn't."
                );
            };

            // Check if the texture is loaded.
            let textures = [material_descriptor.color_texture_index];
            let all_textures_loaded = textures.iter().all(|texture_index_opt| {
                texture_index_opt
                    .map(|color_texture_index| {
                        material_bank_gpu
                            .material_textures
                            .get(color_texture_index)
                            .expect("Texture index should exist if descriptor exists.")
                            .is_some()
                    })
                    .unwrap_or(true)
            });

            if all_textures_loaded {
                // Material is done loading.
                finished_materials.push(*material);
            }
        }

        let mut to_write_material_descriptors = Vec::new();

        // Remove finished materials from the loading set and use later for gpu data writing.
        for finished_material in &finished_materials {
            // Register onto the gpu if the material is done loading from some texture update.
            material_bank_gpu
                .loading_materials
                .remove(finished_material);
            let material_descriptor_id = material_bank_gpu
                .material_map
                .get(finished_material)
                .expect("If we just finished loading the material this frame we should ");
            to_write_material_descriptors.push(*material_descriptor_id);
        }

        // Ensure the material descriptor buffer is allocated and has capacity.
        // TODO: Query struct size via Shader resource reflection and make writer for offsets
        // easier by querying by field names.
        const MATERIAL_DESCRIPTOR_SHADER_SIZE: u64 = /*4 bytes for uint*/ 4 * /*2 uints*/ 2;
        let required_buffer_size = (material_bank_gpu.material_descriptors.len()).max(1) as u64
            * MATERIAL_DESCRIPTOR_SHADER_SIZE;

        const MATERIAL_DESCRIPTOR_BUFFER_NAME: &str = "material_descriptor_buffer";
        if let Some(existing_buffer) = &mut material_bank_gpu.material_descriptor_buffer {
            let buffer_info = device.get_buffer_info(existing_buffer);
            if buffer_info.size < required_buffer_size {
                *existing_buffer = device.create_buffer(GfxBufferCreateInfo {
                    name: MATERIAL_DESCRIPTOR_BUFFER_NAME.to_owned(),
                    size: required_buffer_size,
                });
            }
        } else {
            material_bank_gpu.material_descriptor_buffer =
                Some(device.create_buffer(GfxBufferCreateInfo {
                    name: MATERIAL_DESCRIPTOR_BUFFER_NAME.to_owned(),
                    size: required_buffer_size,
                }));

            // Write null for all material indices so when loading the shader knows the textures
            // don't exist.
        }
        let descriptor_buffer = material_bank_gpu.material_descriptor_buffer.unwrap();

        // Register any updated materials from texture loading onto the gpu since we now know
        // their new texture resource handles.
        for descriptor_id in to_write_material_descriptors.iter() {
            let descriptor = material_bank_gpu
                .material_descriptors
                .get(*descriptor_id)
                .expect("Material descriptor should exist if we are writing it.");

            let mut buf = [u32::MAX; 2];
            if let Some(texture_index) = descriptor.color_texture_index {
                assert!(
                    material_bank_gpu.material_textures.has_value(texture_index),
                    "Descriptor is referencing a texture index that doesn't exist."
                );
                buf[0] = texture_index.index() as u32;
            }
            if let Some(sampler_index) = descriptor.color_sampler_index {
                buf[1] = sampler_index.index() as u32;
            }

            let offset = descriptor_id.index() as u64 * MATERIAL_DESCRIPTOR_SHADER_SIZE;
            log::info!(
                "Writing material descriptor for material id {:?}: texture index {:?}, sampler index {:?} at offset {}",
                descriptor.material_id,
                buf[0],
                buf[1],
                offset
            );
            device.write_buffer_slice(&descriptor_buffer, offset, bytemuck::cast_slice(&buf));
        }
    }

    pub fn get_textures(&self) -> Vec<Option<ResourceId<Image>>> {
        self.material_textures
            .iter_all()
            .map(|tex| tex.map(|id| id.clone()).flatten())
            .collect::<Vec<_>>()
    }

    pub fn get_samplers(&self) -> Vec<Option<ResourceId<Sampler>>> {
        self.material_samplers
            .iter_all()
            .map(|tex| tex.map(|id| id.clone()))
            .collect::<Vec<_>>()
    }

    pub fn get_descriptor_buffer(&self) -> ResourceId<Buffer> {
        self.material_descriptor_buffer
            .clone()
            .expect("Material descriptor buffer should exist by now.")
    }
}
