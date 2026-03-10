use serenity::fast_rng::FastRng;
use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

fn parse_path(args: &[String], idx: usize, default: &str) -> PathBuf {
    args.get(idx)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Usage:
    // cargo run --bin sweep_gaussian_u16_csv -- [hist_csv] [map_csv]
    let args: Vec<String> = env::args().skip(1).collect();
    let hist_path = parse_path(&args, 0, "/tmp/serenity_gaussian_u16_hist.csv");
    let map_path = parse_path(&args, 1, "/tmp/serenity_gaussian_u16_map.csv");

    let mut hist = [0u32; 256];
    let lut = FastRng::gaussian8_table_fp();

    let map_file = File::create(&map_path)?;
    let mut map_out = BufWriter::new(map_file);
    writeln!(map_out, "u16_input,gaussian8")?;
    for idx in 0u16..=u16::MAX {
        let out = map_gaussian8_from_u16(idx, lut);
        hist[out as usize] += 1;
        writeln!(map_out, "{idx},{out}")?;
    }
    map_out.flush()?;

    let hist_file = File::create(&hist_path)?;
    let mut hist_out = BufWriter::new(hist_file);
    writeln!(hist_out, "value,count")?;
    for (value, count) in hist.iter().enumerate() {
        writeln!(hist_out, "{value},{count}")?;
    }
    hist_out.flush()?;

    eprintln!("wrote deterministic u16 map: {}", map_path.display());
    eprintln!("wrote deterministic u16 histogram: {}", hist_path.display());
    Ok(())
}

#[inline]
fn map_gaussian8_from_u16(idx: u16, lut: &[u16; 257]) -> u8 {
    let hi = (idx >> 8) as usize;
    let lo = (idx & 0x00FF) as i32;
    let b1 = lut[hi] as i32;
    let b2 = lut[hi + 1] as i32;
    let d = b2 - b1;
    let y_fp = b1 + ((d * lo + 128) >> 8);
    let y_u8 = (y_fp + 128) >> 8;
    y_u8.clamp(0, 255) as u8
}
