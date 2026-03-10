use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::pixels::Color;
use sdl3::pixels::PixelFormatEnum;

use serenity::fast_rng::FastRng;

fn lerp_u8(a: u8, b: u8, num: usize, den: usize) -> u8 {
    if den == 0 {
        return a;
    }
    let av = a as usize;
    let bv = b as usize;
    let v = (av * (den - num) + bv * num) / den;
    v as u8
}

fn make_palette_256() -> Vec<[u8; 3]> {
    // Soft sky palette: deep navy -> blue -> cyan -> white.
    let points: [(usize, [u8; 3]); 4] = [
        (0, [4, 10, 28]),
        (96, [34, 76, 178]),
        (192, [132, 206, 255]),
        (255, [245, 252, 255]),
    ];
    let mut out = vec![[0u8; 3]; 256];
    for w in points.windows(2) {
        let (i0, c0) = (w[0].0, w[0].1);
        let (i1, c1) = (w[1].0, w[1].1);
        let den = i1 - i0;
        for (k, slot) in out.iter_mut().enumerate().take(i1 + 1).skip(i0) {
            let num = k - i0;
            let r = lerp_u8(c0[0], c1[0], num, den);
            let g = lerp_u8(c0[1], c1[1], num, den);
            let b = lerp_u8(c0[2], c1[2], num, den);
            *slot = [r, g, b];
        }
    }
    out
}

fn make_gradient_buffer16(width: usize, height: usize) -> Vec<u16> {
    let mut out = vec![0u16; width * height];
    let max_sum = (width - 1) + (height - 1);
    for y in 0..height {
        let y_from_bottom = (height - 1) - y;
        for x in 0..width {
            let sum = x + y_from_bottom;
            // True 16-bit grayscale ramp: 0..65535
            out[y * width + x] = ((sum * 65535) / max_sum) as u16;
        }
    }
    out
}

fn next_bits_u128(rng: &mut FastRng, bits: u16) -> u128 {
    if bits == 0 {
        return 0;
    }
    let mut remaining = bits;
    let mut out: u128 = 0;
    while remaining > 0 {
        let take = remaining.min(64);
        out = (out << take) | rng.next_bits(take as u8) as u128;
        remaining -= take;
    }
    out
}

fn range_hi_for_bits(bits: u16) -> u32 {
    if bits == 0 {
        0
    } else if bits >= 16 {
        65535
    } else {
        (1u32 << bits) - 1
    }
}

fn make_noise_buffer_for_bits(width: usize, height: usize, seed: u64, bits: u16) -> Vec<u16> {
    if bits == 0 {
        return vec![0u16; width * height];
    }

    let bits_per_sample = bits;
    let range: u128 = range_hi_for_bits(bits) as u128 + 1;

    let mut rng = FastRng::new(seed);
    let mut out = vec![0u16; width * height];
    for p in &mut out {
        let raw = next_bits_u128(&mut rng, bits_per_sample);
        *p = (raw % range) as u16;
    }
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let palette256 = make_palette_256();

    let sdl = sdl3::init()?;
    let video = sdl.video()?;

    let initial_width: u32 = 1024;
    let initial_height: u32 = 768;
    let window = video
        .window("Serenity SDL3", initial_width, initial_height)
        .position_centered()
        .resizable()
        .build()?;

    let mut canvas = window.into_canvas();
    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();
    let _ = canvas.present();

    let texture_creator = canvas.texture_creator();
    let mut width = initial_width;
    let mut height = initial_height;
    let mut texture = texture_creator
        .create_texture_streaming(Some(PixelFormatEnum::ARGB8888.into()), width, height)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut gradient16 = make_gradient_buffer16(width as usize, height as usize);
    let mut noise_bits: u16 = 8;
    let mut noise = make_noise_buffer_for_bits(width as usize, height as usize, 0x5EED_F00D, noise_bits);

    let mut events = sdl.event_pump()?;
    const MIN_BITS: u16 = 0;
    const MAX_BITS: u16 = 16;
    println!(
        "Indexed mode | Noise bits={} (adds 0..{}, '+'/'-' to change by 1)",
        noise_bits,
        range_hi_for_bits(noise_bits)
    );
    'running: loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                Event::KeyDown {
                    keycode: Some(keycode),
                    repeat,
                    ..
                } => {
                    if !repeat {
                        let mut changed = false;
                        match keycode {
                            Keycode::Plus | Keycode::KpPlus | Keycode::Equals => {
                                if noise_bits < MAX_BITS {
                                    noise_bits += 1;
                                    changed = true;
                                }
                            }
                            Keycode::Minus | Keycode::KpMinus => {
                                if noise_bits > MIN_BITS {
                                    noise_bits -= 1;
                                    changed = true;
                                }
                            }
                            _ => {}
                        }
                        if changed {
                            noise = make_noise_buffer_for_bits(
                                width as usize,
                                height as usize,
                                0x5EED_F00D,
                                noise_bits,
                            );
                            println!(
                                "Key: {:?} -> Noise bits={} (adds 0..{})",
                                keycode,
                                noise_bits,
                                range_hi_for_bits(noise_bits)
                            );
                        }
                    }
                }
                _ => {}
            }
        }

        let (current_w, current_h) = canvas.output_size()?;
        if current_w > 0 && current_h > 0 && (current_w != width || current_h != height) {
            width = current_w;
            height = current_h;
            texture = texture_creator
                .create_texture_streaming(Some(PixelFormatEnum::ARGB8888.into()), width, height)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            gradient16 = make_gradient_buffer16(width as usize, height as usize);
            noise = make_noise_buffer_for_bits(
                width as usize,
                height as usize,
                0x5EED_F00D,
                noise_bits,
            );
        }

        texture
            .with_lock(None, |buf: &mut [u8], pitch: usize| {
                for y in 0..height as usize {
                    let row = &mut buf[y * pitch..(y + 1) * pitch];
                    for x in 0..width as usize {
                        let idx = y * width as usize + x;
                        let noise_term = noise[idx];
                        let blended16 = gradient16[idx].saturating_add(noise_term);
                        let c = (blended16 >> 8) as u8;
                        let off = x * 4;
                        // ARGB8888 little-endian memory order: B, G, R, A.
                        let [r, g, b] = palette256[c as usize];
                        row[off] = b;
                        row[off + 1] = g;
                        row[off + 2] = r;
                        row[off + 3] = 0xFF;
                    }
                }
            })
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        canvas.copy(&texture, None, None)?;
        let _ = canvas.present();
    }

    Ok(())
}
