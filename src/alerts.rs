use crate::model::Batteries;

#[derive(Default)]
pub struct LowBattery {
    device: Option<u64>,
    notified: [bool; 3],
}
impl LowBattery {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn update(
        &mut self,
        device: u64,
        batteries: &Batteries,
        enabled: bool,
        headphones: bool,
        clock: u64,
    ) -> Option<String> {
        if !enabled {
            self.reset();
            return None;
        }
        if self.device != Some(device) {
            self.reset();
            self.device = Some(device);
        }
        let mut changed = false;
        let mut parts = Vec::new();
        for (i, sample) in batteries.parts.iter().enumerate() {
            let Some(sample) = sample else { continue };
            if sample.at > clock || clock - sample.at > 15_000 {
                continue;
            }
            if sample.charging == Some(true) || sample.level >= 20 {
                self.notified[i] = false;
                continue;
            }
            if sample.level > 10 {
                continue;
            }
            if !self.notified[i] {
                self.notified[i] = true;
                changed = true;
            }
            let label = if headphones && i == 0 {
                "Headphones"
            } else {
                ["Left", "Right", "Case"][i]
            };
            parts.push(format!("{label} {}", batteries.value(i)));
        }
        changed.then(|| parts.join(" · "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Sample;
    fn batteries(levels: [Option<u8>; 3], at: u64) -> Batteries {
        Batteries {
            parts: levels.map(|level| {
                level.map(|level| Sample {
                    level,
                    at,
                    charging: Some(false),
                })
            }),
            approximate: true,
        }
    }
    #[test]
    fn only_ten_percent_alerts_once_without_threshold_jitter() {
        let mut alerts = LowBattery::default();
        for (level, notify) in [
            (30, false),
            (20, false),
            (11, false),
            (10, true),
            (11, false),
            (10, false),
            (0, false),
        ] {
            let result = alerts.update(
                1,
                &batteries([Some(level), None, None], 1000),
                true,
                false,
                1000,
            );
            assert_eq!(result.is_some(), notify, "level {level}");
        }
    }
    #[test]
    fn starting_below_ten_emits_one_warning_instead_of_two() {
        let mut alerts = LowBattery::default();
        let low = batteries([None, Some(0), None], 1000);
        assert_eq!(
            alerts.update(1, &low, true, false, 1000).as_deref(),
            Some("Right ≈0%")
        );
        assert_eq!(alerts.update(1, &low, true, false, 1001), None);
    }
    #[test]
    fn low_parts_are_combined_instead_of_overwriting_each_other() {
        let mut alerts = LowBattery::default();
        let mut low = batteries([Some(10), Some(20), Some(10)], 1000);
        assert_eq!(
            alerts.update(1, &low, true, false, 1000).as_deref(),
            Some("Left ≈10% · Case ≈10%")
        );
        low.parts[1].as_mut().unwrap().level = 10;
        assert_eq!(
            alerts.update(1, &low, true, false, 1000).as_deref(),
            Some("Left ≈10% · Right ≈10% · Case ≈10%")
        );
        assert_eq!(alerts.update(1, &low, true, false, 1001), None);
    }
    #[test]
    fn charging_or_recovery_rearms_an_alert_but_missing_data_does_not() {
        let mut alerts = LowBattery::default();
        let mut low = batteries([Some(10), None, None], 1000);
        assert!(alerts.update(1, &low, true, false, 1000).is_some());
        assert_eq!(
            alerts.update(1, &Batteries::default(), true, false, 1001),
            None
        );
        assert_eq!(alerts.update(1, &low, true, false, 1002), None);
        for charging in [true, false] {
            low.parts[0].as_mut().unwrap().charging = Some(charging);
            low.parts[0].as_mut().unwrap().level = if charging { 10 } else { 20 };
            assert_eq!(alerts.update(1, &low, true, false, 1003), None);
            low.parts[0].as_mut().unwrap().charging = Some(false);
            low.parts[0].as_mut().unwrap().level = 10;
            assert!(alerts.update(1, &low, true, false, 1004).is_some());
        }
    }
    #[test]
    fn disabled_stale_and_future_observations_do_not_alert() {
        let mut alerts = LowBattery::default();
        let low = batteries([Some(10), None, None], 1000);
        assert_eq!(alerts.update(1, &low, false, false, 1000), None);
        assert_eq!(alerts.update(1, &low, true, false, 16001), None);
        assert_eq!(alerts.update(1, &low, true, false, 999), None);
        assert!(alerts.update(1, &low, true, false, 1000).is_some());
        assert_eq!(alerts.update(1, &low, false, false, 1000), None);
        assert!(alerts.update(1, &low, true, false, 1000).is_some());
    }
    #[test]
    fn another_device_has_its_own_alert_and_exact_levels_keep_their_format() {
        let mut alerts = LowBattery::default();
        let mut low = batteries([Some(7), None, None], 1000);
        low.approximate = false;
        assert_eq!(
            alerts.update(1, &low, true, true, 1000).as_deref(),
            Some("Headphones 7%")
        );
        assert_eq!(alerts.update(1, &low, true, true, 1001), None);
        assert_eq!(
            alerts.update(2, &low, true, true, 1002).as_deref(),
            Some("Headphones 7%")
        );
    }
}
