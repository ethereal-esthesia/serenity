use sdl3::keyboard::Keycode;
use sdl3::pixels::Color;
use sdl3::pixels::PixelFormatEnum;
use std::time::Instant;
use std::{io::Write, path::Path};

use serenity::cli::{CommonRunConfig, parse_common_args_from};
use serenity::fast_rng::FastRng;
use serenity::palette::{Palette256, palette_256};
use serenity::runtime::input::{WindowInputState, process_events_with_keydown, sync_cursor_visibility};

const PANEL_SIZE: usize = 32;
const NOISE_SEED: u64 = 0x5EED_F00D;

type RunConfig = CommonRunConfig;

struct RenderState<'a> {
    width: u32,
    height: u32,
    texture: sdl3::render::Texture<'a>,
    scene: SceneBuffers,
}

struct PerfStats {
    start: Instant,
    frames: u64,
    draw_ms_total: f64,
    present_ms_total: f64,
    resize_rebuilds: u64,
}

impl PerfStats {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            frames: 0,
            draw_ms_total: 0.0,
            present_ms_total: 0.0,
            resize_rebuilds: 0,
        }
    }

    fn record_draw_ms(&mut self, draw_ms: f64) {
        self.draw_ms_total += draw_ms;
    }

    fn record_present_ms(&mut self, present_ms: f64) {
        self.present_ms_total += present_ms;
        self.frames += 1;
    }

    fn record_resize_rebuild(&mut self) {
        self.resize_rebuilds += 1;
    }

    fn maybe_print_and_reset(&mut self, cfg: NoiseConfig) {
        let elapsed = self.start.elapsed();
        if elapsed.as_secs_f64() < 1.0 || self.frames == 0 {
            return;
        }
        let secs = elapsed.as_secs_f64();
        let fps = self.frames as f64 / secs;
        let frame_ms = (secs * 1000.0) / self.frames as f64;
        let draw_ms = self.draw_ms_total / self.frames as f64;
        let present_ms = self.present_ms_total / self.frames as f64;
        println!(
            "stats: mode={} shift={} panel={} black={} fps={:.1} frame_ms={:.3} draw_ms={:.3} present_ms={:.3} resize_rebuilds={}",
            cfg.mode.label(),
            cfg.shift,
            cfg.panel_mode,
            cfg.black_screen_mode,
            fps,
            frame_ms,
            draw_ms,
            present_ms,
            self.resize_rebuilds
        );
        self.start = Instant::now();
        self.frames = 0;
        self.draw_ms_total = 0.0;
        self.present_ms_total = 0.0;
        self.resize_rebuilds = 0;
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoiseMode {
    Linear,
    Gaussian,
}

impl NoiseMode {
    fn toggled(self) -> Self {
        match self {
            Self::Linear => Self::Gaussian,
            Self::Gaussian => Self::Linear,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Gaussian => "Gaussian",
        }
    }
}

fn shifted_noise_max(shift: i8) -> u64 {
    if shift >= 0 {
        255u64 >> (shift as u8).min(7)
    } else {
        255u64 << ((-shift) as u8).min(7)
    }
}

#[inline]
fn shift_noise_u8(v: u8, shift: i8) -> u16 {
    if shift >= 0 {
        let s = (shift as u8).min(7);
        (v >> s) as u16
    } else {
        let s = ((-shift) as u8).min(7);
        ((v as u16) << s) as u16
    }
}

fn make_noise_buffer_linear_shift(width: usize, height: usize, seed: u64, shift: i8) -> Vec<u16> {
    let mut rng = FastRng::new(seed);
    let mut out = vec![0u16; width * height];
    for p in &mut out {
        *p = shift_noise_u8(rng.next_u8(), shift);
    }
    out
}

fn make_noise_buffer_gaussian_shift(width: usize, height: usize, seed: u64, shift: i8) -> Vec<u16> {
    let mut rng = FastRng::new(seed);
    let mut out = vec![0u16; width * height];
    for p in &mut out {
        *p = shift_noise_u8(rng.next_gaussian8(), shift);
    }
    out
}

fn make_noise_buffer(width: usize, height: usize, seed: u64, mode: NoiseMode, noise_shift: i8) -> Vec<u16> {
    match mode {
        NoiseMode::Linear => make_noise_buffer_linear_shift(width, height, seed, noise_shift),
        NoiseMode::Gaussian => make_noise_buffer_gaussian_shift(width, height, seed, noise_shift),
    }
}

fn make_noise_panel(seed: u64, mode: NoiseMode, noise_shift: i8) -> Vec<u16> {
    make_noise_buffer(PANEL_SIZE, PANEL_SIZE, seed, mode, noise_shift)
}

fn make_noise_full_if_needed(
    panel_mode: bool,
    width: usize,
    height: usize,
    seed: u64,
    mode: NoiseMode,
    noise_shift: i8,
) -> Option<Vec<u16>> {
    if panel_mode {
        None
    } else {
        Some(make_noise_buffer(width, height, seed, mode, noise_shift))
    }
}

#[derive(Clone, Copy, Debug)]
struct NoiseConfig {
    mode: NoiseMode,
    shift: i8,
    panel_mode: bool,
    black_screen_mode: bool,
}

impl Default for NoiseConfig {
    fn default() -> Self {
        Self {
            mode: NoiseMode::Gaussian,
            shift: -2,
            panel_mode: false,
            black_screen_mode: false,
        }
    }
}

struct SceneBuffers {
    gradient16: Vec<u16>,
    noise: Option<Vec<u16>>,
    noise_panel: Vec<u16>,
}

fn rebuild_noise_buffers(width: usize, height: usize, cfg: NoiseConfig) -> (Option<Vec<u16>>, Vec<u16>) {
    (
        make_noise_full_if_needed(cfg.panel_mode, width, height, NOISE_SEED, cfg.mode, cfg.shift),
        make_noise_panel(NOISE_SEED, cfg.mode, cfg.shift),
    )
}

fn rebuild_scene_buffers(width: usize, height: usize, cfg: NoiseConfig) -> SceneBuffers {
    let (noise, noise_panel) = rebuild_noise_buffers(width, height, cfg);
    SceneBuffers {
        gradient16: make_gradient_buffer16(width, height),
        noise,
        noise_panel,
    }
}

fn print_config(prefix: &str, cfg: NoiseConfig) {
    println!(
        "{} mode={} shift={} source=0..255 shifted=0..{} panel={} black={}",
        prefix,
        cfg.mode.label(),
        cfg.shift,
        shifted_noise_max(cfg.shift),
        cfg.panel_mode,
        cfg.black_screen_mode
    );
}

fn handle_keydown(keycode: Keycode, cfg: &mut NoiseConfig, width: usize, height: usize, scene: &mut SceneBuffers) {
    let mut changed_noise = false;
    match keycode {
        Keycode::Space => {
            cfg.mode = cfg.mode.toggled();
            changed_noise = true;
        }
        Keycode::Tab | Keycode::P => {
            cfg.panel_mode = !cfg.panel_mode;
            changed_noise = true;
            println!(
                "Panel mode: {} (tile={}x{})",
                cfg.panel_mode, PANEL_SIZE, PANEL_SIZE
            );
        }
        Keycode::Grave => {
            cfg.black_screen_mode = !cfg.black_screen_mode;
            print_config("Black toggle:", *cfg);
        }
        Keycode::Equals | Keycode::KpPlus => {
            cfg.shift = (cfg.shift + 1).min(7);
            changed_noise = true;
            print_config("Noise shift:", *cfg);
        }
        Keycode::Minus | Keycode::KpMinus => {
            cfg.shift = (cfg.shift - 1).max(-7);
            changed_noise = true;
            print_config("Noise shift:", *cfg);
        }
        _ => {}
    }

    if changed_noise {
        let (noise, noise_panel) = rebuild_noise_buffers(width, height, *cfg);
        scene.noise = noise;
        scene.noise_panel = noise_panel;
        if keycode == Keycode::Space {
            print_config("Noise mode:", *cfg);
        }
    }
}

fn render_frame(
    texture: &mut sdl3::render::Texture<'_>,
    width: usize,
    height: usize,
    palette256: &[[u8; 3]],
    scene: &SceneBuffers,
    cfg: NoiseConfig,
) -> Result<(), std::io::Error> {
    let noise_full_ref = scene.noise.as_ref();
    let panel_enabled = cfg.panel_mode;
    let black_enabled = cfg.black_screen_mode;
    texture
        .with_lock(None, |buf: &mut [u8], pitch: usize| {
            if panel_enabled {
                for y in 0..height {
                    let row = &mut buf[y * pitch..(y + 1) * pitch];
                    let panel_row_base = (y & 31) * PANEL_SIZE;
                    for x in 0..width {
                        let idx = y * width + x;
                        let noise_term = scene.noise_panel[panel_row_base + (x & 31)];
                        let base16 = if black_enabled { 0 } else { scene.gradient16[idx] };
                        let c = (base16.saturating_add(noise_term) >> 8) as u8;
                        let off = x * 4;
                        let [r, g, b] = palette256[c as usize];
                        row[off] = b;
                        row[off + 1] = g;
                        row[off + 2] = r;
                        row[off + 3] = 0xFF;
                    }
                }
            } else {
                let noise_ref = noise_full_ref.expect("full noise buffer missing");
                for y in 0..height {
                    let row = &mut buf[y * pitch..(y + 1) * pitch];
                    for x in 0..width {
                        let idx = y * width + x;
                        let noise_term = noise_ref[idx];
                        let base16 = if black_enabled { 0 } else { scene.gradient16[idx] };
                        let c = (base16.saturating_add(noise_term) >> 8) as u8;
                        let off = x * 4;
                        let [r, g, b] = palette256[c as usize];
                        row[off] = b;
                        row[off + 1] = g;
                        row[off + 2] = r;
                        row[off + 3] = 0xFF;
                    }
                }
            }
        })
        .map_err(|e| std::io::Error::other(e.to_string()))
}

fn write_scene_ppm<P: AsRef<Path>>(
    path: P,
    width: usize,
    height: usize,
    palette256: &[[u8; 3]],
    scene: &SceneBuffers,
    cfg: NoiseConfig,
) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    writeln!(file, "P6")?;
    writeln!(file, "{} {}", width, height)?;
    writeln!(file, "255")?;
    for y in 0..height {
        let panel_row_base = (y & 31) * PANEL_SIZE;
        for x in 0..width {
            let idx = y * width + x;
            let noise_term = if cfg.panel_mode {
                scene.noise_panel[panel_row_base + (x & 31)]
            } else {
                scene.noise.as_ref().expect("full noise buffer missing")[idx]
            };
            let base16 = if cfg.black_screen_mode { 0 } else { scene.gradient16[idx] };
            let c = (base16.saturating_add(noise_term) >> 8) as u8;
            let [r, g, b] = palette256[c as usize];
            file.write_all(&[r, g, b])?;
        }
    }
    Ok(())
}

