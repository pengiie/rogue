pub mod voxel {
    use crate::engine::voxel::voxel::VoxelModelSchema;

    pub const TERRAIN_REGION_CHUNK_LENGTH: u32 = 2;
    pub const TERRAIN_REGION_TREE_HEIGHT: u32 = TERRAIN_REGION_CHUNK_LENGTH.trailing_zeros();

    // This MUST be a multiple of 4 to be best compatible with all voxel models
    pub const TERRAIN_CHUNK_METER_LENGTH: f32 = 16.0;
    pub const TERRAIN_CHUNK_VOXEL_LENGTH: u32 =
        (TERRAIN_CHUNK_METER_LENGTH * VOXELS_PER_METER as f32) as u32;
    pub const TERRAIN_CHUNK_VOXEL_VOLUME: u32 = TERRAIN_CHUNK_VOXEL_LENGTH.pow(3);

    pub const VOXELS_PER_METER: u32 = 16;
    pub const VOXEL_METER_LENGTH: f32 = 1.0 / VOXELS_PER_METER as f32;

    pub const MODEL_ESVO_SCHEMA: VoxelModelSchema = 1;
    pub const MODEL_FLAT_SCHEMA: VoxelModelSchema = 2;
    pub const MODEL_THC_SCHEMA: VoxelModelSchema = 3;

    pub mod attachment {
        use crate::engine::voxel::attachment::AttachmentId;

        pub const MAX_ID: AttachmentId = 2;
    }
}

pub mod gfx {
    /// The # of milliseconds that have to pass between attempts to invalidate pipelines.
    /// Pipeline invalidation is just checking if any shader files were modified, and invalidating
    /// the the entire pipeline cache.
    pub const PIPELINE_INVALIDATION_TIMER_MS: u32 = 250;

    pub const CAMERA_NEAR_PLANE: f32 = 0.01;
    pub const CAMERA_FAR_PLANE: f32 = 10_000.0;
}

pub mod io {
    pub const EDITOR_SETTINGS_FILE: &str = "editor::editor_settings::json";
    pub const SETTINGS_FILE: &str = "settings::json";
    pub const REGION_FILE_HEADER: &str = "vcr ";
}

pub mod actions {
    pub mod keybind {
        use crate::engine::input::keyboard::Key;

        pub const EDITOR_TOGGLE: Key = Key::E;
    }

    pub const EDITOR_TOGGLE: &str = "editor_toggle";
}

pub mod egui {
    pub mod icons {
        pub const FOLDER: &str = "icon_folder";
        pub const FOLDER_ASSET: &str = "ui::icons::folder::png";

        pub const LUA_FILE: &str = "icon_lua_file";
        pub const LUA_FILE_ASSET: &str = "ui::icons::lua_file::png";

        pub const TEXT_FILE: &str = "icon_text_file";
        pub const TEXT_FILE_ASSET: &str = "ui::icons::text_file::png";

        pub const VOXEL_MODEL_FILE: &str = "icon_voxel_model_file";
        pub const VOXEL_MODEL_FILE_ASSET: &str = "ui::icons::voxel_model_file::png";

        pub const UNKNOWN: &str = "icon_unknown";
        pub const UNKNOWN_ASSET: &str = "ui::icons::folder::png";
    }
}
