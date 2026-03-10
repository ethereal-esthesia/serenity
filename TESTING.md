# Serenity Test Runs

This file contains practical, copy-paste examples for common test and data-dump runs.

## 1) Run all tests

```bash
cd /Users/shane/Project/serenity
cargo test
```

## 2) Run tests with output

```bash
cd /Users/shane/Project/serenity
cargo test -- --nocapture
```

## 3) Run the ignored RNG benchmark test

```bash
cd /Users/shane/Project/serenity
cargo test benchmark_legacy_vs_refactor -- --ignored --nocapture
```

## 4) Dump Gaussian sample data + histogram + raw LUT table

```bash
cd /Users/shane/Project/serenity
cargo run --bin dump_gaussian_csv -- \
  200000 \
  1311768467463790320 \
  /tmp/serenity_gaussian_hist.csv \
  /tmp/serenity_gaussian_raw.csv \
  /tmp/serenity_gaussian_table.csv
```

Outputs:
- `/tmp/serenity_gaussian_raw.csv` (`sample_index,value`)
- `/tmp/serenity_gaussian_hist.csv` (`value,count`)
- `/tmp/serenity_gaussian_table.csv` (`index,lut_value`)

## 5) Deterministic sweep: map every `u16` input once and bin results

```bash
cd /Users/shane/Project/serenity
cargo run --bin sweep_gaussian_u16_csv -- \
  /tmp/serenity_gaussian_u16_hist.csv \
  /tmp/serenity_gaussian_u16_map.csv
```

Outputs:
- `/tmp/serenity_gaussian_u16_hist.csv` (`value,count`) deterministic, no RNG sampling noise
- `/tmp/serenity_gaussian_u16_map.csv` (`u16_input,gaussian8`) full 0..65535 mapping

## 6) Run the SDL window app

```bash
cd /Users/shane/Project/serenity
cargo run
```
