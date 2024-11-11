use std::collections::HashMap;

use nalgebra::Vector3;

use crate::common::color::{Color, ColorSpaceSrgb, ColorSpaceSrgbLinear, ColorSpaceXYZ};

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Attachment {
    name: &'static str,

    // Size in terms of u32s, due to that being the buffer array stride.
    size: u32,
    id: u8,
}

impl Attachment {
    pub const PTMATERIAL_ID: AttachmentId = 0;
    pub const NORMAL_ID: AttachmentId = 1;
    pub const EMMISIVE_ID: AttachmentId = 2;
    pub const MAX_ATTACHMENT_ID: AttachmentId = 2;

    pub const PTMATERIAL: Attachment =
        Attachment::new(Attachment::PTMATERIAL_ID, "pathtracing_material", 1);
    pub const NORMAL: Attachment = Attachment::new(Attachment::NORMAL_ID, "normal", 1);
    pub const EMMISIVE: Attachment = Attachment::new(Attachment::EMMISIVE_ID, "emmisive", 1);

    const fn new(id: AttachmentId, name: &'static str, size: u32) -> Self {
        Attachment { name, size, id }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn id(&self) -> AttachmentId {
        self.id
    }

    pub fn encode_ptmaterial(mat: &PTMaterial) -> u32 {
        mat.encode()
    }

    pub fn decode_ptmaterial(val: &u32) -> PTMaterial {
        PTMaterial::decode(*val)
    }

    pub fn encode_emmisive(candela: u32) -> u32 {
        candela
    }

    /// Returns the candela of the emmisive material, the color is implied to be the diffuse color.
    pub fn decode_emissive(val: u32) -> u32 {
        val
    }

    pub fn encode_normal(normal: &Vector3<f32>) -> u32 {
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

pub type AttachmentId = u8;

/// A path tracing material that uses specific 2 bits to determine the material type.
pub enum PTMaterial {
    Diffuse { albedo: Color<ColorSpaceSrgb> },
}

impl PTMaterial {
    pub fn diffuse(albedo: Color<ColorSpaceSrgb>) -> Self {
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
                    albedo: Color::<ColorSpaceSrgb>::new(r, g, b),
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
                let srgb = albedo;
                f.debug_struct("Diffuse").field("albedo", &srgb).finish()
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AttachmentMap {
    map: HashMap<AttachmentId, Attachment>,
}

impl AttachmentMap {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn register_attachment(&mut self, attachment: &Attachment) {
        if let Some(old) = self.map.insert(attachment.id(), attachment.clone()) {
            // This shouldn't be a performance issue since attachment map inheritance or
            // construction is rare. If this is in a hot loop then that is an upstream design
            // issue.
            assert_eq!(
                old.name,
                attachment.name(),
                "Overriding existing attachment with different name but the same id"
            );
        }
    }

    pub fn get_attachment(&self, id: AttachmentId) -> &Attachment {
        self.map.get(&id).expect(&format!(
            "Attachment with id {} doesn't exist in the attachment map.",
            id
        ))
    }

    pub fn contains(&self, attachment_id: AttachmentId) -> bool {
        self.map.contains_key(&attachment_id)
    }

    pub fn inherit_other(&mut self, other: &AttachmentMap) {
        for (_, attachment) in other.iter() {
            self.register_attachment(attachment);
        }
    }

    pub fn name(&self, id: AttachmentId) -> &str {
        self.get_attachment(id).name()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u8, &Attachment)> {
        self.map.iter()
    }
}
