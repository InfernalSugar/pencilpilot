//! Request budgets for the picture sources' APIs.
//!
//! Every source has a minimum gap between calls and one or more windows
//! ("at most N calls per hour / day / month"). Calls are remembered as unix
//! seconds in `data/ref-service/budget-<source>.json`, so a restart doesn't
//! reset a monthly limit. A 429 (or a "0 calls left" header) pauses the
//! source until its limit resets. A paused or spent source is skipped by the
//! refill worker; it never waits for hours.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    /// unix seconds of recent calls (oldest first)
    calls: VecDeque<u64>,
    /// paused until this unix second (0 = not paused)
    #[serde(default)]
    paused_until: u64,
}

struct State {
    saved: Saved,
    last: Option<Instant>,
}

pub struct Budget {
    name: &'static str,
    gap: Duration,
    /// (window length in seconds, max calls in it)
    windows: Vec<(u64, u32)>,
    path: Option<PathBuf>,
    state: Mutex<State>,
}

impl Budget {
    /// `path`: where to remember calls (`None`: memory only, for tests).
    pub fn new(
        name: &'static str,
        gap: Duration,
        windows: Vec<(u64, u32)>,
        path: Option<PathBuf>,
    ) -> Self {
        let saved = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self {
            name,
            gap,
            windows,
            path,
            state: Mutex::new(State { saved, last: None }),
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    fn longest(&self) -> u64 {
        self.windows.iter().map(|w| w.0).max().unwrap_or(0)
    }

    /// Why a call can't be made now (paused or a window is full), if so.
    fn blocked(&self, s: &Saved, now: u64) -> Option<String> {
        if s.paused_until > now {
            return Some(format!(
                "{} paused for {} s",
                self.name,
                s.paused_until - now
            ));
        }
        for &(len, max) in &self.windows {
            let used = s.calls.iter().filter(|&&t| t + len > now).count();
            if used >= max as usize {
                return Some(format!("{} budget spent ({max} per {len} s)", self.name));
            }
        }
        None
    }

    /// True if a call could be made now (not paused, budget left).
    pub async fn available(&self) -> bool {
        let s = self.state.lock().await;
        self.blocked(&s.saved, now_secs()).is_none()
    }

    /// Wait for the minimum gap, then record one call. `Err` if the source
    /// is paused or its budget is spent (the caller skips the source).
    pub async fn take(&self) -> Result<(), String> {
        let mut s = self.state.lock().await;
        let now = now_secs();
        if let Some(why) = self.blocked(&s.saved, now) {
            return Err(why);
        }
        if let Some(t) = s.last {
            tokio::time::sleep(self.gap.saturating_sub(t.elapsed())).await;
        }
        s.last = Some(Instant::now());
        let keep = self.longest();
        let now = now_secs();
        s.saved.calls.retain(|&t| t + keep > now);
        s.saved.calls.push_back(now);
        self.persist(&s.saved);
        Ok(())
    }

    /// Pause the source for `secs` (after a 429, or when the API says no
    /// calls are left).
    pub async fn pause(&self, secs: u64) {
        let mut s = self.state.lock().await;
        let until = now_secs() + secs.max(1);
        if until > s.saved.paused_until {
            s.saved.paused_until = until;
            tracing::warn!(source = self.name, secs, "source paused (rate limit)");
            self.persist(&s.saved);
        }
    }

    /// Calls remembered in the longest window (tests).
    #[cfg(test)]
    pub async fn calls(&self) -> usize {
        self.state.lock().await.saved.calls.len()
    }

    fn persist(&self, s: &Saved) {
        if let Some(p) = &self.path
            && let Ok(text) = serde_json::to_string(s)
        {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                std::fs::rename(&tmp, p).ok();
            }
        }
    }
}

/// Seconds to pause after an HTTP 429: the `Retry-After` header when it is a
/// number of seconds, else `default`.
pub fn retry_after(headers: &reqwest::header::HeaderMap, default: u64) -> u64 {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map_or(default, |s| s.clamp(1, 7 * 24 * 3600))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn windows_limit_calls() {
        let b = Budget::new("t", Duration::ZERO, vec![(3600, 2)], None);
        assert!(b.take().await.is_ok());
        assert!(b.available().await);
        assert!(b.take().await.is_ok());
        assert!(!b.available().await);
        assert!(b.take().await.unwrap_err().contains("budget spent"));
    }

    #[tokio::test]
    async fn pause_blocks_and_persists() {
        let dir = std::env::temp_dir().join(format!("pp-budget-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("budget-t.json");
        std::fs::remove_file(&path).ok();
        let b = Budget::new("t", Duration::ZERO, vec![(60, 10)], Some(path.clone()));
        b.take().await.unwrap();
        b.pause(600).await;
        assert!(b.take().await.unwrap_err().contains("paused"));
        // a restart remembers both the call and the pause
        let again = Budget::new("t", Duration::ZERO, vec![(60, 10)], Some(path));
        assert!(!again.available().await);
        assert_eq!(again.state.lock().await.saved.calls.len(), 1);
    }

    #[tokio::test]
    async fn gap_is_respected() {
        let b = Budget::new("t", Duration::from_millis(60), vec![(60, 10)], None);
        let t = Instant::now();
        b.take().await.unwrap();
        b.take().await.unwrap();
        assert!(t.elapsed() >= Duration::from_millis(55));
    }

    #[test]
    fn retry_after_header() {
        let mut h = reqwest::header::HeaderMap::new();
        assert_eq!(retry_after(&h, 60), 60);
        h.insert(reqwest::header::RETRY_AFTER, "120".parse().unwrap());
        assert_eq!(retry_after(&h, 60), 120);
        h.insert(reqwest::header::RETRY_AFTER, "soon".parse().unwrap());
        assert_eq!(retry_after(&h, 60), 60);
    }
}
