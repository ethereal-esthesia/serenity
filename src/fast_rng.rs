/// Fast non-cryptographic PRNG with a shared bitstream interface.
///
/// Core algorithm: `xorshift64*` (Marsaglia/Vigna family).
/// - Internal state period: `2^64 - 1` (for non-zero seed).
/// - `next_bits(1..=64)` is user-callable shared-stream access.
/// - Typed outputs (`next_u16/u32/u64/bool`) use fast cached paths.
///
/// This is designed for speed and long non-repeat state duration, not security.
#[derive(Clone, Copy, Debug)]
pub struct FastRng {
    state: u64,
    bit_buffer: u64,
    bits_left: u8,
    cache16: u64,
    left16: u8,
    cache32: u64,
    left32: u8,
    cache_bool: u64,
    left_bool: u8,
}

impl FastRng {
    /// Creates a generator from a seed.
    ///
    /// Seed value `0` is remapped to a fixed non-zero constant so the generator
    /// always advances.
    pub fn new(seed: u64) -> Self {
        let state = if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed };
        Self {
            state,
            bit_buffer: 0,
            bits_left: 0,
            cache16: 0,
            left16: 0,
            cache32: 0,
            left32: 0,
            cache_bool: 0,
            left_bool: 0,
        }
    }

    /// Returns the next `bits` pseudorandom bits as an unsigned value.
    ///
    /// Valid range: `1..=64`.
    /// Returned bits are packed in the low `bits` of the return value.
    #[inline]
    pub fn next_bits(&mut self, bits: u8) -> u64 {
        assert!((1..=64).contains(&bits), "bits must be in 1..=64");
        let mut remaining = bits;
        let mut out = 0u64;

        while remaining > 0 {
            if self.bits_left == 0 {
                self.bit_buffer = self.next_word64();
                self.bits_left = 64;
            }

            let take = remaining.min(self.bits_left);
            let shift = self.bits_left - take;
            let mask = mask_low_bits(take);
            let chunk = (self.bit_buffer >> shift) & mask;

            if take == 64 {
                out = chunk;
            } else {
                out = (out << take) | chunk;
            }
            self.bits_left -= take;
            remaining -= take;
        }

        out
    }

    /// Returns the next 16-bit pseudorandom value.
    #[inline]
    pub fn next_u16(&mut self) -> u16 {
        if self.left16 == 0 {
            self.cache16 = self.next_word64();
            self.left16 = 4;
        }
        let shift = (self.left16 - 1) * 16;
        self.left16 -= 1;
        ((self.cache16 >> shift) & 0xFFFF) as u16
    }

    /// Returns the next 32-bit pseudorandom value.
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        if self.left32 == 0 {
            self.cache32 = self.next_word64();
            self.left32 = 2;
        }
        let shift = (self.left32 - 1) * 32;
        self.left32 -= 1;
        ((self.cache32 >> shift) & 0xFFFF_FFFF) as u32
    }

    /// Returns the next 64-bit pseudorandom value.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.next_bits(64)
    }

    /// Returns the next pseudorandom boolean.
    #[inline]
    pub fn next_bool(&mut self) -> bool {
        if self.left_bool == 0 {
            self.cache_bool = self.next_word64();
            self.left_bool = 64;
        }
        let bit = (self.cache_bool >> 63) != 0;
        self.cache_bool <<= 1;
        self.left_bool -= 1;
        bit
    }

    /// Returns the internal state for diagnostics/testing.
    #[inline]
    pub fn state(&self) -> u64 {
        self.state
    }

    #[inline]
    fn next_word64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

