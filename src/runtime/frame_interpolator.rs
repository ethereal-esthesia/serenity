use crate::runtime::io_timestamp::IoTimestamp;

#[derive(Debug, Clone, Copy)]
pub struct TimedFrameRef<'a> {
    pub timestamp: IoTimestamp,
    pub pixels: &'a [u16],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterpolatedFrame {
    pub timestamp: IoTimestamp,
    pub pixels: Vec<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameMix {
    pub left_timestamp: IoTimestamp,
    pub right_timestamp: IoTimestamp,
    pub alpha_0_to_1: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterpolationError {
    EmptyInput,
    MismatchedBufferLengths,
    OutOfSequenceTimestamps {
        index: usize,
        prev_ts: IoTimestamp,
        current_ts: IoTimestamp,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderGateError {
    NoFrameInFlight,
    OutOfSequence { expected: u64, got: u64 },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RenderFrameGate {
    next_request_seq: u64,
    in_flight_seq: Option<u64>,
}

impl RenderFrameGate {
    pub fn new(start_seq: u64) -> Self {
        Self {
            next_request_seq: start_seq,
            in_flight_seq: None,
        }
    }

    pub fn request_next_if_ready(&mut self) -> Option<u64> {
        if self.in_flight_seq.is_some() {
            return None;
        }
        let seq = self.next_request_seq;
        self.next_request_seq = self.next_request_seq.saturating_add(1);
        self.in_flight_seq = Some(seq);
        Some(seq)
    }

    pub fn complete_frame(&mut self, seq: u64) -> Result<(), RenderGateError> {
        let Some(expected) = self.in_flight_seq else {
            return Err(RenderGateError::NoFrameInFlight);
        };
        if seq != expected {
            return Err(RenderGateError::OutOfSequence { expected, got: seq });
        }
        self.in_flight_seq = None;
        Ok(())
    }
}

pub struct FrameInterpolator;

impl FrameInterpolator {
    fn compute_alpha_q16(
        left: IoTimestamp,
        right: IoTimestamp,
        target: IoTimestamp,
    ) -> u32 {
        if target <= left {
            return 0;
        }
        if target >= right {
            return 65_536;
        }
        let span = right.raw().saturating_sub(left.raw());
        if span == 0 {
            return 65_536;
        }
        let offset = target.raw().saturating_sub(left.raw());
        // Rounded fixed-point Q16 ratio in [0, 65536].
        let q16 = ((offset as u128 * 65_536u128) + (span as u128 / 2)) / span as u128;
        q16.min(65_536) as u32
    }

    pub fn mix_from_timestamps(
        timestamps: &[IoTimestamp],
        target_timestamp: IoTimestamp,
    ) -> Result<FrameMix, InterpolationError> {
        if timestamps.is_empty() {
            return Err(InterpolationError::EmptyInput);
        }
        for (idx, pair) in timestamps.windows(2).enumerate() {
            if pair[1] <= pair[0] {
                return Err(InterpolationError::OutOfSequenceTimestamps {
                    index: idx + 1,
                    prev_ts: pair[0],
                    current_ts: pair[1],
                });
            }
        }

        if target_timestamp <= timestamps[0] {
            return Ok(FrameMix {
                left_timestamp: timestamps[0],
                right_timestamp: timestamps[0],
                alpha_0_to_1: 0.0,
            });
        }
        let last = timestamps.len() - 1;
        if target_timestamp >= timestamps[last] {
            return Ok(FrameMix {
                left_timestamp: timestamps[last],
                right_timestamp: timestamps[last],
                alpha_0_to_1: 1.0,
            });
        }

        for pair in timestamps.windows(2) {
            let a = pair[0];
            let b = pair[1];
            if target_timestamp < a || target_timestamp > b {
                continue;
            }
            if target_timestamp == a {
                return Ok(FrameMix {
                    left_timestamp: a,
                    right_timestamp: b,
                    alpha_0_to_1: 0.0,
                });
            }
            if target_timestamp == b {
                return Ok(FrameMix {
                    left_timestamp: a,
                    right_timestamp: b,
                    alpha_0_to_1: 1.0,
                });
            }
            let alpha_q16 = Self::compute_alpha_q16(a, b, target_timestamp);
            return Ok(FrameMix {
                left_timestamp: a,
                right_timestamp: b,
                alpha_0_to_1: alpha_q16 as f64 / 65_536.0,
            });
        }

        Ok(FrameMix {
            left_timestamp: timestamps[last],
            right_timestamp: timestamps[last],
            alpha_0_to_1: 1.0,
        })
    }

    pub fn interpolate_u16(
        frames: &[TimedFrameRef<'_>],
        target_timestamp: IoTimestamp,
    ) -> Result<InterpolatedFrame, InterpolationError> {
        let (frame, _) = Self::interpolate_u16_with_mix(frames, target_timestamp)?;
        Ok(frame)
    }

    pub fn interpolate_u16_with_mix(
        frames: &[TimedFrameRef<'_>],
        target_timestamp: IoTimestamp,
    ) -> Result<(InterpolatedFrame, FrameMix), InterpolationError> {
        if frames.is_empty() {
            return Err(InterpolationError::EmptyInput);
        }
        let len = frames[0].pixels.len();
        if frames.iter().any(|f| f.pixels.len() != len) {
            return Err(InterpolationError::MismatchedBufferLengths);
        }

        for (idx, pair) in frames.windows(2).enumerate() {
            if pair[1].timestamp <= pair[0].timestamp {
                return Err(InterpolationError::OutOfSequenceTimestamps {
                    index: idx + 1,
                    prev_ts: pair[0].timestamp,
                    current_ts: pair[1].timestamp,
                });
            }
        }

        if target_timestamp <= frames[0].timestamp {
            return Ok((
                InterpolatedFrame {
                    timestamp: target_timestamp,
                    pixels: frames[0].pixels.to_vec(),
                },
                FrameMix {
                    left_timestamp: frames[0].timestamp,
                    right_timestamp: frames[0].timestamp,
                    alpha_0_to_1: 0.0,
                },
            ));
        }
        let last = frames.len() - 1;
        if target_timestamp >= frames[last].timestamp {
            return Ok((
                InterpolatedFrame {
                    timestamp: target_timestamp,
                    pixels: frames[last].pixels.to_vec(),
                },
                FrameMix {
                    left_timestamp: frames[last].timestamp,
                    right_timestamp: frames[last].timestamp,
                    alpha_0_to_1: 1.0,
                },
            ));
        }

        for pair in frames.windows(2) {
            let a = pair[0];
            let b = pair[1];
            if target_timestamp < a.timestamp || target_timestamp > b.timestamp {
                continue;
            }
            if target_timestamp == a.timestamp {
                return Ok((
                    InterpolatedFrame {
                        timestamp: target_timestamp,
                        pixels: a.pixels.to_vec(),
                    },
                    FrameMix {
                        left_timestamp: a.timestamp,
                        right_timestamp: b.timestamp,
                        alpha_0_to_1: 0.0,
                    },
                ));
            }
            if target_timestamp == b.timestamp {
                return Ok((
                    InterpolatedFrame {
                        timestamp: target_timestamp,
                        pixels: b.pixels.to_vec(),
                    },
                    FrameMix {
                        left_timestamp: a.timestamp,
                        right_timestamp: b.timestamp,
                        alpha_0_to_1: 1.0,
                    },
                ));
            }
            let alpha_q16 = Self::compute_alpha_q16(a.timestamp, b.timestamp, target_timestamp);
            let inv_q16 = 65_536u32.saturating_sub(alpha_q16);
            let mut out = Vec::with_capacity(len);
            for i in 0..len {
                let av = a.pixels[i] as u64;
                let bv = b.pixels[i] as u64;
                let mixed =
                    ((av * inv_q16 as u64) + (bv * alpha_q16 as u64) + 32_768u64) >> 16;
                out.push(mixed.min(u16::MAX as u64) as u16);
            }
            return Ok((
                InterpolatedFrame {
                    timestamp: target_timestamp,
                    pixels: out,
                },
                FrameMix {
                    left_timestamp: a.timestamp,
                    right_timestamp: b.timestamp,
                    alpha_0_to_1: alpha_q16 as f64 / 65_536.0,
                },
            ));
        }

        // Unreachable due to boundary checks.
        Ok((
            InterpolatedFrame {
                timestamp: target_timestamp,
                pixels: frames[last].pixels.to_vec(),
            },
            FrameMix {
                left_timestamp: frames[last].timestamp,
                right_timestamp: frames[last].timestamp,
                alpha_0_to_1: 1.0,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::io_timestamp::IoTimestamp;

    use super::{FrameInterpolator, InterpolationError, RenderFrameGate, RenderGateError, TimedFrameRef};

    fn hz_to_period_ns(hz: f64) -> u64 {
        ((1_000_000_000.0 / hz).round() as u64).max(1)
    }

    fn generate_timestamps_ns(duration_ns: u64, segments: &[(u64, f64)]) -> Vec<u64> {
        assert!(!segments.is_empty(), "segments must not be empty");
        let mut out = Vec::new();
        let mut t = 0u64;
        let mut seg_idx = 0usize;
        while t <= duration_ns {
            out.push(t);
            while seg_idx + 1 < segments.len() && t >= segments[seg_idx].0 {
                seg_idx += 1;
            }
            let hz = segments[seg_idx].1;
            t = t.saturating_add(hz_to_period_ns(hz));
        }
        out
    }

    fn generate_constant_hz_timestamps_exact_ns(duration_ns: u64, hz: u64) -> Vec<u64> {
        assert!(hz > 0, "hz must be > 0");
        let mut out = Vec::new();
        let mut i = 0u64;
        loop {
            let t = ((i as u128 * 1_000_000_000u128 + (hz as u128 / 2)) / hz as u128) as u64;
            if t > duration_ns {
                break;
            }
            out.push(t);
            i = i.saturating_add(1);
        }
        out
    }

    #[test]
    fn interpolate_none_for_empty_input() {
        assert_eq!(
            FrameInterpolator::interpolate_u16(&[], IoTimestamp::from_raw(100))
                .expect_err("empty"),
            InterpolationError::EmptyInput
        );
    }

    #[test]
    fn interpolate_none_for_mismatched_buffer_lengths() {
        let a = [1u16, 2u16, 3u16];
        let b = [4u16, 5u16];
        let frames = [
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(0),
                pixels: &a,
            },
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(10),
                pixels: &b,
            },
        ];
        assert_eq!(
            FrameInterpolator::interpolate_u16(&frames, IoTimestamp::from_raw(5))
                .expect_err("mismatch"),
            InterpolationError::MismatchedBufferLengths
        );
    }

    #[test]
    fn interpolate_single_frame_passthrough() {
        let a = [10u16, 20u16, 30u16];
        let frames = [TimedFrameRef {
            timestamp: IoTimestamp::from_raw(1000),
            pixels: &a,
        }];
        let out = FrameInterpolator::interpolate_u16(&frames, IoTimestamp::from_raw(2000))
            .expect("frame");
        assert_eq!(out.pixels, a);
        assert_eq!(out.timestamp, IoTimestamp::from_raw(2000));
    }

    #[test]
    fn interpolate_two_frames_midpoint_math() {
        let a = [0u16, 100u16, 1000u16];
        let b = [100u16, 200u16, 2000u16];
        let frames = [
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(0),
                pixels: &a,
            },
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(100),
                pixels: &b,
            },
        ];
        let out = FrameInterpolator::interpolate_u16(&frames, IoTimestamp::from_raw(50))
            .expect("frame");
        assert_eq!(out.pixels, vec![50, 150, 1500]);
    }

    #[test]
    fn interpolate_uses_bracketing_frames_from_many() {
        let a = [0u16, 0u16];
        let b = [100u16, 200u16];
        let c = [200u16, 400u16];
        let frames = [
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(0),
                pixels: &a,
            },
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(10),
                pixels: &b,
            },
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(20),
                pixels: &c,
            },
        ];
        let out = FrameInterpolator::interpolate_u16(&frames, IoTimestamp::from_raw(15))
            .expect("frame");
        // halfway between b (10ns) and c (20ns)
        assert_eq!(out.pixels, vec![150, 300]);
    }

    #[test]
    fn interpolate_clamps_to_edges() {
        let a = [10u16, 20u16];
        let b = [110u16, 120u16];
        let frames = [
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(100),
                pixels: &a,
            },
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(200),
                pixels: &b,
            },
        ];
        let before = FrameInterpolator::interpolate_u16(&frames, IoTimestamp::from_raw(50))
            .expect("frame");
        let after = FrameInterpolator::interpolate_u16(&frames, IoTimestamp::from_raw(250))
            .expect("frame");
        assert_eq!(before.pixels, a);
        assert_eq!(after.pixels, b);
    }

    #[test]
    fn interpolate_errors_on_out_of_sequence_timestamps() {
        let a = [0u16, 0u16];
        let b = [10u16, 10u16];
        let c = [20u16, 20u16];
        let frames = [
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(0),
                pixels: &a,
            },
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(20),
                pixels: &c,
            },
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(10),
                pixels: &b,
            },
        ];
        let err = FrameInterpolator::interpolate_u16(&frames, IoTimestamp::from_raw(12))
            .expect_err("out-of-sequence");
        assert_eq!(
            err,
            InterpolationError::OutOfSequenceTimestamps {
                index: 2,
                prev_ts: IoTimestamp::from_raw(20),
                current_ts: IoTimestamp::from_raw(10)
            }
        );
    }

    #[test]
    fn interpolate_reports_mix_percentages() {
        let a = [0u16];
        let b = [100u16];
        let frames = [
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(1000),
                pixels: &a,
            },
            TimedFrameRef {
                timestamp: IoTimestamp::from_raw(2000),
                pixels: &b,
            },
        ];
        let targets = [1000u64, 1250, 1500, 1750, 2000];
        for t in targets {
            let (_frame, mix) =
                FrameInterpolator::interpolate_u16_with_mix(&frames, IoTimestamp::from_raw(t))
                    .expect("mix");
            let pct = mix.alpha_0_to_1 * 100.0;
            println!(
                "mix target={}ns between [{}..{}] => {:.1}%",
                t,
                mix.left_timestamp.raw(),
                mix.right_timestamp.raw(),
                pct
            );
        }
    }

    #[test]
    fn render_gate_rejects_out_of_sequence_completion_and_blocks_new_request() {
        let mut gate = RenderFrameGate::new(1);
        let seq = gate.request_next_if_ready().expect("first request");
        assert_eq!(seq, 1);
        assert!(gate.request_next_if_ready().is_none(), "must not request while in flight");

        let err = gate.complete_frame(2).expect_err("should reject out-of-sequence");
        assert_eq!(err, RenderGateError::OutOfSequence { expected: 1, got: 2 });
        assert!(
            gate.request_next_if_ready().is_none(),
            "still blocked until expected frame completes"
        );

        gate.complete_frame(1).expect("complete expected sequence");
        assert_eq!(gate.request_next_if_ready(), Some(2), "ready for next request");
    }

    #[test]
    fn mix_from_timestamps_errors_on_out_of_sequence() {
        let err = FrameInterpolator::mix_from_timestamps(
            &[
                IoTimestamp::from_raw(100),
                IoTimestamp::from_raw(300),
                IoTimestamp::from_raw(200),
            ],
            IoTimestamp::from_raw(250),
        )
        .expect_err("out of sequence");
        assert_eq!(
            err,
            InterpolationError::OutOfSequenceTimestamps {
                index: 2,
                prev_ts: IoTimestamp::from_raw(300),
                current_ts: IoTimestamp::from_raw(200)
            }
        );
    }

    #[test]
    fn simulation_120hz_input_to_23hz_output() {
        let duration_ns = 2_000_000_000u64;
        let input_ts: Vec<IoTimestamp> = generate_timestamps_ns(duration_ns, &[(duration_ns, 120.0)])
            .into_iter()
            .map(IoTimestamp::from_raw)
            .collect();
        let output_targets =
            generate_timestamps_ns(duration_ns, &[(duration_ns, 23.0)]);

        let mut interpolated = 0usize;
        for t in output_targets {
            let mix = FrameInterpolator::mix_from_timestamps(&input_ts, IoTimestamp::from_raw(t))
                .expect("mix");
            assert!(mix.alpha_0_to_1 >= 0.0 && mix.alpha_0_to_1 <= 1.0);
            if mix.alpha_0_to_1 > 0.0 && mix.alpha_0_to_1 < 1.0 {
                interpolated += 1;
            }
        }
        assert!(interpolated > 0, "expected non-trivial interpolation for 120->23 conversion");
    }

    #[test]
    fn simulation_23hz_input_to_120hz_output() {
        let duration_ns = 2_000_000_000u64;
        let input_ts: Vec<IoTimestamp> = generate_timestamps_ns(duration_ns, &[(duration_ns, 23.0)])
            .into_iter()
            .map(IoTimestamp::from_raw)
            .collect();
        let output_targets =
            generate_timestamps_ns(duration_ns, &[(duration_ns, 120.0)]);

        let mut interpolated = 0usize;
        for t in output_targets {
            let mix = FrameInterpolator::mix_from_timestamps(&input_ts, IoTimestamp::from_raw(t))
                .expect("mix");
            assert!(mix.alpha_0_to_1 >= 0.0 && mix.alpha_0_to_1 <= 1.0);
            if mix.alpha_0_to_1 > 0.0 && mix.alpha_0_to_1 < 1.0 {
                interpolated += 1;
            }
        }
        assert!(interpolated > 0, "expected non-trivial interpolation for 23->120 conversion");
    }

    #[test]
    fn simulation_25hz_input_to_120hz_output() {
        // 25Hz and 120Hz re-align every 200ms (gcd=5Hz).
        let duration_ns = 200_000_000u64;
        let input_ts: Vec<IoTimestamp> = generate_constant_hz_timestamps_exact_ns(duration_ns, 25)
            .into_iter()
            .map(IoTimestamp::from_raw)
            .collect();
        let output_targets = generate_constant_hz_timestamps_exact_ns(duration_ns, 120);

        let mut interpolated = 0usize;
        let mut exact_realign = 0usize;
        for t in output_targets {
            let mix = FrameInterpolator::mix_from_timestamps(&input_ts, IoTimestamp::from_raw(t))
                .expect("mix");
            assert!(mix.alpha_0_to_1 >= 0.0 && mix.alpha_0_to_1 <= 1.0);
            if mix.left_timestamp == mix.right_timestamp {
                exact_realign += 1;
            }
            if mix.alpha_0_to_1 > 0.0 && mix.alpha_0_to_1 < 1.0 {
                interpolated += 1;
            }
        }
        assert!(interpolated > 0, "expected non-trivial interpolation for 25->120 conversion");
        assert!(
            exact_realign >= 2,
            "expected at least start and end exact realignment points for one full cycle"
        );
    }

    #[test]
    fn simulation_midstream_input_rate_change() {
        let duration_ns = 3_000_000_000u64;
        // 120Hz through ~1s, then 30Hz.
        let input_ts: Vec<IoTimestamp> = generate_timestamps_ns(
            duration_ns,
            &[(1_000_000_000, 120.0), (duration_ns, 30.0)],
        )
        .into_iter()
        .map(IoTimestamp::from_raw)
        .collect();
        let output_targets =
            generate_timestamps_ns(duration_ns, &[(duration_ns, 60.0)]);

        for t in output_targets {
            let mix = FrameInterpolator::mix_from_timestamps(&input_ts, IoTimestamp::from_raw(t))
                .expect("mix");
            assert!(mix.alpha_0_to_1 >= 0.0 && mix.alpha_0_to_1 <= 1.0);
            assert!(mix.left_timestamp <= mix.right_timestamp);
        }
    }

    #[test]
    fn simulation_midstream_input_and_output_rate_changes() {
        let duration_ns = 3_000_000_000u64;
        let input_ts: Vec<IoTimestamp> = generate_timestamps_ns(
            duration_ns,
            &[
                (1_000_000_000, 120.0),
                (2_000_000_000, 48.0),
                (duration_ns, 24.0),
            ],
        )
        .into_iter()
        .map(IoTimestamp::from_raw)
        .collect();
        let output_targets = generate_timestamps_ns(
            duration_ns,
            &[
                (1_000_000_000, 23.0),
                (2_000_000_000, 60.0),
                (duration_ns, 30.0),
            ],
        );

        for t in output_targets {
            let mix = FrameInterpolator::mix_from_timestamps(&input_ts, IoTimestamp::from_raw(t))
                .expect("mix");
            assert!(mix.alpha_0_to_1 >= 0.0 && mix.alpha_0_to_1 <= 1.0);
            assert!(mix.left_timestamp <= mix.right_timestamp);
        }
    }
}
