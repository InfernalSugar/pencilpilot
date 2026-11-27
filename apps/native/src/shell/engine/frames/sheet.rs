//! The exposure sheet: each layer's cels (a drawing held for some frames, with its mark),
//! as the interface shows it and the engine plays it. Plain data and the edits on it; the
//! engine ([`super`]) keeps the painter's layers and drawings in step.
//!
//! A track's cels follow each other from frame 1 with no gaps: an empty exposure is a cel
//! with drawing 0 (a blank frame). Frames after a track's last cel show nothing on it. The
//! cut is as long as its longest cel layer; a static layer is one cel all cut long.

use std::collections::BTreeMap;

use crate::state::{ColourStatus, State};

/// How a drawing is marked in the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mark {
    #[default]
    None,
    Key,
    Breakdown,
    InBetween,
}

/// A drawing exposed for `hold` frames (drawing 0: a blank frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cel {
    pub drawing: u32,
    pub hold: u32,
    pub mark: Mark,
}

impl Cel {
    pub const fn new(drawing: u32, hold: u32, mark: Mark) -> Cel {
        Cel {
            drawing,
            hold,
            mark,
        }
    }
}

/// One layer's exposures.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// the painter's layer (0 until the engine has made it)
    pub layer: u32,
    /// 'C': a drawing per exposure; 'S': one drawing all cut long
    pub kind: char,
    /// 0–1, as the panel's slider sets it
    pub opacity: f32,
    pub cels: Vec<Cel>,
}

impl Track {
    pub fn new(layer: u32, kind: char) -> Track {
        Track {
            layer,
            kind,
            opacity: 1.0,
            cels: Vec::new(),
        }
    }

    /// Where the last cel ends (a frame count).
    pub fn end(&self) -> u32 {
        self.cels.iter().map(|c| c.hold).sum()
    }

    /// Each cel with its first frame (0-based).
    pub fn spans(&self) -> Vec<(u32, Cel)> {
        let mut at = 0;
        self.cels
            .iter()
            .map(|c| {
                let s = (at, *c);
                at += c.hold;
                s
            })
            .collect()
    }

    pub fn start_of(&self, i: usize) -> u32 {
        self.cels[..i.min(self.cels.len())]
            .iter()
            .map(|c| c.hold)
            .sum()
    }

    /// The cel exposed on `frame` and its first frame.
    pub fn at(&self, frame: u32) -> Option<(usize, u32)> {
        let mut at = 0;
        for (i, c) in self.cels.iter().enumerate() {
            if frame < at + c.hold {
                return Some((i, at));
            }
            at += c.hold;
        }
        None
    }

    /// The drawing exposed on `frame` (0: none).
    pub fn drawing_at(&self, frame: u32) -> u32 {
        self.at(frame).map_or(0, |(i, _)| self.cels[i].drawing)
    }

    /// The drawings in the order they first show (no blanks, each once).
    pub fn drawings(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        for c in &self.cels {
            if c.drawing != 0 && !v.contains(&c.drawing) {
                v.push(c.drawing);
            }
        }
        v
    }

    /// The first frame a drawing shows on.
    pub fn first_frame_of(&self, drawing: u32) -> Option<u32> {
        self.spans()
            .into_iter()
            .find(|(_, c)| c.drawing == drawing)
            .map(|(a, _)| a)
    }

    /// A blank cel so the track reaches `frame`.
    fn pad_to(&mut self, frame: u32) {
        let end = self.end();
        if frame > end {
            match self.cels.last_mut() {
                Some(c) if c.drawing == 0 => c.hold += frame - end,
                _ => self.cels.push(Cel::new(0, frame - end, Mark::None)),
            }
        }
    }

