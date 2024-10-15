use nalgebra::{ComplexField, Matrix3, Vector3};

#[derive(Copy)]
pub struct Color<S: ColorSpace = ColorSpaceSrgb> {
    xyz: Vector3<f32>,
    _marker: std::marker::PhantomData<S>,
}

impl<S: ColorSpace> Color<S> {
    pub fn new(r: f32, g: f32, b: f32) -> Self {
        Self {
            xyz: Vector3::new(r, g, b),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn into_color_space<N: ColorSpace>(&self) -> Color<N>
    where
        N: ColorSpaceTransitionFrom<S>,
    {
        Color {
            xyz: N::transition(self.xyz),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn r(&self) -> f32 {
        self.xyz.x
    }

    pub fn g(&self) -> f32 {
        self.xyz.y
    }

    pub fn b(&self) -> f32 {
        self.xyz.z
    }

    pub fn rgb_vec(&self) -> Vector3<f32> {
        self.xyz
    }
}

impl Color<ColorSpaceSrgb> {
    pub fn new_srgb(r: f32, g: f32, b: f32) -> Self {
        Self::new(r, g, b)
    }
}

impl<T: ColorSpace> std::fmt::Debug for Color<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Color").field("xyz", &self.xyz).finish()
    }
}

impl<T: ColorSpace> Clone for Color<T> {
    fn clone(&self) -> Self {
        Self {
            xyz: self.xyz.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

pub trait ColorSpace {}
pub trait ColorSpaceTransitionFrom<From: ColorSpace> {
    fn transition(xyz: Vector3<f32>) -> Vector3<f32>;
}

/// The CIE 1931 XYZ color space with a D65 standard illuminant.
pub struct ColorSpaceXYZ;
impl ColorSpace for ColorSpaceXYZ {}

pub struct ColorSpaceSrgb;
impl ColorSpace for ColorSpaceSrgb {}
impl ColorSpaceTransitionFrom<ColorSpaceSrgbLinear> for ColorSpaceSrgb {
    fn transition(xyz: Vector3<f32>) -> Vector3<f32> {
        // Gamma correction.
        // Source: https://en.wikipedia.org/wiki/SRGB#From_CIE_XYZ_to_sRGB
        xyz.map(|x| {
            if x <= 0.0031308 {
                12.92 * x
            } else {
                1.055 * x.powf(1.0 / 2.4) - 0.055
            }
        })
    }
}

impl ColorSpaceTransitionFrom<ColorSpaceSrgb> for ColorSpaceSrgbLinear {
    fn transition(xyz: Vector3<f32>) -> Vector3<f32> {
        // Linearize.
        // Source: https://en.wikipedia.org/wiki/SRGB#From_sRGB_to_CIE_XYZ
        xyz.map(|x| {
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        })
    }
}

impl ColorSpaceTransitionFrom<ColorSpaceXYZ> for ColorSpaceSrgb {
    fn transition(xyz: Vector3<f32>) -> Vector3<f32> {
        <ColorSpaceSrgb as ColorSpaceTransitionFrom<ColorSpaceSrgbLinear>>::transition(
            <ColorSpaceSrgbLinear as ColorSpaceTransitionFrom<ColorSpaceXYZ>>::transition(xyz),
        )
    }
}

pub struct ColorSpaceSrgbLinear;
impl ColorSpace for ColorSpaceSrgbLinear {}
impl ColorSpaceTransitionFrom<ColorSpaceXYZ> for ColorSpaceSrgbLinear {
    #[rustfmt::skip]
    fn transition(xyz: Vector3<f32>) -> Vector3<f32>  {
        // Linear transformation matrix from CIE 1931 XYZ to Linear Srgb.
        // Source: https://en.wikipedia.org/wiki/SRGB#From_CIE_XYZ_to_sRGB
        let m = Matrix3::new(
            3.2406, -1.5372, -0.4986,
            -0.9689, 1.8758, 0.0415,
            0.0557, 0.2040, 1.0570,
        );

        m * xyz
    }
}
