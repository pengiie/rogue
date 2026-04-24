use crate::asset::asset::AssetLoader;

pub struct AudioAsset {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
}

impl AudioAsset {
    pub const SAMPLE_RATE: u32 = 48000;
    pub const CHANNELS: u16 = 2;

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }
    pub fn duration_in_frames(&self) -> u32 {
        self.samples.len() as u32 / self.channels as u32
    }
}

impl AssetLoader for AudioAsset {
    fn load(
        data: &crate::asset::asset::AssetFile,
    ) -> std::result::Result<Self, crate::asset::asset::AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        let mut reader =
            hound::WavReader::open(data.path().path()).map_err(|err| anyhow::anyhow!(err))?;
        let mut samples = match (reader.spec().sample_format, reader.spec().bits_per_sample) {
            (hound::SampleFormat::Int, 16) => reader
                .samples::<i16>()
                .into_iter()
                .map(|s| s.map(|s| s as f32 / i16::MAX as f32))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| anyhow::anyhow!(err))?,
            (hound::SampleFormat::Float, 32) => reader
                .samples::<f32>()
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| anyhow::anyhow!(err))?,
            _ => {
                return Err(
                    anyhow::anyhow!("Unsupported audio format: {:?}", reader.spec()).into(),
                );
            }
        };
        let mut sample_rate = reader.spec().sample_rate;
        let mut channels = reader.spec().channels;

        if channels != Self::CHANNELS {
            if channels == 1 && Self::CHANNELS == 2 {
                log::info!(
                    "Converting mono audio to stereo, samples len is {}",
                    samples.len()
                );
                let mut stereo_samples = Vec::with_capacity(samples.len() * 2);
                for sample in samples {
                    stereo_samples.push(sample);
                    stereo_samples.push(sample);
                }
                samples = stereo_samples;
                channels = Self::CHANNELS;
            } else {
                return Err(anyhow::anyhow!(
                    "Unsupported number of channels: {}, expected {}",
                    channels,
                    Self::CHANNELS
                )
                .into());
            }
        }

        if sample_rate != Self::SAMPLE_RATE {
            assert_eq!(channels, Self::CHANNELS);

            log::info!(
                "Resampling audio from {} Hz to {} Hz, samples len is {}",
                sample_rate,
                Self::SAMPLE_RATE,
                samples.len()
            );
            const RESAMPLE_CHUNK_SIZE: usize = 1024;
            let input_len = samples.len() as usize;
            let input_frame_count = input_len / channels as usize;
            let input_padding = (input_frame_count as f32 / RESAMPLE_CHUNK_SIZE as f32).ceil()
                as usize
                * RESAMPLE_CHUNK_SIZE
                - input_frame_count;
            log::info!(
                "Padding audio with {} frames ({} samples) to be a multiple of 1024 frames for resampling",
                input_padding,
                input_padding * channels as usize
            );
            samples.extend(vec![0.0; input_padding * channels as usize]);
            let input_frame_count = input_frame_count + input_padding;

            let buffer_input = rubato::audioadapter_buffers::direct::InterleavedSlice::new(
                &samples,
                Self::CHANNELS as usize,
                input_frame_count,
            )
            .map_err(|err| anyhow::anyhow!(err))?;
            log::info!(
                "Resampling audio with {} frames and {} channels",
                input_frame_count,
                channels
            );

            use rubato::Resampler;
            let mut resampler = rubato::Fft::<f32>::new(
                sample_rate as usize,
                Self::SAMPLE_RATE as usize,
                RESAMPLE_CHUNK_SIZE,
                1,
                Self::CHANNELS as usize,
                rubato::FixedSync::Input,
            )
            .map_err(|err| anyhow::anyhow!(err))?;

            let num_input_chunks = input_frame_count / RESAMPLE_CHUNK_SIZE;
            let output_frames_per_chunk = resampler.output_frames_max();
            let additional_frames = resampler.output_delay();
            let output_frame_count = num_input_chunks * output_frames_per_chunk + additional_frames;

            let new_buffer_len = output_frame_count * Self::CHANNELS as usize;
            let mut resampled_samples = vec![0.0f32; new_buffer_len];
            let mut buffer_out = rubato::audioadapter_buffers::direct::InterleavedSlice::new_mut(
                &mut resampled_samples,
                Self::CHANNELS as usize,
                output_frame_count,
            )
            .map_err(|err| anyhow::anyhow!(err))?;

            let (_, resampled_output_len) = resampler
                .process_all_into_buffer(&buffer_input, &mut buffer_out, input_frame_count, None)
                .map_err(|err| anyhow::anyhow!(err))?;
            samples = resampled_samples;
        }

        log::info!(
            "Loaded audio asset with {} samples at {} Hz and {} channels",
            samples.len(),
            sample_rate,
            channels
        );

        //if let Ok(mut writer) = hound::WavWriter::create(
        //    format!(
        //        "resampled_{}.wav",
        //        data.path().path().file_name().unwrap().to_string_lossy()
        //    ),
        //    hound::WavSpec {
        //        channels: Self::CHANNELS,
        //        sample_rate: Self::SAMPLE_RATE,
        //        bits_per_sample: 32,
        //        sample_format: hound::SampleFormat::Float,
        //    },
        //) {
        //    for sample in &samples {
        //        writer.write_sample(*sample).ok();
        //    }
        //}

        Ok(Self {
            samples,
            sample_rate: Self::SAMPLE_RATE,
            channels: Self::CHANNELS,
        })
    }
}
