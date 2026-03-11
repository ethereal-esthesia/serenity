use serenity::cli::{CommonRunConfig, parse_common_args_from};
use serenity::global_input::{GlobalInputCapture, ModifierState};
use sdl3::event::Event;
use sdl3::keyboard::{Keycode, Mod};
use sdl3::pixels::Color;
use sdl3::pixels::PixelFormatEnum;
use sdl3::rect::Rect;
use serenity::palette::{Palette256, palette_256};
use serenity::pixel_buffer::{
    DebandingDistribution, DebandingFilter, PixelBuffer, make_gradient_buffer16,
};
use std::f32::consts::TAU;
use std::time::Instant;

type RunConfig = CommonRunConfig;

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

struct RenderState<'a> {
    width: u32,
    height: u32,
    texture: sdl3::render::Texture<'a>,
    pixels: PixelBuffer,
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

fn parse_args() -> Result<RunConfig, Box<dyn std::error::Error>> {
    parse_common_args_from(std::env::args().skip(1))
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

fn build_render_state<'a>(
    texture_creator: &'a sdl3::render::TextureCreator<sdl3::video::WindowContext>,
    width: u32,
    height: u32,
    debug: bool,
    palette: Palette256,
    deband: DebandConfig,
) -> Result<RenderState<'a>, Box<dyn std::error::Error>> {
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
    Ok(RenderState {
        width,
        height,
        texture,
        pixels,
    })
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

fn keycode_label(keycode: Keycode) -> String {
    format!("{:?}", keycode)
        .to_uppercase()
        .replace('_', " ")
        .replace(':', " ")
        .replace(',', " ")
}

fn process_events(events: &mut sdl3::EventPump, keys_down: &mut Vec<String>) -> bool {
    let is_modifier = |keycode: Keycode| {
        matches!(
            keycode,
            Keycode::LShift
                | Keycode::RShift
                | Keycode::LCtrl
                | Keycode::RCtrl
                | Keycode::LAlt
                | Keycode::RAlt
                | Keycode::LGui
                | Keycode::RGui
        )
    };

    for event in events.poll_iter() {
        match event {
            Event::Quit { .. } => return true,
            Event::KeyDown {
                keycode: Some(keycode),
                repeat: false,
                ..
            } => {
                if keycode == Keycode::Escape {
                    return true;
                }
                if !is_modifier(keycode) {
                    let label = keycode_label(keycode);
                    if !keys_down.contains(&label) {
                        keys_down.push(label);
                    }
                }
            }
            Event::KeyUp {
                keycode: Some(keycode),
                repeat: false,
                ..
            } => {
                if !is_modifier(keycode) {
                    let label = keycode_label(keycode);
                    keys_down.retain(|k| k != &label);
                }
            }
            _ => {}
        }
    }
    false
}

fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x13, 0x11, 0x11, 0x0E],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1F],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x12, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0E, 0x11, 0x10, 0x0E, 0x01, 0x11, 0x0E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x01, 0x02, 0x06, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        ' ' => [0x00; 7],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        '/' => [0x01, 0x02, 0x04, 0x08, 0x10, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x06],
        _ => [0x1F, 0x01, 0x02, 0x04, 0x00, 0x04, 0x00], // '?'
    }
}

fn draw_rounded_box(
    canvas: &mut sdl3::render::Canvas<sdl3::video::Window>,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    radius: i32,
    color: Color,
) -> Result<(), Box<dyn std::error::Error>> {
    let w_i32 = w as i32;
    let h_i32 = h as i32;
    let r = radius.max(0).min((w_i32.min(h_i32)) / 2);
    canvas.set_draw_color(color);
    canvas.fill_rect(Rect::new(x + r, y, (w_i32 - 2 * r) as u32, h))?;
    canvas.fill_rect(Rect::new(x, y + r, r as u32, (h_i32 - 2 * r) as u32))?;
    canvas.fill_rect(Rect::new(
        x + w_i32 - r,
        y + r,
        r as u32,
        (h_i32 - 2 * r) as u32,
    ))?;

    for dy in 0..r {
        for dx in 0..r {
            if dx * dx + dy * dy <= r * r {
                canvas.fill_rect(Rect::new(x + r - 1 - dx, y + r - 1 - dy, 1, 1))?;
                canvas.fill_rect(Rect::new(x + w_i32 - r + dx, y + r - 1 - dy, 1, 1))?;
                canvas.fill_rect(Rect::new(x + r - 1 - dx, y + h_i32 - r + dy, 1, 1))?;
                canvas.fill_rect(Rect::new(x + w_i32 - r + dx, y + h_i32 - r + dy, 1, 1))?;
            }
        }
    }
    Ok(())
}

fn draw_text_5x7(
    canvas: &mut sdl3::render::Canvas<sdl3::video::Window>,
    x: i32,
    y: i32,
    text: &str,
    scale: i32,
    color: Color,
) -> Result<(), Box<dyn std::error::Error>> {
    let s = scale.max(1);
    canvas.set_draw_color(color);
    for (ci, ch) in text.chars().enumerate() {
        let glyph = glyph_5x7(ch);
        let bx = x + (ci as i32) * (6 * s);
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if (bits >> (4 - col)) & 1 == 1 {
                    canvas.fill_rect(Rect::new(
                        bx + col * s,
                        y + (row as i32) * s,
                        s as u32,
                        s as u32,
                    ))?;
                }
            }
        }
    }
    Ok(())
}

