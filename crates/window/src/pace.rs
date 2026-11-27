//! Drawing late: a frame is drawn just before the compositor needs it for the next refresh
//! (predicted from when the last frames were shown), so it carries the newest input.
//!
//! How early to start is measured, not guessed:
//! - the time a frame takes to make (from starting it to handing it over), its 95th
//!   percentile over the last 64 frames, plus some slack;
//! - the slack starts at 1 ms; a frame that misses its refresh adds 1 ms (up to half a frame);
//!   50 on time take 0.1 ms off (down to 1 ms);
//! - three misses within a second: frames are drawn as soon as the compositor asks for half
//!   a second (a busy machine), then late again;
//! - when making a frame takes more than three quarters of one, it's drawn at once.

use std::collections::{BTreeMap, VecDeque};

use crate::Presented;

const SLACK_START: u64 = 1_000_000;
const SLACK_MIN: u64 = 1_000_000;
const WORK_KEPT: usize = 64;
/// refreshes without a frame shown after which the next is drawn at once
const PAUSE_REFRESHES: u64 = 2;
/// frames on time before the slack tightens by 0.1 ms
const ON_TIME_TO_TIGHTEN: u32 = 50;
/// a rest from drawing late, after `MISSES_TO_REST` misses within `MISS_WINDOW`
const COOL_DOWN: u64 = 500_000_000;
const MISS_WINDOW: u64 = 1_000_000_000;
const MISSES_TO_REST: usize = 3;

