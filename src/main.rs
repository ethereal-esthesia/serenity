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

#[derive(Debug, Default, PartialEq, Eq)]
struct RunConfig {
    debug: bool,
    screenshot_path: Option<String>,
}

#[derive(Clone, Copy, Debug)]
struct DebandConfig {
    seed: u64,
    shift: i8,
    dist: DebandingDistribution,
}

struct FpsCounter {
    start: Instant,
    frames: u64,
}

impl FpsCounter {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            frames: 0,
        }
    }

    fn tick(&mut self) -> Option<(f64, f64)> {
        self.frames += 1;
        let elapsed = self.start.elapsed();
        if elapsed.as_secs_f64() < 1.0 {
            return None;
        }
        let secs = elapsed.as_secs_f64();
        let fps = self.frames as f64 / secs;
        let frame_ms = (secs * 1000.0) / self.frames as f64;
        self.start = Instant::now();
        self.frames = 0;
        Some((fps, frame_ms))
    }
}

fn parse_args_from(
    args: impl IntoIterator<Item = String>,
) -> Result<RunConfig, Box<dyn std::error::Error>> {
    let mut config = RunConfig::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--debug" => config.debug = true,
            "--screenshot" => {
                let path = args.next().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "--screenshot requires a file path",
                    )
                })?;
                config.screenshot_path = Some(path);
            }
            _ => {}
        }
    }
    Ok(config)
}

fn parse_args() -> Result<RunConfig, Box<dyn std::error::Error>> {
    parse_args_from(std::env::args().skip(1))
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Copy, Debug)]
struct NeonPatternParams {
    w1_freq: f32,
    w1_speed: f32,
    w2_freq: f32,
    w2_speed: f32,
    w3_freq: f32,
    w3_speed: f32,
    w4_freq: f32,
    w4_speed: f32,
    depth_base: f32,
    depth_w1: f32,
    depth_w2: f32,
    depth_w3: f32,
    shimmer_min: f32,
    lane_a_min: f32,
    lane_b_min: f32,
    lane_a_u: f32,
    lane_a_v: f32,
    lane_a_speed: f32,
    lane_b_u: f32,
    lane_b_v: f32,
    lane_b_speed: f32,
    neon_a_weight: f32,
    neon_b_weight: f32,
    neon_mix_base: f32,
    neon_mix_shimmer: f32,
    base_bias: f32,
    depth_gain: f32,
    shimmer_gain: f32,
    neon_gain: f32,
}

impl Default for NeonPatternParams {
    fn default() -> Self {
        Self {
            w1_freq: 13.0,
            w1_speed: 0.07,
            w2_freq: 17.0,
            w2_speed: 0.09,
            w3_freq: 9.0,
            w3_speed: 0.05,
            w4_freq: 21.0,
            w4_speed: 0.11,
            depth_base: 0.50,
            depth_w1: 0.19,
            depth_w2: 0.14,
            depth_w3: 0.10,
            shimmer_min: 0.70,
            lane_a_min: 0.84,
            lane_b_min: 0.88,
            lane_a_u: 31.0,
            lane_a_v: 7.0,
            lane_a_speed: 0.13,
            lane_b_u: 7.0,
            lane_b_v: 29.0,
            lane_b_speed: 0.16,
            neon_a_weight: 0.7,
            neon_b_weight: 0.5,
            neon_mix_base: 0.35,
            neon_mix_shimmer: 0.65,
            base_bias: 6500.0,
            depth_gain: 30000.0,
            shimmer_gain: 4500.0,
            neon_gain: 15500.0,
        }
    }
}

fn build_render_targets<'a>(
    texture_creator: &'a sdl3::render::TextureCreator<sdl3::video::WindowContext>,
    width: u32,
    height: u32,
    debug: bool,
    palette: Palette256,
    deband: DebandConfig,
) -> Result<(sdl3::render::Texture<'a>, PixelBuffer), Box<dyn std::error::Error>> {
    let texture = texture_creator
        .create_texture_streaming(Some(PixelFormatEnum::ARGB8888.into()), width, height)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut pixels = PixelBuffer::new_with_debug(width as usize, height as usize, palette_256(palette), debug);
    pixels.set_base(make_gradient_buffer16(width as usize, height as usize));
    pixels.add_filter(Box::new(DebandingFilter::new(
        deband.seed,
        deband.shift,
        deband.dist,
    )));
    if debug {
        println!(
            "[main:filter:init] debanding_filter dist={:?} shift={} seed=0x{:016X}",
            deband.dist, deband.shift, deband.seed
        );
    }
    Ok((texture, pixels))
}

