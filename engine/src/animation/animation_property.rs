use crate::animation::animation::AnimationRadians;
use crate::animation::animation_channel;
use crate::animation::animation_channel::{
    AnimationPropertyChannel, AnimationPropertyChannelTypeInfo, GameComponentAnimationChannelData,
};
use std::collections::HashMap;

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

pub struct AnimationPropertyApplyData {
    pub channel_values: HashMap<
        String,
        (
            /*owned_value_ptr*/ *mut u8,
            AnimationPropertyChannelTypeInfo,
        ),
    >,
}

impl AnimationPropertyApplyData {
    pub fn new() -> Self {
        Self {
            channel_values: HashMap::new(),
        }
    }

    // Safety: value_ptr must be pointer owned pointer which transfers ownership to this struct.
    pub unsafe fn add_channel_value(
        &mut self,
        value_ptr: *mut u8,
        channel_type_info: AnimationPropertyChannelTypeInfo,
    ) {
        self.channel_values.insert(
            channel_type_info.channel_name.clone(),
            (value_ptr, channel_type_info),
        );
    }

    pub fn get_channel<T: AnimationPropertyChannel>(&self, channel_name: &str) -> Option<T> {
        let (value_ptr, channel_type_info) = self.channel_values.get(channel_name)?;
        assert_eq!(
            channel_type_info.type_info.type_id(),
            std::any::TypeId::of::<T>(),
            "Channel {} is not of type {}",
            channel_name,
            std::any::type_name::<T>(),
        );
        // Safety: We assert the type of the channel value is T.
        let val = unsafe { &*((*value_ptr) as *const T) }.clone();
        Some(val)
    }
}

impl Drop for AnimationPropertyApplyData {
    fn drop(&mut self) {
        for (_, (value_ptr, channel_type_info)) in self.channel_values.iter() {
            // We need to deallocate the memory we allocated for the channel values after applying
            // the animation.
            unsafe {
                std::alloc::dealloc(*value_ptr, channel_type_info.type_info.layout(1));
            }
        }
    }
}

/// The grouping of keyframe channels which an entity component exposes.
pub trait AnimationProperty: Clone {
    const ID: &'static str;

    fn channels() -> Vec<AnimationPropertyChannelTypeInfo>;
    fn get_channel_data(&self, channel: &str) -> GameComponentAnimationChannelData;
    unsafe fn set_channel_data(&mut self, channel: &str, new_value: *const u8);
    fn set_property_data(&mut self, property_data: AnimationPropertyApplyData);
}

pub trait AnimationPropertyMethods {
    fn get_channel_data(&self, channel: &str) -> GameComponentAnimationChannelData;
    unsafe fn set_channel_data(&mut self, channel: &str, new_value: *const u8);
    fn set_property_data(&mut self, property_data: AnimationPropertyApplyData);
}

impl<T: AnimationProperty> AnimationPropertyMethods for T {
    fn get_channel_data(&self, channel: &str) -> GameComponentAnimationChannelData {
        AnimationProperty::get_channel_data(self, channel)
    }

    unsafe fn set_channel_data(&mut self, channel: &str, new_value: *const u8) {
        AnimationProperty::set_channel_data(self, channel, new_value)
    }

    fn set_property_data(&mut self, property_data: AnimationPropertyApplyData) {
        AnimationProperty::set_property_data(self, property_data)
    }
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

    fn get_channel_data(&self, channel: &str) -> GameComponentAnimationChannelData {
        match channel {
            "x" => GameComponentAnimationChannelData::new(self.x),
            "y" => GameComponentAnimationChannelData::new(self.y),
            "z" => GameComponentAnimationChannelData::new(self.z),
            _ => panic!("No channel named {} for Vector3<f32>", channel),
        }
    }

    unsafe fn set_channel_data(&mut self, channel: &str, new_value: *const u8) {
        let new_value = *(new_value as *const f32);
        match channel {
            "x" => self.x = new_value,
            "y" => self.y = new_value,
            "z" => self.z = new_value,
            _ => panic!("No channel named {} for Vector3<f32>", channel),
        }
    }

    fn set_property_data(&mut self, property_data: AnimationPropertyApplyData) {
        self.x = property_data.get_channel::<f32>("x").unwrap_or(self.x);
        self.y = property_data.get_channel::<f32>("y").unwrap_or(self.y);
        self.z = property_data.get_channel::<f32>("z").unwrap_or(self.z);
    }
}

impl AnimationProperty for nalgebra::UnitQuaternion<f32> {
    const ID: &'static str = "UnitQuaternion<f32>";

    fn channels() -> Vec<AnimationPropertyChannelTypeInfo> {
        vec![
            AnimationPropertyChannelTypeInfo::new::<AnimationRadians>("pitch".to_owned()),
            AnimationPropertyChannelTypeInfo::new::<AnimationRadians>("yaw".to_owned()),
            AnimationPropertyChannelTypeInfo::new::<AnimationRadians>("roll".to_owned()),
        ]
    }

    fn get_channel_data(&self, channel: &str) -> GameComponentAnimationChannelData {
        let (yaw, pitch, roll) = animation_channel::quat_to_euler_angles(self);
        let (yaw, pitch, roll) = (
            AnimationRadians(yaw),
            AnimationRadians(pitch),
            AnimationRadians(roll),
        );
        match channel {
            "pitch" => GameComponentAnimationChannelData::new(pitch),
            "yaw" => GameComponentAnimationChannelData::new(yaw),
            "roll" => GameComponentAnimationChannelData::new(roll),
            _ => panic!("No channel named {} for UnitQuaternion<f32>", channel),
        }
    }

    unsafe fn set_channel_data(&mut self, channel: &str, new_value: *const u8) {
        let new_value = *(new_value as *const AnimationRadians);
        let (mut yaw, mut pitch, mut roll) = animation_channel::quat_to_euler_angles(self);
        let (mut yaw, mut pitch, mut roll) = (
            AnimationRadians(yaw),
            AnimationRadians(pitch),
            AnimationRadians(roll),
        );
        match channel {
            "pitch" => {
                pitch = new_value;
            }
            "yaw" => {
                yaw = new_value;
            }
            "roll" => {
                roll = new_value;
            }
            _ => panic!("No channel named {} for UnitQuaternion<f32>", channel),
        }
        *self = animation_channel::euler_angles_to_quat(yaw.0, pitch.0, roll.0);
    }

    fn set_property_data(&mut self, property_data: AnimationPropertyApplyData) {
        let (o_yaw, o_pitch, o_roll) = animation_channel::quat_to_euler_angles(self);
        let (o_yaw, o_pitch, o_roll) = (
            AnimationRadians(o_yaw),
            AnimationRadians(o_pitch),
            AnimationRadians(o_roll),
        );
        let yaw = property_data
            .get_channel::<AnimationRadians>("yaw")
            .unwrap_or(o_yaw);
        let pitch = property_data
            .get_channel::<AnimationRadians>("pitch")
            .unwrap_or(o_pitch);
        let roll = property_data
            .get_channel::<AnimationRadians>("roll")
            .unwrap_or(o_roll);
        *self = animation_channel::euler_angles_to_quat(yaw.0, pitch.0, roll.0);
    }
}
