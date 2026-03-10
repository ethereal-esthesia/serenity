#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Palette256 {
    SoftSky,
}

#[inline]
fn lerp_u8(a: u8, b: u8, num: usize, den: usize) -> u8 {
    if den == 0 {
        return a;
    }
    let av = a as usize;
    let bv = b as usize;
    let v = (av * (den - num) + bv * num) / den;
    v as u8
}

fn make_soft_sky_palette_256() -> Vec<[u8; 3]> {
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

pub fn palette_256(kind: Palette256) -> Vec<[u8; 3]> {
    match kind {
        Palette256::SoftSky => make_soft_sky_palette_256(),
    }
}