    /// Put `cel` in after the exposure on `frame`, the later ones moving along; on a blank
    /// frame it goes on that frame, into the blank (the later cels keep their frames while
    /// the blank lasts). Returns its first frame.
    pub fn insert_after(&mut self, frame: u32, cel: Cel) -> u32 {
        let cel = Cel {
            hold: cel.hold.max(1),
            ..cel
        };
        match self.at(frame) {
            Some((i, start)) if self.cels[i].drawing == 0 => {
                let blank = self.cels[i];
                let before = frame - start;
                let hold = cel.hold.min(blank.hold - before);
                let after = blank.hold - before - hold;
                let mut put = Vec::new();
                if before > 0 {
                    put.push(Cel::new(0, before, Mark::None));
                }
                put.push(Cel { hold, ..cel });
                if after > 0 {
                    put.push(Cel::new(0, after, Mark::None));
                }
                self.cels.splice(i..=i, put);
                frame
            }
            Some((i, start)) => {
                self.cels.insert(i + 1, cel);
                start + self.cels[i].hold
            }
            None => {
                self.pad_to(frame);
                self.cels.push(cel);
                frame
            }
        }
    }

    /// The exposure on `frame` held `hold` frames (the later ones move). False if there is
    /// none there.
    pub fn set_hold(&mut self, frame: u32, hold: u32) -> bool {
        let Some((i, _)) = self.at(frame) else {
            return false;
        };
        self.cels[i].hold = hold.max(1);
        true
    }

    /// One more frame on `frame`: its exposure is held one longer; past the end, blank
    /// frames up to it.
    pub fn insert_frame(&mut self, frame: u32) {
        match self.at(frame) {
            Some((i, _)) => self.cels[i].hold += 1,
            None => self.pad_to(frame + 1),
        }
    }

    /// One frame less on `frame`: its exposure is held one shorter (gone at 0).
    pub fn remove_frame(&mut self, frame: u32) -> bool {
        let Some((i, _)) = self.at(frame) else {
            return false;
        };
        if self.cels[i].hold > 1 {
            self.cels[i].hold -= 1;
        } else {
            self.cels.remove(i);
        }
        true
    }

    /// The exposure on `frame` held over the blank after it, or to the cut's end when it's
    /// the last; else one frame longer.
    pub fn extend_hold(&mut self, frame: u32, cut: u32) -> bool {
        let Some((i, start)) = self.at(frame) else {
            return false;
        };
        if i + 1 < self.cels.len() && self.cels[i + 1].drawing == 0 {
            let blank = self.cels.remove(i + 1);
            self.cels[i].hold += blank.hold;
        } else if i + 1 == self.cels.len() && start + self.cels[i].hold < cut {
            self.cels[i].hold = cut - start;
        } else {
            self.cels[i].hold += 1;
        }
        true
    }

    /// The exposures in the opposite order.
    pub fn reverse(&mut self) {
        self.cels.reverse();
    }

    /// Every exposure held `n` frames.
    pub fn retime(&mut self, n: u32) {
        for c in &mut self.cels {
            c.hold = n.max(1);
        }
    }

    pub fn set_mark(&mut self, frame: u32, mark: Mark) -> bool {
        let Some((i, _)) = self.at(frame) else {
            return false;
        };
        self.cels[i].mark = mark;
        true
    }

    /// One drawing all cut long (a static layer): the one on `frame`, else the first.
    pub fn make_static(&mut self, frame: u32, cut: u32) {
        let d = match self.drawing_at(frame) {
            0 => self.drawings().first().copied().unwrap_or(0),
            d => d,
        };
        let mark = self
            .cels
            .iter()
            .find(|c| c.drawing == d)
            .map_or(Mark::None, |c| c.mark);
        self.cels = vec![Cel::new(d, cut.max(1), mark)];
    }
}

/// A drawing's small picture for the strips (straight RGBA8, `w` × `h`).
#[derive(Clone, PartialEq)]
pub struct Thumb {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl std::fmt::Debug for Thumb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Thumb({}×{})", self.w, self.h)
    }
}

/// The cut's exposure sheet as the interface shows it (in [`State::sheet`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sheet {
    /// a track per layer, in the order of [`State::layers`] (top first)
    pub tracks: Vec<Track>,
    /// each drawing's state in the Colour workspace
    pub status: BTreeMap<u32, ColourStatus>,
    /// each drawing's small picture (the engine keeps them up to date)
    pub thumbs: BTreeMap<u32, Thumb>,
    /// playback's loop range (first and last frame, 0-based), if one is set
    pub loop_range: Option<(u32, u32)>,
    /// the demo's drawings are painted when the engine starts
    pub demo: bool,
}

