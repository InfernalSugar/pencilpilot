//! Input as the app sees it: pointer, keyboard and pen events in logical pixels, each from
//! one moment ("frame") of the compositor's.
//!
//! Wayland sends a moment piece by piece (a pen's position, then its pressure, then its
//! tilt…) and ends it with a frame event. The gatherers here put the pieces together. They
//! don't touch Wayland, so recorded sequences can test them.

use std::collections::HashMap;

/// A pen's tool: the tip, the eraser end, or another kind of stylus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PenTool {
    Pen,
    Eraser,
    Brush,
    Pencil,
    Airbrush,
    Finger,
    Mouse,
    Lens,
}

/// Where a pen is and how it's held.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PenSample {
    /// on the window, logical pixels
    pub x: f64,
    pub y: f64,
    /// 0 (not touching) to 1 (as hard as the pen reads)
    pub pressure: f32,
    /// degrees from upright, towards +x and +y
    pub tilt_x: f32,
    pub tilt_y: f32,
    /// degrees, clockwise (pens that read it)
    pub rotation: f32,
    /// 0 (touching) to 1 (as far as the tablet reads), while hovering
    pub distance: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenPhase {
    /// came close enough to be read, over the window
    Enter,
    /// moving, not touching
    Hover,
    /// touched down
    Down,
    /// moving while touching
    Move,
    /// lifted
    Up,
    /// went out of range, or off the window
    Leave,
}

/// A pen event: one moment of one tool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenEvent {
    pub phase: PenPhase,
    pub tool: PenTool,
    /// the tool's own serial number, where the tablet reports one (tells two pens apart)
    pub tool_serial: u64,
    pub sample: PenSample,
    /// side buttons held (BTN_STYLUS = 1, BTN_STYLUS2 = 2, BTN_STYLUS3 = 4)
    pub buttons: u32,
    /// the compositor's timestamp, ms
    pub time_ms: u32,
}

pub const BTN_STYLUS: u32 = 0x14b;
pub const BTN_STYLUS2: u32 = 0x14c;
pub const BTN_STYLUS3: u32 = 0x149;

fn stylus_bit(button: u32) -> u32 {
    match button {
        BTN_STYLUS => 1,
        BTN_STYLUS2 => 2,
        BTN_STYLUS3 => 4,
        _ => 0,
    }
}

/// One piece of a pen moment, as the compositor sends it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PenPiece {
    Type(PenTool),
    Serial(u64),
    ProximityIn,
    ProximityOut,
    Down,
    Up,
    Motion(f64, f64),
    /// 0–65535
    Pressure(u32),
    /// 0–65535
    Distance(u32),
    Tilt(f64, f64),
    Rotation(f64),
    Button(u32, bool),
    Frame(u32),
}

/// Gathers one tool's pieces into events. The axes keep their last values between
/// moments: the compositor only sends what changed.
#[derive(Debug, Default)]
pub struct PenGatherer {
    tool: Option<PenTool>,
    serial: u64,
    now: PenSample,
    touching: bool,
    near: bool,
    buttons: u32,
    // this moment's
    came: bool,
    went: bool,
    down: bool,
    up: bool,
    moved: bool,
}

impl PenGatherer {
    /// Take a piece; a frame ends the moment and returns its events (oldest first).
    pub fn take(&mut self, p: PenPiece) -> Vec<PenEvent> {
        match p {
            PenPiece::Type(t) => self.tool = Some(t),
            PenPiece::Serial(s) => self.serial = s,
            PenPiece::ProximityIn => self.came = true,
            PenPiece::ProximityOut => self.went = true,
            PenPiece::Down => self.down = true,
            PenPiece::Up => self.up = true,
            PenPiece::Motion(x, y) => {
                self.now.x = x;
                self.now.y = y;
                self.moved = true;
            }
            PenPiece::Pressure(v) => {
                self.now.pressure = v as f32 / 65535.0;
                self.moved = true;
            }
            PenPiece::Distance(v) => {
                self.now.distance = v as f32 / 65535.0;
                self.moved = true;
            }
            PenPiece::Tilt(x, y) => {
                self.now.tilt_x = x as f32;
                self.now.tilt_y = y as f32;
                self.moved = true;
            }
            PenPiece::Rotation(r) => {
                self.now.rotation = r as f32;
                self.moved = true;
            }
            PenPiece::Button(b, pressed) => {
                let bit = stylus_bit(b);
                if pressed {
                    self.buttons |= bit
                } else {
                    self.buttons &= !bit
                }
                self.moved = true;
            }
            PenPiece::Frame(time) => return self.frame(time),
        }
        Vec::new()
    }

