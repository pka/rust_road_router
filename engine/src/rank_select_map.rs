// this module contains a data structure to efficiently map a large noncensecutive
// global id space into a smaller consecutive id space from 0 to n where the order is preserved

use std::mem::size_of;
use std::cmp::min;

use std::heap::{Heap, Alloc, Layout};

// the number of bytes in one L1 cache line
// hardcoded for now, no idea, if we can retreave it during compilation
const CACHE_LINE_WIDTH: usize = 64; // bytes

// number of bits (not bytes) in one 64 bit uint
const STORAGE_BITS: usize = size_of::<u64>() * 8;

// this is a little helper data structure
// what we need is a vector of bits with a few extras
// - it needs to be efficiently, no idea if rust is currently doing that,
//   could be that it uses one byte per bit, I couldnt find any definitive answer in the docs
// - we need access to the ints containing the actualy bits, the do popcount on them
// - the memory needs to be aligned to the cache line width
// -> create a thin wrapper around a Vec<u64>, where we do the first allocation ourselves, and have operations to set individual bits

#[derive(Debug)]
pub struct BitVec {
    data: Vec<u64>,
    size: usize
}

impl BitVec {
    pub fn new(size: usize) -> BitVec {
        // ceiling to the right number of u64s
        let num_ints = (size + STORAGE_BITS - 1) / STORAGE_BITS;
        let data = unsafe {
            let pointer = Heap.alloc_zeroed(Layout::from_size_align(num_ints * size_of::<usize>(), CACHE_LINE_WIDTH).unwrap()).unwrap();
            // TODO: freeing will supply a different alignment (the one of u64)
            // appearently this is not a problem, but it could be some day
            // so we probably should also do the dropping ourselves
            Vec::from_raw_parts(pointer as *mut u64, num_ints, num_ints)
        };

        BitVec { data, size }
    }

    pub fn get(&self, index: usize) -> bool {
        assert!(index < self.size, "index: {} size: {}", index, self.size);
        // shifting a 1 bit to the right place and masking
        self.data[index / STORAGE_BITS] & (1 << (index % STORAGE_BITS)) != 0
    }

    pub fn set(&mut self, index: usize) {
        assert!(index < self.size, "index: {} size: {}", index, self.size);
        // shifting a 1 bit to the right place and then eighter set through | or negate and unset with &
        self.data[index / STORAGE_BITS] |= 1 << (index % STORAGE_BITS);
    }

    pub fn unset(&mut self, index: usize) {
        assert!(index < self.size, "index: {} size: {}", index, self.size);
        // shifting a 1 bit to the right place and then eighter set through | or negate and unset with &
        self.data[index / STORAGE_BITS] &= !(1 << (index % STORAGE_BITS));
    }

    pub fn len(&self) -> usize {
        self.size
    }
}

// the actual map data structure.
// made up of the bitvec with one bit for each global id
// and an vec containing prefix sums of the number of elements
// for each global id in our id space we set the corresponding bit in the bitvector
// the local id is then the number of bits set in the bitvector before the id itself
// the prefix sum array is there so we do not need to always count everything before
// but just the ones in the current cache line, since everything before is
// already counted in the prefix sum. Conveniently couting ones in the ids cache line
// is super fast. Since updates require us to update the prefixes we use the data structure
// in two phases, first insert, and then after compile access

#[derive(Debug)]
pub struct RankSelectMap {
    contained_keys_flags: BitVec,
    prefix_sum: Vec<usize>,
}

const BITS_PER_PREFIX: usize = CACHE_LINE_WIDTH * 8;
const INTS_PER_PREFIX: usize = BITS_PER_PREFIX / STORAGE_BITS;

impl RankSelectMap {
    pub fn new(bit_vec: BitVec) -> RankSelectMap {
        let max_index = bit_vec.len();
        let mut this = RankSelectMap {
            contained_keys_flags: bit_vec,
            // the number of elements in the prefix vector is ceiled and one extra element
            // is added in the back containing the total number of elements
            prefix_sum: vec![0; (max_index + BITS_PER_PREFIX - 1) / BITS_PER_PREFIX + 1],
        };
        this.compile();
        this
    }

