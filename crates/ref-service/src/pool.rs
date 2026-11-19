//! The ready-made pool of vetted references.
//!
//! * `fresh[slot]`: vetted pictures not yet shown, served first.
//! * `library[slot]`: everything ever served for that slot, reused (randomly,
//!   avoiding what the student just saw) when `fresh` runs dry.
//! * `demand`: which slots students ask for; the refill worker tops those up
//!   first, so downloads follow real use instead of mass-downloading.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use rand::Rng;
use rand::seq::IndexedRandom;
use serde::{Deserialize, Serialize};

use crate::model::{LiveRef, Slot};

/// Near-duplicate if the perceptual hashes differ in at most this many bits.
pub const DUPLICATE_BITS: u32 = 10;
/// Fruitless refill steps before a slot is paused.
pub const MISS_LIMIT: u32 = 6;
pub const PAUSE: std::time::Duration = std::time::Duration::from_secs(30 * 60);

#[derive(Default, Serialize, Deserialize)]
pub struct PoolState {
    pub refs: HashMap<String, LiveRef>,
    pub fresh: BTreeMap<String, VecDeque<String>>,
    pub library: BTreeMap<String, Vec<String>>,
    pub blocked: HashSet<String>,
    /// Pixabay ids already looked at (accepted or rejected): never re-download.
    pub seen: HashSet<u64>,
    /// the same for the other sources, as `source:id` (`openverse:<uuid>`)
    #[serde(default)]
    pub seen_keys: HashSet<String>,
    pub phashes: Vec<(String, u64)>,
    /// reason code → number of pictures rejected for it (monitoring)
    #[serde(default)]
    pub rejected: BTreeMap<String, u32>,
    /// ids the picture sorter (ADR-0009) has checked; the rest are checked at startup
    #[serde(default)]
    pub sorted: HashSet<String>,
    /// the sorting rules `sorted` was checked under (`TopicCheck::rules`);
    /// other rules mean every picture is checked again
    #[serde(default)]
    pub sorted_rules: Option<String>,
    /// slot key → (slot, request count)
    #[serde(skip)]
    pub demand: HashMap<String, (Slot, u32)>,
    /// slot key → (refill steps that added nothing to it, paused until)
    #[serde(skip)]
    pub misses: HashMap<String, (u32, Option<std::time::Instant>)>,
}

impl PoolState {
    /// Has the search hit with this key (`pixabay:123`, `met:456`) been
    /// looked at before?
    pub fn is_seen(&self, key: &str) -> bool {
        match key.strip_prefix("pixabay:").map(str::parse::<u64>) {
            Some(Ok(id)) => self.seen.contains(&id),
            _ => self.seen_keys.contains(key),
        }
    }

    pub fn mark_seen(&mut self, key: &str) {
        match key.strip_prefix("pixabay:").map(str::parse::<u64>) {
            Some(Ok(id)) => {
                self.seen.insert(id);
            }
            _ => {
                self.seen_keys.insert(key.to_string());
            }
        }
    }

    pub fn note_demand(&mut self, slot: &Slot) {
        self.demand
            .entry(slot.key())
            .or_insert_with(|| (slot.clone(), 0))
            .1 += 1;
    }

    pub fn fresh_len(&self, slot: &Slot) -> usize {
        self.fresh.get(&slot.key()).map_or(0, VecDeque::len)
    }

    /// Hand out a reference for `slot`. `topic == "*"` means any topic.
    pub fn take(&mut self, slot: &Slot, exclude: &[String], rng: &mut impl Rng) -> Option<LiveRef> {
        let keys: Vec<String> = if slot.topic == "*" {
            let prefix = format!("{}/", slot.category);
            let suffix = format!("/{}", slot.tier);
            let mut ks: Vec<String> = self
                .fresh
                .keys()
                .chain(self.library.keys())
                .filter(|k| k.starts_with(&prefix) && k.ends_with(&suffix))
                .cloned()
                .collect();
            ks.sort();
            ks.dedup();
            // fullest fresh queue first, so "Surprise me" spreads the load
            ks.sort_by_key(|k| std::cmp::Reverse(self.fresh.get(k).map_or(0, VecDeque::len)));
            ks
        } else {
            vec![slot.key()]
        };
        let usable = |id: &String, s: &Self| !s.blocked.contains(id) && !exclude.contains(id);

        for key in &keys {
            while let Some(id) = self.fresh.get_mut(key).and_then(VecDeque::pop_front) {
                if usable(&id, self) && self.refs.contains_key(&id) {
                    self.library
                        .entry(key.clone())
                        .or_default()
                        .push(id.clone());
                    return self.refs.get(&id).cloned();
                }
            }
        }
        let pool: Vec<&String> = keys
            .iter()
            .filter_map(|k| self.library.get(k))
            .flatten()
            .filter(|id| usable(id, self))
            .collect();
        pool.choose(rng).and_then(|id| self.refs.get(*id).cloned())
    }