    fn frame(&mut self, time_ms: u32) -> Vec<PenEvent> {
        let mut out = Vec::new();
        let (tool, serial, buttons) =
            (self.tool.unwrap_or(PenTool::Pen), self.serial, self.buttons);
        let ev = move |phase, s: PenSample| PenEvent {
            phase,
            tool,
            tool_serial: serial,
            sample: s,
            buttons,
            time_ms,
        };
        if self.came {
            self.near = true;
            out.push(ev(PenPhase::Enter, self.now));
        }
        if self.down && !self.touching {
            self.touching = true;
            out.push(ev(PenPhase::Down, self.now));
        } else if self.moved && self.near {
            out.push(ev(
                if self.touching {
                    PenPhase::Move
                } else {
                    PenPhase::Hover
                },
                self.now,
            ));
        }
        if self.up && self.touching {
            self.touching = false;
            // lifted: no pressure from here
            let mut s = self.now;
            s.pressure = 0.0;
            self.now.pressure = 0.0;
            out.push(ev(PenPhase::Up, s));
        }
        if self.went {
            if self.touching {
                // out of range while touching (the compositor should lift it first): lift it
                self.touching = false;
                self.now.pressure = 0.0;
                out.push(ev(PenPhase::Up, self.now));
            }
            self.near = false;
            self.buttons = 0;
            out.push(ev(PenPhase::Leave, self.now));
        }
        (self.came, self.went, self.down, self.up, self.moved) =
            (false, false, false, false, false);
        out
    }
}

/// A pointer event: one moment of the mouse or touchpad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEvent {
    Enter {
        x: f64,
        y: f64,
    },
    Leave,
    Move {
        x: f64,
        y: f64,
        time_ms: u32,
    },
    /// a button pressed or released (evdev codes: BTN_LEFT = 0x110, BTN_RIGHT = 0x111…)
    Button {
        button: u32,
        pressed: bool,
        x: f64,
        y: f64,
        time_ms: u32,
    },
    /// a scroll, in logical pixels (a wheel's notch is about 15); `wheel` when from a wheel
    Scroll {
        dx: f64,
        dy: f64,
        wheel: bool,
        x: f64,
        y: f64,
        time_ms: u32,
    },
}

/// One piece of a pointer moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerPiece {
    Enter(f64, f64),
    Leave,
    Motion(u32, f64, f64),
    Button(u32, u32, bool),
    /// (time, vertical?, value)
    Axis(u32, bool, f64),
    /// a wheel as the source of this moment's scroll
    WheelSource,
    Frame,
}

/// Gathers pointer pieces; with a version-5+ pointer, a frame ends each moment.
#[derive(Debug, Default)]
pub struct PointerGatherer {
    x: f64,
    y: f64,
    time: u32,
    pending: Vec<PointerEvent>,
    scroll: (f64, f64),
    wheel: bool,
    moved: bool,
}

