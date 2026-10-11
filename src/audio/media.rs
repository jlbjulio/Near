use crate::bluetooth::wait;
use crate::model::{Media, now};
use windows::Media::Control::*;
#[derive(Default)]
pub struct Transport {
    manager: Option<GlobalSystemMediaTransportControlsSessionManager>,
    paused: Option<(GlobalSystemMediaTransportControlsSession, String, u64)>,
}
impl Transport {
    pub fn initialize(&mut self) {
        self.manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .ok()
            .and_then(|op| wait(op, 5).ok());
    }
    fn session(&self) -> Option<GlobalSystemMediaTransportControlsSession> {
        self.manager.as_ref()?.GetCurrentSession().ok()
    }
    pub fn read(&self) -> Media {
        let Some(s) = self.session() else {
            return Media::default();
        };
        let Ok(playback) = s.GetPlaybackInfo() else {
            return Media::default();
        };
        let Ok(controls) = playback.Controls() else {
            return Media::default();
        };
        let props = s
            .TryGetMediaPropertiesAsync()
            .ok()
            .and_then(|op| wait(op, 2).ok());
        Media {
            title: props
                .as_ref()
                .and_then(|p| p.Title().ok())
                .map(|s| s.to_string_lossy())
                .unwrap_or_default(),
            artist: props
                .and_then(|p| p.Artist().ok())
                .map(|s| s.to_string_lossy())
                .unwrap_or_default(),
            playing: playback.PlaybackStatus().ok()
                == Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing),
            previous: controls.IsPreviousEnabled().unwrap_or(false),
            next: controls.IsNextEnabled().unwrap_or(false),
            toggle: controls.IsPlayPauseToggleEnabled().unwrap_or(false),
        }
    }
    pub fn command(&mut self, command: &str) {
        self.paused = None;
        let Some(s) = self.session() else {
            return;
        };
        let op = match command {
            "previous" => s.TrySkipPreviousAsync(),
            "next" => s.TrySkipNextAsync(),
            _ => s.TryTogglePlayPauseAsync(),
        };
        if let Ok(op) = op {
            let _ = wait(op, 2);
        }
    }
    pub fn reset(&mut self) {
        self.paused = None;
    }
    fn title(s: &GlobalSystemMediaTransportControlsSession) -> String {
        s.TryGetMediaPropertiesAsync()
            .ok()
            .and_then(|op| wait(op, 2).ok())
            .and_then(|p| p.Title().ok())
            .map(|v| v.to_string_lossy())
            .unwrap_or_default()
    }
    pub fn automatic_pause(&mut self) {
        let Some(s) = self.session() else {
            return;
        };
        if s.GetPlaybackInfo().and_then(|p| p.PlaybackStatus()).ok()
            != Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
        {
            return;
        }
        let title = Self::title(&s);
        if s.TryPauseAsync().ok().and_then(|op| wait(op, 2).ok()) == Some(true) {
            self.paused = Some((s, title, now()));
        }
    }
    pub fn automatic_resume(&mut self) {
        let Some((paused, title, at)) = self.paused.take() else {
            return;
        };
        let Some(current) = self.session() else {
            return;
        };
        if current != paused
            || now().saturating_sub(at) > 60_000
            || Self::title(&current) != title
            || current
                .GetPlaybackInfo()
                .and_then(|p| p.PlaybackStatus())
                .ok()
                != Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused)
        {
            return;
        }
        if let Ok(op) = current.TryPlayAsync() {
            let _ = wait(op, 2);
        }
    }
}
pub trait EarPlayback {
    fn reset(&mut self);
    fn automatic_pause(&mut self);
    fn automatic_resume(&mut self);
}
impl EarPlayback for Transport {
    fn reset(&mut self) {
        Transport::reset(self);
    }
    fn automatic_pause(&mut self) {
        Transport::automatic_pause(self);
    }
    fn automatic_resume(&mut self) {
        Transport::automatic_resume(self);
    }
}
#[derive(Default)]
pub struct EarAutomation {
    source: u64,
    last: u64,
    previous: [Option<bool>; 2],
    ear_at: [u64; 2],
}
impl EarAutomation {
    pub fn reset(&mut self, transport: &mut impl EarPlayback) {
        *self = Self::default();
        transport.reset();
    }
    pub fn update(
        &mut self,
        reading: &crate::model::Reading,
        transport: &mut Transport,
        pause: bool,
    ) -> Option<[Option<bool>; 2]> {
        self.update_at(reading, transport, pause, now())
    }
    fn update_at(
        &mut self,
        reading: &crate::model::Reading,
        transport: &mut impl EarPlayback,
        pause: bool,
        clock: u64,
    ) -> Option<[Option<bool>; 2]> {
        if clock.saturating_sub(reading.at) > 3_000 || reading.at <= self.last {
            return None;
        }
        if self.source != reading.address
            || reading.approximate && reading.at.saturating_sub(self.last) > 10_000
        {
            self.reset(transport);
            self.source = reading.address;
        }
        let before: [Option<bool>; 2] = std::array::from_fn(|i| {
            if reading.at.saturating_sub(self.ear_at[i]) < 3_000 {
                self.previous[i]
            } else {
                None
            }
        });
        let baseline = self.last == 0;
        self.last = reading.at;
        for i in 0..2 {
            if reading.ears[i].is_some() {
                self.previous[i] = reading.ears[i];
                self.ear_at[i] = reading.at;
            }
        }
        if pause && !baseline {
            if (0..2).any(|i| before[i] == Some(true) && reading.ears[i] == Some(false)) {
                transport.automatic_pause();
            } else if (0..2).any(|i| before[i] == Some(false) && reading.ears[i] == Some(true)) {
                transport.automatic_resume();
            }
        } else if !pause {
            transport.reset();
        }
        Some(std::array::from_fn(|i| {
            if reading.at.saturating_sub(self.ear_at[i]) < 3_000 {
                self.previous[i]
            } else {
                None
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Reading;
    #[derive(Default)]
    struct Playback {
        commands: Vec<&'static str>,
    }
    impl EarPlayback for Playback {
        fn reset(&mut self) {}
        fn automatic_pause(&mut self) {
            self.commands.push("pause");
        }
        fn automatic_resume(&mut self) {
            self.commands.push("resume");
        }
    }
    fn reading(at: u64, address: u64, ears: [Option<bool>; 2]) -> Reading {
        Reading {
            address,
            model: 0x201b,
            name: String::new(),
            signal: -50,
            at,
            battery: [Some(80); 3],
            charging: [None; 3],
            ears,
            lid: None,
            approximate: true,
        }
    }
    #[test]
    fn partial_sensor_packets_expire_without_inventing_removal() {
        let mut ears = EarAutomation::default();
        let mut transport = Transport::default();
        assert_eq!(
            ears.update_at(
                &reading(1000, 1, [Some(true); 2]),
                &mut transport,
                true,
                1000
            ),
            Some([Some(true); 2])
        );
        assert_eq!(
            ears.update_at(
                &reading(1100, 1, [None, Some(false)]),
                &mut transport,
                true,
                1100
            ),
            Some([Some(true), Some(false)])
        );
        assert_eq!(
            ears.update_at(&reading(4200, 1, [None; 2]), &mut transport, true, 4200),
            Some([None; 2])
        );
    }
    #[test]
    fn changing_headphones_does_not_reuse_previous_ear_states() {
        let mut ears = EarAutomation::default();
        let mut transport = Transport::default();
        ears.update_at(
            &reading(1000, 1, [Some(false); 2]),
            &mut transport,
            true,
            1000,
        );
        assert_eq!(
            ears.update_at(
                &reading(1100, 2, [None, Some(true)]),
                &mut transport,
                true,
                1100
            ),
            Some([None, Some(true)])
        );
    }
    #[test]
    fn stale_or_duplicate_packets_do_not_overwrite_sensor_state() {
        let mut ears = EarAutomation::default();
        let mut transport = Transport::default();
        assert_eq!(
            ears.update_at(
                &reading(1000, 1, [Some(false); 2]),
                &mut transport,
                true,
                5000
            ),
            None
        );
        ears.update_at(
            &reading(5000, 1, [Some(true); 2]),
            &mut transport,
            true,
            5000,
        );
        assert_eq!(
            ears.update_at(
                &reading(5000, 1, [Some(false); 2]),
                &mut transport,
                true,
                5001
            ),
            None
        );
        assert_eq!(
            ears.update_at(
                &reading(4999, 1, [Some(false); 2]),
                &mut transport,
                true,
                5001
            ),
            None
        );
        assert_eq!(
            ears.update_at(
                &reading(5002, 1, [None, Some(false)]),
                &mut transport,
                true,
                5002
            ),
            Some([Some(true), Some(false)])
        );
    }
    #[test]
    fn a_long_sensor_gap_starts_a_new_baseline() {
        let mut ears = EarAutomation::default();
        let mut transport = Transport::default();
        ears.update_at(
            &reading(1000, 1, [Some(true); 2]),
            &mut transport,
            true,
            1000,
        );
        assert_eq!(
            ears.update_at(
                &reading(12000, 1, [Some(false), None]),
                &mut transport,
                true,
                12000
            ),
            Some([Some(false), None])
        );
    }
    #[test]
    fn either_single_earbud_pauses_and_resumes_without_the_other_sensor() {
        for side in 0..2 {
            let mut ears = EarAutomation::default();
            let mut playback = Playback::default();
            let mut worn = [None; 2];
            worn[side] = Some(true);
            ears.update_at(&reading(1000, 1, worn), &mut playback, true, 1000);
            assert!(playback.commands.is_empty());
            let mut removed = worn;
            removed[side] = Some(false);
            ears.update_at(&reading(1100, 1, removed), &mut playback, true, 1100);
            assert_eq!(playback.commands, ["pause"]);
            ears.update_at(&reading(1200, 1, worn), &mut playback, true, 1200);
            assert_eq!(playback.commands, ["pause", "resume"]);
        }
    }
    #[test]
    fn missing_sensor_data_and_disabled_detection_send_no_playback_commands() {
        for enabled in [false, true] {
            let mut ears = EarAutomation::default();
            let mut playback = Playback::default();
            ears.update_at(
                &reading(1000, 1, [Some(true), None]),
                &mut playback,
                enabled,
                1000,
            );
            ears.update_at(&reading(1100, 1, [None; 2]), &mut playback, enabled, 1100);
            if !enabled {
                ears.update_at(
                    &reading(1200, 1, [Some(false), None]),
                    &mut playback,
                    enabled,
                    1200,
                );
            }
            assert!(playback.commands.is_empty());
        }
    }
    #[test]
    fn expired_individual_sensors_do_not_trigger_playback() {
        for before in [false, true] {
            let mut ears = EarAutomation::default();
            let mut playback = Playback::default();
            ears.update_at(
                &reading(1000, 1, [Some(before), None]),
                &mut playback,
                true,
                1000,
            );
            ears.update_at(&reading(3000, 1, [None; 2]), &mut playback, true, 3000);
            ears.update_at(
                &reading(4100, 1, [Some(!before), None]),
                &mut playback,
                true,
                4100,
            );
            assert!(
                playback.commands.is_empty(),
                "expired sensor triggered {:?}",
                playback.commands
            );
        }
    }
    #[test]
    fn airpods_4_single_sensor_frames_reach_playback_automation() {
        use crate::bluetooth::{advertisement, selection};
        use crate::model::Paired;
        for primary_left in [false, true] {
            let paired = Paired {
                address: 1,
                model: Some(0x201b),
                ..Default::default()
            };
            let mut frame = [0u8; 27];
            frame[..9].copy_from_slice(&[7, 25, 1, 0x1b, 0x20, 0, 0x98, 0x09, 0]);
            let in_ear = 2;
            let side = usize::from(!primary_left);
            frame[5] = if primary_left { 0x20 } else { 0 } | in_ear;
            let worn = advertisement::parse(&frame, 1, -50, 1000).unwrap();
            assert_eq!(worn.ears[side], Some(true));
            assert_eq!(worn.ears[1 - side], None);
            frame[5] &= !in_ear;
            let removed = advertisement::parse(&frame, 1, -50, 1100).unwrap();
            assert_eq!(removed.ears[side], Some(false));
            let mut ears = EarAutomation::default();
            let mut playback = Playback::default();
            for r in [worn, removed] {
                let selected = selection::select(Some(&paired), &[r]);
                assert!(selected.sensor_reliable);
                let reading = selected.reading.unwrap();
                ears.update_at(&reading, &mut playback, true, reading.at);
            }
            assert_eq!(playback.commands, ["pause"]);
        }
    }
}
