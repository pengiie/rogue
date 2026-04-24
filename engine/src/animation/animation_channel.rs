use crate::animation::animation::{AnimationInterpolation, AnimationRadians, AnimationTrackId};
use crate::animation::animation_property::AnimationPropertyMethods;
use crate::common::dyn_vec::{DynVecCloneable, TypeInfoCloneable};
use crate::common::vtable;
use crate::entity::component::GameComponentMethods;
use crate::entity::ecs_world::{ECSWorld, Entity};
use bitflags::__private::serde::Deserializer;
use bitflags::__private::serde::de::{Error, MapAccess, SeqAccess};
use nalgebra::UnitQuaternion;
use std::ptr::NonNull;
use std::time::Duration;

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

    pub fn remove_nearby_keyframes(&mut self, time: Duration, remove_radius: Duration) {
        let mut i = 0;
        while i < self.times.len() {
            if self.times[i].abs_diff(time) <= remove_radius {
                self.times.remove(i);
                self.values.remove(i);
                self.interpolation.remove(i);
            } else {
                i += 1;
            }
        }
    }

    pub fn record_keyframe_euler(&mut self, time: Duration, euler_angle: AnimationRadians) {
        assert_eq!(
            self.channel_type_info.type_info.type_id(),
            std::any::TypeId::of::<AnimationRadians>(),
            "Can't call AnimationTrackChannel::record_keyframe_euler for channel {}.{} since it is not of type AnimationRadians",
            self.track_id.to_string(),
            self.channel_type_info.channel_name
        );
        // Safety: We assert the type of this channel.
        let data_ptr = &euler_angle as *const AnimationRadians as *const u8;
        unsafe {
            self.add_keyframe(time, data_ptr, AnimationInterpolation::Linear);
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
        let channel_data = game_component_dyn
            .get_animation_property(&self.track_id.component_property)
            .get_channel_data(self.channel_type_info.channel_name.as_str());
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
            // TODO: This is technically not correct as this "takes ownership" of value_ptr but
            // unless it holds some references or has sepecific drop data like a Vec, we are fine.
            // We want a copy from funciton instead or something but like the other serialization
            // TODO do that with DynVecCloneable cleanup.
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

    /// Returns Some(new_value) if the channel value was modified by the user through the editor UI.
    pub fn show_ui(
        &self,
        ui: &mut egui::Ui,
        base_entity: Entity,
        ecs_world: &ECSWorld,
    ) -> Option<GameComponentAnimationChannelData> {
        let entity = self
            .track_id
            .get_entity(ecs_world, base_entity)
            .expect("Couldn't find entity, this should have been checked before showing the UI.");
        let component_type = ecs_world
            .game_component_names
            .get(&self.track_id.component_name)
            .unwrap();
        let game_component = ecs_world.game_components.get(component_type).unwrap();
        let component = ecs_world.get_unchecked(entity, *component_type);
        let game_component_dyn =
            unsafe { game_component.as_dyn_mut(component.get_component_ptr() as *mut u8) };
        let animation_property =
            game_component_dyn.get_animation_property(&self.track_id.component_property);
        let channel_name = self.channel_type_info.channel_name.as_str();
        let channel_data = animation_property.get_channel_data(channel_name);
        assert_eq!(
            channel_data.type_info.type_id(),
            self.channel_type_info.type_info.type_id()
        );
        let did_change = unsafe {
            self.channel_type_info
                .fn_caller
                .show_ui_erased(channel_data.data_ptr, ui)
        };
        if did_change {
            // Safety: We use the same channel name to get so the pointer type should be correct.
            unsafe {
                animation_property.set_channel_data(channel_name, channel_data.data_ptr);
            }
        }
        return did_change.then_some(channel_data);
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

impl serde::Serialize for AnimationTrackChannel {
    fn serialize<S>(&self, se: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use bitflags::__private::serde::Serializer;
        use serde::ser::SerializeStruct;
        let mut s = se.serialize_struct("AnimationTrackChannel", 3)?;
        s.serialize_field("channel_type_info", &self.channel_type_info)?;
        s.serialize_field("times", &self.times)?;
        s.serialize_field(
            "values",
            &ValuesSerializer {
                serialize_vtable: self.channel_type_info.fn_caller.erased_serialize_vtable,
                values: &self.values,
            },
        )?;
        s.serialize_field("interpolation", &self.interpolation)?;
        s.end()
    }
}

struct ValuesSerializer<'a> {
    serialize_vtable: *const (),
    values: &'a DynVecCloneable,
}

impl serde::ser::Serialize for ValuesSerializer<'_> {
    fn serialize<S>(&self, se: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use bitflags::__private::serde::Serializer;
        use serde::ser::SerializeSeq;
        let values = self.values;
        let len = values.len();
        let mut seq = se.serialize_seq(Some(len))?;
        for i in 0..len {
            let val_ptr = values.get_unchecked(i).as_ptr();
            // values dyn vec is made with same type info as the stored channel.
            let dyn_serialize = unsafe {
                &*(std::mem::transmute::<(*const (), *const ()), *const dyn erased_serde::Serialize>(
                    (val_ptr as *const (), self.serialize_vtable),
                ))
            };
            seq.serialize_element(dyn_serialize)?;
        }
        seq.end()
    }
}

struct ValuesDeserializer<'a> {
    channel_type: &'a AnimationPropertyChannelTypeInfo,
}

