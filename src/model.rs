use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Near")
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Preferences {
    #[serde(alias = "Flyout")]
    pub popup: bool,
    #[serde(alias = "Motion")]
    pub motion: bool,
    #[serde(alias = "LowBattery")]
    pub low_battery: bool,
    #[serde(alias = "EarDetection")]
    pub ear_detection: bool,
    #[serde(alias = "SpeakerSwitch")]
    pub speaker_switch: bool,
    #[serde(alias = "PreferredAddress")]
    pub preferred_address: u64,
    #[serde(alias = "SpeakersId")]
    pub speakers_id: Option<String>,
    pub disconnected_addresses: Vec<u64>,
}
#[derive(Clone, Debug)]
pub enum PreferenceChange {
    TogglePopup,
    ToggleMotion,
    ToggleLowBattery,
    ToggleEarDetection,
    ToggleSpeakerSwitch,
    Speakers(Option<String>),
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            popup: true,
            motion: true,
            low_battery: true,
            ear_detection: true,
            speaker_switch: true,
            preferred_address: 0,
            speakers_id: None,
            disconnected_addresses: vec![],
        }
    }
}
impl Preferences {
    pub fn apply(&mut self, change: PreferenceChange) {
        match change {
            PreferenceChange::TogglePopup => self.popup = !self.popup,
            PreferenceChange::ToggleMotion => self.motion = !self.motion,
            PreferenceChange::ToggleLowBattery => self.low_battery = !self.low_battery,
            PreferenceChange::ToggleEarDetection => self.ear_detection = !self.ear_detection,
            PreferenceChange::ToggleSpeakerSwitch => self.speaker_switch = !self.speaker_switch,
            PreferenceChange::Speakers(value) => self.speakers_id = value,
        }
    }
    pub fn load() -> Self {
        std::fs::read(data_dir().join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_slice(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(data_dir())?;
        let tmp = data_dir().join("settings.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        // MoveFileEx replaces atomically on Windows, including existing settings.
        crate::shell::replace_file(&tmp, &data_dir().join("settings.json"))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paired {
    pub address: u64,
    pub name: String,
    pub connected: bool,
    pub available: bool,
    pub model: Option<u16>,
    pub container: Option<windows::core::GUID>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub address: u64,
    pub model: u16,
    pub name: String,
    pub signal: i16,
    pub at: u64,
    pub battery: [Option<u8>; 3],
    pub charging: [Option<bool>; 3],
    pub ears: [Option<bool>; 2],
    pub lid: Option<bool>,
    pub approximate: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Sample {
    pub level: u8,
    pub at: u64,
    pub charging: Option<bool>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Batteries {
    pub parts: [Option<Sample>; 3],
    pub approximate: bool,
}
impl Batteries {
    pub fn remember(&mut self, r: &Reading) {
        self.approximate = r.approximate;
        for i in 0..3 {
            if let Some(level) = r.battery[i] {
                if self.parts[i].is_none_or(|old| r.at >= old.at) {
                    self.parts[i] = Some(Sample {
                        level,
                        at: r.at,
                        charging: r.charging[i],
                    });
                }
            } else if let Some(old) = &mut self.parts[i] {
                if r.at >= old.at {
                    old.charging = None;
                }
            }
        }
    }
    pub fn expire(&mut self, t: u64) {
        for part in &mut self.parts {
            if part.is_some_and(|v| t.saturating_sub(v.at) > 600_000) {
                *part = None;
            } else if let Some(sample) = part {
                if t.saturating_sub(sample.at) > 15_000 {
                    sample.charging = None;
                }
            }
        }
    }
    pub fn value(&self, i: usize) -> String {
        self.parts[i]
            .map(|p| format!("{}{}%", if self.approximate { "≈" } else { "" }, p.level))
            .unwrap_or_else(|| "—".into())
    }
    pub fn tray_level(&self) -> Option<u8> {
        self.parts[..2].iter().flatten().map(|s| s.level).min()
    }
    pub fn charging(&self) -> bool {
        self.parts[..2]
            .iter()
            .flatten()
            .any(|s| s.charging == Some(true))
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub instance: String,
    pub container: Option<windows::core::GUID>,
    pub speakers: bool,
    pub state: u32,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Media {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub previous: bool,
    pub next: bool,
    pub toggle: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ViewModel {
    pub preferences: Preferences,
    pub paired: Vec<Paired>,
    pub chosen: Option<Paired>,
    pub nearby: Vec<Reading>,
    pub batteries: Batteries,
    pub model: u16,
    pub name: String,
    pub state: String,
    pub note: String,
    pub media: Media,
    pub endpoints: Vec<Endpoint>,
    pub message: String,
    pub bluetooth: bool,
    pub popup_revision: u64,
    pub alert_revision: u64,
    pub alert: String,
    pub connecting: bool,
    pub disconnecting: bool,
    pub available: bool,
}
impl PartialEq for Preferences {
    fn eq(&self, o: &Self) -> bool {
        self.popup == o.popup
            && self.motion == o.motion
            && self.low_battery == o.low_battery
            && self.ear_detection == o.ear_detection
            && self.speaker_switch == o.speaker_switch
            && self.preferred_address == o.preferred_address
            && self.speakers_id == o.speakers_id
            && self.disconnected_addresses == o.disconnected_addresses
    }
}
impl Default for ViewModel {
    fn default() -> Self {
        Self {
            preferences: Preferences::load(),
            paired: vec![],
            chosen: None,
            nearby: vec![],
            batteries: Batteries::default(),
            model: 0,
            name: "AirPods".into(),
            state: "Not connected".into(),
            note: "Open your case to see battery levels.".into(),
            media: Media::default(),
            endpoints: vec![],
            message: String::new(),
            bluetooth: true,
            popup_revision: 0,
            alert_revision: 0,
            alert: String::new(),
            connecting: false,
            disconnecting: false,
            available: false,
        }
    }
}
impl ViewModel {
    pub fn connection_action(&self) -> Option<(&'static str, bool)> {
        let paired = self.chosen.as_ref()?;
        if self.disconnecting {
            return Some(("Disconnecting…", false));
        }
        if self.connecting {
            return Some(("Connecting…", false));
        }
        if paired.connected {
            return Some(("Disconnect", true));
        }
        if self.available
            || self
                .preferences
                .disconnected_addresses
                .contains(&paired.address)
        {
            return Some(("Connect", true));
        }
        None
    }
}
pub enum Command {
    Connect,
    Disconnect,
    Select(u64),
    Media(&'static str),
    Preferences(PreferenceChange),
    Refresh,
    Restart,
    Quit,
}
pub enum Event {
    Reading(Reading),
    Command(Command),
    Standard(u64, String, u8),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_settings_combinations_round_trip_without_changing_device_choices() {
        for mask in 0..32 {
            let p = Preferences {
                popup: mask & 1 != 0,
                motion: mask & 2 != 0,
                low_battery: mask & 4 != 0,
                ear_detection: mask & 8 != 0,
                speaker_switch: mask & 16 != 0,
                preferred_address: 123,
                speakers_id: Some("Speakers \"Desk\" · USB".into()),
                disconnected_addresses: vec![123, 456],
            };
            let reloaded: Preferences =
                serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
            assert_eq!(reloaded, p, "settings mask {mask}");
        }
    }
    #[test]
    fn delayed_setting_commands_preserve_other_changes_and_manual_disconnects() {
        let mut p = Preferences {
            preferred_address: 456,
            disconnected_addresses: vec![123, 456],
            ..Default::default()
        };
        p.apply(PreferenceChange::Speakers(Some("desk".into())));
        p.apply(PreferenceChange::ToggleEarDetection);
        p.apply(PreferenceChange::TogglePopup);
        p.apply(PreferenceChange::ToggleMotion);
        p.apply(PreferenceChange::ToggleLowBattery);
        p.apply(PreferenceChange::ToggleSpeakerSwitch);
        assert_eq!(
            p,
            Preferences {
                popup: false,
                motion: false,
                low_battery: false,
                ear_detection: false,
                speaker_switch: false,
                preferred_address: 456,
                speakers_id: Some("desk".into()),
                disconnected_addresses: vec![123, 456],
            }
        );
        p.apply(PreferenceChange::ToggleEarDetection);
        assert!(p.ear_detection);
        assert!(!p.popup && !p.motion && !p.low_battery && !p.speaker_switch);
        p.apply(PreferenceChange::Speakers(None));
        assert_eq!(p.speakers_id, None);
        assert_eq!(p.preferred_address, 456);
        assert_eq!(p.disconnected_addresses, [123, 456]);
    }
    #[test]
    fn two_toggle_clicks_from_the_same_ui_snapshot_restore_the_setting() {
        let mut current = Preferences::default();
        for change in [
            PreferenceChange::TogglePopup,
            PreferenceChange::ToggleMotion,
            PreferenceChange::ToggleLowBattery,
            PreferenceChange::ToggleEarDetection,
            PreferenceChange::ToggleSpeakerSwitch,
        ] {
            current.apply(change.clone());
            current.apply(change);
            assert_eq!(current, Preferences::default());
        }
    }
    #[test]
    fn charging_expires_before_the_cached_battery() {
        let mut b = Batteries {
            parts: [
                Some(Sample {
                    level: 88,
                    at: 1000,
                    charging: Some(true),
                }),
                None,
                None,
            ],
            approximate: false,
        };
        b.expire(16001);
        assert_eq!(b.parts[0].unwrap().level, 88);
        assert_eq!(b.parts[0].unwrap().charging, None);
        b.expire(601001);
        assert_eq!(b.parts, [None; 3]);
    }
    #[test]
    fn old_packets_do_not_replace_new_battery_levels() {
        let mut b = Batteries::default();
        let mut r = Reading {
            address: 1,
            model: 0x201b,
            name: String::new(),
            signal: -50,
            at: 2000,
            battery: [Some(80), None, None],
            charging: [Some(true), None, None],
            ears: [None; 2],
            lid: None,
            approximate: true,
        };
        b.remember(&r);
        r.at = 1000;
        r.battery[0] = Some(10);
        b.remember(&r);
        assert_eq!(b.parts[0].unwrap().level, 80);
        r.battery[0] = None;
        b.remember(&r);
        assert_eq!(b.parts[0].unwrap().charging, Some(true));
        r.at = 3000;
        r.battery[0] = None;
        b.remember(&r);
        assert_eq!(b.parts[0].unwrap().level, 80);
        assert_eq!(b.parts[0].unwrap().charging, None);
    }
    #[test]
    fn previous_preferences_migrate_without_restoring_removed_features() {
        let p:Preferences=serde_json::from_str(r#"{"Flyout":false,"EarDetection":false,"PreferredAddress":123,"Voice":true,"Hotkeys":[]}"#).unwrap();
        assert!(!p.popup);
        assert!(!p.ear_detection);
        assert_eq!(p.preferred_address, 123);
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("Voice"));
        assert!(!json.contains("Hotkeys"));
    }
    #[test]
    fn connection_button_follows_presence_and_manual_intent() {
        let mut vm = ViewModel {
            preferences: Preferences::default(),
            ..Default::default()
        };
        assert_eq!(vm.connection_action(), None);
        vm.chosen = Some(Paired {
            address: 7,
            ..Default::default()
        });
        assert_eq!(vm.connection_action(), None);
        vm.available = true;
        assert_eq!(vm.connection_action(), Some(("Connect", true)));
        vm.chosen.as_mut().unwrap().connected = true;
        assert_eq!(vm.connection_action(), Some(("Disconnect", true)));
        vm.disconnecting = true;
        assert_eq!(vm.connection_action(), Some(("Disconnecting…", false)));
        vm.disconnecting = false;
        vm.chosen.as_mut().unwrap().connected = false;
        vm.available = false;
        vm.preferences.disconnected_addresses.push(7);
        assert_eq!(vm.connection_action(), Some(("Connect", true)));
        vm.connecting = true;
        assert_eq!(vm.connection_action(), Some(("Connecting…", false)));
        let saved = serde_json::to_string(&vm.preferences).unwrap();
        let reloaded: Preferences = serde_json::from_str(&saved).unwrap();
        assert_eq!(reloaded.disconnected_addresses, vec![7]);
    }
}