#[derive(Debug, Clone)]
pub struct Pacer {
    slack_ns: u64,
    on_time: u32,
    /// never draw late
    at_once: bool,
    /// drawing at once until then (after misses)
    cool_until: u64,
    misses: VecDeque<u64>,
    work: VecDeque<u64>,
    last_shown: Option<u64>,
    refresh_ns: u32,
    /// frame → the refresh it was drawn for
    targets: BTreeMap<u64, u64>,
    /// frames that missed the refresh they were drawn for
    pub late: u32,
    /// times it went to drawing at once for a while
    pub cool_downs: u32,
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
            slack_ns: SLACK_START,
            on_time: 0,
            at_once,
            cool_until: 0,
            misses: VecDeque::new(),
            work: VecDeque::new(),
            last_shown: None,
            refresh_ns: 0,
            targets: BTreeMap::new(),
            late: 0,
            cool_downs: 0,
        }
    }

    /// How long a frame takes to make: the 95th percentile of the recent ones.
    fn work_ns(&self) -> u64 {
        if self.work.is_empty() {
            return 1_000_000;
        }
        let mut w: Vec<u64> = self.work.iter().copied().collect();
        w.sort_unstable();
        w[((w.len() - 1) * 95) / 100]
    }

    /// How long before the refresh a frame is started, ns.
    pub fn margin_ns(&self) -> u64 {
        self.work_ns() + self.slack_ns
    }

    /// When the next frame should be drawn, from `now`; None: now (no refresh known yet,
    /// drawing at once, or after a pause).
    pub fn plan(&self, now: u64) -> Option<Plan> {
        let (false, Some(last), p) = (self.at_once, self.last_shown, self.refresh_ns) else {
            return None;
        };
        if p == 0 || now < self.cool_until {
            return None;
        }
        let p = p as u64;
        // after a pause (even a short one: between two strokes) the compositor has gone idle
        // and its rhythm is stale: drawing late would miss, so draw now and plan from there
        if now.saturating_sub(last) > PAUSE_REFRESHES * p {
            return None;
        }
        let margin = self.margin_ns();
        if margin * 4 > p * 3 {
            return None;
        }
        let n = (now + margin + 200_000).saturating_sub(last).div_ceil(p);
        let target = last + n.max(1) * p;
        Some(Plan {
            target,
            draw_at: target - margin,
        })
    }

    /// A frame was handed over, drawn to `plan`; making it took `work_ns` (from starting it
    /// to handing it over; None: don't count it, e.g. a frame no input asked for).
    pub fn drawn(&mut self, frame: u64, plan: Option<Plan>, work_ns: Option<u64>) {
        if let Some(p) = plan {
            self.targets.insert(frame, p.target);
        }
        let Some(work_ns) = work_ns else { return };
        // a one-off (a document's first upload, fonts joining) says nothing about the next
        // frame: what took over two refreshes isn't counted
        if self.refresh_ns > 0 && work_ns > 2 * self.refresh_ns as u64 {
            return;
        }
        self.work.push_back(work_ns);
        if self.work.len() > WORK_KEPT {
            self.work.pop_front();
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
                // missed its refresh: more room, and after two in a second, a rest
                self.late += 1;
                self.slack_ns = (self.slack_ns + 1_000_000).min(refresh / 2);
                self.on_time = 0;
                self.misses.push_back(p.at_ns);
                while self
                    .misses
                    .front()
                    .is_some_and(|&t| t + MISS_WINDOW < p.at_ns)
                {
                    self.misses.pop_front();
                }
                if self.misses.len() >= MISSES_TO_REST {
                    self.cool_until = p.at_ns + COOL_DOWN;
                    self.cool_downs += 1;
                    self.misses.clear();
                }
            } else {
                self.on_time += 1;
                if self.on_time >= ON_TIME_TO_TIGHTEN {
                    self.slack_ns = self.slack_ns.saturating_sub(100_000).max(SLACK_MIN);
                    self.on_time = 0;
                }
            }
        }
        self.targets.retain(|&f, _| f > p.frame);
    }

    /// About how long from starting a frame to its being shown, ns (0: no refresh known).
    pub fn show_delay_ns(&self, now: u64) -> u64 {
        match self.plan(now) {
            Some(_) => self.margin_ns(),
            None => self.refresh_ns as u64,
        }
    }

    /// The refresh period, ns (0: not known yet).
    pub fn refresh_ns(&self) -> u64 {
        self.refresh_ns as u64
    }

    /// Whether frames may be drawn late (false: always at once).
    pub fn drawing_late(&self) -> bool {
        !self.at_once
    }

    /// The margin before the refresh, ms.
    pub fn margin_ms(&self) -> f64 {
        self.margin_ns() as f64 / 1e6
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: u64 = 6_060_606;

    fn shown(frame: u64, at_ns: u64) -> Presented {
        Presented {
            frame,
            at_ns,
            refresh_ns: P as u32,
        }
    }

    #[test]
    fn plans_just_before_the_next_refresh() {
        let mut p = Pacer::new(false);
        assert_eq!(p.plan(0), None, "no refresh known yet: now");
        p.shown(&shown(1, 1_000_000_000));
        // no work measured yet: 1 ms + 1 ms slack
        let plan = p.plan(1_001_000_000).unwrap();
        assert_eq!(plan.target, 1_000_000_000 + P);
        assert_eq!(plan.draw_at, 1_000_000_000 + P - 2_000_000);
        // too close to that refresh: the one after
        let plan = p.plan(1_004_000_000).unwrap();
        assert_eq!(plan.target, 1_000_000_000 + 2 * P);
        // after a pause, even a short one: at once
        assert_eq!(p.plan(1_100_000_000), None);
        assert_eq!(p.plan(1_000_000_000 + 3 * P), None);
    }

    #[test]
    fn the_margin_follows_the_work() {
        let mut p = Pacer::new(false);
        p.shown(&shown(1, 1_000_000_000));
        for f in 0..64 {
            p.drawn(100 + f, None, Some(1_500_000));
        }
        assert_eq!(p.margin_ns(), 2_500_000);
        // slow frames: a bigger margin; too slow for late drawing: at once
        for f in 0..64 {
            p.drawn(200 + f, None, Some(3_800_000));
        }
        assert_eq!(
            p.plan(1_001_000_000),
            None,
            "4.8 ms of a 6 ms frame: at once"
        );
        // a one-off of half a second isn't counted
        let mut q = Pacer::new(false);
        q.shown(&shown(1, 1_000_000_000));
        q.drawn(2, None, Some(465_000_000));
        q.drawn(3, None, None);
        assert_eq!(q.margin_ns(), 2_000_000);
    }

    #[test]
    fn three_misses_in_a_second_draw_at_once_for_a_while() {
        let mut p = Pacer::new(false);
        p.shown(&shown(1, 1_000_000_000));
        let mut t = 1_000_000_000u64;
        for f in 2..5 {
            let plan = p.plan(t + 1_000_000).unwrap();
            p.drawn(f, Some(plan), Some(1_000_000));
            // shown a whole refresh late
            t = plan.target + P;
            p.shown(&shown(f, t));
        }
        assert_eq!((p.late, p.cool_downs), (3, 1));
        assert_eq!(p.plan(t + 1_000_000), None, "resting: at once");
        // and back to late after the second
        p.shown(&shown(10, t + COOL_DOWN + P));
        assert!(p.plan(t + COOL_DOWN + P + 500_000).is_some());
    }
}
