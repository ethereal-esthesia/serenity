use std::sync::OnceLock;
use std::time::Instant;

pub const INPUT_TIMESTAMP_PAYLOAD_MASK: u64 = 0x7FFF_FFFF_FFFF_FFFF;
static TS_START: OnceLock<Instant> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct IoTimestamp(u64);

impl IoTimestamp {
    pub fn current_time() -> Self {
        let start = TS_START.get_or_init(Instant::now);
        Self((start.elapsed().as_nanos() as u64) & INPUT_TIMESTAMP_PAYLOAD_MASK)
    }

    pub fn now() -> Self {
        Self::current_time()
    }

    pub fn from_raw(raw: u64) -> Self {
        Self(raw & INPUT_TIMESTAMP_PAYLOAD_MASK)
    }

    pub fn raw(self) -> u64 {
        self.0
    }

    pub fn wrapping_add(self, delta: u64) -> Self {
        Self::from_raw(self.0.wrapping_add(delta))
    }

    pub fn wrapping_delta_since(self, earlier: Self) -> u64 {
        self.0.wrapping_sub(earlier.0) & INPUT_TIMESTAMP_PAYLOAD_MASK
    }
}

#[cfg(test)]
mod tests {
    use super::{INPUT_TIMESTAMP_PAYLOAD_MASK, IoTimestamp};

    #[test]
    fn input_timestamp_masks_top_bit_on_from_raw() {
        let raw = u64::MAX;
        let ts = IoTimestamp::from_raw(raw);
        assert_eq!(ts.raw(), INPUT_TIMESTAMP_PAYLOAD_MASK);
    }

    #[test]
    fn input_timestamp_wrapping_add_rolls_at_payload_boundary() {
        let near_wrap = IoTimestamp::from_raw(INPUT_TIMESTAMP_PAYLOAD_MASK - 2);
        let wrapped = near_wrap.wrapping_add(5);
        assert_eq!(wrapped.raw(), 2);
    }

    #[test]
    fn input_timestamp_wrapping_delta_handles_rollover() {
        let earlier = IoTimestamp::from_raw(INPUT_TIMESTAMP_PAYLOAD_MASK - 5);
        let later = IoTimestamp::from_raw(3);
        assert_eq!(later.wrapping_delta_since(earlier), 9);
    }

    #[test]
    fn input_timestamp_wrapping_delta_zero_for_same_value() {
        let ts = IoTimestamp::from_raw(123_456);
        assert_eq!(ts.wrapping_delta_since(ts), 0);
    }
}