impl<'de> serde::de::DeserializeSeed<'de> for ValuesDeserializer<'_> {
    type Value = DynVecCloneable;

    fn deserialize<D>(self, de: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        de.deserialize_seq(self)
    }
}

impl<'de> serde::de::Visitor<'de> for ValuesDeserializer<'_> {
    type Value = DynVecCloneable;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("values array")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        // TODO: Ditch serde since we could do this so much cleaner with our own format.
        struct ChannelValueDeserializer<'a> {
            fn_caller: &'a AnimationPropertyChannelFnCaller,
            dst_ptr: *mut u8,
        }

        impl<'de> serde::de::DeserializeSeed<'de> for ChannelValueDeserializer<'_> {
            type Value = ();

            fn deserialize<D>(self, de: D) -> Result<Self::Value, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let mut erased_de = <dyn erased_serde::Deserializer>::erase(de);
                // Safety: We allocate memory for dst_ptr with the layout of this fn caller type.
                unsafe {
                    self.fn_caller
                        .deserialize_erased(self.dst_ptr, &mut erased_de)
                }
                .map_err(|err| serde::de::Error::custom(err))?;
                Ok(())
            }
        }
        let mut values = DynVecCloneable::new(self.channel_type.type_info.clone());
        let mut index = 0;
        let data =
            GameComponentAnimationChannelData::new_alloc(self.channel_type.type_info.clone());
        while let Some(value) = seq.next_element_seed(ChannelValueDeserializer {
            fn_caller: &self.channel_type.fn_caller,
            dst_ptr: data.data_ptr,
        })? {
            // TODO: This isnt actually a Clone copy its just a straight byte copy, should be fine
            // for animation channel data but its really semantically not correct to call this
            // with push, probably deal with this when DynVecCloneable is collapsed to DynVec.
            // Safety: We allocate the dst_ptr with the same type info as the dynvec.
            unsafe { values.push_ptr(data.data_ptr) };
            index += 1;
        }
        Ok(values)
    }
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum AnimationTrackChannelFields {
    ChannelTypeInfo,
    Times,
    Values,
    Interpolation,
}

pub struct AnimationTrackChannelsDeserializer<'a> {
    pub track_id: &'a AnimationTrackId,
}

impl<'de> serde::de::DeserializeSeed<'de> for AnimationTrackChannelsDeserializer<'_> {
    type Value = Vec<AnimationTrackChannel>;

    fn deserialize<D>(self, de: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        de.deserialize_seq(self)
    }
}

impl<'de> serde::de::Visitor<'de> for AnimationTrackChannelsDeserializer<'_> {
    type Value = Vec<AnimationTrackChannel>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("AnimationTrackChannel[]")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut channels = Vec::new();
        while let Some(channel) = seq.next_element_seed(AnimationTrackChannelDeserializer {
            track_id: self.track_id,
        })? {
            channels.push(channel);
        }
        Ok(channels)
    }
}

struct AnimationTrackChannelDeserializer<'a> {
    track_id: &'a AnimationTrackId,
}

