pub struct Camera {
    projection_matrix: glam::f32::Mat4,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            projection_matrix: glam::f32::Mat4::IDENTITY,
        }
    }
}
