use crate::model::Paired;
use std::collections::HashMap;

#[derive(Default)]
pub struct AutoConnect {
    attempts: HashMap<u64, (u8, u64, u64)>,
}
impl AutoConnect {
    pub fn ready(
        &mut self,
        p: &Paired,
        available: bool,
        paused: &[u64],
        busy: bool,
        t: u64,
    ) -> bool {
        self.attempts
            .retain(|_, (_, _, seen)| t.saturating_sub(*seen) < 120_000);
        if p.connected {
            self.attempts.remove(&p.address);
            return false;
        }
        if !available || paused.contains(&p.address) || busy {
            return false;
        }
        let entry = self.attempts.entry(p.address).or_insert((0, 0, t));
        entry.2 = t;
        if entry.0 >= 3 || entry.0 != 0 && t.saturating_sub(entry.1) < 30_000 {
            return false;
        }
        entry.0 += 1;
        entry.1 = t;
        true
    }
    pub fn reset(&mut self, address: u64) {
        self.attempts.remove(&address);
    }
    pub fn attempted(&mut self, address: u64, t: u64) {
        self.attempts.insert(address, (1, t, t));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auto_connect_requires_presence_and_respects_manual_disconnect() {
        let p = Paired {
            address: 7,
            ..Default::default()
        };
        let mut auto = AutoConnect::default();
        assert!(!auto.ready(&p, false, &[], false, 1));
        assert!(!auto.ready(&p, true, &[7], false, 1));
        assert!(!auto.ready(&p, true, &[], true, 1));
        assert!(auto.ready(&p, true, &[], false, 1));
        assert!(!auto.ready(&p, true, &[], false, 20_000));
    }
    #[test]
    fn retries_are_bounded_and_a_later_presence_session_can_retry() {
        let p = Paired {
            address: 7,
            ..Default::default()
        };
        let mut auto = AutoConnect::default();
        for t in [1, 30_001, 60_001] {
            assert!(auto.ready(&p, true, &[], false, t));
        }
        assert!(!auto.ready(&p, true, &[], false, 90_001));
        assert!(auto.ready(&p, true, &[], false, 220_001));
    }
    #[test]
    fn connected_devices_are_never_reconnected() {
        let p = Paired {
            address: 7,
            connected: true,
            ..Default::default()
        };
        assert!(!AutoConnect::default().ready(&p, true, &[], false, 1));
    }
    #[test]
    fn a_manual_attempt_is_not_immediately_repeated_in_the_background() {
        let p = Paired {
            address: 7,
            ..Default::default()
        };
        let mut auto = AutoConnect::default();
        auto.attempted(7, 10_000);
        assert!(!auto.ready(&p, true, &[], false, 10_001));
        assert!(auto.ready(&p, true, &[], false, 40_000));
    }
}
