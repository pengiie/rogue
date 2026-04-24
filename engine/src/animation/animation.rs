use std::{
    collections::{HashMap, HashSet},
    ptr::NonNull,
    time::Duration,
};

use nalgebra::UnitQuaternion;

use crate::animation::animation_channel;
use crate::animation::animation_channel::{
    AnimationPropertyChannel, AnimationPropertyChannelTypeInfo, AnimationTrackChannel,
    AnimationTrackChannelsDeserializer, GameComponentAnimationChannelData,
};
use crate::animation::animation_property::{
    AnimationPropertyApplyData, AnimationPropertyMethods, AnimationPropertyTypeInfo,
};
use crate::{
    common::{
        dyn_vec::{DynVec, DynVecCloneable, TypeInfo, TypeInfoCloneable},
        vtable,
    },
    entity::ecs_world::{ECSWorld, Entity},
    physics::transform::Transform,
    resource::ResMut,
};

#[derive(Clone)]
pub struct AnimationTrack {
    pub track_id: AnimationTrackId,
    pub channels: Vec<AnimationTrackChannel>,
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum AnimationInterpolation {
    Step,
    Linear,
    Cubic,
}

impl AnimationTrack {
    pub fn new(track_id: AnimationTrackId, property_type_info: AnimationPropertyTypeInfo) -> Self {
        let channels = property_type_info
            .channels
            .into_iter()
            .map(|channel_info| AnimationTrackChannel::new(track_id.clone(), channel_info))
            .collect();
        Self { track_id, channels }
    }

    pub fn update(&self, ecs_world: &mut ECSWorld, elapsed_time: Duration) {}
}

impl serde::Serialize for AnimationTrack {
    fn serialize<S>(&self, se: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut s = se.serialize_struct("AnimationTrack", 3)?;
        s.serialize_field("track_id", &self.track_id)?;
        s.serialize_field("channels", &self.channels)?;
        s.end()
    }
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum AnimationTrackField {
    TrackId,
    Channels,
}

impl<'de> serde::Deserialize<'de> for AnimationTrack {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &["property", "track_id", "values"];
        de.deserialize_struct("AnimationTrack", FIELDS, AnimationTrackVisitor)
    }
}

struct AnimationTrackVisitor;
impl<'de> serde::de::Visitor<'de> for AnimationTrackVisitor {
    type Value = AnimationTrack;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("AnimationTrack")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut track_id = None;
        let mut channels = None;
        while let Some(key) = map.next_key()? {
            match key {
                AnimationTrackField::TrackId => {
                    if track_id.is_some() {
                        return Err(serde::de::Error::duplicate_field("track_id"));
                    }
                    track_id = Some(map.next_value()?);
                }
                AnimationTrackField::Channels => {
                    if channels.is_some() {
                        return Err(serde::de::Error::duplicate_field("channels"));
                    }
                    let Some(track_id) = &track_id else {
                        return Err(serde::de::Error::custom(
                            "track_id must come before channels",
                        ));
                    };
                    channels =
                        Some(map.next_value_seed(AnimationTrackChannelsDeserializer { track_id })?);
                }
            }
        }
        let track_id = track_id.ok_or_else(|| serde::de::Error::missing_field("track_id"))?;
        let channels = channels.ok_or_else(|| serde::de::Error::missing_field("channels"))?;
        Ok(AnimationTrack { track_id, channels })
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash, Debug)]
pub struct AnimationTrackId {
    // Empty for root, with each string representing the entity name.
    pub entity_traversal: Vec<String>,
    pub component_name: String,
    pub component_property: String,
}

impl ToString for AnimationTrackId {
    fn to_string(&self) -> String {
        format!(
            "{}.{}.{}",
            self.entity_traversal.join("."),
            self.component_name,
            self.component_property
        )
    }
}

impl AnimationTrackId {
    pub fn get_entity(&self, ecs_world: &ECSWorld, base_entity: Entity) -> Option<Entity> {
        let mut entity = base_entity;
        for name in &self.entity_traversal {
            let Some(child) = ecs_world.get_child_by_name(entity, name) else {
                return None;
            };
            entity = child;
        }
        return Some(entity);
    }

    pub fn matches_prefix(
        &self,
        entity_traversal: &[String],
        component_name: Option<&String>,
        component_property: Option<&String>,
    ) -> bool {
        if self.entity_traversal.len() < entity_traversal.len() {
            return false;
        }
        if &self.entity_traversal[0..entity_traversal.len()] != entity_traversal {
            return false;
        }
        if let Some(component_name) = component_name {
            if entity_traversal.len() < self.entity_traversal.len() {
                return false;
            }
            if &self.component_name != component_name {
                return false;
            }
        }
        if let Some(component_property) = component_property {
            assert!(component_name.is_some());
            if &self.component_property != component_property {
                log::info!(
                    "Prefix match failed on component property, expected {}, got {}",
                    component_property,
                    self.component_property
                );
                return false;
            }
        }
        return true;
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Animation {
    pub tracks: Vec<AnimationTrack>,
    pub duration: Duration,
}
crate::impl_asset_load_save_serde!(Animation);

impl Animation {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            duration: Duration::ZERO,
        }
    }

    pub fn update_duration(&mut self) {
        let mut max_time = Duration::ZERO;
        for track in &self.tracks {
            for channel in &track.channels {
                if let Some(last_time) = channel.times.last() {
                    if *last_time > max_time {
                        max_time = *last_time;
                    }
                }
            }
        }
        self.duration = max_time;
    }

    pub fn contains_track(&self, track_id: &AnimationTrackId) -> bool {
        self.tracks.iter().any(|track| &track.track_id == track_id)
    }

