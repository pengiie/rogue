use rand::{seq::SliceRandom, SeedableRng};

use crate::noise::Noise;

pub struct PerlinNoise {
    permutations: [u8; 512],
}

impl PerlinNoise {
    pub fn new(seed: u64) -> Self {
        let mut permutations = [0u8; 512];

        for i in (1..256).rev() {
            permutations[i] = i as u8;
        }
        permutations.shuffle(&mut rand::rngs::StdRng::seed_from_u64(seed));

        for i in 0..256 {
            permutations[i + 256] = permutations[i];
        }

        Self { permutations }
    }

    pub fn noise_2d(&self, x: f32, y: f32) -> f32 {
        let xi = (x.floor() as i32 % 255) as u8;
        let yi = (y.floor() as i32 % 255) as u8;

        let xf = x - x.floor();
        let yf = y - y.floor();

        let p = &self.permutations;
        let get_p = |i: u8, j: u8| -> u8 { p[p[i as usize] as usize + j as usize] };
        let aa = get_p(xi, yi);
        let ab = get_p(xi, yi.wrapping_add(1));
        let ba = get_p(xi.wrapping_add(1), yi);
        let bb = get_p(xi.wrapping_add(1), yi.wrapping_add(1));

        let daa = Self::grad(aa, xf, yf, 0.0);
        let dab = Self::grad(ab, xf, yf - 1.0, 0.0);
        let dba = Self::grad(ba, xf - 1.0, yf, 0.0);
        let dbb = Self::grad(bb, xf - 1.0, yf - 1.0, 0.0);

        let u = Self::fade(xf);
        let v = Self::fade(yf);

        let x1 = Self::lerp(daa, dba, u);
        let x2 = Self::lerp(dab, dbb, u);
        let result = Self::lerp(x1, x2, v);
        result
    }

    pub fn noise_2d_simd(&self, x: wide::f32x8, y: f32) -> wide::f32x8 {
        todo!()
    }

    pub fn noise_3d(&self, x: f32, y: f32, z: f32) -> f32 {
        let xi = (x.floor() as i32 % 255) as u8;
        let yi = (y.floor() as i32 % 255) as u8;
        let zi = (z.floor() as i32 % 255) as u8;

        let xf = x - x.floor();
        let yf = y - y.floor();
        let zf = z - z.floor();

        let p = &self.permutations;
        let get_p = |i: u8, j: u8, k: u8| -> u8 {
            p[p[p[i as usize] as usize + j as usize] as usize + k as usize]
        };
        let aaa = get_p(xi, yi, zi);
        let aba = get_p(xi, yi.wrapping_add(1), zi);
        let aab = get_p(xi, yi, zi.wrapping_add(1));
        let abb = get_p(xi, yi.wrapping_add(1), zi.wrapping_add(1));
        let baa = get_p(xi.wrapping_add(1), yi, zi);
        let bba = get_p(xi.wrapping_add(1), yi.wrapping_add(1), zi);
        let bab = get_p(xi.wrapping_add(1), yi, zi.wrapping_add(1));
        let bbb = get_p(xi.wrapping_add(1), yi.wrapping_add(1), zi.wrapping_add(1));

        let daaa = Self::grad(aaa, xf, yf, zf);
        let daba = Self::grad(aba, xf, yf - 1.0, zf);
        let daab = Self::grad(aab, xf, yf, zf - 1.0);
        let dabb = Self::grad(abb, xf, yf - 1.0, zf - 1.0);
        let dbaa = Self::grad(baa, xf - 1.0, yf, zf);
        let dbba = Self::grad(bba, xf - 1.0, yf - 1.0, zf);
        let dbab = Self::grad(bab, xf - 1.0, yf, zf - 1.0);
        let dbbb = Self::grad(bbb, xf - 1.0, yf - 1.0, zf - 1.0);

        // Interpolation weights
        let u = Self::fade(xf);
        let v = Self::fade(yf);
        let w = Self::fade(zf);

        // Trilinear interpolation
        let x1 = Self::lerp(daaa, dbaa, u);
        let x2 = Self::lerp(daba, dbba, u);
        let x3 = Self::lerp(daab, dbab, u);
        let x4 = Self::lerp(dabb, dbbb, u);
        let y1 = Self::lerp(x1, x2, v);
        let y2 = Self::lerp(x3, x4, v);
        let result = Self::lerp(y1, y2, w);
        result
    }

    pub fn fade(t: f32) -> f32 {
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    fn grad(hash: u8, x: f32, y: f32, z: f32) -> f32 {
        let h = hash & 15;
        let u = if h < 8 { x } else { y };
        let v = if h < 4 {
            y
        } else if h == 12 || h == 14 {
            x
        } else {
            z
        };

        let a = if (h & 1) == 0 { u } else { -u };
        let b = if (h & 2) == 0 { v } else { -v };
        a + b
    }

    fn lerp(a: f32, b: f32, t: f32) -> f32 {
        a * (1.0 - t) + t * b
    }
}

impl Noise for PerlinNoise {
    fn noise_2d(&self, x: f32, y: f32) -> f32 {
        Self::noise_2d(self, x, y)
    }

    fn noise_3d(&self, x: f32, y: f32, z: f32) -> f32 {
        Self::noise_3d(self, x, y, z)
    }
}
