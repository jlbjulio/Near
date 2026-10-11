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
#[derive(Default)]
pub struct EarAutomation {
    source: u64,
    last: u64,
    previous: [Option<bool>; 2],
    ear_at: [u64; 2],
}
impl EarAutomation {
    pub fn reset(&mut self, transport: &mut Transport) {
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
        transport: &mut Transport,
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
        let before = self.previous;
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
}