#[inline]
fn mask_low_bits(bits: u8) -> u64 {
    if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

#[cfg(test)]
mod tests {
    use super::FastRng;
    use std::collections::HashSet;
    use std::time::Instant;

    #[derive(Clone, Copy, Debug)]
    struct LegacyFastRng {
        state: u64,
        cache16: u64,
        left16: u8,
        cache32: u64,
        left32: u8,
        cache_bool: u64,
        left_bool: u8,
    }

    impl LegacyFastRng {
        fn new(seed: u64) -> Self {
            let state = if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed };
            Self {
                state,
                cache16: 0,
                left16: 0,
                cache32: 0,
                left32: 0,
                cache_bool: 0,
                left_bool: 0,
            }
        }

        fn next_word64(&mut self) -> u64 {
            let mut x = self.state;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.state = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn next_u16(&mut self) -> u16 {
            if self.left16 == 0 {
                self.cache16 = self.next_word64();
                self.left16 = 4;
            }
            let shift = (self.left16 - 1) * 16;
            self.left16 -= 1;
            ((self.cache16 >> shift) & 0xFFFF) as u16
        }

        fn next_u32(&mut self) -> u32 {
            if self.left32 == 0 {
                self.cache32 = self.next_word64();
                self.left32 = 2;
            }
            let shift = (self.left32 - 1) * 32;
            self.left32 -= 1;
            ((self.cache32 >> shift) & 0xFFFF_FFFF) as u32
        }

        fn next_u64(&mut self) -> u64 {
            self.next_word64()
        }

        fn next_bool(&mut self) -> bool {
            if self.left_bool == 0 {
                self.cache_bool = self.next_word64();
                self.left_bool = 64;
            }
            let bit = (self.cache_bool >> 63) != 0;
            self.cache_bool <<= 1;
            self.left_bool -= 1;
            bit
        }
    }

    #[test]
    fn deterministic_u16_100_values_seed_one() {
        let mut rng = FastRng::new(1);
        let got: Vec<u16> = (0..100).map(|_| rng.next_u16()).collect();
        let want = vec![
            18404, 52811, 35180, 56605, 43983, 42664, 57465, 25885, 47569, 3471, 60275, 8023, 19892, 6304, 47899,
            413, 3681, 39344, 19802, 42496, 51303, 19403, 17123, 43737, 53330, 45784, 54382, 29057, 44145, 36088,
            52785, 14733, 22194, 45346, 59720, 12344, 49146, 45624, 16973, 14997, 32691, 14449, 24252, 11486,
            11603, 26223, 36059, 47772, 10150, 49393, 20433, 21008, 48245, 11760, 49829, 39679, 57329, 51528,
            60514, 7777, 41449, 14843, 37518, 3633, 12061, 42500, 45627, 98, 5636, 24438, 12419, 1714, 33006,
            9254, 35214, 20904, 58132, 49549, 38433, 10945, 31231, 49122, 23436, 64919, 20190, 12397, 43439, 9297,
            43775, 28227, 33634, 11162, 6471, 61276, 8051, 25704, 13198, 6107, 2383, 62298,
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn deterministic_u32_100_values_seed_one() {
        let mut rng = FastRng::new(1);
        let got: Vec<u32> = (0..100).map(|_| rng.next_u32()).collect();
        let want = vec![
            1206177355, 2305613085, 2882512552, 3766052125, 3117485455, 3950190423, 1303648416, 3139109277,
            241277360, 1297786368, 3362212811, 1122216665, 3495080664, 3564007809, 2893122808, 3459332493,
            1454551330, 3913822264, 3220877880, 1112357525, 2142451825, 1589390558, 760440431, 2363210396,
            665239793, 1339118096, 3161796080, 3265633023, 3757164872, 3965853281, 2716416507, 2458783281,
            790472196, 2990211170, 369385334, 813893298, 2163090470, 2307805608, 3809788301, 2518756033,
            2046803938, 1535966615, 1323184237, 2846827601, 2868866627, 2204248986, 424144732, 527656040,
            864950235, 156234586, 3629865355, 1896183576, 2259644924, 1800129870, 3802126848, 4344695, 1624164765,
            57283470, 1196917027, 1644184286, 1200209218, 3500219380, 1433451912, 24606094, 3937712542, 2952623507,
            1144922877, 2111727739, 1252007992, 2026824914, 154181161, 1818716517, 30745847, 505539047, 1420417193,
            1960973545, 2318660238, 1208575440, 1593341701, 2483401811, 3234753069, 2859289403, 1055613557,
            1816760021, 2445364992, 2793786888, 3376974990, 3349542603, 1719368924, 1721014145, 1181879744,
            1203818629, 3614340549, 994169933, 2386603773, 2337734414, 4212058701, 1125617168, 3571637491,
            2723598058,
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn deterministic_u64_100_values_seed_one() {
        let mut rng = FastRng::new(1);
        let got: Vec<u64> = (0..100).map(|_| rng.next_u64()).collect();
        let want = vec![
            5180492295206395165, 12380297144915551517, 13389498078930870103, 5599127315341312413, 1036278371763004928,
            14440594066559445721, 15011257152325972353, 12425867847131019661, 6247250396617125944, 13833565160122170005,
            9201760523219905758, 3266066784064354972, 2857183156271927824, 13579810763486632703, 16136900254885879393,
            11666920062338338353, 3395052233207513186, 1586497929965930162, 9290402829247074728, 16362916159997160129,
            8790955976569978263, 5683033027344540753, 12321688341755079578, 1821687753238340712, 3714932972148749146,
            15590152990504613656, 9705101050952535374, 16330010467407907703, 6975734549047808910, 5140719488634733278,
            5154859343167953908, 6156629082453276046, 16912346591891649939, 4917406315268958331, 5377333381997454546,
            662203045973027173, 132052407858358759, 6100645392572093673, 9958569893954151888, 6843350499631412307,
            13893158644849920827, 4533825706345991893, 10502762670217088520, 14503997144809469643, 7384633300059723649,
            5076134849488670853, 15523474455555855437, 10250385155882942222, 18090654370752859664, 15340066219736092394,
            9292306551185417010, 9710099824454476361, 10950204648648239645, 18265202189753212147, 12938298868361205183,
            1097506238823554310, 3981025109545545383, 9614443989403512475, 14687928383637241122, 14509198178551166940,
            1283240728273926319, 14884106528519911077, 7920655668905146242, 11162609671198085556, 9614685027640453186,
            4514369400360770984, 7914601462538430019, 18423373093828771847, 11893600306064336880, 9851121316524906020,
            6007322564067837536, 3097785403020755619, 1002057477822110378, 11978821425635806102, 11225768358660346166,
            11200210501376489628, 14675923873205262360, 11162536626324036860, 18139014884728417860, 10283637037437045201,
            4896378486469952264, 17648678288878106362, 12879487607590523534, 16421194008812108288, 2654824939079830204,
            10357621024011365755, 755355639912490571, 785755645652698595, 8465687434013315149, 18385364471079630273,
            17850228928978541911, 3634680862140810320, 12093154495361685689, 4113871482182316095, 17302682078131068095,
            14053149880551416423, 5795942151951570562, 10986100478148725124, 10672003212881643257, 15161545059816271271,
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn deterministic_bool_100_values_seed_one() {
        let mut rng = FastRng::new(1);
        let got: Vec<u8> = (0..100).map(|_| u8::from(rng.next_bool())).collect();
        let want = vec![
            0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 1,
            0, 0, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0,
            1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0,
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn next_bits_validates_range() {
        let mut rng = FastRng::new(1);
        assert_eq!(rng.next_bits(64), 5180492295206395165);
    }

    #[test]
    #[should_panic]
    fn next_bits_panics_on_zero() {
        let mut rng = FastRng::new(1);
        let _ = rng.next_bits(0);
    }

    #[test]
    fn legacy_and_refactor_match_when_called_per_type() {
        let mut old = LegacyFastRng::new(0x1234_5678_9ABC_DEF0);
        let mut new = FastRng::new(0x1234_5678_9ABC_DEF0);
        for _ in 0..10_000 {
            assert_eq!(new.next_u16(), old.next_u16());
        }

        let mut old = LegacyFastRng::new(0x1234_5678_9ABC_DEF0);
        let mut new = FastRng::new(0x1234_5678_9ABC_DEF0);
        for _ in 0..10_000 {
            assert_eq!(new.next_u32(), old.next_u32());
        }

        let mut old = LegacyFastRng::new(0x1234_5678_9ABC_DEF0);
        let mut new = FastRng::new(0x1234_5678_9ABC_DEF0);
        for _ in 0..10_000 {
            assert_eq!(new.next_u64(), old.next_u64());
        }

        let mut old = LegacyFastRng::new(0x1234_5678_9ABC_DEF0);
        let mut new = FastRng::new(0x1234_5678_9ABC_DEF0);
        for _ in 0..100_000 {
            assert_eq!(new.next_bool(), old.next_bool());
        }
    }

    #[test]
    fn zero_seed_is_remapped_and_advances() {
        let mut rng = FastRng::new(0);
        assert_ne!(rng.state(), 0);
        let a = rng.next_u16();
        let b = rng.next_u16();
        assert_ne!(a, b);
        assert_ne!(rng.state(), 0);
    }

    #[test]
    fn different_seeds_diverge_quickly() {
        let mut a = FastRng::new(1);
        let mut b = FastRng::new(2);
        let seq_a: Vec<u16> = (0..64).map(|_| a.next_u16()).collect();
        let seq_b: Vec<u16> = (0..64).map(|_| b.next_u16()).collect();
        assert_ne!(seq_a, seq_b);
    }

    #[test]
    fn long_run_state_no_repeats_in_large_window() {
        let mut rng = FastRng::new(0x1234_5678_9ABC_DEF0);
        let mut seen = HashSet::with_capacity(200_000);
        for _ in 0..200_000 {
            let before = rng.state();
            assert!(seen.insert(before), "state repeated early");
            let _ = rng.next_u64();
        }
    }

    #[test]
    fn bit_balance_sanity() {
        let mut rng = FastRng::new(0xA5A5_5A5A_D3C3_B4B4);
        let samples = 250_000usize;
        let mut ones = 0usize;
        for _ in 0..samples {
            ones += rng.next_u16().count_ones() as usize;
        }

        let total_bits = samples * 16;
        let ratio = ones as f64 / total_bits as f64;
        assert!((0.49..0.51).contains(&ratio), "bit ratio out of range: {ratio}");
    }

    #[test]
    fn high_byte_histogram_sanity() {
        let mut rng = FastRng::new(0xDEAD_BEEF_CAFE_BABE);
        let mut buckets = [0usize; 256];
        let samples = 262_144usize;

        for _ in 0..samples {
            let v = rng.next_u16();
            buckets[(v >> 8) as usize] += 1;
        }

        let expected = samples as f64 / 256.0;
        let mut chi2 = 0.0f64;
        for &count in &buckets {
            let d = count as f64 - expected;
            chi2 += (d * d) / expected;
        }

        // Very loose sanity bound for df=255. We only want to catch severe bias.
        assert!(chi2 > 120.0 && chi2 < 420.0, "chi2 sanity failed: {chi2}");
    }

    #[test]
    fn mixed_calls_consume_shared_stream() {
        let mut rng = FastRng::new(1);
        let a = rng.next_bits(1);
        let b = rng.next_bits(15);
        let c = rng.next_u16();
        // `next_bits` uses the shared bitstream and consumes from the first word:
        // 0x47E4CE4B896CDD15 -> 1 bit (0), then 15 bits (0x47E4).
        // Typed outputs use independent fast-path caches; `next_u16` then starts
        // from its own next generated word and yields 0xABCF.
        assert_eq!(a, 0);
        assert_eq!(b, 0x47E4);
        assert_eq!(c, 0xABCF);
    }

    #[test]
    #[ignore = "benchmark: run with `cargo test benchmark_legacy_vs_refactor -- --ignored --nocapture`"]
    fn benchmark_legacy_vs_refactor() {
        const N16: usize = 8_000_000;
        const N32: usize = 8_000_000;
        const NB: usize = 16_000_000;

        let mut old = LegacyFastRng::new(0xDEAD_BEEF_CAFE_BABE);
        let start_old = Instant::now();
        let mut sum_old = 0u64;
        for _ in 0..N16 {
            sum_old = sum_old.wrapping_add(old.next_u16() as u64);
        }
        for _ in 0..N32 {
            sum_old = sum_old.wrapping_add(old.next_u32() as u64);
        }
        for _ in 0..NB {
            sum_old ^= old.next_bool() as u64;
        }
        let old_dur = start_old.elapsed();

        let mut new = FastRng::new(0xDEAD_BEEF_CAFE_BABE);
        let start_new = Instant::now();
        let mut sum_new = 0u64;
        for _ in 0..N16 {
            sum_new = sum_new.wrapping_add(new.next_u16() as u64);
        }
        for _ in 0..N32 {
            sum_new = sum_new.wrapping_add(new.next_u32() as u64);
        }
        for _ in 0..NB {
            sum_new ^= new.next_bool() as u64;
        }
        let new_dur = start_new.elapsed();

        assert_eq!(sum_old, sum_new, "output checksum mismatch old vs new");
        eprintln!(
            "legacy={:?} refactor={:?} speedup={:.3}x checksum={}",
            old_dur,
            new_dur,
            old_dur.as_secs_f64() / new_dur.as_secs_f64(),
            sum_new
        );
    }
}
