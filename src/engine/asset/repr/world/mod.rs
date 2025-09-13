use std::{ops::Deref, os::unix::fs::FileExt};

use anyhow::bail;
use nalgebra::Vector3;

use crate::{
    common::morton,
    consts,
    engine::asset::{
        asset::{AssetFile, AssetLoadError, AssetLoader, AssetSaver},
        util::{AssetByteReader, AssetByteWriter},
    },
};

pub mod region;
