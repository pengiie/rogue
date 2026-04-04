use std::{collections::HashMap, time::Duration};

use crate::{
    common::dyn_vec::{DynVec, DynVecCloneable, TypeInfo, TypeInfoCloneable},
    entity::ecs_world::{ECSWorld, Entity},
    physics::transform::Transform,
    resource::ResMut,
};

#[derive(Clone)]
pub struct AnimationTrackChannel {
    pub track_id: AnimationTrackId,
    pub channel_type_info: AnimationPropertyChannelTypeInfo,
    pub times: Vec<Duration>,
    pub values: DynVecCloneable,
    // Easiest to just store interpolation as a separate array instead of cramming it into values
    // since that'll make the types more confusing.
    pub interpolation: Vec<AnimationInterpolation>,
}

impl AnimationTrackChannel {
    pub fn new(
        track_id: AnimationTrackId,
        channel_type_info: AnimationPropertyChannelTypeInfo,
    ) -> Self {
        let type_info = channel_type_info.type_info.clone();
        Self {
            track_id,
            channel_type_info: channel_type_info,
            times: Vec::new(),
            values: DynVecCloneable::new(type_info),
            interpolation: Vec::new(),
        }
    }

    pub fn record_keyframe(
        &mut self,
        ecs_world: &mut ECSWorld,
        base_entity: Entity,
        time: Duration,
    ) {
        let entity = self.track_id.get_entity(ecs_world, base_entity).expect(
            "Couldn't find entity, this should have been checked before recording a keyframe.",
        );
        let component_type = ecs_world
            .game_component_names
            .get(&self.track_id.component_name)
            .unwrap();
        let game_component = ecs_world.game_components.get(component_type).unwrap();
        let component = ecs_world.get_unchecked(entity, *component_type);
        // Safety: We get the game component type using the same component type as we are fetching
        // the component data.
        let game_component_dyn =
            unsafe { game_component.as_dyn_mut(component.get_component_ptr() as *mut u8) };
        let channel_data = game_component_dyn.get_animation_channel(
            &self.track_id.component_property,
            &self.channel_type_info.channel_name,
        );
        assert_eq!(
            channel_data.type_info.type_id(),
            self.channel_type_info.type_info.type_id()
        );
        // Safety: We assert the type of the channel data is the type of this channel.
        unsafe { self.add_keyframe(time, channel_data.data_ptr, AnimationInterpolation::Linear) };
    }

    unsafe fn add_keyframe(
        &mut self,
        time: Duration,
        value_ptr: *const u8,
        interpolation: AnimationInterpolation,
    ) {
        if self.times.is_empty()
            || self
                .times
                .last()
                .map_or(false, |keyframe_time| keyframe_time < &time)
        {
            self.times.push(time);
            self.values.push_ptr(value_ptr);
            self.interpolation.push(interpolation);
            return;
        }

        let mut i = 0;
        while i < self.times.len() && self.times[i] < time {
            i += 1;
        }
        self.times.insert(i, time);
        self.values.insert_ptr(i, value_ptr);
        self.interpolation.insert(i, interpolation);
        assert!(self.times.is_sorted());
    }

    pub fn find_interpolation_indices(
        &self,
        time: Duration,
    ) -> Option<(
        /*start_index*/ usize,
        /*end_index*/ usize,
        /*t*/ f32,
    )> {
        if self.times.is_empty() {
            return None;
        }

        let mut i = 0;
        while i < self.times.len() && self.times[i] < time {
            i += 1;
        }

        if i == 0 {
            // Time is before the first keyframe.
            return Some((0, 0, 0.0));
        } else if i == self.times.len() {
            // Time is after the last keyframe.
            let last_index = i - 1;
            return Some((last_index, last_index, 0.0));
        } else {
            // Time is between keyframes i-1 and i, since i > 0 means we are after the first
            // keyframe.
            let t0 = self.times[i - 1];
            let t1 = self.times[i];
            let t = (time - t0).as_secs_f32() / (t1 - t0).as_secs_f32();
            return Some((i - 1, i, t));
        }
    }
}

