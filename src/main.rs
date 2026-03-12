use serenity::cli::{CommonRunConfig, parse_common_args_from};
use serenity::global_input::{GlobalInputCapture, ModifierState};
use serenity::runtime::input::{
    WindowInputState, process_events_with_debug, should_enable_global_capture, sync_cursor_visibility,
};
use sdl3::keyboard::Mod;
use sdl3::pixels::Color;
use sdl3::pixels::PixelFormatEnum;
use sdl3::rect::Rect;
use serenity::palette::{Palette256, palette_256};
use serenity::pixel_buffer::{
    DebandingDistribution, DebandingFilter, PixelBuffer, make_gradient_buffer16,
};
use sdl3::render::TextureCreator;
use std::f32::consts::TAU;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
#[cfg(feature = "hud_ttf")]
use std::path::{Path, PathBuf};

#[cfg(feature = "hud_ttf")]
type HudFont = sdl3::ttf::Font<'static>;
#[cfg(not(feature = "hud_ttf"))]
type HudFont = ();

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

struct ThreadEventPanel {
    lines: Vec<String>,
    scroll_from_bottom: usize,
}

impl ThreadEventPanel {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            scroll_from_bottom: 0,
        }
    }

    fn set_lines(&mut self, lines: Vec<String>) {
        self.lines = lines;
        let max = self.max_scroll_for_rows(6);
        if self.scroll_from_bottom > max {
            self.scroll_from_bottom = max;
        }
    }

    fn scroll_lines(&mut self, delta_lines: i32, visible_rows: usize) {
        if self.lines.is_empty() || delta_lines == 0 {
            return;
        }
        if delta_lines > 0 {
            self.scroll_from_bottom = self.scroll_from_bottom.saturating_add(delta_lines as usize);
        } else {
            self.scroll_from_bottom = self
                .scroll_from_bottom
                .saturating_sub((-delta_lines) as usize);
        }
        let max = self.max_scroll_for_rows(visible_rows);
        if self.scroll_from_bottom > max {
            self.scroll_from_bottom = max;
        }
    }

    fn max_scroll_for_rows(&self, visible_rows: usize) -> usize {
        self.lines.len().saturating_sub(visible_rows)
    }

    fn visible_lines(&self, visible_rows: usize) -> &[String] {
        if self.lines.is_empty() {
            return &[];
        }
        let end = self.lines.len().saturating_sub(self.scroll_from_bottom);
        let start = end.saturating_sub(visible_rows);
        &self.lines[start..end]
    }
}

fn mods_to_labels(mods: Mod, fn_mod: bool) -> String {
    let mut labels: Vec<&str> = Vec::new();
    if mods.contains(Mod::CAPSMOD) {
        labels.push("CAPS");
    }
    if mods.contains(Mod::LSHIFTMOD) {
        labels.push("LSHIFT");
    }
    if mods.contains(Mod::RSHIFTMOD) {
        labels.push("RSHIFT");
    }
    if mods.contains(Mod::LCTRLMOD) {
        labels.push("LCTRL");
    }
    if mods.contains(Mod::RCTRLMOD) {
        labels.push("RCTRL");
    }
    if mods.contains(Mod::LALTMOD) {
        labels.push("LALT");
    }
    if mods.contains(Mod::RALTMOD) {
        labels.push("RALT");
    }
    if mods.contains(Mod::LGUIMOD) {
        labels.push("LGUI");
    }
    if mods.contains(Mod::RGUIMOD) {
        labels.push("RGUI");
    }
    if fn_mod {
        labels.push("FN");
    }
    if labels.is_empty() {
        "MODS[]".to_string()
    } else {
        format!("MODS[{}]", labels.join(" "))
    }
}

