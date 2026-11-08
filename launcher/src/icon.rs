// The app icon, drawn here (no game art): an 80s sunset over the sea with a palm silhouette, rounded square.
// Shared by build.rs (the .ico inside the exe) and the window.

#[allow(dead_code)]
pub fn draw(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut px = vec![0u8; (size * size * 4) as usize];
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t.clamp(0.0, 1.0);
    // distance from p to segment a-b
    let seg = |p: (f32, f32), a: (f32, f32), b: (f32, f32)| {
        let (vx, vy, wx, wy) = (b.0 - a.0, b.1 - a.1, p.0 - a.0, p.1 - a.1);
        let t = ((wx * vx + wy * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
        ((wx - t * vx).powi(2) + (wy - t * vy).powi(2)).sqrt()
    };
    // palm: a leaning trunk and five fronds (unit square coordinates)
    let trunk = [(0.30, 0.95), (0.27, 0.75), (0.29, 0.55), (0.34, 0.38)];
    let top = (0.34, 0.38);
    let fronds = [(0.10, 0.42), (0.16, 0.30), (0.40, 0.24), (0.58, 0.32), (0.60, 0.46)];
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = ((x as f32 + 0.5) / s, (y as f32 + 0.5) / s);
            let r = 0.2;
            let (dx, dy) = ((fx - 0.5).abs() - (0.5 - r), (fy - 0.5).abs() - (0.5 - r));
            let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() - r;
            let alpha = (0.5 - outside * s).clamp(0.0, 1.0);
            let horizon = 0.66;
            // sky: purple to pink to orange towards the horizon
            let mut c = if fy < horizon {
                let t = fy / horizon;
                [lerp(40.0, 250.0, t * t), lerp(20.0, 90.0, t * t), lerp(90.0, 120.0, t)]
            } else {
                // sea: deep teal with light lines
                let line = ((fy - horizon) * s / 2.5) as i32 % 2 == 0;
                if line { [30.0, 110.0, 150.0] } else { [12.0, 40.0, 80.0] }
            };
            // sun above the horizon, striped near the bottom
            let (sx, sy) = (fx - 0.62, fy - horizon);
            let d = (sx * sx + sy * sy).sqrt();
            let stripe = fy > horizon - 0.12 && ((((horizon - fy) * s) / (s / 22.0)) as i32 % 2 == 1);
            if fy < horizon && d < 0.27 && !stripe {
                let t = (fy - (horizon - 0.27)) / 0.27;
                c = [255.0, lerp(225.0, 70.0, t), lerp(90.0, 150.0, t)];
            }
            // palm silhouette (near black)
            let w = 1.6 / s;
            let mut ink = trunk.windows(2).map(|p| seg((fx, fy), p[0], p[1])).fold(f32::MAX, f32::min) - (0.028 + w);
            for f in fronds {
                let mid = ((top.0 + f.0) / 2.0, (top.1 + f.1) / 2.0 - 0.04);
                let dist = seg((fx, fy), top, mid).min(seg((fx, fy), mid, f));
                ink = ink.min(dist - (0.022 + w));
            }
            if ink < 0.0 {
                let a = (-ink * s).clamp(0.0, 1.0);
                c = [lerp(c[0], 18.0, a), lerp(c[1], 10.0, a), lerp(c[2], 30.0, a)];
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
