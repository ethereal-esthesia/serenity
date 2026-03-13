#[derive(Debug, Clone, Copy)]
pub struct TimedFrameRef<'a> {
    pub timestamp_ns: u64,
    pub pixels: &'a [u16],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterpolatedFrame {
    pub timestamp_ns: u64,
    pub pixels: Vec<u16>,
}

pub struct FrameInterpolator;

impl FrameInterpolator {
    pub fn interpolate_u16(
        frames: &[TimedFrameRef<'_>],
        target_timestamp_ns: u64,
    ) -> Option<InterpolatedFrame> {
        if frames.is_empty() {
            return None;
        }
        let len = frames[0].pixels.len();
        if frames.iter().any(|f| f.pixels.len() != len) {
            return None;
        }

        let mut ordered: Vec<TimedFrameRef<'_>> = frames.to_vec();
        ordered.sort_by_key(|f| f.timestamp_ns);

        if target_timestamp_ns <= ordered[0].timestamp_ns {
            return Some(InterpolatedFrame {
                timestamp_ns: target_timestamp_ns,
                pixels: ordered[0].pixels.to_vec(),
            });
        }
        let last = ordered.len() - 1;
        if target_timestamp_ns >= ordered[last].timestamp_ns {
            return Some(InterpolatedFrame {
                timestamp_ns: target_timestamp_ns,
                pixels: ordered[last].pixels.to_vec(),
            });
        }

        for pair in ordered.windows(2) {
            let a = pair[0];
            let b = pair[1];
            if target_timestamp_ns < a.timestamp_ns || target_timestamp_ns > b.timestamp_ns {
                continue;
            }
            if target_timestamp_ns == a.timestamp_ns {
                return Some(InterpolatedFrame {
                    timestamp_ns: target_timestamp_ns,
                    pixels: a.pixels.to_vec(),
                });
            }
            if target_timestamp_ns == b.timestamp_ns {
                return Some(InterpolatedFrame {
                    timestamp_ns: target_timestamp_ns,
                    pixels: b.pixels.to_vec(),
                });
            }
            let span = b.timestamp_ns.saturating_sub(a.timestamp_ns);
            if span == 0 {
                return Some(InterpolatedFrame {
                    timestamp_ns: target_timestamp_ns,
                    pixels: b.pixels.to_vec(),
                });
            }
            let alpha = (target_timestamp_ns.saturating_sub(a.timestamp_ns)) as f64 / span as f64;
            let mut out = Vec::with_capacity(len);
            for i in 0..len {
                let av = a.pixels[i] as f64;
                let bv = b.pixels[i] as f64;
                let v = av + (bv - av) * alpha;
                out.push(v.round().clamp(0.0, u16::MAX as f64) as u16);
            }
            return Some(InterpolatedFrame {
                timestamp_ns: target_timestamp_ns,
                pixels: out,
            });
        }

        // Should be unreachable due to boundary clamps above.
        Some(InterpolatedFrame {
            timestamp_ns: target_timestamp_ns,
            pixels: ordered[last].pixels.to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameInterpolator, TimedFrameRef};

    #[test]
    fn interpolate_none_for_empty_input() {
        assert!(FrameInterpolator::interpolate_u16(&[], 100).is_none());
    }

    #[test]
    fn interpolate_none_for_mismatched_buffer_lengths() {
        let a = [1u16, 2u16, 3u16];
        let b = [4u16, 5u16];
        let frames = [
            TimedFrameRef {
                timestamp_ns: 0,
                pixels: &a,
            },
            TimedFrameRef {
                timestamp_ns: 10,
                pixels: &b,
            },
        ];
        assert!(FrameInterpolator::interpolate_u16(&frames, 5).is_none());
    }

    #[test]
    fn interpolate_single_frame_passthrough() {
        let a = [10u16, 20u16, 30u16];
        let frames = [TimedFrameRef {
            timestamp_ns: 1000,
            pixels: &a,
        }];
        let out = FrameInterpolator::interpolate_u16(&frames, 2000).expect("frame");
        assert_eq!(out.pixels, a);
        assert_eq!(out.timestamp_ns, 2000);
    }

    #[test]
    fn interpolate_two_frames_midpoint_math() {
        let a = [0u16, 100u16, 1000u16];
        let b = [100u16, 200u16, 2000u16];
        let frames = [
            TimedFrameRef {
                timestamp_ns: 0,
                pixels: &a,
            },
            TimedFrameRef {
                timestamp_ns: 100,
                pixels: &b,
            },
        ];
        let out = FrameInterpolator::interpolate_u16(&frames, 50).expect("frame");
        assert_eq!(out.pixels, vec![50, 150, 1500]);
    }

    #[test]
    fn interpolate_uses_bracketing_frames_from_many() {
        let a = [0u16, 0u16];
        let b = [100u16, 200u16];
        let c = [200u16, 400u16];
        let d = [300u16, 600u16];
        let frames = [
            TimedFrameRef {
                timestamp_ns: 30,
                pixels: &d,
            },
            TimedFrameRef {
                timestamp_ns: 0,
                pixels: &a,
            },
            TimedFrameRef {
                timestamp_ns: 20,
                pixels: &c,
            },
            TimedFrameRef {
                timestamp_ns: 10,
                pixels: &b,
            },
        ];
        let out = FrameInterpolator::interpolate_u16(&frames, 15).expect("frame");
        // halfway between b (10ns) and c (20ns)
        assert_eq!(out.pixels, vec![150, 300]);
    }

    #[test]
    fn interpolate_clamps_to_edges() {
        let a = [10u16, 20u16];
        let b = [110u16, 120u16];
        let frames = [
            TimedFrameRef {
                timestamp_ns: 100,
                pixels: &a,
            },
            TimedFrameRef {
                timestamp_ns: 200,
                pixels: &b,
            },
        ];
        let before = FrameInterpolator::interpolate_u16(&frames, 50).expect("frame");
        let after = FrameInterpolator::interpolate_u16(&frames, 250).expect("frame");
        assert_eq!(before.pixels, a);
        assert_eq!(after.pixels, b);
    }
}
