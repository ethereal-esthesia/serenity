use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::pixels::Color;
use sdl3::pixels::PixelFormatEnum;

use serenity::fast_rng::FastRng;

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

fn make_noise_buffer9(width: usize, height: usize, seed: u64) -> Vec<u16> {
    let mut rng = FastRng::new(seed);
    let mut out = vec![0u16; width * height];
    for p in &mut out {
        *p = (rng.next_bits(9) & 0x01FF) as u16;
    }
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let mut noise9 = make_noise_buffer9(width as usize, height as usize, 0x5EED_F00D);

    let mut events = sdl.event_pump()?;
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
            noise9 = make_noise_buffer9(width as usize, height as usize, 0x5EED_F00D);
        }

        texture
            .with_lock(None, |buf: &mut [u8], pitch: usize| {
                for y in 0..height as usize {
                    let row = &mut buf[y * pitch..(y + 1) * pitch];
                    for x in 0..width as usize {
                        let idx = y * width as usize + x;
                        let blended16 = gradient16[idx].saturating_add(noise9[idx]);
                        let c = (blended16 >> 8) as u8;
                        let off = x * 4;
                        // ARGB8888 little-endian memory order: B, G, R, A.
                        row[off] = c;
                        row[off + 1] = c;
                        row[off + 2] = c;
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
