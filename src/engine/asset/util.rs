use std::{
    fs::File,
    io::{Read, Seek, Write},
};

use anyhow::bail;

use crate::engine::voxel::attachment::AttachmentMap;

pub struct AssetByteWriter {
    file: File,
    bytes: Vec<u8>,
}

impl AssetByteWriter {
    pub fn new(file: std::fs::File, header: &str, version: u32) -> Self {
        assert!(header.len() == 4);
        let mut bytes = header.chars().map(|char| char as u8).collect::<Vec<u8>>();
        bytes.extend_from_slice(bytemuck::bytes_of(&version));
        Self { file, bytes }
    }

    pub fn write<T: bytemuck::Pod>(&mut self, data: &T) {
        self.bytes.extend_from_slice(bytemuck::bytes_of(data));
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub fn write_slice<T: bytemuck::Pod>(&mut self, s: &[T]) {
        self.bytes
            .extend_from_slice(bytemuck::cast_slice::<T, u8>(s));
    }

    pub fn cursor_pos(&self) -> u64 {
        self.bytes.len() as u64
    }

    // Returns the byte position.
    pub fn write_later<T: bytemuck::Pod>(&mut self) -> u64 {
        let byte_len = std::mem::size_of::<T>();
        let curr_pos = self.bytes.len();
        self.bytes.resize(curr_pos + byte_len, 0);
        return curr_pos as u64;
    }

    pub fn write_at<T: bytemuck::Pod>(&mut self, position: u64, data: &T) {
        let bytes = bytemuck::bytes_of(data);
        self.bytes.as_mut_slice()[position as usize..(position as usize + bytes.len())]
            .copy_from_slice(bytes);
    }

    pub fn finish_writes(&mut self) -> anyhow::Result<()> {
        self.file.set_len(self.bytes.len() as u64)?;
        self.file.write_all(&self.bytes)?;
        Ok(())
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

pub struct AssetByteReader {
    file: File,
    version: u32,
}

impl AssetByteReader {
    pub fn new(mut file: std::fs::File, header: &str) -> anyhow::Result<Self> {
        assert!(header.len() == 4);
        let mut bytes = header.chars().map(|char| char as u8).collect::<Vec<u8>>();
        let mut buf = [0u8; 4];
        file.read_exact(&mut buf)?;
        if (bytes.as_slice() != &buf) {
            bail!("File header bytes do not match.");
        }

        // Read the version.
        file.read_exact(&mut buf)?;
        let version = u32::from_le_bytes(buf);

        Ok(Self { file, version })
    }

    pub fn read<T: bytemuck::Pod>(&mut self) -> anyhow::Result<T> {
        let mut buf = vec![0u8; std::mem::size_of::<T>()];
        self.file.read_exact(&mut buf)?;
        return Ok(*bytemuck::from_bytes(&buf));
    }

    pub fn read_to_slice<T: bytemuck::Pod>(&mut self, buf: &mut [T]) -> anyhow::Result<()> {
        let read_bytes = self.file.read(bytemuck::cast_slice_mut::<T, u8>(buf))?;
        assert_eq!(read_bytes, buf.len() * std::mem::size_of::<T>());
        return Ok(());
    }

    pub fn cursor_pos(&mut self) -> anyhow::Result<u64> {
        Ok(self.file.stream_position()?)
    }

    pub fn version(&self) -> u32 {
        self.version
    }
}