impl PointerGatherer {
    pub fn take(&mut self, p: PointerPiece) -> Vec<PointerEvent> {
        match p {
            PointerPiece::Enter(x, y) => {
                (self.x, self.y) = (x, y);
                self.pending.push(PointerEvent::Enter { x, y });
            }
            PointerPiece::Leave => self.pending.push(PointerEvent::Leave),
            PointerPiece::Motion(t, x, y) => {
                (self.x, self.y, self.time) = (x, y, t);
                self.moved = true;
            }
            PointerPiece::Button(t, button, pressed) => {
                self.time = t;
                self.flush_move();
                self.pending.push(PointerEvent::Button {
                    button,
                    pressed,
                    x: self.x,
                    y: self.y,
                    time_ms: t,
                });
            }
            PointerPiece::Axis(t, vertical, v) => {
                self.time = t;
                if vertical {
                    self.scroll.1 += v
                } else {
                    self.scroll.0 += v
                }
            }
            PointerPiece::WheelSource => self.wheel = true,
            PointerPiece::Frame => {
                self.flush_move();
                if self.scroll != (0.0, 0.0) {
                    self.pending.push(PointerEvent::Scroll {
                        dx: self.scroll.0,
                        dy: self.scroll.1,
                        wheel: self.wheel,
                        x: self.x,
                        y: self.y,
                        time_ms: self.time,
                    });
                }
                (self.scroll, self.wheel) = ((0.0, 0.0), false);
                return std::mem::take(&mut self.pending);
            }
        }
        Vec::new()
    }

    fn flush_move(&mut self) {
        if self.moved {
            self.pending.push(PointerEvent::Move {
                x: self.x,
                y: self.y,
                time_ms: self.time,
            });
            self.moved = false;
        }
    }
}

/// Which modifier keys are held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// the Super / Windows / Command key
    pub logo: bool,
    pub caps_lock: bool,
}

/// A key pressed or released.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyEvent {
    /// the key's symbol in the current layout (xkb keysym: `xkbcommon_dl::keysyms`)
    pub keysym: u32,
    /// the keysym's name ("a", "Escape", "BackSpace"…)
    pub name: String,
    /// what it types, on press (none for keys that type nothing)
    pub text: Option<String>,
    pub pressed: bool,
    pub mods: Mods,
    /// the key's code as the keyboard reports it (evdev)
    pub code: u32,
    pub time_ms: u32,
}

/// Everything the window hands the app, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    Pointer(PointerEvent),
    Key(KeyEvent),
    Pen(PenEvent),
    /// the window gained (true) or lost the keyboard
    Focus(bool),
    /// keys held repeat at `rate` a second after `delay_ms` (rate 0: no repeating)
    KeyRepeat {
        rate: i32,
        delay_ms: i32,
    },
    /// text pasted from the system's clipboard ([`crate::Window::request_paste`])
    Paste(String),
}