fn push_main_visible_event(
    feed: &mut VecDeque<String>,
    last_signature: &mut String,
    keys: &[String],
    mods: Mod,
    fn_mod: bool,
) {
    let key_sig = if keys.is_empty() {
        "KEYS[]".to_string()
    } else {
        format!("KEYS[{}]", keys.join(" + "))
    };
    let sig = format!("{} {}", key_sig, mods_to_labels(mods, fn_mod));
    if *last_signature == sig {
        return;
    }
    *last_signature = sig.clone();
    feed.push_back(sig);
    while feed.len() > 32 {
        let _ = feed.pop_front();
    }
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
    #[cfg_attr(not(feature = "hud_ttf"), allow(unused_variables))]
    texture_creator: &TextureCreator<sdl3::video::WindowContext>,
    #[cfg_attr(not(feature = "hud_ttf"), allow(unused_variables))]
    hud_font: Option<&HudFont>,
    keys_down: &[String],
    optional_keys: &[String],
    mods: Mod,
    fn_mod: bool,
    show_mod_keycodes: bool,
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
    if mods.contains(Mod::CAPSMOD) {
        mods_active.push(if show_mod_keycodes { "CAPS|KC57" } else { "CAPS" });
    }
    if mods.contains(Mod::LSHIFTMOD) {
        mods_active.push(if show_mod_keycodes { "LSHIFT|KC56" } else { "LSHIFT" });
    }
    if mods.contains(Mod::RSHIFTMOD) {
        mods_active.push(if show_mod_keycodes { "RSHIFT|KC60" } else { "RSHIFT" });
    }
    if mods.contains(Mod::LCTRLMOD) {
        mods_active.push(if show_mod_keycodes { "LCTRL|KC59" } else { "LCTRL" });
    }
    if mods.contains(Mod::RCTRLMOD) {
        mods_active.push(if show_mod_keycodes { "RCTRL|KC62" } else { "RCTRL" });
    }
    if mods.contains(Mod::LALTMOD) {
        mods_active.push(if show_mod_keycodes { "LALT|KC58" } else { "LALT" });
    }
    if mods.contains(Mod::RALTMOD) {
        mods_active.push(if show_mod_keycodes { "RALT|KC61" } else { "RALT" });
    }
    if mods.contains(Mod::LGUIMOD) {
        mods_active.push(if show_mod_keycodes { "LGUI|KC55" } else { "LGUI" });
    }
    if mods.contains(Mod::RGUIMOD) {
        mods_active.push(if show_mod_keycodes { "RGUI|KC54" } else { "RGUI" });
    }
    if fn_mod {
        mods_active.push(if show_mod_keycodes { "FN|KC63" } else { "FN" });
    }
    let row2_text = if mods_active.is_empty() {
        "MODS".to_string()
    } else {
        format!("MODS {}", mods_active.join(" "))
    };
    let row3_text = if optional_keys.is_empty() {
        String::new()
    } else {
        format!("OPT {}", optional_keys.join(" + "))
    };
    let pad = 10i32;
    #[cfg(feature = "hud_ttf")]
    if let Some(font) = hud_font {
        let (row1_w_px, row1_h_px) = font.size_of(&row1_text).unwrap_or((140, 20));
        let (row2_w_px, row2_h_px) = font.size_of(&row2_text).unwrap_or((140, 16));
        let (row3_w_px, row3_h_px) = if row3_text.is_empty() {
            (0, 0)
        } else {
            font.size_of(&row3_text).unwrap_or((140, 16))
        };
        let box_w = ((row1_w_px as i32)
            .max(row2_w_px as i32)
            .max(row3_w_px as i32)
            + pad * 2)
            .max(140) as u32;
        let box_h = (row1_h_px as i32
            + row2_h_px as i32
            + row3_h_px as i32
            + pad * if row3_text.is_empty() { 3 } else { 4 }) as u32;
        draw_rounded_box(canvas, 12, 12, box_w, box_h, 10, Color::RGB(16, 24, 36))?;
        draw_text_ttf(
            canvas,
            texture_creator,
            font,
            12 + pad,
            12 + pad,
            &row1_text,
            Color::RGB(210, 232, 255),
        )?;
        draw_text_ttf(
            canvas,
            texture_creator,
            font,
            12 + pad,
            12 + pad * 2 + row1_h_px as i32,
            &row2_text,
            Color::RGB(186, 200, 220),
        )?;
        if !row3_text.is_empty() {
            draw_text_ttf(
                canvas,
                texture_creator,
                font,
                12 + pad,
                12 + pad * 3 + row1_h_px as i32 + row2_h_px as i32,
                &row3_text,
                Color::RGB(170, 190, 210),
            )?;
        }
        return Ok(());
    }

    let row2_w = (row2_text.chars().count() as i32 * 6 - 1) * row2_scale;
    let row3_w = if row3_text.is_empty() {
        0
    } else {
        (row3_text.chars().count() as i32 * 6 - 1) * row2_scale
    };
    let box_w = (row1_w.max(row2_w).max(row3_w) + pad * 2).max(140) as u32;
    let box_h = (row1_h
        + row2_h
        + if row3_text.is_empty() { 0 } else { row2_h }
        + pad * if row3_text.is_empty() { 3 } else { 4 }) as u32;
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
    if !row3_text.is_empty() {
        draw_text_5x7(
            canvas,
            12 + pad,
            12 + pad * 3 + row1_h + row2_h,
            &row3_text,
            row2_scale,
            Color::RGB(170, 190, 210),
        )?;
    }
    Ok(())
}

