//! PencilPilot grading engine.
//!
//! Pure maths, no I/O: takes the student's drawing and the reference as
//! grayscale bitmaps and returns measurements and a score. Methods are
//! added in milestone M3 (see docs/planning/MILESTONES.md).

use serde::{Deserialize, Serialize};

/// A grayscale image, row-major, one byte per pixel (0 = black, 255 = white).
#[derive(Debug, Clone)]
pub struct Gray {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Gray {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, GradeError> {
        if pixels.len() != (width as usize) * (height as usize) {
            return Err(GradeError::SizeMismatch);
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GradeError {
    SizeMismatch,
    DifferentDimensions,
}

/// Result of grading one drawing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grade {
    /// Final score, 1.0–10.0.
    pub score: f32,
    /// Raw per-method similarity values in 0.0–1.0.
    pub measurements: Vec<(String, f32)>,
}

/// Placeholder grader (M0): compares ink coverage only. Replaced in M3.
pub fn grade(drawing: &Gray, reference: &Gray) -> Result<Grade, GradeError> {
    if drawing.width != reference.width || drawing.height != reference.height {
        return Err(GradeError::DifferentDimensions);
    }
    let ink = |g: &Gray| {
        g.pixels.iter().filter(|&&p| p < 128).count() as f32 / g.pixels.len().max(1) as f32
    };
    let (a, b) = (ink(drawing), ink(reference));
    let similarity = if a.max(b) == 0.0 {
        1.0
    } else {
        a.min(b) / a.max(b)
    };
    Ok(Grade {
        score: 1.0 + 9.0 * similarity,
        measurements: vec![("ink_coverage".into(), similarity)],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(fill: u8) -> Gray {
        Gray::new(4, 4, vec![fill; 16]).unwrap()
    }

    #[test]
    fn identical_images_score_ten() {
        let g = grade(&img(0), &img(0)).unwrap();
        assert!((g.score - 10.0).abs() < 1e-5);
    }

    #[test]
    fn blank_vs_full_scores_one() {
        let g = grade(&img(255), &img(0)).unwrap();
        assert!((g.score - 1.0).abs() < 1e-5);
    }

    #[test]
    fn rejects_bad_sizes() {
        assert_eq!(
            Gray::new(2, 2, vec![0; 3]).unwrap_err(),
            GradeError::SizeMismatch
        );
        let small = Gray::new(2, 2, vec![0; 4]).unwrap();
        assert_eq!(
            grade(&small, &img(0)).unwrap_err(),
            GradeError::DifferentDimensions
        );
    }
}
