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

    pub fn one_bits(&self) -> usize {
        self.data.iter().map(|x| x.count_ones() as usize).sum()
    }

    pub fn get_bit(&self, bit: usize) -> bool {
        (self.data[bit / 32] & (1 << (bit % 32))) > 0
    }

    // Starting with the most lsb for values.
    pub fn set_bits(&mut self, start_bit: usize, count: u32, values: u32) {
        assert!(
            start_bit + count as usize <= self.bits,
            "Cannot set bits out of range of this bitset."
        );
        assert!(
            values.leading_zeros() >= 32 - count,
            "Values cannot contain more bits than in count, unused bits should be zero."
        );
        let n = start_bit / 32;
        let bit_offset = start_bit as u32 % 32;
        let mask = (1u32 << count) - 1;
        self.data[n] &= !(mask << bit_offset);
        self.data[n] |= (values & mask) << bit_offset;
        if bit_offset + count > 32 {
            let remaining_bits = count - (32 - bit_offset);
            self.data[n + 1] &= !((1 << remaining_bits) - 1);
            self.data[n + 1] |= values >> (count - (remaining_bits as u32));
        }
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