fn parse_args() -> Result<RunConfig, Box<dyn std::error::Error>> {
    parse_common_args_from(std::env::args().skip(1))
}

fn build_render_state<'a>(
    texture_creator: &'a sdl3::render::TextureCreator<sdl3::video::WindowContext>,
    width: u32,
    height: u32,
    cfg: NoiseConfig,
) -> Result<RenderState<'a>, Box<dyn std::error::Error>> {
    let texture = texture_creator
        .create_texture_streaming(Some(PixelFormatEnum::ARGB8888.into()), width, height)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let scene = rebuild_scene_buffers(width as usize, height as usize, cfg);
    Ok(RenderState {
        width,
        height,
        texture,
        scene,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut run = parse_args()?;
    let palette256 = palette_256(Palette256::SoftSky);

    let sdl = sdl3::init()?;
    let video = sdl.video()?;
    sdl.mouse().show_cursor(true);

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
    let mut cfg = NoiseConfig::default();
    let mut render = build_render_state(&texture_creator, initial_width, initial_height, cfg)?;

    let mut events = sdl.event_pump()?;
    let mut stats = PerfStats::new();
    let mut input_state = WindowInputState::default();
    #[cfg(debug_assertions)]
    println!("Debug build detected: run `cargo run --release --bin noise_texture_test` for real perf numbers.");
    println!(
        "Indexed mode | Space=noise mode | Tab=32x32 panel | `=black screen | +/- shift (-7..7)"
    );
    print_config("Current:", cfg);
    'running: loop {
        if process_events_with_keydown(&mut events, &mut input_state, |keycode| {
            handle_keydown(
                keycode,
                &mut cfg,
                render.width as usize,
                render.height as usize,
                &mut render.scene,
            );
            false
        }) {
            break 'running;
        }

        let (current_w, current_h) = canvas.output_size()?;
        if current_w > 0
            && current_h > 0
            && (current_w != render.width || current_h != render.height)
        {
            render = build_render_state(&texture_creator, current_w, current_h, cfg)?;
            stats.record_resize_rebuild();
        }

        let draw_start = Instant::now();
        render_frame(
            &mut render.texture,
            render.width as usize,
            render.height as usize,
            &palette256,
            &render.scene,
            cfg,
        )?;
        if let Some(path) = run.screenshot_path.take() {
            write_scene_ppm(
                &path,
                render.width as usize,
                render.height as usize,
                &palette256,
                &render.scene,
                cfg,
            )?;
            println!("[noise_texture_test:screenshot] wrote {}", path);
        }
        stats.record_draw_ms(draw_start.elapsed().as_secs_f64() * 1000.0);
        canvas.copy(&render.texture, None, None)?;

        let present_start = Instant::now();
        let _ = canvas.present();
        if !input_state.window_shown {
            canvas.window_mut().show();
            canvas.window_mut().raise();
            input_state.window_shown = true;
        }
        sync_cursor_visibility(&sdl, &mut input_state);
        stats.record_present_ms(present_start.elapsed().as_secs_f64() * 1000.0);
        stats.maybe_print_and_reset(cfg);
    }

    sdl.mouse().show_cursor(true);
    Ok(())
}