fn draw_thread_event_panel(
    canvas: &mut sdl3::render::Canvas<sdl3::video::Window>,
    #[cfg_attr(not(feature = "hud_ttf"), allow(unused_variables))]
    texture_creator: &TextureCreator<sdl3::video::WindowContext>,
    #[cfg_attr(not(feature = "hud_ttf"), allow(unused_variables))]
    hud_font: Option<&HudFont>,
    panel: &ThreadEventPanel,
    x: i32,
    y: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let title = "THREAD EVENTS";
    let visible_rows = 6usize;
    let lines = panel.visible_lines(visible_rows);
    let pad = 8i32;
    let row_h = 16i32;
    let box_w = 700u32;
    let box_h = (pad * 2 + row_h * (1 + visible_rows as i32)) as u32;
    draw_rounded_box(canvas, x, y, box_w, box_h, 10, Color::RGB(14, 20, 30))?;

    #[cfg(feature = "hud_ttf")]
    if let Some(font) = hud_font {
        draw_text_ttf(
            canvas,
            texture_creator,
            font,
            x + pad,
            y + pad,
            title,
            Color::RGB(196, 220, 242),
        )?;
        for (i, line) in lines.iter().rev().enumerate() {
            draw_text_ttf(
                canvas,
                texture_creator,
                font,
                x + pad,
                y + pad + row_h * (i as i32 + 1),
                line,
                Color::RGB(150, 170, 190),
            )?;
        }
        return Ok(());
    }

    draw_text_5x7(
        canvas,
        x + pad,
        y + pad,
        title,
        1,
        Color::RGB(196, 220, 242),
    )?;
    for (i, line) in lines.iter().rev().enumerate() {
        draw_text_5x7(
            canvas,
            x + pad,
            y + pad + row_h * (i as i32 + 1),
            line,
            1,
            Color::RGB(150, 170, 190),
        )?;
    }
    Ok(())
}

#[cfg(feature = "hud_ttf")]
fn draw_text_ttf(
    canvas: &mut sdl3::render::Canvas<sdl3::video::Window>,
    texture_creator: &TextureCreator<sdl3::video::WindowContext>,
    font: &HudFont,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
) -> Result<(), Box<dyn std::error::Error>> {
    let surface = font
        .render(text)
        .blended(color)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let texture = texture_creator
        .create_texture_from_surface(&surface)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let q = texture.query();
    canvas.copy(&texture, None, Rect::new(x, y, q.width, q.height))?;
    Ok(())
}