impl Sheet {
    /// The design's example cut: Clean, Tie-down, Rough and a static Background (drawings
    /// numbered across the cut, the painter's ids).
    pub fn demo() -> Sheet {
        use Mark::{Breakdown as B, Key as K, None as N};
        let track = |kind: char, cels: &[(u32, u32, Mark)]| Track {
            layer: 0,
            kind,
            opacity: 1.0,
            cels: cels.iter().map(|&(d, h, m)| Cel::new(d, h, m)).collect(),
        };
        let status = [
            ColourStatus::Done,
            ColourStatus::Done,
            ColourStatus::Done,
            ColourStatus::Flatted,
            ColourStatus::Flatted,
        ];
        Sheet {
            tracks: vec![
                track(
                    'C',
                    &[
                        (1, 6, K),
                        (2, 6, N),
                        (3, 6, B),
                        (4, 6, K),
                        (5, 8, N),
                        (6, 16, K),
                    ],
                ),
                track('C', &[(7, 12, K), (8, 12, N), (9, 24, K)]),
                track('C', &[(10, 12, N), (11, 12, N), (12, 12, N), (13, 12, N)]),
                track('S', &[(14, 48, N)]),
            ],
            status: (1..).zip(status).collect(),
            thumbs: BTreeMap::new(),
            loop_range: None,
            demo: true,
        }
    }

    pub fn max_drawing(&self) -> u32 {
        self.tracks
            .iter()
            .flat_map(|t| t.cels.iter().map(|c| c.drawing))
            .max()
            .unwrap_or(0)
    }
}