    pub fn create_track(
        &mut self,
        track_id: AnimationTrackId,
        property_type_info: AnimationPropertyTypeInfo,
    ) {
        assert_eq!(
            track_id.component_property,
            property_type_info.property_name
        );
        if self.contains_track(&track_id) {
            panic!(
                "Animation already contains track with id {}",
                track_id.to_string()
            );
        }
        self.tracks
            .push(AnimationTrack::new(track_id, property_type_info));
    }

    pub fn get_channel(
        &self,
        track_id: &AnimationTrackId,
        channel_name: &str,
    ) -> Option<&AnimationTrackChannel> {
        self.tracks.iter().find_map(|track| {
            if &track.track_id == track_id {
                track
                    .channels
                    .iter()
                    .find(|channel| channel.channel_type_info.channel_name == channel_name)
            } else {
                None
            }
        })
    }

    pub fn get_track(&self, track_id: &AnimationTrackId) -> Option<&AnimationTrack> {
        self.tracks.iter().find(|track| &track.track_id == track_id)
    }

    pub fn get_track_mut(&mut self, track_id: &AnimationTrackId) -> Option<&mut AnimationTrack> {
        self.tracks
            .iter_mut()
            .find(|track| &track.track_id == track_id)
    }

    pub fn get_channel_values(
        &self,
        ecs_world: &ECSWorld,
        base_entity: Entity,
    ) -> Vec<(
        AnimationTrackId,
        AnimationPropertyChannelTypeInfo,
        GameComponentAnimationChannelData,
    )> {
        let mut channel_values = Vec::new();
        for track in &self.tracks {
            let Some(track_entity) = track.track_id.get_entity(ecs_world, base_entity) else {
                continue;
            };
            let component_type_id = ecs_world
                .game_component_names
                .get(&track.track_id.component_name)
                .unwrap();
            let game_component_type = ecs_world.game_components.get(component_type_id).unwrap();
            let component_ref = ecs_world.get_unchecked(track_entity, *component_type_id);
            // Safety: We access the game component type and component data with the same
            // type id.
            let game_component_dyn = unsafe {
                game_component_type.as_dyn_mut(component_ref.get_component_ptr() as *mut u8)
            };
            let animation_property = game_component_dyn
                .get_animation_property(track.track_id.component_property.as_str());
            for channel in &track.channels {
                let channel_data = animation_property
                    .get_channel_data(channel.channel_type_info.channel_name.as_str());
                assert_eq!(
                    channel_data.type_info.type_id(),
                    channel.channel_type_info.type_info.type_id()
                );
                channel_values.push((
                    track.track_id.clone(),
                    channel.channel_type_info.clone(),
                    channel_data,
                ));
            }
        }
        channel_values
    }

    pub fn apply_animation(&self, ecs_world: &ECSWorld, base_entity: Entity, u: f32) {
        for track in &self.tracks {
            let component_type_id = ecs_world
                .game_component_names
                .get(&track.track_id.component_name)
                .unwrap();
            let game_component_type = ecs_world.game_components.get(component_type_id).unwrap();
            let Some(track_entity) = track.track_id.get_entity(ecs_world, base_entity) else {
                continue;
            };
            let component_ref = ecs_world.get_unchecked(track_entity, *component_type_id);
            // Safety: We access the game component type and component data with the same
            // type id.
            let game_component_dyn = unsafe {
                game_component_type.as_dyn_mut(component_ref.get_component_ptr() as *mut u8)
            };

            // Collect each interpolated channel value, then apply it to the animation property
            // encompassing those channels, done this way to allow for intermediate conversion
            // mainly for rotations.
            let mut property_apply_data = AnimationPropertyApplyData::new();
            for channel in &track.channels {
                if let Some((start_index, end_index, t)) = channel.find_interpolation_indices(
                    Duration::from_secs_f32(u * self.duration.as_secs_f32()),
                ) {
                    // Safety: We insert this into property_apply_data which handles deallocation.
                    let dst_ptr =
                        unsafe { std::alloc::alloc(channel.channel_type_info.type_info.layout(1)) };
                    assert!(!dst_ptr.is_null(), "Failed to allocate memory.");
                    let a_ptr = channel.values.get_unchecked(start_index).as_ptr() as *const u8;
                    let b_ptr = channel.values.get_unchecked(end_index).as_ptr() as *const u8;
                    // Safety: Each value is allocated with the same channel type info.
                    unsafe {
                        channel
                            .channel_type_info
                            .fn_caller
                            .update_erased(dst_ptr, a_ptr, b_ptr, t);
                    }
                    // Safety: dst_ptr is allocated above and is not referenced later, relieving
                    // ownership.
                    unsafe {
                        property_apply_data
                            .add_channel_value(dst_ptr, channel.channel_type_info.clone());
                    }
                }
            }

            game_component_dyn
                .get_animation_property(track.track_id.component_property.as_str())
                .set_property_data(property_apply_data);
        }
    }
}

#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct AnimationRadians(pub f32);

impl AnimationPropertyChannel for AnimationRadians {
    const ID: &'static str = "AnimationRadians";

    fn update(dst: &mut Self, a: &Self, b: &Self, t: f32) {
        dst.0 = (1.0 - t) * a.0 + t * b.0;
    }

    fn show_ui(val: &mut Self, ui: &mut egui::Ui) -> bool {
        let mut x = val.0.to_degrees();
        let res = ui.add(egui::DragValue::new(&mut x).suffix("°"));
        *val = AnimationRadians(x.to_radians());
        return res.changed();
    }
}
