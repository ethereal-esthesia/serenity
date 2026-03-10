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

const NOISE_RSHIFT: u8 = 1;

fn make_noise_buffer_0_511(width: usize, height: usize, seed: u64) -> Vec<u16> {
    let mut rng = FastRng::new(seed);
    let mut out = vec![0u16; width * height];
    for p in &mut out {
        let hi = rng.next_u8() as u16;
        let lo = rng.next_u8() as u16;
        let raw16 = (hi << 8) | lo;
        *p = ((raw16 >> NOISE_RSHIFT) % 511) as u16;
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
    let mut noise = make_noise_buffer_0_511(width as usize, height as usize, 0x5EED_F00D);

    let mut events = sdl.event_pump()?;
    println!("Indexed mode | Fixed noise range: 0..510 (u16 from cached u8, >> {NOISE_RSHIFT})");
    'running: loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
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
            noise = make_noise_buffer_0_511(width as usize, height as usize, 0x5EED_F00D);
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
