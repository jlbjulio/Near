use crate::model::{Paired, Reading};
use std::collections::HashMap;

/// A broadcast at least this strong comes from headphones close to this PC, not from someone
/// else's pair of the same model across the room.
const CLOSE: i16 = -65;
/// How recently the case must have been seen holding both earbuds for one coming out to count.
const CASE_WINDOW: u64 = 15_000;
/// How long headphones must have gone unseen for their reappearance to count as arriving.
const AWAY: u64 = 60_000;
/// How long a connection may be tried after the moment that allowed it.
const ARMED_FOR: u64 = 120_000;
/// With the Bluetooth link already up, Windows or the phone is connecting; give it this long.
const LINK_GRACE: u64 = 10_000;

/// When Near may connect paired headphones on its own: only right after they are taken into
/// use near this PC (`arm`), with at most three tries 30 s apart in the two minutes after.
/// Connecting ends the arming, so when the phone or Windows takes the headphones later, Near
/// does not pull them back until they are taken into use again.
///
/// Taken into use means: an earbud comes out of a case seen moments ago (AirPods with a
/// verified case layout), headphones without case information show up after a minute away
/// (AirPods Max, single-unit models), or the link comes up for headphones that broadcast
/// nothing.
#[derive(Default)]
pub struct AutoConnect {
    /// Per address: tries made, time of the last try, and when it was armed.
    armed: HashMap<u64, (u8, u64, u64)>,
    /// Per address: when the case was last seen holding both earbuds.
    cased: HashMap<u64, u64>,
    /// Per address: when the headphones' broadcast was last seen.
    seen: HashMap<u64, u64>,
    /// Per address: whether the Bluetooth link was up at the last look.
    link: HashMap<u64, bool>,
}
impl AutoConnect {
    /// Allows tries from now on; an arming already running keeps its count of tries.
    fn arm(&mut self, address: u64, t: u64) {
        self.armed.entry(address).or_insert((0, 0, t));
    }
    /// A fresh reading of `address`'s broadcast. `case_layout`: the model reports whether both
    /// earbuds are in the case.
    pub fn sighted(&mut self, address: u64, r: &Reading, case_layout: bool) {
        let close = r.signal >= CLOSE;
        let away = self
            .seen
            .get(&address)
            .is_none_or(|at| r.at.saturating_sub(*at) > AWAY);
        self.seen.insert(address, r.at);
        if case_layout {
            if r.cased {
                self.cased.insert(address, r.at);
            } else if close
                && self
                    .cased
                    .get(&address)
                    .is_some_and(|at| r.at.saturating_sub(*at) < CASE_WINDOW)
            {
                self.cased.remove(&address);
                self.arm(address, r.at);
            }
        } else if close && away {
            self.arm(address, r.at);
        }
    }
    /// The Bluetooth link state of headphones that broadcast nothing: it coming up arms them.
    pub fn link(&mut self, address: u64, up: bool, t: u64) {
        if up && self.link.insert(address, up) == Some(false) {
            self.arm(address, t);
        } else {
            self.link.insert(address, up);
        }
    }
    pub fn ready(
        &mut self,
        p: &Paired,
        available: bool,
        paused: &[u64],
        busy: bool,
        t: u64,
    ) -> bool {
        self.armed
            .retain(|_, (_, _, armed)| t.saturating_sub(*armed) < ARMED_FOR);
        if p.connected {
            self.armed.remove(&p.address);
            return false;
        }
        if !available || paused.contains(&p.address) || busy {
            return false;
        }
        let Some(entry) = self.armed.get_mut(&p.address) else {
            return false;
        };
        if p.available && entry.0 == 0 && t.saturating_sub(entry.2) < LINK_GRACE {
            return false;
        }
        if entry.0 >= 3 || entry.0 != 0 && t.saturating_sub(entry.1) < 30_000 {
            return false;
        }
        entry.0 += 1;
        entry.1 = t;
        true
    }
    /// Lets an armed connection try again right away; does not arm one.
    pub fn reset(&mut self, address: u64) {
        if let Some(entry) = self.armed.get_mut(&address) {
            entry.0 = 0;
            entry.1 = 0;
        }
    }
    pub fn attempted(&mut self, address: u64, t: u64) {
        self.armed.insert(address, (1, t, t));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pods() -> Paired {
        Paired {
            address: 7,
            ..Default::default()
        }
    }
    fn reading(cased: bool, signal: i16, at: u64) -> Reading {
        Reading {
            address: 99,
            model: 0,
            name: String::new(),
            signal,
            at,
            battery: [None; 3],
            charging: [None; 3],
            ears: [None; 2],
            lid: None,
            cased,
            approximate: true,
        }
    }
    fn armed_by_earbud_out() -> AutoConnect {
        let mut auto = AutoConnect::default();
        auto.sighted(7, &reading(true, -50, 1), true);
        auto.sighted(7, &reading(false, -50, 2), true);
        auto
    }
    #[test]
    fn nearby_headphones_alone_never_trigger_a_connection() {
        let mut auto = AutoConnect::default();
        for at in [1, 1_000, 2_000] {
            auto.sighted(7, &reading(false, -40, at), true);
        }
        for t in [3_000, 30_001, 60_001, 300_001] {
            assert!(!auto.ready(&pods(), true, &[], false, t));
        }
    }
    #[test]
    fn taking_an_earbud_out_connects_unless_paused_busy_or_away() {
        let p = pods();
        let mut auto = armed_by_earbud_out();
        assert!(!auto.ready(&p, false, &[], false, 3));
        assert!(!auto.ready(&p, true, &[7], false, 3));
        assert!(!auto.ready(&p, true, &[], true, 3));
        assert!(auto.ready(&p, true, &[], false, 3));
        assert!(!auto.ready(&p, true, &[], false, 20_000));
    }
    #[test]
    fn a_weak_frame_does_not_use_up_the_earbud_coming_out() {
        let mut auto = AutoConnect::default();
        auto.sighted(7, &reading(true, -60, 1), true);
        auto.sighted(7, &reading(false, -80, 2), true);
        assert!(!auto.ready(&pods(), true, &[], false, 3));
        auto.sighted(7, &reading(false, -55, 4), true);
        assert!(auto.ready(&pods(), true, &[], false, 5));
    }
    #[test]
    fn a_case_seen_long_ago_or_far_away_arms_nothing() {
        let mut auto = AutoConnect::default();
        auto.sighted(7, &reading(true, -50, 1), true);
        auto.sighted(7, &reading(false, -50, 20_000), true);
        assert!(!auto.ready(&pods(), true, &[], false, 20_001));
        auto.sighted(7, &reading(true, -50, 30_000), true);
        auto.sighted(7, &reading(false, -75, 31_000), true);
        assert!(!auto.ready(&pods(), true, &[], false, 31_001));
    }
    #[test]
    fn retries_are_bounded_and_coming_out_again_does_not_reset_them() {
        let p = pods();
        let mut auto = armed_by_earbud_out();
        for t in [3, 30_003, 60_003] {
            assert!(auto.ready(&p, true, &[], false, t));
            auto.sighted(7, &reading(true, -50, t), true);
            auto.sighted(7, &reading(false, -50, t + 1), true);
        }
        assert!(!auto.ready(&p, true, &[], false, 90_003));
        assert!(!auto.ready(&p, true, &[], false, 125_000), "arming expired");
    }
    #[test]
    fn once_connected_the_phone_can_take_them_without_a_fight() {
        let mut p = pods();
        let mut auto = armed_by_earbud_out();
        assert!(auto.ready(&p, true, &[], false, 3));
        p.connected = true;
        assert!(!auto.ready(&p, true, &[], false, 5_000));
        // The phone takes them back: no new try until an earbud comes out again.
        p.connected = false;
        for t in [10_000, 40_000, 70_000] {
            auto.sighted(7, &reading(false, -40, t), true);
            assert!(!auto.ready(&p, true, &[], false, t));
        }
    }
    #[test]
    fn headphones_without_a_case_connect_when_they_arrive() {
        let mut auto = AutoConnect::default();
        auto.sighted(7, &reading(false, -50, 1), false);
        assert!(auto.ready(&pods(), true, &[], false, 2));
        let mut p = pods();
        p.connected = true;
        assert!(!auto.ready(&p, true, &[], false, 3));
        // Still around: no new arming.
        auto.sighted(7, &reading(false, -50, 30_000), false);
        assert!(!auto.ready(&pods(), true, &[], false, 30_001));
        // Back after more than a minute away.
        auto.sighted(7, &reading(false, -50, 100_000), false);
        assert!(auto.ready(&pods(), true, &[], false, 100_001));
    }
    #[test]
    fn headphones_that_broadcast_nothing_connect_when_their_link_comes_up() {
        let mut p = pods();
        let mut auto = AutoConnect::default();
        auto.link(7, false, 1);
        assert!(!auto.ready(&p, true, &[], false, 2));
        auto.link(7, true, 3);
        p.available = true;
        assert!(
            !auto.ready(&p, true, &[], false, 4),
            "Windows gets a moment first"
        );
        assert!(auto.ready(&p, true, &[], false, 10_004));
    }
    #[test]
    fn a_manual_attempt_is_not_immediately_repeated_in_the_background() {
        let p = pods();
        let mut auto = AutoConnect::default();
        auto.attempted(7, 10_000);
        assert!(!auto.ready(&p, true, &[], false, 10_001));
        assert!(auto.ready(&p, true, &[], false, 40_000));
    }
}
