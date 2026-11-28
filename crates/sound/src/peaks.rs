//! The waveform the timeline draws: the loudest point of each frame (0–1), as the current
//! app's `peaks`.

use crate::{Pcm, RATE};

/// The loudest sample of each frame at `fps`, both channels (0–1).
pub fn peaks(p: &Pcm, fps: u32) -> Vec<f32> {
    let per = RATE as f64 / fps.max(1) as f64;
    let n = (p.len() as f64 / per).ceil() as usize;
    let mut out = vec![0f32; n];
    let chans: Vec<&[f32]> = match &p.right {
        Some(r) => vec![&p.left, r],
        None => vec![&p.left],
    };
    for d in chans {
        for (f, o) in out.iter_mut().enumerate() {
            let a = (f as f64 * per).floor() as usize;
            let b = (((f + 1) as f64 * per).floor() as usize).min(d.len());
            for v in &d[a.min(b)..b] {
                let v = v.abs();
                if v > *o {
                    *o = v;
                }
            }
        }
    }
    for o in &mut out {
        *o = o.min(1.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_frame_its_loudest_point() {
        let mut left = vec![0f32; 4500];
        left[10] = -0.75;
        left[2100] = 0.25;
        let p = Pcm {
            left,
            right: Some({
                let mut r = vec![0f32; 4500];
                r[2200] = -0.5;
                r
            }),
        };
        // 24 fps: 2000 samples a frame, three frames (the last one partial)
        assert_eq!(peaks(&p, 24), vec![0.75, 0.5, 0.0]);
    }
}
