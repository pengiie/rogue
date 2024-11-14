use nalgebra::Vector3;

fn split(x: u32) -> u64 {
    let mut x = x as u64 & 0x1f_ffff; //            0000000000000000000000000000000000000000000111111111111111111111
    x = (x | (x << 32)) & 0x001f_0000_0000_ffff; // 0000000000011111000000000000000000000000000000001111111111111111
    x = (x | (x << 16)) & 0x001f_0000_ff00_00ff; // 0000000000011111000000000000000011111111000000000000000011111111
    x = (x | (x << 8)) & 0x100f_00f0_0f00_f00f; //  0000000100001111000000001111000000001111000000001111000000001111
    x = (x | (x << 4)) & 0x10c3_0c30_c30c_30c3; //  0001000011000011000011000011000011000011000011000011000011000011
    x = (x | (x << 2)) & 0x1249_2492_4924_9249; //  0001001001001001001001001001001001001001001001001001001001001001
    x
}

/// Compacts the starting with bit 0 skipping every two bits resulting in a 21 bit result.
fn compact(x: u64) -> u32 {
    let mut x = x & 0x1249_2492_4924_9249; //       0001001001001001001001001001001001001001001001001001001001001001
    x = (x | (x >> 2)) & 0x10c3_0c30_c30c_30c3; //  0001000011000011000011000011000011000011000011000011000011000011
    x = (x | (x >> 4)) & 0x100f_00f0_0f00_f00f; //  0000000100001111000000001111000000001111000000001111000000001111
    x = (x | (x >> 8)) & 0x001f_0000_ff00_00ff; //  0000000000011111000000000000000011111111000000000000000011111111
    x = (x | (x >> 16)) & 0x001f_0000_0000_ffff; // 0000000000011111000000000000000000000000000000001111111111111111
    x = (x | (x >> 32)) & 0x1f_ffff; //             0000000000000000000000000000000000000000000111111111111111111111
    x as u32
}

pub fn morton_encode(position: Vector3<u32>) -> u64 {
    split(position.x) | (split(position.y) << 1) | (split(position.z) << 2)
}

pub fn morton_decode(morton: u64) -> Vector3<u32> {
    Vector3::new(compact(morton), compact(morton >> 1), compact(morton >> 2))
}

pub fn morton_traversal(mut morton: u64, height: u32) -> u64 {
    let mut reverse = 0u64;
    for i in 0..height {
        reverse = (reverse << 3) | (morton & 7);
        morton >>= 3;
    }

    reverse
}

mod tests {
    use crate::common::morton::morton_traversal;

    #[test]
    fn test_traversal() {
        let a = 0x2E; // 101110
        let b = 0x35; // 110101
        assert_eq!(morton_traversal(a, 2), b);
    }
}
