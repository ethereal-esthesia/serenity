use serenity::runtime::frame_interpolator::{FrameInterpolator, InterpolationError};
use serenity::runtime::io_timestamp::IoTimestamp;

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

fn print_mix_summary(label: &str, alphas: &[f64]) {
    let mut b0 = 0usize;
    let mut b25 = 0usize;
    let mut b50 = 0usize;
    let mut b75 = 0usize;
    let mut b100 = 0usize;

    for alpha in alphas {
        if *alpha <= 0.0 {
            b0 += 1;
        } else if *alpha < 0.25 {
            b25 += 1;
        } else if *alpha < 0.5 {
            b50 += 1;
        } else if *alpha < 0.75 {
            b75 += 1;
        } else if *alpha < 1.0 {
            b100 += 1;
        } else {
            b100 += 1;
        }
    }

    let total = alphas.len().max(1) as f64;
    println!(
        "{label}: total={} bins={{0%:{:.1} 0-25:{:.1} 25-50:{:.1} 50-75:{:.1} 75-100:{:.1}}}",
        alphas.len(),
        (b0 as f64 / total) * 100.0,
        (b25 as f64 / total) * 100.0,
        (b50 as f64 / total) * 100.0,
        (b75 as f64 / total) * 100.0,
        (b100 as f64 / total) * 100.0,
    );
}

#[test]
fn sim_120hz_input_to_23hz_output() {
    let duration_ns = 2_000_000_000u64;
    let input_ts: Vec<IoTimestamp> = generate_timestamps_ns(duration_ns, &[(duration_ns, 120.0)])
        .into_iter()
        .map(IoTimestamp::from_raw)
        .collect();
    let output_targets = generate_timestamps_ns(duration_ns, &[(duration_ns, 23.0)]);

    let mut alphas = Vec::new();
    for t in output_targets {
        let mix = FrameInterpolator::mix_from_timestamps(&input_ts, IoTimestamp::from_raw(t))
            .expect("mix");
        assert!((0.0..=1.0).contains(&mix.alpha_0_to_1));
        alphas.push(mix.alpha_0_to_1);
    }

    print_mix_summary("sim_120_to_23", &alphas);
    assert!(
        alphas.iter().any(|a| *a > 0.0 && *a < 1.0),
        "expected non-trivial interpolation for 120->23 conversion"
    );
}

#[test]
fn sim_23hz_input_to_120hz_output() {
    let duration_ns = 2_000_000_000u64;
    let input_ts: Vec<IoTimestamp> = generate_timestamps_ns(duration_ns, &[(duration_ns, 23.0)])
        .into_iter()
        .map(IoTimestamp::from_raw)
        .collect();
    let output_targets = generate_timestamps_ns(duration_ns, &[(duration_ns, 120.0)]);

    let mut alphas = Vec::new();
    for t in output_targets {
        let mix = FrameInterpolator::mix_from_timestamps(&input_ts, IoTimestamp::from_raw(t))
            .expect("mix");
        assert!((0.0..=1.0).contains(&mix.alpha_0_to_1));
        alphas.push(mix.alpha_0_to_1);
    }

    print_mix_summary("sim_23_to_120", &alphas);
    assert!(
        alphas.iter().any(|a| *a > 0.0 && *a < 1.0),
        "expected non-trivial interpolation for 23->120 conversion"
    );
}

#[test]
fn sim_25hz_input_to_120hz_output() {
    // 25Hz and 120Hz re-align every 200ms (gcd=5Hz).
    let duration_ns = 200_000_000u64;
    let input_ts: Vec<IoTimestamp> = generate_constant_hz_timestamps_exact_ns(duration_ns, 25)
        .into_iter()
        .map(IoTimestamp::from_raw)
        .collect();
    let output_targets = generate_constant_hz_timestamps_exact_ns(duration_ns, 120);

    let mut alphas = Vec::new();
    let mut exact_realign = 0usize;
    for t in output_targets {
        let mix = FrameInterpolator::mix_from_timestamps(&input_ts, IoTimestamp::from_raw(t))
            .expect("mix");
        assert!((0.0..=1.0).contains(&mix.alpha_0_to_1));
        if mix.left_timestamp == mix.right_timestamp {
            exact_realign += 1;
        }
        alphas.push(mix.alpha_0_to_1);
    }

    print_mix_summary("sim_25_to_120", &alphas);
    assert!(
        alphas.iter().any(|a| *a > 0.0 && *a < 1.0),
        "expected non-trivial interpolation for 25->120 conversion"
    );
    assert!(
        exact_realign >= 2,
        "expected at least start and end exact realignment points for one full cycle"
    );
}

#[test]
fn sim_midstream_input_and_output_rate_changes() {
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

    let mut alphas = Vec::new();
    for t in output_targets {
        let mix = FrameInterpolator::mix_from_timestamps(&input_ts, IoTimestamp::from_raw(t))
            .expect("mix");
        assert!((0.0..=1.0).contains(&mix.alpha_0_to_1));
        assert!(mix.left_timestamp <= mix.right_timestamp);
        alphas.push(mix.alpha_0_to_1);
    }

    print_mix_summary("sim_midstream_both_change", &alphas);
}

#[test]
fn sim_rejects_out_of_sequence_input_timestamps() {
    let err = FrameInterpolator::mix_from_timestamps(
        &[
            IoTimestamp::from_raw(100),
            IoTimestamp::from_raw(300),
            IoTimestamp::from_raw(200),
        ],
        IoTimestamp::from_raw(250),
    )
    .expect_err("out-of-sequence");

    assert_eq!(
        err,
        InterpolationError::OutOfSequenceTimestamps {
            index: 2,
            prev_ts: IoTimestamp::from_raw(300),
            current_ts: IoTimestamp::from_raw(200),
        }
    );
}
