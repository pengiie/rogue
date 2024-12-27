use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, Sample,
};
use log::warn;
use rogue_macros::Resource;

pub const SAMPLE_RATE: u32 = 48000;
pub const CHANNEL_FREQUENCY: u32 = 24000;

#[derive(Resource)]
pub struct Audio {
    host: cpal::Host,
    device: Option<AudioDevice>,
}

pub struct AudioDevice {
    device: cpal::Device,
    output_config: cpal::SupportedStreamConfig,
    sample_format: cpal::SampleFormat,
    output_stream: cpal::Stream,

    is_playing: bool,
}

impl Audio {
    pub fn new() -> Self {
        let host = cpal::default_host();
        let device = if let Some(device) = host.default_output_device() {
            if let Some(output_config) =
                device
                    .supported_output_configs()
                    .map_or(None, |mut configs| {
                        configs
                            .find(|config| config.channels() == 2)
                            .map(|config_range| {
                                config_range.with_sample_rate(cpal::SampleRate(SAMPLE_RATE))
                            })
                    })
            {
                let sample_format = output_config.sample_format();
                let output_stream = match sample_format {
                    cpal::SampleFormat::F32 => device.build_output_stream(
                        &output_config.config(),
                        Self::cpal_write_output_data::<f32>,
                        Self::cpal_error_callback,
                        None,
                    ),
                    cpal::SampleFormat::U8 => device.build_output_stream(
                        &output_config.config(),
                        Self::cpal_write_output_data::<f32>,
                        Self::cpal_error_callback,
                        None,
                    ),
                    cpal::SampleFormat::U32 => device.build_output_stream(
                        &output_config.config(),
                        Self::cpal_write_output_data::<f32>,
                        Self::cpal_error_callback,
                        None,
                    ),
                    sample_format => panic!("Unsupported sample format '{sample_format}'"),
                }
                .unwrap();
                output_stream
                    .play()
                    .expect("Failed to start the output_stream");

                Some(AudioDevice {
                    device,
                    output_config,
                    sample_format,
                    output_stream,

                    is_playing: false,
                })
            } else {
                None
            }
        } else {
            None
        };

        Self { host, device }
    }

    fn cpal_error_callback(error: cpal::StreamError) {}

    fn cpal_write_output_data<T>(data: &mut [T], _: &cpal::OutputCallbackInfo)
    where
        T: Sample + FromSample<f32>,
    {
        // Silence
        for sample in data {
            *sample = T::EQUILIBRIUM;
        }
    }
}

pub struct Sound {}