#[derive(Clone)]
pub struct AnimationTrack {
    pub track_id: AnimationTrackId,
    pub channels: Vec<AnimationTrackChannel>,
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
enum AnimationInterpolation {
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
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        todo!()
    }
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum AnimationTrackField {
    TrackId,
    Times,
    Values,
    Interpolation,
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
        let property = None;
        let mut times = None;
        let mut values = None;
        let mut interpolation = None;
        while let Some(key) = map.next_key::<AnimationTrackField>()? {
            match key {
                AnimationTrackField::TrackId => {}
                AnimationTrackField::Times => {
                    times = Some(map.next_value::<Vec<Duration>>()?);
                }
                AnimationTrackField::Values => {
                    //let values = map.next_value::<DynVec>()?;
                }
                AnimationTrackField::Interpolation => {
                    interpolation = Some(map.next_value::<Vec<AnimationInterpolation>>()?);
                }
            }
        }

        let track_id = property.ok_or_else(|| serde::de::Error::missing_field("track_id"))?;
        let times = times.ok_or_else(|| serde::de::Error::missing_field("times"))?;
        let values = values.ok_or_else(|| serde::de::Error::missing_field("values"))?;
        let interpolation =
            interpolation.ok_or_else(|| serde::de::Error::missing_field("interpolation"))?;
        Ok(todo!())
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash)]
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
            let Some(child) = ecs_world.get_child_by_name(base_entity, name) else {
                return None;
            };
            entity = child;
        }
        return Some(entity);
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

    pub fn get_track(&self, track_id: &AnimationTrackId) -> Option<&AnimationTrack> {
        self.tracks.iter().find(|track| &track.track_id == track_id)
    }

    pub fn get_track_mut(&mut self, track_id: &AnimationTrackId) -> Option<&mut AnimationTrack> {
        self.tracks
            .iter_mut()
            .find(|track| &track.track_id == track_id)
    }

    pub fn apply_animation(&self, ecs_world: &ECSWorld, base_entity: Entity, u: f32) {
        for track in &self.tracks {
            let component_type_id = ecs_world
                .game_component_names
                .get(&track.track_id.component_name)
                .unwrap();
            let game_component_type = ecs_world.game_components.get(component_type_id).unwrap();
            for channel in &track.channels {
                if let Some((start_index, end_index, t)) = channel.find_interpolation_indices(
                    Duration::from_secs_f32(u * self.duration.as_secs_f32()),
                ) {
                    let Some(channel_entity) = channel.track_id.get_entity(ecs_world, base_entity)
                    else {
                        continue;
                    };
                    let component_ref = ecs_world.get_unchecked(channel_entity, *component_type_id);
                    // Safety: We access the game component type and component data with the same
                    // type id.
                    let game_component_dyn = unsafe {
                        game_component_type.as_dyn_mut(component_ref.get_component_ptr() as *mut u8)
                    };
                    let channel_data = game_component_dyn.get_animation_channel(
                        &channel.track_id.component_property,
                        &channel.channel_type_info.channel_name,
                    );
                    assert_eq!(
                        channel_data.type_info.type_id(),
                        channel.channel_type_info.type_info.type_id()
                    );
                    let dst_ptr = channel_data.data_ptr;
                    let a_ptr = channel.values.get_unchecked(start_index).as_ptr() as *const u8;
                    let b_ptr = channel.values.get_unchecked(end_index).as_ptr() as *const u8;
                    unsafe {
                        channel
                            .channel_type_info
                            .update_fn
                            .update_erased(dst_ptr, a_ptr, b_ptr, t);
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct AnimationPropertyTypeInfo {
    pub property_name: String,
    pub property_id: String,
    pub channels: Vec<AnimationPropertyChannelTypeInfo>,
}

impl AnimationPropertyTypeInfo {
    pub fn new<T: AnimationProperty>(property_name: String) -> Self {
        Self {
            property_name,
            property_id: T::ID.to_string(),
            channels: T::channels(),
        }
    }
}

#[derive(Clone)]
pub struct AnimationPropertyChannelTypeInfo {
    pub channel_name: String,
    pub channel_id: String,
    // Type of the channel we are storing.
    pub type_info: TypeInfoCloneable,
    pub update_fn: AnimationPropertyUpdateFnCaller,
}

impl AnimationPropertyChannelTypeInfo {
    fn new<T: AnimationPropertyChannel>(channel_name: String) -> Self {
        Self {
            channel_name: channel_name.clone(),
            channel_id: T::ID.to_owned(),
            type_info: TypeInfoCloneable::new::<T>(),
            update_fn: AnimationPropertyUpdateFnCaller::new::<T>(),
        }
    }
}

// Need to do this since AnimationProperty::update relies on the type of Self which is basically a
// generic so we need an erased version of it to call.
type AnimationPropertyUpdateErasedFn = unsafe fn(dst: *mut u8, a: *const u8, b: *const u8, t: f32);
#[derive(Clone)]
pub struct AnimationPropertyUpdateFnCaller {
    update_fn_erased: AnimationPropertyUpdateErasedFn,
}

impl AnimationPropertyUpdateFnCaller {
    pub fn new<T: AnimationPropertyChannel>() -> Self {
        unsafe fn update_erased<T: AnimationPropertyChannel>(
            dst: *mut u8,
            a: *const u8,
            b: *const u8,
            t: f32,
        ) {
            let dst = &mut *(dst as *mut T);
            let a = &*(a as *const T);
            let b = &*(b as *const T);
            T::update(dst, a, b, t);
        }

        Self {
            update_fn_erased: update_erased::<T>,
        }
    }

    unsafe fn update_erased(&self, dst: *mut u8, a: *const u8, b: *const u8, t: f32) {
        (self.update_fn_erased)(dst, a, b, t);
    }
}

pub struct AnimationPropertyChannelUpdateCtx<'a> {
    ecs_world: &'a mut ECSWorld,
    base_entity: Entity,
    property_id: AnimationTrackId,
}

pub struct GameComponentAnimationChannelData<'a> {
    // Type of `value`, used to assert this type is the type expected for this channel name in the
    // `AnimationTrackChannel`.
    pub type_info: TypeInfoCloneable,
    pub data_ptr: *mut u8,
    pub marker: std::marker::PhantomData<&'a mut ()>,
}

impl<'a> GameComponentAnimationChannelData<'a> {
    pub fn new<T: Clone + 'static>(data_ptr: &mut T) -> Self {
        Self {
            type_info: TypeInfoCloneable::new::<T>(),
            data_ptr: data_ptr as *mut T as *mut u8,
            marker: std::marker::PhantomData,
        }
    }
}

/// The grouping of keyframe channels which an entity component exposes.
pub trait AnimationProperty: Clone {
    const ID: &'static str;

    fn channels() -> Vec<AnimationPropertyChannelTypeInfo>;
    fn get_channel_data<'a>(&'a mut self, channel: &str) -> GameComponentAnimationChannelData<'a>;
}

impl AnimationProperty for nalgebra::Vector3<f32> {
    const ID: &'static str = "Vector3<f32>";

    fn channels() -> Vec<AnimationPropertyChannelTypeInfo> {
        vec![
            AnimationPropertyChannelTypeInfo::new::<f32>("x".to_owned()),
            AnimationPropertyChannelTypeInfo::new::<f32>("y".to_owned()),
            AnimationPropertyChannelTypeInfo::new::<f32>("z".to_owned()),
        ]
    }

    fn get_channel_data<'a>(&'a mut self, channel: &str) -> GameComponentAnimationChannelData<'a> {
        match channel {
            "x" => GameComponentAnimationChannelData::new(&mut self.x),
            "y" => GameComponentAnimationChannelData::new(&mut self.y),
            "z" => GameComponentAnimationChannelData::new(&mut self.z),
            _ => panic!("No channel named {} for Vector3<f32>", channel),
        }
    }
}

