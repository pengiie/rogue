use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, atomic::AtomicBool},
};

use cpal::{
    FromSample, Sample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use ringbuf::traits::{Consumer, Observer};
use rogue_macros::{Resource, game_component};

use crate::{
    asset::asset::{AssetStatus, Assets, GameAssetPath},
    audio::audio_asset::AudioAsset,
    common::freelist::{FreeList, FreeListHandle},
    entity::ecs_world::ECSWorld,
    resource::{Res, ResMut},
};

pub type SoundId = FreeListHandle<AudioAsset>;

pub const SAMPLE_RATE: u32 = 48000;
pub const AUDIO_BUFFER_SIZE: usize = 1024;
pub const RING_BUFFER_SIZE: usize = AUDIO_BUFFER_SIZE * 5;
#[derive(Resource)]
pub struct Audio {
    host: cpal::Host,
    device: Option<AudioDevice>,

    loading_sounds: HashSet<GameAssetPath>,
    sound_bank: SoundBank,
    playing_sounds: PlayingSounds,
    path_to_sound: HashMap<GameAssetPath, SoundId>,
}

type SoundBank = Arc<std::sync::RwLock<FreeList<AudioAsset>>>;
type PlayingSounds = Arc<std::sync::RwLock<FreeList<PlayingSound>>>;

pub struct AudioDevice {
    device: cpal::Device,
    output_config: cpal::SupportedStreamConfig,
    sample_format: cpal::SampleFormat,
    output_stream: cpal::Stream,
    keep_producer_alive: Arc<AtomicBool>,

    is_playing: bool,
}

impl AudioDevice {}

struct AudioProducerContext {
    sample_producer: ringbuf::HeapProd<f32>,
    sound_bank: SoundBank,
    playing_sounds: PlayingSounds,
    keep_producer_alive: Arc<AtomicBool>,
}

impl Audio {
    pub fn new() -> Self {
        let host = cpal::default_host();

        let sound_bank = Arc::new(std::sync::RwLock::new(FreeList::new()));
        let playing_sounds = Arc::new(std::sync::RwLock::new(FreeList::new()));

        let device = host.default_output_device().and_then(|device| {
            if let Some(output_config) =
                device
                    .supported_output_configs()
                    .map_or(None, |mut configs| {
                        configs
                            .find(|config| {
                                let matches_buffer_size = match config.buffer_size() {
                                    cpal::SupportedBufferSize::Range { min, max } => {
                                        *min <= AUDIO_BUFFER_SIZE as u32
                                            && AUDIO_BUFFER_SIZE as u32 <= *max
                                    }
                                    cpal::SupportedBufferSize::Unknown => false,
                                };
                                config.channels() == 2 && matches_buffer_size
                            })
                            .map(|config_range| {
                                config_range.with_sample_rate(cpal::SampleRate(SAMPLE_RATE))
                            })
                    })
            {
                use ringbuf::traits::Split;
                let (mut sample_prod, sample_cons) = ringbuf::HeapRb::new(RING_BUFFER_SIZE).split();

                let sample_format = output_config.sample_format();
                let Ok(output_stream) = (match sample_format {
                    cpal::SampleFormat::F32 => device.build_output_stream(
                        &output_config.config(),
                        Self::create_cpal_output_callback::<f32>(sample_cons),
                        Self::cpal_error_callback,
                        None,
                    ),
                    cpal::SampleFormat::U16 => device.build_output_stream(
                        &output_config.config(),
                        Self::create_cpal_output_callback::<u16>(sample_cons),
                        Self::cpal_error_callback,
                        None,
                    ),
                    cpal::SampleFormat::U8 => device.build_output_stream(
                        &output_config.config(),
                        Self::create_cpal_output_callback::<u8>(sample_cons),
                        Self::cpal_error_callback,
                        None,
                    ),
                    cpal::SampleFormat::U32 => device.build_output_stream(
                        &output_config.config(),
                        Self::create_cpal_output_callback::<u32>(sample_cons),
                        Self::cpal_error_callback,
                        None,
                    ),
                    sample_format => panic!("Unsupported sample format '{sample_format}'"),
                }) else {
                    return None;
                };

                output_stream
                    .play()
                    .expect("Failed to start the output_stream");

                let keep_producer_alive = Arc::new(AtomicBool::new(true));
                Self::spawn_producer_thread(AudioProducerContext {
                    sample_producer: sample_prod,
                    sound_bank: sound_bank.clone(),
                    playing_sounds: playing_sounds.clone(),
                    keep_producer_alive: keep_producer_alive.clone(),
                });

                Some(AudioDevice {
                    device,
                    output_config,
                    sample_format,
                    output_stream,
                    keep_producer_alive,

                    is_playing: false,
                })
            } else {
                None
            }
        });

        Self {
            host,
            device,
            path_to_sound: HashMap::new(),
            sound_bank,
            loading_sounds: HashSet::new(),

            playing_sounds,
        }
    }

    pub fn spawn_producer_thread(producer_context: AudioProducerContext) {
        std::thread::spawn(move || {
            let AudioProducerContext {
                sample_producer: mut producer,
                sound_bank: sounds,
                keep_producer_alive,
                playing_sounds,
            } = producer_context;
            let mut last_playing_sound_count = 0;
            while keep_producer_alive.load(std::sync::atomic::Ordering::Relaxed) {
                if producer.is_full() {
                    // TODO: Use condvar.
                    continue;
                }
                let mut free_samples_size = producer.vacant_len();
                let mut free_frames_size = producer.vacant_len() / AudioAsset::CHANNELS as usize;
                // Ensure we generate samples for each channel as the same time.
                if free_frames_size % AudioAsset::CHANNELS as usize != 0 {
                    free_frames_size = free_frames_size
                        .saturating_sub(free_frames_size % AudioAsset::CHANNELS as usize);
                }
                if free_frames_size < 1 {
                    continue;
                }
                let mut generated_samples =
                    vec![0.0; free_frames_size * AudioAsset::CHANNELS as usize];

                let sound_bank = sounds.read().unwrap();
                let mut playing_sounds = playing_sounds.write().unwrap();
                if playing_sounds.len() == 0 {
                    continue;
                }

                let mut finished_playing_sounds = Vec::new();
                for (playing_handle, playing_sound) in playing_sounds.iter_with_handle_mut() {
                    let sound = sound_bank.get(playing_sound.sound_id).unwrap();
                    let to_generate_frame_count = sound
                        .duration_in_frames()
                        .saturating_sub(
                            playing_sound.current_sample_index / AudioAsset::CHANNELS as u32,
                        )
                        .min(free_frames_size as u32);
                    if to_generate_frame_count < 1 {
                        finished_playing_sounds.push(playing_handle);
                        continue;
                    }

                    for i in 0..to_generate_frame_count {
                        let sample_index = i * AudioAsset::CHANNELS as u32;
                        let in_sample_index = playing_sound.current_sample_index + sample_index;
                        let l_sample = sound.samples()[in_sample_index as usize];
                        let r_sample = sound.samples()[(in_sample_index + 1) as usize];
                        generated_samples[sample_index as usize] += l_sample;
                        generated_samples[sample_index as usize + 1] += r_sample;
                    }
                }
                for play_sound_handle in finished_playing_sounds {
                    playing_sounds.remove(play_sound_handle);
                }

                use ringbuf::traits::Producer;
                let pushed_count = producer.push_slice(&generated_samples);
                for (_, playing_sound) in playing_sounds.iter_with_handle_mut() {
                    playing_sound.current_sample_index += pushed_count as u32;
                }
                //assert_eq!(
                //    pushed_count,
                //    generated_samples.len(),
                //    "Producer thread should be the only one generating samples."
                //);
            }
        });
    }

    pub fn on_update(mut audio: ResMut<Audio>, mut assets: ResMut<Assets>) {
        let audio = &mut *audio;
        let assets = &mut *assets;
        let Some(project_dir) = assets.project_dir().clone() else {
            return;
        };

        let mut finished_loading_sounds = Vec::new();
        for path in &audio.loading_sounds {
            if audio.path_to_sound.contains_key(path) {
                finished_loading_sounds.push(path.clone());
                continue;
            }

            let asset_path = path.as_file_asset_path(&project_dir);
            let Some(asset_handle) = assets.get_asset_handle::<AudioAsset>(&asset_path) else {
                assets.load_asset::<AudioAsset>(asset_path);
                continue;
            };
            match assets.get_asset_status(&asset_handle) {
                AssetStatus::InProgress => {}
                AssetStatus::Saved => unreachable!(),
                AssetStatus::Loaded => {
                    let asset = assets.take_asset::<AudioAsset>(&asset_handle).unwrap();
                    let sound_id = audio.sound_bank.write().unwrap().push(*asset);
                    audio.path_to_sound.insert(path.clone(), sound_id);
                    finished_loading_sounds.push(path.clone());
                }
                AssetStatus::NotFound => {
                    log::error!(
                        "Audio asset not found at path {}",
                        path.as_relative_path_str()
                    );
                    finished_loading_sounds.push(path.clone());
                }
                AssetStatus::Error(error) => {
                    log::error!(
                        "Failed to load audio asset at path {}: {}",
                        path.as_relative_path_str(),
                        error
                    );
                    finished_loading_sounds.push(path.clone());
                }
            }
        }
        for path in finished_loading_sounds {
            audio.loading_sounds.remove(&path);
        }

        let Some(device) = &mut audio.device else {
            return;
        };
    }

    pub fn play_sound(&mut self, sound_id: SoundId, play_info: PlaySoundInfo) {
        let playing_sound = PlayingSound {
            playback_info: play_info,
            sound_id,
            current_sample_index: 0,
        };
        self.playing_sounds.write().unwrap().push(playing_sound);
    }

    pub fn ensure_sound_loaded(&mut self, path: &GameAssetPath) {
        self.loading_sounds.insert(path.clone());
    }

    fn cpal_error_callback(error: cpal::StreamError) {}

    fn create_cpal_output_callback<T: cpal::Sample>(
        mut consumer: ringbuf::HeapCons<f32>,
    ) -> impl FnMut(&mut [T], &cpal::OutputCallbackInfo) + Send + 'static
    where
        T: Sample + FromSample<f32>,
    {
        move |data, _| {
            for sample in data.iter_mut() {
                *sample = T::from_sample(consumer.try_pop().unwrap_or(0.0).clamp(-0.4, 0.4));
            }
        }
    }
}

impl Drop for AudioDevice {
    fn drop(&mut self) {
        self.output_stream
            .pause()
            .expect("Failed to pause output stream");
        self.keep_producer_alive
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

pub struct PlayingSound {
    playback_info: PlaySoundInfo,
    sound_id: SoundId,
    current_sample_index: u32,
}

#[derive(Clone)]
pub struct PlaySoundInfo {
    pub speed: f32,
    pub pitch_shift: f32,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[game_component(name = "AudioPlayer")]
#[serde(default)]
pub struct AudioPlayer {
    pub sounds: HashMap<String, Option<GameAssetPath>>,
    #[serde(skip)]
    to_play_sounds: HashMap<String, PlaySoundInfo>,
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioPlayer {
    pub fn new() -> Self {
        Self {
            sounds: HashMap::new(),
            to_play_sounds: HashMap::new(),
        }
    }

    pub fn on_update(ecs_world: Res<ECSWorld>, mut audio: ResMut<Audio>) {
        let audio = &mut *audio;
        for (_, audio_player) in ecs_world.query::<&mut AudioPlayer>().into_iter() {
            for path in audio_player.sounds.values() {
                let Some(path) = path else {
                    continue;
                };
                if !audio.path_to_sound.contains_key(path) {
                    audio.ensure_sound_loaded(path);
                }
            }

            // If a sound isn't loaded by the time we try playing it, just skip it since it won't
            // be obvious to the consumer of this API when the sound will start playing then.
            for (to_play_sound, play_info) in audio_player.to_play_sounds.drain() {
                let Some(Some(path)) = audio_player.sounds.get(&to_play_sound) else {
                    continue;
                };
                let Some(sound_id) = audio.path_to_sound.get(path) else {
                    continue;
                };
                let sound_id = *sound_id;
                audio.play_sound(sound_id, play_info);
            }
        }
    }

    pub fn play_sound(&mut self, sound: &str, info: PlaySoundInfo) {
        self.to_play_sounds.insert(sound.to_owned(), info);
    }
}