    /// Add a vetted reference. Returns false if it's a near-duplicate.
    pub fn add(&mut self, r: LiveRef, phash: Option<u64>) -> bool {
        if let Some(h) = phash {
            if self
                .phashes
                .iter()
                .any(|(_, o)| (h ^ o).count_ones() <= DUPLICATE_BITS)
            {
                return false;
            }
            self.phashes.push((r.id.clone(), h));
        }
        let key = Slot {
            category: r.category.clone(),
            topic: r.topic.clone(),
            tier: r.tier,
        }
        .key();
        self.fresh.entry(key).or_default().push_back(r.id.clone());
        self.refs.insert(r.id.clone(), r);
        true
    }

    /// Block a reported reference everywhere.
    pub fn block(&mut self, id: &str) -> bool {
        let known = self.refs.contains_key(id);
        self.blocked.insert(id.to_string());
        for q in self.fresh.values_mut() {
            q.retain(|x| x != id);
        }
        for l in self.library.values_mut() {
            l.retain(|x| x != id);
        }
        known
    }

    /// Remove every reference for which `drop` is true from the pool (refs,
    /// fresh and served queues, near-duplicate hashes). Their Pixabay ids stay
    /// in `seen`, so they are never downloaded again. Returns the removed refs.
    pub fn remove_where(&mut self, mut drop: impl FnMut(&LiveRef) -> bool) -> Vec<LiveRef> {
        let ids: HashSet<String> = self
            .refs
            .values()
            .filter(|r| drop(r))
            .map(|r| r.id.clone())
            .collect();
        if ids.is_empty() {
            return Vec::new();
        }
        for q in self.fresh.values_mut() {
            q.retain(|x| !ids.contains(x));
        }
        for l in self.library.values_mut() {
            l.retain(|x| !ids.contains(x));
        }
        self.fresh.retain(|_, q| !q.is_empty());
        self.library.retain(|_, l| !l.is_empty());
        self.phashes.retain(|(id, _)| !ids.contains(id));
        self.sorted.retain(|id| !ids.contains(id));
        let mut out: Vec<LiveRef> = ids.iter().filter_map(|id| self.refs.remove(id)).collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// Move a reference to another category/topic (same tier), keeping its
    /// place as fresh or already served. Returns false if `id` is unknown.
    pub fn refile(&mut self, id: &str, category: &str, topic: &str) -> bool {
        let Some(r) = self.refs.get_mut(id) else {
            return false;
        };
        let from = Slot {
            category: r.category.clone(),
            topic: r.topic.clone(),
            tier: r.tier,
        }
        .key();
        r.category = category.to_string();
        r.topic = topic.to_string();
        let to = Slot {
            category: category.to_string(),
            topic: topic.to_string(),
            tier: r.tier,
        }
        .key();
        if from == to {
            return true;
        }
        if let Some(q) = self.fresh.get_mut(&from) {
            let before = q.len();
            q.retain(|x| x != id);
            if q.len() < before {
                self.fresh
                    .entry(to.clone())
                    .or_default()
                    .push_back(id.to_string());
            }
        }
        if let Some(l) = self.library.get_mut(&from) {
            let before = l.len();
            l.retain(|x| x != id);
            if l.len() < before {
                self.library.entry(to).or_default().push(id.to_string());
            }
        }
        self.fresh.retain(|_, q| !q.is_empty());
        self.library.retain(|_, l| !l.is_empty());
        true
    }

    /// The slot most in need of refilling: asked-for slots below `target`
    /// fresh pictures, fewest first, then most requested.
    pub fn neediest(&self, target: usize) -> Option<Slot> {
        let now = std::time::Instant::now();
        self.demand
            .values()
            .filter(|(s, _)| self.fresh_len(s) < target)
            .filter(|(s, _)| {
                !self
                    .misses
                    .get(&s.key())
                    .and_then(|m| m.1)
                    .is_some_and(|until| until > now)
            })
            .min_by_key(|(s, n)| (self.fresh_len(s), std::cmp::Reverse(*n), s.key()))
            .map(|(s, _)| s.clone())
    }

    /// Record a refill step for `slot`. After `MISS_LIMIT` steps in a row that
    /// add nothing to it (e.g. Beginner pictures are rare for that topic), the
    /// slot is paused for `PAUSE` and the web app uses curated pictures.
    pub fn record_step(&mut self, slot: &Slot, gained: bool) {
        let m = self.misses.entry(slot.key()).or_insert((0, None));
        if gained {
            *m = (0, None);
        } else {
            m.0 += 1;
            if m.0 >= MISS_LIMIT {
                *m = (0, Some(std::time::Instant::now() + PAUSE));
            }
        }
    }

    pub fn summary(&self) -> BTreeMap<String, (usize, usize)> {
        let mut out = BTreeMap::new();
        for (k, q) in &self.fresh {
            out.entry(k.clone()).or_insert((0, 0)).0 = q.len();
        }
        for (k, l) in &self.library {
            out.entry(k.clone()).or_insert((0, 0)).1 = l.len();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn r(id: &str, cat: &str, topic: &str, tier: u8) -> LiveRef {
        LiveRef {
            id: id.into(),
            source: "pixabay".into(),
            source_id: 1,
            category: cat.into(),
            topic: topic.into(),
            page_url: String::new(),
            author: "a".into(),
            author_url: String::new(),
            license: "l".into(),
            license_url: String::new(),
            tags: vec![],
            width: 1000,
            height: 800,
            title: String::new(),
            attribution: String::new(),
            provider: String::new(),
            kind: String::new(),
            keypoints: vec![],
            difficulty: 20.0,
            tier,
            image_url: format!("/refimg/{id}.jpg"),
            live: true,
        }
    }
    fn slot(cat: &str, topic: &str, tier: u8) -> Slot {
        Slot {
            category: cat.into(),
            topic: topic.into(),
            tier,
        }
    }

    #[test]
    fn serves_fresh_first_then_library_without_repeating_excluded() {
        let mut p = PoolState::default();
        let mut rng = StdRng::seed_from_u64(1);
        p.add(r("a", "animals", "pets", 1), None);
        p.add(r("b", "animals", "pets", 1), None);
        let s = slot("animals", "pets", 1);
        assert_eq!(p.take(&s, &[], &mut rng).unwrap().id, "a");
        assert_eq!(p.take(&s, &[], &mut rng).unwrap().id, "b");
        assert_eq!(p.fresh_len(&s), 0);
        // library reuse, avoiding the excluded one
        for _ in 0..10 {
            assert_eq!(p.take(&s, &["b".into()], &mut rng).unwrap().id, "a");
        }
        assert!(
            p.take(&slot("animals", "birds", 1), &[], &mut rng)
                .is_none()
        );
    }

    #[test]
    fn any_topic_and_blocking() {
        let mut p = PoolState::default();
        let mut rng = StdRng::seed_from_u64(2);
        p.add(r("a", "animals", "pets", 2), None);
        p.add(r("b", "animals", "birds", 2), None);
        p.add(r("c", "anime", "chibi", 2), None);
        p.block("a");
        let got = p.take(&slot("animals", "*", 2), &[], &mut rng).unwrap();
        assert_eq!(got.id, "b");
        assert!(p.take(&slot("animals", "*", 3), &[], &mut rng).is_none());
    }

    #[test]
    fn near_duplicates_are_rejected() {
        let mut p = PoolState::default();
        assert!(p.add(r("a", "animals", "pets", 1), Some(0xFFFF_0000)));
        assert!(!p.add(r("b", "animals", "pets", 1), Some(0xFFFF_0003)));
        assert!(p.add(r("c", "animals", "pets", 1), Some(0x0000_FFFF)));
    }

    #[test]
    fn remove_where_purges_everywhere_but_keeps_seen() {
        let mut p = PoolState::default();
        let mut rng = StdRng::seed_from_u64(3);
        let mut vector = r("v", "real-life", "vehicles", 1);
        vector.page_url = "https://pixabay.com/vectors/pizza-courier-1454239/".into();
        vector.source_id = 1454239;
        let mut photo = r("p", "real-life", "vehicles", 1);
        photo.page_url = "https://pixabay.com/photos/bike-2/".into();
        p.seen.insert(1454239);
        p.add(vector, Some(0xFFFF_0000));
        p.add(photo, Some(0x0000_FFFF));
        let s = slot("real-life", "vehicles", 1);
        // serve "v" once so it is in the library too
        assert_eq!(p.take(&s, &[], &mut rng).unwrap().id, "v");

        let gone = p.remove_where(|r| !crate::model::kind_allowed(&r.category, r.kind()));
        assert_eq!(
            gone.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["v"]
        );
        assert!(!p.refs.contains_key("v"));
        assert!(p.phashes.iter().all(|(id, _)| id != "v"));
        assert!(p.seen.contains(&1454239), "never re-downloaded");
        for _ in 0..5 {
            assert_eq!(p.take(&s, &[], &mut rng).unwrap().id, "p");
        }
        assert!(p.remove_where(|_| false).is_empty());
    }

    #[test]
    fn seen_keys_per_source() {
        let mut p = PoolState::default();
        p.mark_seen("pixabay:12");
        p.mark_seen("openverse:abc");
        assert!(p.seen.contains(&12), "Pixabay ids stay in the old set");
        assert!(p.is_seen("pixabay:12"));
        assert!(p.is_seen("openverse:abc"));
        assert!(!p.is_seen("met:12"));
        assert!(!p.is_seen("pixabay:13"));
    }

    #[test]
    fn near_duplicates_across_sources() {
        let mut p = PoolState::default();
        let mut a = r("px-1", "animals", "birds", 2);
        a.source = "pixabay".into();
        let mut b = r("ov-1", "animals", "birds", 2);
        b.source = "openverse".into();
        assert!(p.add(a, Some(0xABCD_0000_1234)));
        assert!(
            !p.add(b, Some(0xABCD_0000_1235)),
            "the same picture from another source is a duplicate"
        );
    }

    #[test]
    fn refile_moves_fresh_and_served_pictures() {
        let mut p = PoolState::default();
        let mut rng = StdRng::seed_from_u64(4);
        p.add(r("a", "anime", "characters", 2), None);
        p.add(r("b", "anime", "characters", 2), None);
        let chars = slot("anime", "characters", 2);
        let scenery = slot("anime", "scenery", 2);
        assert_eq!(p.take(&chars, &[], &mut rng).unwrap().id, "a"); // a is served
        assert!(p.refile("a", "anime", "scenery"));
        assert!(p.refile("b", "anime", "scenery"));
        assert!(!p.refile("zzz", "anime", "scenery"));
        assert_eq!(p.fresh_len(&scenery), 1);
        assert_eq!(p.library[&scenery.key()], ["a"]);
        assert!(p.take(&chars, &[], &mut rng).is_none());
        assert_eq!(p.refs["a"].topic, "scenery");
        assert_eq!(p.take(&scenery, &[], &mut rng).unwrap().id, "b");
    }

    #[test]
    fn neediest_prefers_empty_then_popular() {
        let mut p = PoolState::default();
        let (a, b) = (slot("animals", "pets", 1), slot("anime", "chibi", 1));
        p.note_demand(&a);
        p.note_demand(&b);
        p.note_demand(&b);
        assert_eq!(p.neediest(3), Some(b.clone()));
        p.add(r("x", "anime", "chibi", 1), None);
        assert_eq!(p.neediest(3), Some(a.clone()));
        for i in 0..3 {
            p.add(r(&format!("p{i}"), "animals", "pets", 1), None);
        }
        assert_eq!(p.neediest(3), Some(b));
    }

    #[test]
    fn fruitless_slots_are_paused() {
        let mut p = PoolState::default();
        let a = slot("animals", "insects", 1);
        p.note_demand(&a);
        for _ in 0..MISS_LIMIT - 1 {
            p.record_step(&a, false);
        }
        assert_eq!(p.neediest(3), Some(a.clone()));
        p.record_step(&a, false);
        assert_eq!(p.neediest(3), None, "paused after MISS_LIMIT misses");
    }
}
