#[derive(Clone)]
pub struct Bitset {
    data: Vec<u32>,
    bits: usize,
}

impl Bitset {
    pub fn required_size_for_count(bits: usize) -> usize {
        bits.next_multiple_of(32) / 32
    }

    pub fn new(bits: usize) -> Self {
        Self {
            data: vec![0; Self::required_size_for_count(bits)],
            bits,
        }
    }

    pub fn new_filled(bits: usize, value: u32) -> Self {
        Self {
            data: vec![value; Self::required_size_for_count(bits)],
            bits,
        }
    }

    pub fn set_bit(&mut self, bit: usize, value: bool) {
        let n = bit / 32;
        let mask = 1 << (bit % 32);
        if value {
            self.data[n] |= mask
        } else {
            self.data[n] &= !mask;
        }
    }

    pub fn get_bit(&self, bit: usize) -> bool {
        (self.data[bit / 32] & (1 << (bit % 32))) > 0
    }

    /// Length of the bitset in the number of bits.
    pub fn bits(&self) -> usize {
        self.bits
    }

    pub fn data(&self) -> &[u32] {
        self.data.as_slice()
    }

    pub fn data_mut(&mut self) -> &mut [u32] {
        self.data.as_mut_slice()
    }
}
