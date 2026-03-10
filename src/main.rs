use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::pixels::Color;
use sdl3::pixels::PixelFormatEnum;
use serenity::pixel_buffer::{
    DebandingDistribution, DebandingFilter, PixelBuffer, make_gradient_buffer16,
    make_soft_sky_palette_256,
};
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (debug, mut screenshot_path) = parse_args()?;
    let deband_seed: u64 = 0x5EED_F00D;
    let deband_shift: i8 = -2;
    let deband_dist = DebandingDistribution::Gaussian;
    if debug {
        println!("[main] debug enabled");
    }

    let palette256 = make_soft_sky_palette_256();

    let sdl = sdl3::init()?;
    let video = sdl.video()?;

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
    let mut window_shown = false;
    println!("Gradient-only mode (default). Press Esc to quit.");

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
                make_soft_sky_palette_256(),
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

    Ok(())
}