#[cfg(feature = "hud_ttf")]
fn find_cascadia_font_path() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("assets/fonts/CascadiaMono-Regular.ttf"),
        PathBuf::from("assets/fonts/CascadiaMono.ttf"),
        PathBuf::from("assets/fonts/CascadiaCode-Regular.ttf"),
    ];
    for path in candidates {
        if Path::new(&path).exists() {
            return Some(path);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let user_candidates = [
            PathBuf::from(format!("{home}/Library/Fonts/CascadiaMono-Regular.ttf")),
            PathBuf::from(format!("{home}/Library/Fonts/CascadiaMono.ttf")),
            PathBuf::from(format!("{home}/Library/Fonts/CascadiaCode-Regular.ttf")),
        ];
        for path in user_candidates {
            if Path::new(&path).exists() {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(feature = "hud_ttf")]
fn load_hud_font(debug: bool) -> Option<HudFont> {
    let ttf = match sdl3::ttf::init() {
        Ok(t) => t,
        Err(err) => {
            if debug {
                println!("[main:hud_font] SDL_ttf init failed; using bitmap HUD: {}", err);
            }
            return None;
        }
    };
    let path = match find_cascadia_font_path() {
        Some(p) => p,
        None => {
            if debug {
                println!(
                    "[main:hud_font] Cascadia Mono not found; expected assets/fonts/CascadiaMono-Regular.ttf (or similar). Using bitmap HUD"
                );
            }
            return None;
        }
    };
    match ttf.load_font(&path, 18.0) {
        Ok(font) => {
            if debug {
                println!("[main:hud_font] using Cascadia font at {}", path.display());
            }
            Some(font)
        }
        Err(err) => {
            if debug {
                println!(
                    "[main:hud_font] failed to load {} ({}); using bitmap HUD",
                    path.display(),
                    err
                );
            }
            None
        }
    }
}

#[cfg(not(feature = "hud_ttf"))]
fn load_hud_font(debug: bool) -> Option<HudFont> {
    if debug {
        println!("[main:hud_font] hud_ttf feature disabled; using bitmap HUD");
    }
    None
}

fn modifier_state_to_sdl_mod(mods: ModifierState) -> Mod {
    let mut out = Mod::NOMOD;
    if mods.caps_lock {
        out |= Mod::CAPSMOD;
    }
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
        println!("[main] naive modifier detection enabled");
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
    let hud_font = load_hud_font(debug);

    let mut events = sdl.event_pump()?;
    let mut fps_counter = FpsCounter::new();
    let neon_start = Instant::now();
    let mut input_state = WindowInputState::with_cursor_hidden(true);
    let mut thread_panel = ThreadEventPanel::new();
    let mut main_visible_feed: VecDeque<String> = VecDeque::new();
    let mut last_visible_signature = String::new();
    let mut prev_window_focused = false;
    let mut attach_request_due: Option<Instant> = None;
    let mut initial_attach_pending = true;
    let mut global_capture: Option<GlobalInputCapture> = None;
    println!("Neon pattern mode (default, neon accents). Press Esc to quit.");

    'running: loop {
        if process_events_with_debug(&mut events, &mut input_state, debug) {
            break 'running;
        }
        if !prev_window_focused && input_state.window_focused {
            if let Some(capture) = &global_capture {
                capture.notify_focus_gained();
            }
            let tap_active = global_capture
                .as_ref()
                .map(|c| c.is_tap_active())
                .unwrap_or(false);
            if initial_attach_pending && !tap_active {
                if let Some(capture) = &global_capture {
                    capture.request_attach();
                }
                initial_attach_pending = false;
                if debug {
                    println!("[main:global_input] focus gained; requesting initial attach now");
                }
            } else if !tap_active {
                attach_request_due = Some(Instant::now() + Duration::from_secs(10));
                if debug {
                    println!("[main:global_input] focus gained; scheduling attach in 10s");
                }
            } else if debug {
                println!("[main:global_input] focus gained; tap already active, no schedule");
                initial_attach_pending = false;
            }
        } else if prev_window_focused && !input_state.window_focused {
            if let Some(capture) = &global_capture {
                capture.notify_focus_lost();
            }
            if attach_request_due.is_some() {
                attach_request_due = None;
                if debug {
                    println!("[main:global_input] focus lost; canceled pending attach schedule");
                }
            } else if debug {
                println!("[main:global_input] focus lost; no pending attach schedule");
            }
        }
        prev_window_focused = input_state.window_focused;

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
        if !input_state.window_shown {
            {
                let window = canvas.window_mut();
                window.show();
                window.raise();
            }
            input_state.window_shown = true;
            global_capture = Some(GlobalInputCapture::start_with_options(
                debug,
                true,
            ));
            if debug {
                println!("[main:global_input] window shown; waiting for focus before initial attach");
            }
        }
        if let (Some(capture), Some(due)) = (&global_capture, attach_request_due)
            && input_state.window_focused
            && Instant::now() >= due
        {
            if debug {
                println!("[main:global_input] requesting global attach now");
            }
            capture.request_attach();
            attach_request_due = None;
        }
        if let Some(capture) = &global_capture
            && capture.is_tap_active()
            && attach_request_due.is_some()
        {
            attach_request_due = None;
            if debug {
                println!("[main:global_input] tap active; cleared pending attach schedule");
            }
        }

        sync_cursor_visibility(&sdl, &mut input_state);

        thread_panel.scroll_lines(input_state.thread_panel_scroll_lines, 6);
        let (hud_keys, hud_optional_keys, hud_mods, hud_fn) = if let Some(capture) = &global_capture {
            capture.set_capture_enabled(should_enable_global_capture(&input_state));
            let snap = capture.snapshot();
            if snap.active {
                if snap
                    .keys_down
                    .iter()
                    .any(|k| k == "ESCAPE" || k.starts_with("ESCAPE|"))
                {
                    break 'running;
                }
                let (optional, regular): (Vec<String>, Vec<String>) = snap
                    .keys_down
                    .into_iter()
                    .partition(|k| k.contains("|HIDU") || k.contains("INJ|"));
                (
                    regular,
                    optional,
                    modifier_state_to_sdl_mod(snap.mods),
                    snap.mods.fn_key,
                )
            } else {
                (
                    input_state.keys_down.clone(),
                    Vec::new(),
                    sdl.keyboard().mod_state(),
                    false,
                )
            }
        } else {
            (
                input_state.keys_down.clone(),
                Vec::new(),
                sdl.keyboard().mod_state(),
                false,
            )
        };
        push_main_visible_event(
            &mut main_visible_feed,
            &mut last_visible_signature,
            &hud_keys,
            hud_mods,
            hud_fn,
        );
        thread_panel.set_lines(main_visible_feed.iter().cloned().collect());
        draw_key_debug_hud(
            &mut canvas,
            &texture_creator,
            hud_font.as_ref(),
            &hud_keys,
            &hud_optional_keys,
            hud_mods,
            hud_fn,
            true,
        )?;
        draw_thread_event_panel(
            &mut canvas,
            &texture_creator,
            hud_font.as_ref(),
            &thread_panel,
            12,
            170,
        )?;
        let _ = canvas.present();
        if debug && let Some((fps, frame_ms)) = fps_counter.tick() {
            println!("[main:fps] fps={:.1} frame_ms={:.3}", fps, frame_ms);
        }
    }

    sdl.mouse().show_cursor(true);
    Ok(())
}