impl<'de> serde::de::DeserializeSeed<'de> for AnimationTrackChannelDeserializer<'_> {
    type Value = AnimationTrackChannel;

    fn deserialize<D>(self, de: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &["channel_type_info", "times", "values", "interpolation"];
        de.deserialize_struct("AnimationTrackChannel", FIELDS, self)
    }
}

impl<'de> serde::de::Visitor<'de> for AnimationTrackChannelDeserializer<'_> {
    type Value = AnimationTrackChannel;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("AnimationTrackChannel")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut channel_type_info = None;
        let mut times = None;
        let mut values = None;
        let mut interpolation = None;
        while let Some(key) = map.next_key()? {
            match key {
                AnimationTrackChannelFields::ChannelTypeInfo => {
                    if channel_type_info.is_some() {
                        return Err(serde::de::Error::duplicate_field("channel_type_info"));
                    }
                    channel_type_info = Some(map.next_value()?);
                }
                AnimationTrackChannelFields::Times => {
                    if times.is_some() {
                        return Err(serde::de::Error::duplicate_field("times"));
                    }
                    times = Some(map.next_value()?);
                }
                AnimationTrackChannelFields::Values => {
                    if values.is_some() {
                        return Err(serde::de::Error::duplicate_field("values"));
                    }
                    let Some(channel_type_info) = &channel_type_info else {
                        return Err(serde::de::Error::custom(
                            "channel_type_info must come before values",
                        ));
                    };
                    values = Some(map.next_value_seed(ValuesDeserializer {
                        channel_type: channel_type_info,
                    })?);
                }
                AnimationTrackChannelFields::Interpolation => {
                    if interpolation.is_some() {
                        return Err(serde::de::Error::duplicate_field("interpolation"));
                    }
                    interpolation = Some(map.next_value()?);
                }
            }
        }

        let channel_type_info = channel_type_info
            .ok_or_else(|| serde::de::Error::missing_field("channel_type_info"))?;
        let times = times.ok_or_else(|| serde::de::Error::missing_field("times"))?;
        let values = values.ok_or_else(|| serde::de::Error::missing_field("values"))?;
        let interpolation =
            interpolation.ok_or_else(|| serde::de::Error::missing_field("interpolation"))?;

        Ok(AnimationTrackChannel {
            track_id: self.track_id.clone(),
            channel_type_info,
            times,
            values,
            interpolation,
        })
    }
}

#[derive(Clone)]
pub struct AnimationPropertyChannelTypeInfo {
    pub channel_name: String,
    pub channel_id: String,
    // Type of the channel we are storing.
    pub type_info: TypeInfoCloneable,
    pub fn_caller: AnimationPropertyChannelFnCaller,
}

impl AnimationPropertyChannelTypeInfo {
    pub fn new<T: AnimationPropertyChannel + for<'de> serde::Deserialize<'de>>(
        channel_name: String,
    ) -> Self {
        Self {
            channel_name: channel_name.clone(),
            channel_id: T::ID.to_owned(),
            type_info: TypeInfoCloneable::new::<T>(),
            fn_caller: AnimationPropertyChannelFnCaller::new::<T>(),
        }
    }

    pub fn from_id(channel_id: &str, channel_name: String) -> Option<Self> {
        match channel_id {
            AnimationRadians::ID => Some(Self::new::<AnimationRadians>(channel_name)),
            f32::ID => Some(Self::new::<f32>(channel_name)),
            _ => None,
        }
    }
}

impl serde::Serialize for AnimationPropertyChannelTypeInfo {
    fn serialize<S>(&self, se: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use bitflags::__private::serde::Serializer;
        use serde::ser::SerializeStruct;
        let mut s = se.serialize_struct("AnimationPropertyChannelTypeInfo", 3)?;
        s.serialize_field("channel_name", &self.channel_name)?;
        s.serialize_field("channel_id", &self.channel_id)?;
        s.end()
    }
}

impl<'de> serde::de::Deserialize<'de> for AnimationPropertyChannelTypeInfo {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        de.deserialize_struct(
            "AnimationPropertyChannelTypeInfo",
            &["channel_name", "channel_id"],
            AnimationPropertyChannelTypeInfoVisitor,
        )
    }
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum AnimationPropertyChannelTypeInfoField {
    ChannelName,
    ChannelId,
}

