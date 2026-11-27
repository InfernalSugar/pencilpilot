//! Drawing late: a frame is drawn just before the compositor needs it for the next refresh
//! (predicted from when the last frames were shown), so it carries the newest input.
//!
//! The margin before that refresh adjusts itself:
//! - a frame that misses its refresh adds 0.5 ms;
//! - 200 on time take 0.1 ms off, down to 1 ms;
//! - where it needs more than three quarters of a frame, drawing late doesn't pay on this
//!   compositor: from then on, frames are drawn as soon as it asks.

use std::collections::BTreeMap;

use crate::Presented;

#[derive(Debug, Clone)]
pub struct Pacer {
    margin_ns: u64,
    on_time: u32,
    at_once: bool,
    last_shown: Option<u64>,
    refresh_ns: u32,
    /// frame → the refresh it was drawn for
    targets: BTreeMap<u64, u64>,
    /// frames that missed the refresh they were drawn for
    pub late: u32,
}

/// A frame's plan: the refresh it's for, and when to draw it (CLOCK_MONOTONIC ns).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub target: u64,
    pub draw_at: u64,
}

impl Default for Pacer {
    fn default() -> Pacer {
        Pacer::new(false)
    }
}

impl Pacer {
    /// `at_once`: draw as soon as the compositor asks (no late drawing).
    pub fn new(at_once: bool) -> Pacer {
        Pacer {
            margin_ns: 3_000_000,
            on_time: 0,
            at_once,
            last_shown: None,
            refresh_ns: 0,
            targets: BTreeMap::new(),
            late: 0,
        }
    }

    /// When the next frame should be drawn, from `now`; None: now (no refresh known yet,
    /// or drawing at once).
    pub fn plan(&self, now: u64) -> Option<Plan> {
        let (false, Some(last), p) = (self.at_once, self.last_shown, self.refresh_ns) else {
            return None;
        };
        if p == 0 {
            return None;
        }
        let p = p as u64;
        // after a pause the refreshes' rhythm is stale (and the compositor may be idle):
        // draw now, and plan again from that frame
        if now.saturating_sub(last) > 8 * p {
            return None;
        }
        let n = (now + self.margin_ns + 200_000)
            .saturating_sub(last)
            .div_ceil(p);
        let target = last + n.max(1) * p;
        Some(Plan {
            target,
            draw_at: target - self.margin_ns,
        })
    }

    /// A frame was handed over, drawn to `plan`.
    pub fn drawn(&mut self, frame: u64, plan: Option<Plan>) {
        if let Some(p) = plan {
            self.targets.insert(frame, p.target);
        }
    }

    /// A frame was shown: learn the refresh, and whether drawing late made it in time.
    pub fn shown(&mut self, p: &Presented) {
        self.last_shown = Some(p.at_ns);
        if p.refresh_ns > 0 {
            self.refresh_ns = p.refresh_ns;
        }
        if let Some(target) = self.targets.remove(&p.frame)
            && self.refresh_ns > 0
        {
            let refresh = self.refresh_ns as u64;
            if p.at_ns > target + refresh / 2 {
                // missed its refresh: more room
                self.late += 1;
                self.margin_ns = (self.margin_ns + 500_000).min(refresh);
                self.on_time = 0;
                if self.margin_ns * 4 > refresh * 3 {
                    self.at_once = true;
                }
            } else {
                self.on_time += 1;
                if self.on_time >= 200 {
                    self.margin_ns = self.margin_ns.saturating_sub(100_000).max(1_000_000);
                    self.on_time = 0;
                }
            }
        }
        self.targets.retain(|&f, _| f > p.frame);
    }

    /// Whether frames are drawn late (false once it didn't pay).
    pub fn drawing_late(&self) -> bool {
        !self.at_once
    }

    /// The margin before the refresh, ms.
    pub fn margin_ms(&self) -> f64 {
        self.margin_ns as f64 / 1e6
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(frame: u64, at_ns: u64) -> Presented {
        Presented {
            frame,
            at_ns,
            refresh_ns: 6_060_606,
        }
    }

    #[test]
    fn plans_just_before_the_next_refresh() {
        let mut p = Pacer::new(false);
        assert_eq!(p.plan(0), None, "no refresh known yet: now");
        p.shown(&shown(1, 1_000_000_000));
        let plan = p.plan(1_001_000_000).unwrap();
        assert_eq!(plan.target, 1_006_060_606);
        assert_eq!(plan.draw_at, 1_003_060_606);
        // too close to that refresh: the one after
        let plan = p.plan(1_004_000_000).unwrap();
        assert_eq!(plan.target, 1_012_121_212);
        // after a pause: at once
        assert_eq!(p.plan(1_100_000_000), None);
    }

    #[test]
    fn misses_add_room_and_give_up_past_three_quarters_of_a_frame() {
        let mut p = Pacer::new(false);
        p.shown(&shown(1, 1_000_000_000));
        let mut t = 1_000_000_000u64;
        for f in 2..20 {
            let plan = p.plan(t + 1_000_000);
            p.drawn(f, plan);
            // shown a whole refresh late every time
            t = plan.unwrap().target + 6_060_606;
            p.shown(&shown(f, t));
            if !p.drawing_late() {
                break;
            }
        }
        assert!(!p.drawing_late(), "{:?}", p);
        assert!(p.late >= 3);
        assert_eq!(p.plan(t), None);
    }
}