fn draw_key_debug_hud(
    canvas: &mut sdl3::render::Canvas<sdl3::video::Window>,
    keys_down: &[String],
    mods: Mod,
) -> Result<(), Box<dyn std::error::Error>> {
    let row1_text = if keys_down.is_empty() {
        "KEYS".to_string()
    } else {
        format!("KEYS {}", keys_down.join(" + "))
    };
    let row1_scale = 2i32;
    let row1_w = (row1_text.chars().count() as i32 * 6 - 1) * row1_scale;
    let row1_h = 7 * row1_scale;
    let row2_scale = 1i32;
    let row2_h = 7 * row2_scale;
    let mut mods_active: Vec<&str> = Vec::new();
    if mods.contains(Mod::LSHIFTMOD) {
        mods_active.push("LSHIFT");
    }
    if mods.contains(Mod::RSHIFTMOD) {
        mods_active.push("RSHIFT");
    }
    if mods.contains(Mod::LCTRLMOD) {
        mods_active.push("LCTRL");
    }
    if mods.contains(Mod::RCTRLMOD) {
        mods_active.push("RCTRL");
    }
    if mods.contains(Mod::LALTMOD) {
        mods_active.push("LALT");
    }
    if mods.contains(Mod::RALTMOD) {
        mods_active.push("RALT");
    }
    if mods.contains(Mod::LGUIMOD) {
        mods_active.push("LGUI");
    }
    if mods.contains(Mod::RGUIMOD) {
        mods_active.push("RGUI");
    }
    let row2_text = if mods_active.is_empty() {
        "MODS".to_string()
    } else {
        format!("MODS {}", mods_active.join(" "))
    };
    let row2_w = (row2_text.chars().count() as i32 * 6 - 1) * row2_scale;

    let pad = 10i32;
    let box_w = (row1_w.max(row2_w) + pad * 2).max(140) as u32;
    let box_h = (row1_h + row2_h + pad * 3) as u32;
    draw_rounded_box(canvas, 12, 12, box_w, box_h, 10, Color::RGB(16, 24, 36))?;
    draw_text_5x7(
        canvas,
        12 + pad,
        12 + pad,
        &row1_text,
        row1_scale,
        Color::RGB(210, 232, 255),
    )?;
    draw_text_5x7(
        canvas,
        12 + pad,
        12 + pad * 2 + row1_h,
        &row2_text,
        row2_scale,
        Color::RGB(186, 200, 220),
    )?;
    Ok(())
}

fn modifier_state_to_sdl_mod(mods: ModifierState) -> Mod {
    let mut out = Mod::NOMOD;
    if mods.lshift {
        out |= Mod::LSHIFTMOD;
    }
    if mods.rshift {
        out |= Mod::RSHIFTMOD;
    }
    if mods.lctrl {
        out |= Mod::LCTRLMOD;
    }
    if mods.rctrl {
        out |= Mod::RCTRLMOD;
    }
    if mods.lalt {
        out |= Mod::LALTMOD;
    }
    if mods.ralt {
        out |= Mod::RALTMOD;
    }
    if mods.lgui {
        out |= Mod::LGUIMOD;
    }
    if mods.rgui {
        out |= Mod::RGUIMOD;
    }
    out
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
    let mut render =
        build_render_state(&texture_creator, initial_width, initial_height, debug, palette, deband)?;

    let mut events = sdl.event_pump()?;
    let mut fps_counter = FpsCounter::new();
    let neon_start = Instant::now();
    let mut window_shown = false;
    let mut keys_down: Vec<String> = Vec::new();
    let global_capture = GlobalInputCapture::start();
    let mut last_capture_status: Option<String> = None;
    println!("Neon pattern mode (default, neon accents). Press Esc to quit.");

    'running: loop {
        if process_events(&mut events, &mut keys_down) {
            break 'running;
        }

        let (current_w, current_h) = canvas.output_size()?;
        if current_w > 0
            && current_h > 0
            && (current_w != render.width || current_h != render.height)
        {
            render =
                build_render_state(&texture_creator, current_w, current_h, debug, palette, deband)?;
        }

        let t = neon_start.elapsed().as_secs_f32();
        render_neon_pattern_frame(
            render.width as usize,
            render.height as usize,
            t,
            neon,
            render.pixels.base_mut(),
        );
        render.pixels.mark_dirty();
        render.pixels.upload_to_texture(&mut render.texture)?;
        if let Some(path) = screenshot_path.take() {
            render.pixels.write_ppm(&path)?;
            println!("[main:screenshot] wrote {}", path);
        }

        canvas.copy(&render.texture, None, None)?;
        let snap = global_capture.snapshot();
        if snap.status != last_capture_status {
            if let Some(msg) = &snap.status {
                println!("[input] {}", msg);
            }
            last_capture_status = snap.status.clone();
        }
        let (hud_keys, hud_mods) = if snap.active {
            (snap.keys_down, modifier_state_to_sdl_mod(snap.mods))
        } else {
            (keys_down.clone(), sdl.keyboard().mod_state())
        };
        draw_key_debug_hud(&mut canvas, &hud_keys, hud_mods)?;
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
