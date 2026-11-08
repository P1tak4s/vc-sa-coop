// The app icon: a retro sunset sun (yellow to magenta, the lower half cut by stripes) on deep navy, rounded square.
// Shared by build.rs (the .ico in the exe) and main.rs (the window icon).

#[allow(dead_code)]
pub fn draw(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut px = vec![0u8; (size * size * 4) as usize];
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = ((x as f32 + 0.5) / s, (y as f32 + 0.5) / s);
            // rounded square mask
            let r = 0.22;
            let (dx, dy) = ((fx - 0.5).abs() - (0.5 - r), (fy - 0.5).abs() - (0.5 - r));
            let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() - r;
            let alpha = (0.5 - outside * s).clamp(0.0, 1.0);
            // night sky: navy to purple
            let mut c = [lerp(14.0, 60.0, fy), lerp(18.0, 20.0, fy), lerp(48.0, 90.0, fy)];
            // the sun
            let (sx, sy, sr) = (fx - 0.5, fy - 0.56, 0.32);
            let d = (sx * sx + sy * sy).sqrt();
            let band = ((fy - 0.56) / 0.32 * 6.0).floor() as i32; // stripes below the middle
            let striped = fy > 0.56 && (((fy - 0.56) * s / (s / 16.0)) as i32 % 2 == 1) && band >= 0;
            if d < sr && !striped {
                let t = ((fy - 0.24) / 0.64).clamp(0.0, 1.0);
                c = [lerp(255.0, 255.0, t), lerp(214.0, 46.0, t), lerp(70.0, 150.0, t)];
            }
            let i = ((y * size + x) * 4) as usize;
            px[i] = c[0] as u8;
            px[i + 1] = c[1] as u8;
            px[i + 2] = c[2] as u8;
            px[i + 3] = (alpha * 255.0) as u8;
        }
    }
    px
}
