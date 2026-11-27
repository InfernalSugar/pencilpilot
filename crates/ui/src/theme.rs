//! The design's tokens (the website's and the design canvas's Bento look): colours, corners
//! and spacing, in logical pixels (the interface scales them).

/// A colour from 0xRRGGBB, opaque.
pub const fn hex(v: u32) -> [f32; 4] {
    [
        ((v >> 16) & 0xff) as f32 / 255.0,
        ((v >> 8) & 0xff) as f32 / 255.0,
        (v & 0xff) as f32 / 255.0,
        1.0,
    ]
}

/// The same colour at `a` opacity.
pub const fn alpha(c: [f32; 4], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// the page behind everything
    pub paper: [f32; 4],
    pub ink: [f32; 4],
    pub card: [f32; 4],
    pub coral: [f32; 4],
    /// on coral
    pub on_coral: [f32; 4],
    /// quiet fills and lines on paper
    pub line: [f32; 4],
    pub muted: [f32; 4],
    /// the timeline's dark card, its raised controls and lines
    pub dark: [f32; 4],
    pub dark_raised: [f32; 4],
    pub dark_line: [f32; 4],
    pub on_dark: [f32; 4],
    pub on_dark_muted: [f32; 4],
    /// keys, onion skin on
    pub highlight: [f32; 4],
    pub card_radius: f32,
    pub control_radius: f32,
    pub control_height: f32,
    pub gap: f32,
    pub pad: f32,
    pub text: f32,
    pub small: f32,
    pub heading: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            paper: hex(0xF6F4EF),
            ink: hex(0x16161A),
            card: hex(0xFFFFFF),
            coral: hex(0xE8492F),
            on_coral: hex(0xFFFFFF),
            line: hex(0xE4E1D8),
            muted: hex(0x55545A),
            dark: hex(0x16161A),
            dark_raised: hex(0x2A2A30),
            dark_line: hex(0x2B2B32),
            on_dark: hex(0xF6F4EF),
            on_dark_muted: hex(0xA9A8AE),
            highlight: hex(0xFFD84D),
            card_radius: 28.0,
            control_radius: 12.0,
            control_height: 44.0,
            gap: 8.0,
            pad: 20.0,
            text: 14.0,
            small: 12.0,
            heading: 28.0,
        }
    }
}
