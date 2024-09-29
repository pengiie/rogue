pub struct Bitset {
    data: Vec<u32>,
}

impl Bitset {
    pub fn new(bits: usize) -> Self {
        Self {
            data: vec![0; bits.next_multiple_of(32)],
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
}