    pub fn len(&self) -> usize {
        match self.prefix_sum.last() {
            Some(&len) => len,
            None => 0,
        }
    }

    fn compile(&mut self) {
        let mut previous = 0;
        // we start from one here, since the first prefix is always zero, and it saves some corner case handling
        for index in 1..self.prefix_sum.len() {
            self.prefix_sum[index] = self.bit_count_entire_range(index - 1) + previous;
            previous = self.prefix_sum[index];
        }
    }

    fn bit_count_entire_range(&self, range_index: usize) -> usize {
        // make sure to not go over the edge
        let range = (range_index * INTS_PER_PREFIX)..min((range_index + 1) * INTS_PER_PREFIX, self.contained_keys_flags.data.len());
        // count_ones will use POPCNT when -C target-cpu=native is set, so this should be crazy fast, since it's all in the cache
        // TODO investigate assembler
        self.contained_keys_flags.data[range].iter().map(|num| num.count_ones() as usize).sum()
    }

    pub fn at(&self, key: usize) -> usize {
        assert!(self.contained_keys_flags.get(key));
        self.prefix_sum[key / BITS_PER_PREFIX] + self.bit_count_partial_range(key)
    }

    pub fn at_or_next_lower(&self, key: usize) -> usize {
        let value = self.prefix_sum[key / BITS_PER_PREFIX] + self.bit_count_partial_range(key);
        if self.contained_keys_flags.get(key) {
            value
        } else {
            value - 1
        }
    }

    pub fn get(&self, key: usize) -> Option<usize> {
        if key < self.contained_keys_flags.size && self.contained_keys_flags.get(key) {
            Some(self.prefix_sum[key / BITS_PER_PREFIX] + self.bit_count_partial_range(key))
        } else {
            None
        }
    }

    fn bit_count_partial_range(&self, key: usize) -> usize {
        let index = key / STORAGE_BITS; // the index of the number containing the bit
        let num = (self.contained_keys_flags.data[index] % (1 << (key % STORAGE_BITS))).count_ones() as usize; // num ones in the number

        let range = ((index / INTS_PER_PREFIX) * INTS_PER_PREFIX)..index; // the range over the numbers before our number
        let sum: usize = self.contained_keys_flags.data[range].iter().map(|num| num.count_ones() as usize).sum(); // num ones in that range
        sum + num
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_and_fill_map() -> RankSelectMap {
        let mut bits = BitVec::new(1000);
        bits.set(31);
        bits.set(52);
        bits.set(2);
        bits.set(130);
        bits.set(0);
        bits.set(149);
        bits.set(999);
        RankSelectMap::new(bits)
    }

    #[test]
    fn at_or_next_lower() {
        let map = create_and_fill_map();
        assert_eq!(map.at_or_next_lower(0), 0);
        assert_eq!(map.at_or_next_lower(1), 0);
        assert_eq!(map.at_or_next_lower(2), 1);
        assert_eq!(map.at_or_next_lower(3), 1);
        assert_eq!(map.at_or_next_lower(52), 3);
    }

    #[test]
    fn test_at() {
        let map = create_and_fill_map();
        assert_eq!(map.at(0), 0);
        assert_eq!(map.at(2), 1);
        assert_eq!(map.at(52), 3);
        assert_eq!(map.at(130), 4);
        assert_eq!(map.at(149), 5);
        assert_eq!(map.at(999), 6);
    }

    #[test]
    fn test_get() {
        let map = create_and_fill_map();
        assert_eq!(map.get(31), Some(2));
        assert_eq!(map.get(32), None);
    }

    #[test]
    fn test_len() {
        let map = create_and_fill_map();
        assert_eq!(map.len(), 7);
    }

    #[test]
    fn test_bug() {
        let mut bits = BitVec::new(1000);
        bits.set(0);
        bits.set(64);
        let map = RankSelectMap::new(bits);
        assert_eq!(map.at(0), 0);
        assert_eq!(map.at(64), 1);
    }
}