impl AnimationProperty for nalgebra::UnitQuaternion<f32> {
    const ID: &'static str = "UnitQuaternion<f32>";

    fn channels() -> Vec<AnimationPropertyChannelTypeInfo> {
        vec![AnimationPropertyChannelTypeInfo::new::<Self>(
            "rotation".to_owned(),
        )]
    }

    fn get_channel_data<'a>(&'a mut self, channel: &str) -> GameComponentAnimationChannelData<'a> {
        match channel {
            "rotation" => GameComponentAnimationChannelData::new(self),
            _ => panic!("No channel named {} for UnitQuaternion<f32>", channel),
        }
    }
}

/// The animatable value.
pub trait AnimationPropertyChannel: Clone + 'static {
    const ID: &'static str;

    fn update(dst: &mut Self, a: &Self, b: &Self, t: f32);
}

impl AnimationPropertyChannel for f32 {
    const ID: &'static str = "f32";

    fn update(dst: &mut Self, a: &Self, b: &Self, t: f32) {
        *dst = (1.0 - t) * a + t * b;
    }
}

impl AnimationPropertyChannel for nalgebra::UnitQuaternion<f32> {
    const ID: &'static str = "UnitQuaternion<f32>";

    fn update(dst: &mut Self, a: &Self, b: &Self, t: f32) {
        *dst = a.slerp(b, t);
    }
}
