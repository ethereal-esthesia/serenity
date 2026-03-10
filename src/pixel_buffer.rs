use sdl3::render::Texture;
use std::io::{self, Write};
use std::path::Path;
use crate::fast_rng::FastRng;

fn lerp_u8(a: u8, b: u8, num: usize, den: usize) -> u8 {
    if den == 0 {
        return a;
    }
    let av = a as usize;
    let bv = b as usize;
    let v = (av * (den - num) + bv * num) / den;
    v as u8
}

pub fn make_soft_sky_palette_256() -> Vec<[u8; 3]> {
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

pub fn make_gradient_buffer16(width: usize, height: usize) -> Vec<u16> {
    let mut out = vec![0u16; width * height];
    let max_sum = (width - 1) + (height - 1);
    for y in 0..height {
        let y_from_bottom = (height - 1) - y;
        for x in 0..width {
            let sum = x + y_from_bottom;
            out[y * width + x] = ((sum * 65535) / max_sum) as u16;
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
pub enum DebandingDistribution {
    Linear,
    Gaussian,
}

pub trait PixelFilter {
    fn name(&self) -> &'static str;
    fn apply(&self, x: usize, y: usize, width: usize, height: usize, value: u16) -> u16;
}

#[derive(Clone, Copy, Debug)]
pub struct DebandingFilter {
    seed: u64,
    shift: i8,
    distribution: DebandingDistribution,
}

impl DebandingFilter {
    pub fn new(seed: u64, shift: i8, distribution: DebandingDistribution) -> Self {
        Self {
            seed,
            shift: shift.clamp(-7, 7),
            distribution,
        }
    }

    pub fn linear(seed: u64, shift: i8) -> Self {
        Self::new(seed, shift, DebandingDistribution::Linear)
    }

    pub fn gaussian(seed: u64, shift: i8) -> Self {
        Self::new(seed, shift, DebandingDistribution::Gaussian)
    }
}

impl PixelFilter for DebandingFilter {
    fn name(&self) -> &'static str {
        "debanding_filter"
    }

    fn apply(&self, x: usize, y: usize, width: usize, _height: usize, value: u16) -> u16 {
        let n = pixel_noise_u8(self.seed, x, y, width);
        let n = match self.distribution {
            DebandingDistribution::Linear => n,
            DebandingDistribution::Gaussian => {
                // Uniform byte -> inverse-CDF LUT -> Gaussian-like byte.
                FastRng::gaussian8_table()[n as usize]
            }
        };
        value.saturating_add(shift_noise_u8(n, self.shift))
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

#[inline]
fn pixel_noise_u8(seed: u64, x: usize, y: usize, width: usize) -> u8 {
    // Deterministic per-pixel hash so all call paths (single pixel, full frame,
    // screenshot, texture upload) produce identical filtered output.
    let idx = (y * width + x) as u64;
    let mut z = idx
        .wrapping_add(seed)
        .wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z & 0xFF) as u8
}

pub struct PixelBuffer {
    width: usize,
    height: usize,
    palette: Vec<[u8; 3]>,
    base16: Vec<u16>,
    detail16: Vec<u16>,
    argb: Vec<u8>,
    dirty: bool,
    debug: bool,
    filters: Vec<Box<dyn PixelFilter>>,
}

impl PixelBuffer {
    pub fn new(width: usize, height: usize, palette: Vec<[u8; 3]>) -> Self {
        Self::new_with_debug(width, height, palette, false)
    }

    pub fn new_with_debug(width: usize, height: usize, palette: Vec<[u8; 3]>, debug: bool) -> Self {
        let len = width * height;
        let me = Self {
            width,
            height,
            palette,
            base16: vec![0; len],
            detail16: vec![0; len],
            argb: vec![0; len * 4],
            dirty: true,
            debug,
            filters: Vec::new(),
        };
        if me.debug {
            println!(
                "[pixel_buffer:init] {}x{} pixels={} palette={} base16={} detail16={} argb_bytes={}",
                me.width,
                me.height,
                len,
                me.palette.len(),
                me.base16.len(),
                me.detail16.len(),
                me.argb.len()
            );
        }
        me
    }

    pub fn set_base(&mut self, base16: Vec<u16>) {
        assert_eq!(base16.len(), self.width * self.height);
        self.base16 = base16;
        self.dirty = true;
    }

    pub fn set_detail(&mut self, detail16: Vec<u16>) {
        assert_eq!(detail16.len(), self.width * self.height);
        self.detail16 = detail16;
        self.dirty = true;
    }

    pub fn clear_detail(&mut self) {
        self.detail16.fill(0);
        self.dirty = true;
    }

    pub fn set_tiled_detail(&mut self, tile16: &[u16], tile_w: usize, tile_h: usize) {
        assert_eq!(tile16.len(), tile_w * tile_h);
        for y in 0..self.height {
            let ty = y % tile_h;
            for x in 0..self.width {
                let tx = x % tile_w;
                self.detail16[y * self.width + x] = tile16[ty * tile_w + tx];
            }
        }
        self.dirty = true;
    }

    pub fn add_filter(&mut self, filter: Box<dyn PixelFilter>) {
        if self.debug {
            println!("[pixel_buffer:filter:add] {}", filter.name());
        }
        self.filters.push(filter);
        self.dirty = true;
    }

    pub fn clear_filters(&mut self) {
        self.filters.clear();
        if self.debug {
            println!("[pixel_buffer:filter:clear]");
        }
        self.dirty = true;
    }

    #[inline]
    pub fn composed_u16(&self, x: usize, y: usize) -> u16 {
        let idx = y * self.width + x;
        let mut out = self.base16[idx].saturating_add(self.detail16[idx]);
        for filter in &self.filters {
            out = filter.apply(x, y, self.width, self.height, out);
        }
        out
    }

    #[inline]
    pub fn pixel_rgb(&self, x: usize, y: usize) -> [u8; 3] {
        let c = (self.composed_u16(x, y) >> 8) as u8;
        self.palette[c as usize]
    }

    fn rebuild_argb_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        for y in 0..self.height {
            for x in 0..self.width {
                let off = (y * self.width + x) * 4;
                let [r, g, b] = self.pixel_rgb(x, y);
                self.argb[off] = b;
                self.argb[off + 1] = g;
                self.argb[off + 2] = r;
                self.argb[off + 3] = 0xFF;
            }
        }
        self.dirty = false;
    }

    pub fn argb_buffer(&mut self) -> &[u8] {
        self.rebuild_argb_if_dirty();
        &self.argb
    }

    pub fn upload_to_texture(&mut self, texture: &mut Texture<'_>) -> io::Result<()> {
        self.rebuild_argb_if_dirty();
        let src = &self.argb;
        let src_pitch = self.width * 4;
        texture
            .with_lock(None, |dst: &mut [u8], dst_pitch: usize| {
                for y in 0..self.height {
                    let s0 = y * src_pitch;
                    let s1 = s0 + src_pitch;
                    let d0 = y * dst_pitch;
                    let d1 = d0 + src_pitch;
                    dst[d0..d1].copy_from_slice(&src[s0..s1]);
                }
            })
            .map_err(|e| io::Error::other(e.to_string()))
    }

    pub fn write_ppm<P: AsRef<Path>>(&mut self, path: P) -> io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        writeln!(file, "P6")?;
        writeln!(file, "{} {}", self.width, self.height)?;
        writeln!(file, "255")?;
        for y in 0..self.height {
            for x in 0..self.width {
                let [r, g, b] = self.pixel_rgb(x, y);
                file.write_all(&[r, g, b])?;
            }
        }
        Ok(())
    }
}
