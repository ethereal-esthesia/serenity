use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::pixels::Color;
use sdl3::pixels::PixelFormatEnum;
use serenity::palette::{Palette256, palette_256};
use serenity::pixel_buffer::{
    DebandingDistribution, DebandingFilter, PixelBuffer, make_gradient_buffer16,
};
use std::f32::consts::TAU;
use std::time::Instant;

fn parse_args() -> Result<(bool, Option<String>), Box<dyn std::error::Error>> {
    let mut debug = false;
    let mut screenshot_path: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--debug" => debug = true,
            "--screenshot" => {
                let path = args.next().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "--screenshot requires a file path",
                    )
                })?;
                screenshot_path = Some(path);
            }
            _ => {}
        }
    }
    Ok((debug, screenshot_path))
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn render_topdown_ocean_frame(width: usize, height: usize, t: f32, base: &mut [u16]) {
    let w = width as f32;
    let h = height as f32;
    for y in 0..height {
        let v = y as f32 / h.max(1.0);
        for x in 0..width {
            let u = x as f32 / w.max(1.0);
            let i = y * width + x;

            // Layered wave field for top-down ocean motion.
            let w1 = ((u * 13.0 + t * 0.07) * TAU).sin();
            let w2 = ((v * 17.0 - t * 0.09) * TAU).sin();
            let w3 = (((u + v) * 9.0 + t * 0.05) * TAU).sin();
            let w4 = (((u - v) * 21.0 - t * 0.11) * TAU).sin();

            let depth = 0.50 + 0.19 * w1 + 0.14 * w2 + 0.10 * w3;
            let shimmer = smoothstep(0.70, 1.0, w4);
            let lane_a = smoothstep(0.84, 1.0, ((u * 31.0 + v * 7.0 - t * 0.13) * TAU).sin());
            let lane_b = smoothstep(0.88, 1.0, ((u * 7.0 - v * 29.0 + t * 0.16) * TAU).sin());
            let neon = (lane_a * 0.7 + lane_b * 0.5) * (0.35 + 0.65 * shimmer);

            // Keep the same palette, but drive it with deeper mids + neon accents.
            let mut v16 = 6500.0 + depth * 30000.0 + shimmer * 4500.0 + neon * 15500.0;
            v16 = v16.clamp(0.0, 65535.0);
            base[i] = v16 as u16;
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (debug, mut screenshot_path) = parse_args()?;
    let deband_seed: u64 = 0x5EED_F00D;
    let deband_shift: i8 = -2;
    let deband_dist = DebandingDistribution::Gaussian;
    if debug {
        println!("[main] debug enabled");
    }

    let palette256 = palette_256(Palette256::SoftSky);

    let sdl = sdl3::init()?;
    let _ = sdl3::hint::set("SDL_RENDER_VSYNC", "1");
    let video = sdl.video()?;
    sdl.mouse().show_cursor(false);

    let initial_width: u32 = 1024;
    let initial_height: u32 = 768;
    let window = video
        .window("Serenity SDL3", initial_width, initial_height)
        .position_centered()
        .resizable()
        .hidden()
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
    let mut pixels = PixelBuffer::new_with_debug(width as usize, height as usize, palette256, debug);
    pixels.set_base(make_gradient_buffer16(width as usize, height as usize));
    pixels.add_filter(Box::new(DebandingFilter::new(
        deband_seed,
        deband_shift,
        deband_dist,
    )));
    if debug {
        println!(
            "[main:filter:init] debanding_filter dist={:?} shift={} seed=0x{:016X}",
            deband_dist, deband_shift, deband_seed
        );
    }

    let mut events = sdl.event_pump()?;
    let mut fps_start = Instant::now();
    let mut fps_frames: u64 = 0;
    let ocean_start = Instant::now();
    let mut window_shown = false;
    println!("Neon pattern mode (default, neon accents). Press Esc to quit.");

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
            pixels = PixelBuffer::new_with_debug(
                width as usize,
                height as usize,
                palette_256(Palette256::SoftSky),
                debug,
            );
            pixels.set_base(make_gradient_buffer16(width as usize, height as usize));
            pixels.add_filter(Box::new(DebandingFilter::new(
                deband_seed,
                deband_shift,
                deband_dist,
            )));
            if debug {
                println!(
                    "[main:filter:init] debanding_filter dist={:?} shift={} seed=0x{:016X}",
                    deband_dist, deband_shift, deband_seed
                );
            }
        }

        let t = ocean_start.elapsed().as_secs_f32();
        render_topdown_ocean_frame(width as usize, height as usize, t, pixels.base_mut());
        pixels.mark_dirty();
        pixels.upload_to_texture(&mut texture)?;
        if let Some(path) = screenshot_path.take() {
            pixels.write_ppm(&path)?;
            println!("[main:screenshot] wrote {}", path);
        }

        canvas.copy(&texture, None, None)?;
        let _ = canvas.present();
        if !window_shown {
            canvas.window_mut().show();
            window_shown = true;
        }
        if debug {
            fps_frames += 1;
            let elapsed = fps_start.elapsed();
            if elapsed.as_secs_f64() >= 1.0 {
                let secs = elapsed.as_secs_f64();
                let fps = fps_frames as f64 / secs;
                let frame_ms = (secs * 1000.0) / fps_frames as f64;
                println!("[main:fps] fps={:.1} frame_ms={:.3}", fps, frame_ms);
                fps_start = Instant::now();
                fps_frames = 0;
            }
        }
    }

    sdl.mouse().show_cursor(true);
    Ok(())
}
