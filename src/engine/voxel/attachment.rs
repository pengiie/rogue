use nalgebra::Vector3;

use crate::common::color::{Color, ColorSpaceSrgb, ColorSpaceSrgbLinear, ColorSpaceXYZ};

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Attachment {
    name: &'static str,

    // Size in terms of u32s
    size: u32,
    renderable_index: u8,
}

impl Attachment {
    pub const PTMATERIAL_RENDER_INDEX: u8 = 0;
    pub const PTMATERIAL: Attachment = Attachment {
        name: "pathtracing_material",
        size: 1,
        renderable_index: Self::PTMATERIAL_RENDER_INDEX,
    };
    pub const NORMAL_RENDER_INDEX: u8 = 1;
    pub const NORMAL: Attachment = Attachment {
        name: "normal",
        size: 1,
        renderable_index: Self::NORMAL_RENDER_INDEX,
    };

    pub const EMMISIVE_RENDER_INDEX: u8 = 2;
    pub const EMMISIVE: Attachment = Attachment {
        name: "emmisive",
        size: 1,
        renderable_index: Self::EMMISIVE_RENDER_INDEX,
    };
    pub const MAX_RENDER_INDEX: u8 = 2;

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn renderable_index(&self) -> u8 {
        self.renderable_index
    }

    pub fn encode_ptmaterial(mat: &PTMaterial) -> u32 {
        mat.encode()
    }

    pub fn decode_ptmaterial(val: &u32) -> PTMaterial {
        PTMaterial::decode(*val)
    }

    pub fn encode_emmisive(candela: f32) -> u32 {
        candela.floor() as u32
    }

    /// Returns the candela of the emmisive material, the color is implied to be the diffuse color.
    pub fn decode_emissive(val: u32) -> f32 {
        val as f32
    }

    pub fn encode_normal(normal: Vector3<f32>) -> u32 {
        assert!(normal.norm() == 1.0);

        let mut x = 0u32;
        x |= (((normal.x * 0.5 + 0.5) * 255.0).ceil() as u32) << 16;
        x |= (((normal.y * 0.5 + 0.5) * 255.0).ceil() as u32) << 8;
        x |= ((normal.z * 0.5 + 0.5) * 255.0).ceil() as u32;

        x
    }

    pub fn decode_normal(normal: u32) -> Vector3<f32> {
        let x = (((normal >> 16) & 0xFF) as f32 / 255.0) * 2.0 - 1.0;
        let y = (((normal >> 8) & 0xFF) as f32 / 255.0) * 2.0 - 1.0;
        let z = ((normal & 0xFF) as f32 / 255.0) * 2.0 - 1.0;

        Vector3::new(x, y, z)
    }
}

/// A path tracing material that uses specific 2 bits to determine the material type.
pub enum PTMaterial {
    Diffuse { albedo: Color<ColorSpaceSrgbLinear> },
}

impl PTMaterial {
    pub fn diffuse(albedo: Color<ColorSpaceSrgbLinear>) -> Self {
        PTMaterial::Diffuse { albedo }
    }
    fn encode(&self) -> u32 {
        match self {
            PTMaterial::Diffuse { albedo } => {
                // Quantized values.
                let qr = (albedo.r() * 255.0).floor() as u32;
                let qg = (albedo.g() * 255.0).floor() as u32;
                let qb = (albedo.b() * 255.0).floor() as u32;

                (qr << 16) | (qg << 8) | qb
            }
        }
    }

    fn decode(val: u32) -> Self {
        let mat_ty = val >> 30;
        match mat_ty {
            0 => {
                let r = ((val >> 24) & 0xFF) as f32 / 255.0;
                let g = ((val >> 16) & 0xFF) as f32 / 255.0;
                let b = ((val >> 8) & 0xFF) as f32 / 255.0;

                PTMaterial::Diffuse {
                    albedo: Color::<ColorSpaceSrgbLinear>::new(r, g, b),
                }
            }
            _ => panic!("Encountered unsupported material type {}", mat_ty),
        }
    }
}

impl std::fmt::Debug for PTMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Diffuse { albedo } => {
                let gamma_corrected = albedo.into_color_space::<ColorSpaceSrgb>();
                f.debug_struct("Diffuse")
                    .field("albedo", &gamma_corrected)
                    .finish()
            }
        }
    }
}
