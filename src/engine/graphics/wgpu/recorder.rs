use crate::{
    common::color::{Color, ColorSpaceSrgb},
    engine::graphics::backend::{GraphicsBackendRecorder, Image, ResourceId},
};

pub struct WgpuRecorder {}

impl GraphicsBackendRecorder for WgpuRecorder {
    fn clear_color(&mut self, image: ResourceId<Image>, color: Color<ColorSpaceSrgb>) {
        todo!()
    }

    fn blit(&mut self, src: ResourceId<Image>, dst: ResourceId<Image>) {
        todo!()
    }
}