fn render_neon_pattern_frame(
    width: usize,
    height: usize,
    t: f32,
    params: NeonPatternParams,
    base: &mut [u16],
) {
    let w = width as f32;
    let h = height as f32;
    for y in 0..height {
        let v = y as f32 / h.max(1.0);
        for x in 0..width {
            let u = x as f32 / w.max(1.0);
            let i = y * width + x;

            // Layered wave field for top-down ocean motion.
            let w1 = ((u * params.w1_freq + t * params.w1_speed) * TAU).sin();
            let w2 = ((v * params.w2_freq - t * params.w2_speed) * TAU).sin();
            let w3 = (((u + v) * params.w3_freq + t * params.w3_speed) * TAU).sin();
            let w4 = (((u - v) * params.w4_freq - t * params.w4_speed) * TAU).sin();

            let depth = params.depth_base + params.depth_w1 * w1 + params.depth_w2 * w2 + params.depth_w3 * w3;
            let shimmer = smoothstep(params.shimmer_min, 1.0, w4);
            let lane_a = smoothstep(
                params.lane_a_min,
                1.0,
                ((u * params.lane_a_u + v * params.lane_a_v - t * params.lane_a_speed) * TAU).sin(),
            );
            let lane_b = smoothstep(
                params.lane_b_min,
                1.0,
                ((u * params.lane_b_u - v * params.lane_b_v + t * params.lane_b_speed) * TAU).sin(),
            );
            let neon = (lane_a * params.neon_a_weight + lane_b * params.neon_b_weight)
                * (params.neon_mix_base + params.neon_mix_shimmer * shimmer);

            // Keep the same palette, but drive it with deeper mids + neon accents.
            let mut v16 = params.base_bias
                + depth * params.depth_gain
                + shimmer * params.shimmer_gain
                + neon * params.neon_gain;
            v16 = v16.clamp(0.0, 65535.0);
            base[i] = v16 as u16;
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let run = parse_args()?;
    let debug = run.debug;
    let mut screenshot_path = run.screenshot_path;
    let deband = DebandConfig {
        seed: 0x5EED_F00D,
        shift: -2,
        dist: DebandingDistribution::Gaussian,
    };
    let neon = NeonPatternParams::default();
    let palette = Palette256::SoftSky;
    if debug {
        println!("[main] debug enabled");
    }

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
    let (mut texture, mut pixels) =
        build_render_targets(&texture_creator, width, height, debug, palette, deband)?;

    let mut events = sdl.event_pump()?;
    let mut fps_counter = FpsCounter::new();
    let neon_start = Instant::now();
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
            (texture, pixels) =
                build_render_targets(&texture_creator, width, height, debug, palette, deband)?;
        }

        let t = neon_start.elapsed().as_secs_f32();
        render_neon_pattern_frame(width as usize, height as usize, t, neon, pixels.base_mut());
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
            canvas.window_mut().raise();
            window_shown = true;
        }
        if debug && let Some((fps, frame_ms)) = fps_counter.tick() {
            println!("[main:fps] fps={:.1} frame_ms={:.3}", fps, frame_ms);
        }
    }

    sdl.mouse().show_cursor(true);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::RunConfig;
    use super::parse_args_from;

    #[test]
    fn parse_args_defaults() {
        let cfg = parse_args_from(Vec::new()).expect("default parse should succeed");
        assert_eq!(cfg, RunConfig::default());
    }

    #[test]
    fn parse_args_debug_and_screenshot() {
        let cfg = parse_args_from(vec![
            "--debug".to_string(),
            "--screenshot".to_string(),
            "/tmp/test.ppm".to_string(),
        ])
        .expect("parse should succeed");
        assert_eq!(
            cfg,
            RunConfig {
                debug: true,
                screenshot_path: Some("/tmp/test.ppm".to_string()),
            }
        );
    }

    #[test]
    fn parse_args_rejects_missing_screenshot_path() {
        let err = parse_args_from(vec!["--screenshot".to_string()]).expect_err("missing path should fail");
        assert!(err.to_string().contains("requires a file path"));
    }
}
