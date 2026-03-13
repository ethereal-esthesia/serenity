use serenity::runtime::frame_interpolator::{FrameInterpolator, InterpolationError};
use serenity::runtime::timestamp::InputTimestamp;

fn parse_csv_ts(input: &str) -> Result<Vec<InputTimestamp>, String> {
    let mut out = Vec::new();
    for raw in input.split(',') {
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        let value: u64 = t
            .parse()
            .map_err(|_| format!("invalid integer value: '{t}'"))?;
        out.push(InputTimestamp::from_raw(value));
    }
    if out.is_empty() {
        return Err("no timestamp values provided".to_string());
    }
    Ok(out)
}

fn usage() {
    eprintln!(
        "Usage:
  frame_mix_tool --timestamps 1000,2000,3000 --target 2500
  frame_mix_tool --timestamps 1000,2000,3000 --targets 1250,1500,2750"
    );
}

fn print_mix_line(target: InputTimestamp, mix: serenity::runtime::frame_interpolator::FrameMix) {
    println!(
        "target={}ns between [{}..{}] => {:.3}% ({:.6})",
        target.raw(),
        mix.left_timestamp.raw(),
        mix.right_timestamp.raw(),
        mix.alpha_0_to_1 * 100.0,
        mix.alpha_0_to_1
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut timestamps_csv: Option<String> = None;
    let mut target_csv: Option<String> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--timestamps" => {
                timestamps_csv = args.next();
            }
            "--target" | "--targets" => {
                target_csv = args.next();
            }
            "--help" | "-h" => {
                usage();
                return Ok(());
            }
            other => {
                return Err(format!("unknown arg: {other}").into());
            }
        }
    }

    let Some(ts_csv) = timestamps_csv else {
        usage();
        return Err("missing --timestamps".into());
    };
    let Some(targets_csv) = target_csv else {
        usage();
        return Err("missing --target or --targets".into());
    };

    let timestamps = parse_csv_ts(&ts_csv)?;
    let targets = parse_csv_ts(&targets_csv)?;

    for target in targets {
        match FrameInterpolator::mix_from_timestamps(&timestamps, target) {
            Ok(mix) => print_mix_line(target, mix),
            Err(InterpolationError::OutOfSequenceTimestamps {
                index,
                prev_ts,
                current_ts,
            }) => {
                return Err(format!(
                    "out-of-sequence timestamps at index {}: prev={} current={}",
                    index, prev_ts.raw(), current_ts.raw()
                )
                .into());
            }
            Err(err) => return Err(format!("mix error: {err:?}").into()),
        }
    }
    Ok(())
}