struct AnimationPropertyChannelTypeInfoVisitor;

impl<'de> serde::de::Visitor<'de> for AnimationPropertyChannelTypeInfoVisitor {
    type Value = AnimationPropertyChannelTypeInfo;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("AnimationPropertyChannelTypeInfo")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut channel_name = None;
        let mut channel_id = None;
        while let Some(key) = map.next_key()? {
            match key {
                AnimationPropertyChannelTypeInfoField::ChannelName => {
                    if channel_name.is_some() {
                        return Err(serde::de::Error::duplicate_field("channel_name"));
                    }
                    channel_name = Some(map.next_value::<String>()?);
                }
                AnimationPropertyChannelTypeInfoField::ChannelId => {
                    if channel_id.is_some() {
                        return Err(serde::de::Error::duplicate_field("channel_id"));
                    }
                    channel_id = Some(map.next_value::<String>()?);
                }
            }
        }
        let channel_name =
            channel_name.ok_or_else(|| serde::de::Error::missing_field("channel_name"))?;
        let channel_id = channel_id
            .as_ref()
            .ok_or_else(|| serde::de::Error::missing_field("channel_id"))?;
        let channel_type = AnimationPropertyChannelTypeInfo::from_id(
            &channel_id,
            channel_name.clone(),
        )
        .ok_or_else(|| {
            serde::de::Error::custom(format!(
                "Unknown channel_id {} while deserializing AnimationPropertyChannelTypeInfo",
                channel_id
            ))
        })?;
        Ok(channel_type)
    }
}

type ChannelValueDeserializerFn =
    unsafe fn(*mut u8, &mut dyn erased_serde::Deserializer) -> erased_serde::Result<()>;

#[derive(Clone)]
pub struct AnimationPropertyChannelFnCaller {
    update_fn_erased: AnimationPropertyUpdateErasedFn,
    show_ui_fn_erased: AnimationPropertyShowUiErasedFn,
    partial_eq_erased: AnimationPropertyPartialEqErasedFn,
    erased_serialize_vtable: *const (),
    deserialize_erased: ChannelValueDeserializerFn,
}

// Safety: The pointers are all to static function pointers and vtables.
unsafe impl Send for AnimationPropertyChannelFnCaller {}

unsafe impl Sync for AnimationPropertyChannelFnCaller {}

impl AnimationPropertyChannelFnCaller {
    pub fn new<T: AnimationPropertyChannel + for<'de> serde::de::Deserialize<'de>>() -> Self {
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
        unsafe fn show_ui_erased<T: AnimationPropertyChannel>(
            val: *mut u8,
            ui: &mut egui::Ui,
        ) -> bool {
            let val = &mut *(val as *mut T);
            T::show_ui(val, ui)
        }

        unsafe fn partial_eq_erased<T: AnimationPropertyChannel>(
            a: *const u8,
            b: *const u8,
        ) -> bool {
            let a = &*(a as *const T);
            let b = &*(b as *const T);
            log::debug!(
                "Comparing values for channel, equal: {} {:?} {:?}",
                a == b,
                a,
                b
            );
            a == b
        }

        unsafe fn erased_deserialize_fn<
            T: AnimationPropertyChannel + for<'de> serde::de::Deserialize<'de>,
        >(
            dst_ptr: *mut u8,
            de: &mut dyn erased_serde::Deserializer,
        ) -> erased_serde::Result<()> {
            let dst_ptr = dst_ptr as *mut T;
            // Safety: dst_ptr should be allocated with the memory layout for this type.
            unsafe { dst_ptr.write(erased_serde::deserialize::<T>(de)?) };
            Ok(())
        }

        let erased_serialize_vtable = {
            // Basically copied from `ECSWorld::register_game_component`.
            // Safety: We never access the contents of the pointer, only extracting the vtable, so
            // should be okay right? Use `without_provenance_mut` since this ptr isn't actually
            // associated with a memory allocation.
            let null =
                unsafe { NonNull::new_unchecked(std::ptr::without_provenance_mut::<T>(0x1234)) };
            let dyn_ref = unsafe { null.as_ref() } as &dyn erased_serde::Serialize;
            // Safety: This reference is in fact a dyn ref, even tho the data ptr might be null :p
            let vtable_ptr =
                unsafe { vtable::get_vtable_ptr(dyn_ref as &dyn erased_serde::Serialize) };
            vtable_ptr
        };

        Self {
            update_fn_erased: update_erased::<T>,
            show_ui_fn_erased: show_ui_erased::<T>,
            partial_eq_erased: partial_eq_erased::<T>,
            erased_serialize_vtable,
            deserialize_erased: erased_deserialize_fn::<T>,
        }
    }

