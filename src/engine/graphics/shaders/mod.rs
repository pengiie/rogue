pub mod ui {
    pub const SOURCE: &str = include_str!("./ui.wgsl");
}

pub mod blit {
    pub const SOURCE: &str = include_str!("./blit.wgsl");
}

pub mod voxel_trace {
    pub const SOURCE: &str = include_str!("./voxel_trace.wgsl");
    pub const WORKGROUP_SIZE: [u32; 3] = [8, 8, 1];
}