/// What the interface reads from the sheet, made to follow it: a track per layer, each
/// layer's kind, the cut's length, static layers all cut long, the current frame inside the
/// cut, and the active layer's drawings (`drawings`, `drawing`, `to_colour`).
pub fn derive(s: &mut State) {
    let n = s.layers.len();
    while s.sheet.tracks.len() < n {
        let kind = s.layers[s.sheet.tracks.len()].kind;
        s.sheet.tracks.push(Track::new(0, kind));
    }
    s.sheet.tracks.truncate(n);
    // the cut: its longest cel layer (a cut with none keeps its length)
    for (l, t) in s.layers.iter().zip(s.sheet.tracks.iter_mut()) {
        if l.kind != t.kind {
            if l.kind == 'S' {
                t.make_static(s.frame, s.frames);
            }
            t.kind = l.kind;
        }
    }
    let longest = s
        .sheet
        .tracks
        .iter()
        .filter(|t| t.kind != 'S')
        .map(Track::end)
        .max()
        .unwrap_or(0);
    if longest > 0 {
        s.frames = longest;
    }
    s.frames = s.frames.max(1);
    for t in s.sheet.tracks.iter_mut().filter(|t| t.kind == 'S') {
        if t.cels.len() != 1 {
            t.make_static(s.frame, s.frames);
        }
        t.cels[0].hold = s.frames;
    }
    s.frame = s.frame.min(s.frames - 1);
    s.active_layer = s.active_layer.min(n.saturating_sub(1));
    let Some(t) = s.sheet.tracks.get(s.active_layer) else {
        s.drawings = 0;
        s.drawing = 0;
        s.to_colour.clear();
        return;
    };
    let ds = t.drawings();
    // the drawing on this frame, or the last one before it (a blank frame)
    let here = t.drawing_at(s.frame);
    let shown = if here != 0 {
        here
    } else {
        t.spans()
            .into_iter()
            .filter(|(a, c)| *a <= s.frame && c.drawing != 0)
            .map(|(_, c)| c.drawing)
            .next_back()
            .unwrap_or(0)
    };
    s.drawings = ds.len() as u32;
    s.drawing = ds.iter().position(|d| *d == shown).unwrap_or(0) as u32;
    s.to_colour = ds
        .iter()
        .map(|d| s.sheet.status.get(d).copied().unwrap_or(ColourStatus::ToDo))
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(cels: &[(u32, u32)]) -> Track {
        Track {
            layer: 1,
            kind: 'C',
            opacity: 1.0,
            cels: cels
                .iter()
                .map(|&(d, h)| Cel::new(d, h, Mark::None))
                .collect(),
        }
    }
    fn holds(t: &Track) -> Vec<(u32, u32)> {
        t.cels.iter().map(|c| (c.drawing, c.hold)).collect()
    }

    #[test]
    fn cels_follow_each_other_and_edits_move_the_later_ones() {
        let mut a = t(&[(1, 6), (2, 6), (3, 6)]);
        assert_eq!(a.end(), 18);
        assert_eq!(a.at(7), Some((1, 6)));
        assert_eq!(a.drawing_at(17), 3);
        assert_eq!(a.drawing_at(18), 0, "past the end: nothing");
        // a new drawing after the exposure on frame 8 (drawing 2): it starts at 12
        assert_eq!(a.insert_after(8, Cel::new(4, 2, Mark::None)), 12);
        assert_eq!(holds(&a), [(1, 6), (2, 6), (4, 2), (3, 6)]);
        // holds
        assert!(a.set_hold(0, 3));
        a.insert_frame(3);
        assert!(a.remove_frame(4));
        assert!(a.remove_frame(9)); // drawing 4: 2 → 1
        assert_eq!(holds(&a), [(1, 3), (2, 6), (4, 1), (3, 6)]);
        assert!(a.remove_frame(9), "held 1: the cel goes");
        assert_eq!(holds(&a), [(1, 3), (2, 6), (3, 6)]);
        a.reverse();
        assert_eq!(holds(&a), [(3, 6), (2, 6), (1, 3)]);
        a.retime(2);
        assert_eq!(a.end(), 6);
    }

    #[test]
    fn blanks_fill_in_and_extend() {
        let mut a = t(&[(1, 2), (0, 6), (2, 2)]);
        // into the blank on frame 4: from frame 4, the later cel stays at frame 8
        assert_eq!(a.insert_after(4, Cel::new(5, 2, Mark::None)), 4);
        assert_eq!(holds(&a), [(1, 2), (0, 2), (5, 2), (0, 2), (2, 2)]);
        assert_eq!(a.first_frame_of(2), Some(8));
        // extend hold: over the blank after it
        assert!(a.extend_hold(4, 20));
        assert_eq!(holds(&a), [(1, 2), (0, 2), (5, 4), (2, 2)]);
        // the last one: to the cut's end
        assert!(a.extend_hold(9, 20));
        assert_eq!(a.end(), 20);
        // past the end: padded with a blank
        let mut b = t(&[(1, 2)]);
        assert_eq!(b.insert_after(5, Cel::new(2, 1, Mark::None)), 5);
        assert_eq!(holds(&b), [(1, 2), (0, 3), (2, 1)]);
        assert_eq!(b.drawings(), [1, 2]);
    }

    #[test]
    fn derive_follows_the_sheet() {
        let mut s = State::demo();
        derive(&mut s);
        assert_eq!(s.frames, 48);
        assert_eq!(s.drawings, 6, "Clean's six drawings");
        assert_eq!(s.drawing, 2, "frame 15 is Clean's drawing 3");
        assert_eq!(s.to_colour.len(), 6);
        // a longer Clean makes the cut longer, and the static Background follows
        s.sheet.tracks[0].set_hold(0, 10);
        derive(&mut s);
        assert_eq!(s.frames, 52);
        assert_eq!(s.sheet.tracks[3].cels[0].hold, 52);
        // static: one drawing
        s.layers[1].kind = 'S';
        derive(&mut s);
        assert_eq!(s.sheet.tracks[1].cels.len(), 1);
        assert_eq!(s.sheet.tracks[1].cels[0].drawing, 8, "the one on frame 15");
    }
}
