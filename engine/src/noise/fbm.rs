use crate::noise::Noise;

pub struct Fbm<T: Noise> {
    noise: T,
    options: FbmOptions,
}

pub struct FbmOptions {
    /// How much the frequency changes each octave.
    pub lacunarity: f32,
    pub octaves: u32,
    /// How much the amplitude changes each octave.
    pub gain: f32,
}

impl Default for FbmOptions {
    fn default() -> Self {
        Self {
            lacunarity: 2.0,
            octaves: 4,
            gain: 0.5,
        }
    }
}

impl<T: Noise> Fbm<T> {
    pub fn new(noise: T, options: FbmOptions) -> Self {
        Self { noise, options }
    }

    pub fn noise_2d(&self, x: f32, y: f32) -> f32 {
        let mut total = 0.0;
        let mut frequency = 1.0;
        let mut amplitude = 1.0;

        for _ in 0..self.options.octaves {
            total += self.noise.noise_2d(x * frequency, y * frequency) * amplitude;
            frequency *= self.options.lacunarity;
            amplitude *= self.options.gain;
        }

        total
    }

    pub fn noise_3d(&self, x: f32, y: f32, z: f32) -> f32 {
        let mut total = 0.0;
        let mut frequency = 1.0;
        let mut amplitude = 1.0;

        for _ in 0..self.options.octaves {
            total += self
                .noise
                .noise_3d(x * frequency, y * frequency, z * frequency)
                * amplitude;
            frequency *= self.options.lacunarity;
            amplitude *= self.options.gain;
        }

        total
    }
}
