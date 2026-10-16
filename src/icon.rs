//! The app icon (headphones on an indigo disc), rasterised at runtime so the
//! window and tray icons need no image decoder.

/// Returns `size`×`size` straight RGBA pixels.
pub fn rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let accent = [0x4f, 0x46, 0xe5];
    let mut px = vec![0u8; (size * size * 4) as usize];
    const SUB: u32 = 4;

    let disc = |x: f32, y: f32| {
        let r = s * 0.44;
        (x - s / 2.0).hypot(y - s / 2.0) <= r
    };
    let band = |x: f32, y: f32| {
        // Upper half of an ellipse outline.
        let (cx, cy) = (s * 0.5, s * 0.47);
        let (rx, ry) = (s * 0.26, s * 0.27);
        let w = s * 0.10;
        if y > cy {
            return false;
        }
        let d = ((x - cx) / rx).hypot((y - cy) / ry);
        let outer = ((x - cx) / (rx + w / 2.0)).hypot((y - cy) / (ry + w / 2.0));
        outer <= 1.0 && d >= 1.0 - w / 2.0 / rx.min(ry)
    };
    let cup = |x: f32, y: f32, x0: f32| {
        let (w, h) = (s * 0.26, s * 0.34);
        let y0 = s * 0.46;
        let r = w * 0.45;
        let qx = (x - (x0 + w / 2.0)).abs() - (w / 2.0 - r);
        let qy = (y - (y0 + h / 2.0)).abs() - (h / 2.0 - r);
        qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) <= r
    };
    let white =
        |x: f32, y: f32| band(x, y) || cup(x, y, s * 0.15) || cup(x, y, s * 0.85 - s * 0.26);

    for py in 0..size {
        for pxi in 0..size {
            let (mut cover, mut lit) = (0u32, 0u32);
            for sy in 0..SUB {
                for sx in 0..SUB {
                    let x = pxi as f32 + (sx as f32 + 0.5) / SUB as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SUB as f32;
                    if disc(x, y) {
                        cover += 1;
                        if white(x, y) {
                            lit += 1;
                        }
                    }
                }
            }
            if cover == 0 {
                continue;
            }
            let t = lit as f32 / cover as f32;
            let i = ((py * size + pxi) * 4) as usize;
            for c in 0..3 {
                px[i + c] = (accent[c] as f32 * (1.0 - t) + 255.0 * t).round() as u8;
            }
            px[i + 3] = (255 * cover / (SUB * SUB)) as u8;
        }
    }
    px
}