/// Pens by the compositor's tool object, so each tool keeps its own state.
pub type PenGatherers<K> = HashMap<K, PenGatherer>;

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(g: &mut PenGatherer, pieces: &[PenPiece]) -> Vec<PenEvent> {
        pieces.iter().flat_map(|p| g.take(*p)).collect()
    }

    #[test]
    fn a_stroke_comes_in_whole_moments() {
        use PenPiece::*;
        let mut g = PenGatherer::default();
        let evs = feed(
            &mut g,
            &[
                Type(PenTool::Pen),
                Serial(0x1234),
                ProximityIn,
                Motion(10.0, 20.0),
                Distance(30000),
                Frame(1000),
                Motion(11.0, 21.0),
                Frame(1004),
                Down,
                Pressure(16384),
                Tilt(-12.5, 30.0),
                Frame(1008),
                Motion(14.0, 22.0),
                Pressure(65535),
                Frame(1012),
                Motion(15.0, 23.0), // only the position: the pressure stays
                Frame(1016),
                Up,
                Frame(1020),
                ProximityOut,
                Frame(1024),
            ],
        );
        let phases: Vec<_> = evs.iter().map(|e| e.phase).collect();
        use PenPhase as P;
        assert_eq!(
            phases,
            [
                P::Enter,
                P::Hover,
                P::Hover,
                P::Down,
                P::Move,
                P::Move,
                P::Up,
                P::Leave
            ]
        );
        let down = evs[3];
        assert_eq!((down.sample.x, down.sample.y), (11.0, 21.0));
        assert!((down.sample.pressure - 0.25).abs() < 0.001);
        assert_eq!((down.sample.tilt_x, down.sample.tilt_y), (-12.5, 30.0));
        assert_eq!(down.time_ms, 1008);
        assert_eq!(down.tool_serial, 0x1234);
        assert_eq!(evs[4].sample.pressure, 1.0);
        assert_eq!(evs[5].sample.pressure, 1.0, "the pressure carries over");
        assert_eq!((evs[5].sample.x, evs[5].sample.y), (15.0, 23.0));
        assert_eq!(evs[6].sample.pressure, 0.0, "lifted");
    }

    #[test]
    fn the_eraser_end_and_side_buttons() {
        use PenPiece::*;
        let mut g = PenGatherer::default();
        let evs = feed(
            &mut g,
            &[
                Type(PenTool::Eraser),
                ProximityIn,
                Motion(5.0, 5.0),
                Button(BTN_STYLUS, true),
                Frame(1),
            ],
        );
        assert_eq!(evs.last().unwrap().tool, PenTool::Eraser);
        assert_eq!(evs.last().unwrap().buttons, 1);
        let evs = feed(
            &mut g,
            &[
                Button(BTN_STYLUS2, true),
                Frame(2),
                Button(BTN_STYLUS, false),
                Frame(3),
            ],
        );
        assert_eq!(evs[0].buttons, 3);
        assert_eq!(evs[1].buttons, 2);
    }

    #[test]
    fn leaving_while_touching_lifts_first() {
        use PenPiece::*;
        let mut g = PenGatherer::default();
        feed(
            &mut g,
            &[
                ProximityIn,
                Motion(1.0, 1.0),
                Down,
                Pressure(40000),
                Frame(1),
            ],
        );
        let evs = feed(&mut g, &[ProximityOut, Frame(2)]);
        let phases: Vec<_> = evs.iter().map(|e| e.phase).collect();
        assert_eq!(phases, [PenPhase::Up, PenPhase::Leave]);
        assert_eq!(evs[0].sample.pressure, 0.0);
    }

    #[test]
    fn a_tap_in_one_moment_is_down_then_up() {
        use PenPiece::*;
        let mut g = PenGatherer::default();
        feed(&mut g, &[ProximityIn, Motion(1.0, 1.0), Frame(1)]);
        let evs = feed(&mut g, &[Down, Pressure(30000), Up, Frame(2)]);
        let phases: Vec<_> = evs.iter().map(|e| e.phase).collect();
        assert_eq!(phases, [PenPhase::Down, PenPhase::Up]);
        assert!(evs[0].sample.pressure > 0.4);
    }

    #[test]
    fn pointer_moments_and_scrolls() {
        use PointerPiece::*;
        let mut g = PointerGatherer::default();
        assert!(g.take(Enter(3.0, 4.0)).is_empty());
        let evs = g.take(Frame);
        assert_eq!(evs, [PointerEvent::Enter { x: 3.0, y: 4.0 }]);
        g.take(Motion(10, 5.0, 6.0));
        g.take(Motion(11, 7.0, 8.0)); // two moves in a moment: the last counts
        g.take(Button(11, 0x110, true));
        let evs = g.take(Frame);
        assert_eq!(
            evs,
            [
                PointerEvent::Move {
                    x: 7.0,
                    y: 8.0,
                    time_ms: 11
                },
                PointerEvent::Button {
                    button: 0x110,
                    pressed: true,
                    x: 7.0,
                    y: 8.0,
                    time_ms: 11
                }
            ]
        );
        g.take(WheelSource);
        g.take(Axis(20, true, 15.0));
        g.take(Axis(20, false, -2.0));
        let evs = g.take(Frame);
        assert_eq!(
            evs,
            [PointerEvent::Scroll {
                dx: -2.0,
                dy: 15.0,
                wheel: true,
                x: 7.0,
                y: 8.0,
                time_ms: 20
            }]
        );
    }
}
