use nalgebra::{ComplexField, Matrix3, Vector3};

#[derive(Copy, PartialEq)]
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
        S: ColorSpaceTransitionInto<N>,
    {
        Color {
            xyz: S::transition(self.xyz),
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

    pub fn black() -> Self {
        Self {
            xyz: Vector3::new(0.0, 0.0, 0.0),
            _marker: std::marker::PhantomData,
        }
    }
}

impl From<nalgebra::Vector3<f32>> for Color<ColorSpaceSrgb> {
    fn from(vec: nalgebra::Vector3<f32>) -> Self {
        Self::new_srgb(vec.x, vec.y, vec.z)
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
impl<S> ColorSpaceTransitionFrom<S> for S
where
    S: ColorSpace,
{
    fn transition(xyz: Vector3<f32>) -> Vector3<f32> {
        xyz
    }
}

pub trait ColorSpaceTransitionInto<Into: ColorSpace> {
    fn transition(xyz: Vector3<f32>) -> Vector3<f32>;
}
impl<C, T> ColorSpaceTransitionInto<T> for C
where
    T: ColorSpace + ColorSpaceTransitionFrom<C>,
    C: ColorSpace,
{
    fn transition(xyz: Vector3<f32>) -> Vector3<f32> {
        T::transition(xyz)
    }
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
impl ColorSpaceTransitionFrom<ColorSpaceSrgb> for ColorSpaceXYZ {
    fn transition(xyz: Vector3<f32>) -> Vector3<f32> {
        <ColorSpaceXYZ as ColorSpaceTransitionFrom<ColorSpaceSrgbLinear>>::transition(
            <ColorSpaceSrgbLinear as ColorSpaceTransitionFrom<ColorSpaceSrgb>>::transition(xyz),
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
impl ColorSpaceTransitionFrom<ColorSpaceSrgbLinear> for ColorSpaceXYZ {
    #[rustfmt::skip]
    fn transition(xyz: Vector3<f32>) -> Vector3<f32>  {
        // Linear transformation matrix from Linear Srgb to CIE 1931 XYZ.
        // Source: https://en.wikipedia.org/wiki/SRGB#From_sRGB_to_CIE_XYZ
        let m = Matrix3::new(
            0.4124, 0.3576, 0.1805,
            0.2126, 0.7152, 0.0722,
            0.0193, 0.1192, 0.9505,
        );

        m * xyz
    }
}

mod tests {
    use nalgebra::Vector3;

    use crate::common::color::{ColorSpaceSrgb, ColorSpaceXYZ};

    use super::Color;

    #[test]
    fn colorspace_to_and_from() {
        let color = Color::<ColorSpaceSrgb>::new(0.5, 0.5, 0.5);
        assert_eq!(
            color
                .into_color_space::<ColorSpaceXYZ>()
                .into_color_space::<ColorSpaceSrgb>()
                .xyz
                .map(|x| x.to_bits()),
            Vector3::new(0.5, 0.5, 0.5).map(|x: f32| x.to_bits())
        );
    }
}
