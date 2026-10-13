use crate::model::{Paired, Reading};

pub struct Selection {
    pub reading: Option<Reading>,
    pub sensor_reliable: bool,
    pub uncertain: bool,
}

pub fn select(paired: Option<&Paired>, fresh: &[Reading]) -> Selection {
    let exact = paired.and_then(|p| fresh.iter().find(|r| r.address == p.address));
    if let Some(r) = exact {
        return Selection {
            reading: Some(r.clone()),
            sensor_reliable: true,
            uncertain: false,
        };
    }
    let candidates: Vec<_> = fresh
        .iter()
        .filter(|r| paired.is_none_or(|p| p.model == Some(r.model)))
        .collect();
    let latest = candidates.iter().map(|r| r.at).max().unwrap_or(0);
    let candidates: Vec<_> = candidates
        .into_iter()
        .filter(|r| latest.saturating_sub(r.at) <= 1_500)
        .collect();
    let Some(first) = candidates.first() else {
        return Selection {
            reading: None,
            sensor_reliable: false,
            uncertain: false,
        };
    };
    if candidates.len() == 1 {
        return Selection {
            reading: Some((*first).clone()),
            sensor_reliable: first.signal >= -75,
            uncertain: false,
        };
    }
    let mut merged = (*first).clone();
    for other in candidates.iter().skip(1) {
        let shared =
            (0..3).any(|i| merged.battery[i].is_some() && merged.battery[i] == other.battery[i]);
        let conflict = (0..3).any(|i| {
            matches!((merged.battery[i], other.battery[i]), (Some(a), Some(b)) if a != b)
                || matches!((merged.charging[i], other.charging[i]), (Some(a), Some(b)) if a != b)
        });
        if merged.model != other.model || !shared || conflict {
            return Selection {
                reading: None,
                sensor_reliable: false,
                uncertain: true,
            };
        }
        for i in 0..3 {
            merged.battery[i] = merged.battery[i].or(other.battery[i]);
            merged.charging[i] = merged.charging[i].or(other.charging[i]);
        }
        merged.at = merged.at.max(other.at);
        merged.signal = merged.signal.max(other.signal);
        // Any recent frame with an earbud out of the case counts.
        merged.cased &= other.cased;
        if merged.lid != other.lid {
            merged.lid = None;
        }
    }
    // Matching battery observations do not establish ownership of ear sensors.
    merged.ears = [None; 2];
    Selection {
        reading: Some(merged),
        sensor_reliable: false,
        uncertain: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reading(address: u64, at: u64) -> Reading {
        Reading {
            address,
            model: 0x201b,
            name: "AirPods 4".into(),
            signal: -45,
            at,
            battery: [Some(80), Some(90), Some(70)],
            charging: [Some(false); 3],
            ears: [Some(true); 2],
            lid: None,
            cased: false,
            approximate: true,
        }
    }
    fn paired() -> Paired {
        Paired {
            address: 99,
            model: Some(0x201b),
            ..Default::default()
        }
    }
    #[test]
    fn any_earbud_out_of_the_case_clears_the_merged_case_state() {
        let mut cased = reading(1, 10_000);
        cased.cased = true;
        let mut other = reading(2, 10_100);
        other.cased = true;
        let both = select(Some(&paired()), &[cased.clone(), other.clone()]);
        assert!(both.reading.unwrap().cased);
        for order in [
            [cased.clone(), reading(2, 10_100)],
            [reading(2, 10_100), cased.clone()],
        ] {
            assert!(!select(Some(&paired()), &order).reading.unwrap().cased);
        }
    }
    #[test]
    fn duplicate_private_advertisers_do_not_imply_multiple_airpods() {
        let result = select(Some(&paired()), &[reading(1, 10_000), reading(2, 10_100)]);
        assert!(!result.uncertain);
        assert_eq!(
            result.reading.unwrap().battery,
            [Some(80), Some(90), Some(70)]
        );
        assert!(!result.sensor_reliable);
    }
    #[test]
    fn rotation_does_not_block_a_newer_battery_update() {
        let old = reading(1, 10_000);
        let mut new = reading(2, 12_000);
        new.battery[0] = Some(90);
        let result = select(Some(&paired()), &[old, new]);
        assert_eq!(result.reading.unwrap().battery[0], Some(90));
    }
    #[test]
    fn conflicting_or_unrelated_observations_are_not_guessed() {
        let a = reading(1, 10_000);
        let mut b = reading(2, 10_100);
        b.battery[0] = Some(30);
        let result = select(Some(&paired()), &[a, b]);
        assert!(result.uncertain && result.reading.is_none());
        let mut other = reading(3, 10_100);
        other.model = 0x2014;
        assert!(select(Some(&paired()), &[other]).reading.is_none());
    }
    #[test]
    fn known_address_wins_over_other_pairs() {
        let ours = reading(99, 10_000);
        let mut other = reading(1, 10_100);
        other.battery[0] = Some(20);
        let result = select(Some(&paired()), &[ours, other]);
        assert_eq!(result.reading.unwrap().address, 99);
        assert!(result.sensor_reliable);
    }
}