    pub unsafe fn update_erased(&self, dst: *mut u8, a: *const u8, b: *const u8, t: f32) {
        (self.update_fn_erased)(dst, a, b, t);
    }

    pub unsafe fn show_ui_erased(&self, val: *mut u8, ui: &mut egui::Ui) -> bool {
        (self.show_ui_fn_erased)(val, ui)
    }

    pub unsafe fn partial_eq_erased(&self, a: *const u8, b: *const u8) -> bool {
        (self.partial_eq_erased)(a, b)
    }

    pub unsafe fn deserialize_erased(
        &self,
        dst_ptr: *mut u8,
        de: &mut dyn erased_serde::Deserializer,
    ) -> erased_serde::Result<()> {
        (self.deserialize_erased)(dst_ptr, de)
    }
}

pub struct GameComponentAnimationChannelData {
    // Type of `value`, used to assert this type is the type expected for this channel name in the
    // `AnimationTrackChannel`.
    pub type_info: TypeInfoCloneable,
    // Owned data ptr.
    data_ptr: *mut u8,
}

impl GameComponentAnimationChannelData {
    pub fn new<T: Clone + 'static>(data_ptr: T) -> Self {
        let data_ptr = Box::into_raw(Box::new(data_ptr));
        Self {
            type_info: TypeInfoCloneable::new::<T>(),
            data_ptr: data_ptr as *mut u8,
        }
    }

    pub fn new_alloc(type_info: TypeInfoCloneable) -> Self {
        let data_ptr = unsafe { std::alloc::alloc(type_info.layout(1)) };
        assert!(!data_ptr.is_null(), "Failed to allocate memory.");
        Self {
            type_info,
            data_ptr,
        }
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.data_ptr as *const u8
    }
}

impl Drop for GameComponentAnimationChannelData {
    fn drop(&mut self) {
        assert!(!self.data_ptr.is_null());
        // Deallocate the owned data.
        unsafe {
            std::alloc::dealloc(self.data_ptr, self.type_info.layout(1));
        }
    }
}

/// (yaw, pitch, roll)
pub fn quat_to_euler_angles(quat: &UnitQuaternion<f32>) -> (f32, f32, f32) {
    let (roll, pitch, yaw) = quat.euler_angles();
    (yaw, pitch, roll)
}

pub fn euler_angles_to_quat(yaw: f32, pitch: f32, roll: f32) -> UnitQuaternion<f32> {
    UnitQuaternion::from_euler_angles(roll, pitch, yaw)
}

/// The animatable value.
pub trait AnimationPropertyChannel:
    Clone + PartialEq + std::fmt::Debug + erased_serde::Serialize + 'static
{
    const ID: &'static str;

    fn update(dst: &mut Self, a: &Self, b: &Self, t: f32);
    fn show_ui(val: &mut Self, ui: &mut egui::Ui) -> bool;
}

impl AnimationPropertyChannel for f32 {
    const ID: &'static str = "f32";

    fn update(dst: &mut Self, a: &Self, b: &Self, t: f32) {
        *dst = (1.0 - t) * a + t * b;
    }

    fn show_ui(val: &mut Self, ui: &mut egui::Ui) -> bool {
        return ui.add(egui::DragValue::new(val)).changed();
    }
}

// Need to do this since AnimationProperty::update relies on the type of Self which is basically a
// generic so we need an erased version of it to call.
type AnimationPropertyUpdateErasedFn = unsafe fn(dst: *mut u8, a: *const u8, b: *const u8, t: f32);
type AnimationPropertyShowUiErasedFn = unsafe fn(val: *mut u8, ui: &mut egui::Ui) -> bool;
type AnimationPropertyPartialEqErasedFn = unsafe fn(a: *const u8, b: *const u8) -> bool;
