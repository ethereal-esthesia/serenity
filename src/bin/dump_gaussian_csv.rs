use serenity::fast_rng::FastRng;
use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

fn parse_arg<T: std::str::FromStr>(args: &[String], idx: usize, default: T) -> T {
    args.get(idx)
        .and_then(|s| s.parse::<T>().ok())
        .unwrap_or(default)
}

fn parse_path(args: &[String], idx: usize, default: &str) -> PathBuf {
    args.get(idx)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Usage:
    // cargo run --bin dump_gaussian_csv -- [samples] [seed] [hist_csv] [raw_csv] [table_csv]
    let args: Vec<String> = env::args().skip(1).collect();

    let samples: usize = parse_arg(&args, 0, 200_000usize);
    let seed: u64 = parse_arg(&args, 1, 0x1234_5678_9ABC_DEF0u64);
    let hist_path = parse_path(&args, 2, "/tmp/serenity_gaussian_hist.csv");
    let raw_path = parse_path(&args, 3, "/tmp/serenity_gaussian_raw.csv");
    let table_path = parse_path(&args, 4, "/tmp/serenity_gaussian_table.csv");

    let mut rng = FastRng::new(seed);
    let mut hist = [0usize; 256];

    let raw_file = File::create(&raw_path)?;
    let mut raw = BufWriter::new(raw_file);
    writeln!(raw, "sample_index,value")?;

    for i in 0..samples {
        let v = rng.next_gaussian8();
        hist[v as usize] += 1;
        writeln!(raw, "{i},{v}")?;
    }
    raw.flush()?;

    let hist_file = File::create(&hist_path)?;
    let mut hist_out = BufWriter::new(hist_file);
    writeln!(hist_out, "value,count")?;
    for (value, count) in hist.iter().enumerate() {
        writeln!(hist_out, "{value},{count}")?;
    }
    hist_out.flush()?;

    let table_file = File::create(&table_path)?;
    let mut table_out = BufWriter::new(table_file);
    writeln!(table_out, "index,lut_value")?;
    for (i, v) in FastRng::gaussian8_table().iter().enumerate() {
        writeln!(table_out, "{i},{v}")?;
    }
    table_out.flush()?;

    eprintln!("wrote raw samples: {}", raw_path.display());
    eprintln!("wrote histogram: {}", hist_path.display());
    eprintln!("wrote raw g8 table: {}", table_path.display());
    Ok(())
}
