macro_rules! include_shader {
    ($e:expr) => {
        include_str!(concat!(
            concat!(env!("CARGO_MANIFEST_DIR"), "/assets/shaders/"),
            $e
        ))
    };
}

pub mod ui {
    pub const SOURCE: &str = include_shader!("ui.wgsl");
}

pub mod blit {
    pub const SOURCE: &str = include_shader!("blit.wgsl");
}

pub mod voxel_trace {
    pub const SOURCE: &str = include_shader!("voxel_trace.wgsl");
    pub const WORKGROUP_SIZE: [u32; 3] = [8, 8, 1];
}
