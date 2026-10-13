use super::{default_device, matches, set_default};
use crate::model::{Endpoint, Paired};
use std::collections::HashMap;
use windows::Win32::Media::Audio::{
    EDataFlow, ERole, eCapture, eCommunications, eConsole, eMultimedia, eRender,
};

fn virtual_device(e: &Endpoint) -> bool {
    let text = format!("{} {}", e.name, e.instance).to_ascii_lowercase();
    [
        "steelseries sonar",
        "steelseriessonar",
        "voicemeeter",
        "vb-audio",
        "virtual audio",
        "root\\sonar",
    ]
    .iter()
    .any(|part| text.contains(part))
}
fn speakers<'a>(
    outputs: &'a [Endpoint],
    paired: &Paired,
    preferred: Option<&str>,
) -> Option<&'a Endpoint> {
    let eligible = |e: &&Endpoint| is_speaker_target(e, Some(paired));
    outputs
        .iter()
        .filter(eligible)
        .find(|e| Some(e.id.as_str()) == preferred)
        .or_else(|| outputs.iter().filter(eligible).find(|e| e.speakers))
}
fn selected_headset<'a>(
    outputs: &'a [Endpoint],
    paired: &Paired,
    current: Option<&str>,
) -> Option<&'a Endpoint> {
    outputs.iter().find(|e| {
        Some(e.id.as_str()) == current && e.state == 1 && !virtual_device(e) && matches(e, paired)
    })
}
pub fn is_speaker_target(e: &Endpoint, paired: Option<&Paired>) -> bool {
    e.state == 1
        && !virtual_device(e)
        && paired.is_none_or(|p| !matches(e, p))
        && !e.instance.to_ascii_uppercase().contains("BTH")
}

trait Routes {
    fn current(&self, flow: EDataFlow, role: ERole) -> Option<String>;
    fn select(&mut self, flow: EDataFlow, role: ERole, id: &str) -> bool;
}
struct WindowsRoutes;
impl Routes for WindowsRoutes {
    fn current(&self, flow: EDataFlow, role: ERole) -> Option<String> {
        default_device(flow, role).ok()
    }
    fn select(&mut self, _flow: EDataFlow, role: ERole, id: &str) -> bool {
        set_default(id, role).is_ok()
    }
}

#[derive(Default)]
pub struct Router {
    previous: HashMap<i32, String>,
    last: String,
    direct: HashMap<(i32, i32), String>,
}
impl Router {
    pub fn reset(&mut self) {
        self.previous.clear();
        self.last.clear();
        self.direct.clear();
    }
    pub fn update(
        &mut self,
        ears: [Option<bool>; 2],
        paired: &Paired,
        outputs: &[Endpoint],
        preferred: Option<&str>,
    ) {
        self.update_with(ears, paired, outputs, preferred, &mut WindowsRoutes);
    }
    fn update_with(
        &mut self,
        ears: [Option<bool>; 2],
        paired: &Paired,
        outputs: &[Endpoint],
        preferred: Option<&str>,
        routes: &mut impl Routes,
    ) {
        if ears.contains(&Some(true)) && !self.previous.is_empty() {
            for (role, id) in std::mem::take(&mut self.previous) {
                let current = routes.current(eRender, ERole(role));
                if current.as_deref() == Some(&self.last)
                    && outputs
                        .iter()
                        .any(|e| e.id == self.last && !virtual_device(e))
                    && selected_headset(outputs, paired, Some(&id)).is_some()
                {
                    routes.select(eRender, ERole(role), &id);
                }
            }
            self.last.clear();
            return;
        }
        if ears != [Some(false); 2] || !self.previous.is_empty() {
            return;
        }
        let Some(target) = speakers(outputs, paired, preferred) else {
            return;
        };
        for role in [eConsole, eMultimedia] {
            let current = routes.current(eRender, role);
            if let Some(headset) = selected_headset(outputs, paired, current.as_deref()) {
                if routes.select(eRender, role, &target.id) {
                    self.previous.insert(role.0, headset.id.clone());
                    self.last = target.id.clone();
                }
            }
        }
    }
    pub fn disconnected(
        &mut self,
        paired: &Paired,
        outputs: &[Endpoint],
        inputs: &[Endpoint],
        preferred: Option<&str>,
    ) {
        self.disconnected_with(paired, outputs, inputs, preferred, &mut WindowsRoutes);
    }
    fn disconnected_with(
        &mut self,
        paired: &Paired,
        outputs: &[Endpoint],
        inputs: &[Endpoint],
        preferred: Option<&str>,
        routes: &mut impl Routes,
    ) {
        for (flow, devices) in [(eRender, outputs), (eCapture, inputs)] {
            for role in [eConsole, eMultimedia, eCommunications] {
                let key = (flow.0, role.0);
                let current = routes.current(flow, role);
                let source = if let Some(id) = &current {
                    let physical = devices
                        .iter()
                        .find(|e| e.id == *id && !virtual_device(e) && matches(e, paired));
                    let Some(physical) = physical else {
                        self.direct.remove(&key);
                        continue;
                    };
                    self.direct.insert(key, id.clone());
                    Some(physical)
                } else {
                    self.direct
                        .get(&key)
                        .and_then(|id| devices.iter().find(|e| e.id == *id))
                };
                if source.is_some_and(|e| e.state == 1) || !self.direct.contains_key(&key) {
                    continue;
                }
                let target = if flow == eRender {
                    speakers(outputs, paired, preferred)
                } else {
                    inputs.iter().find(|e| {
                        e.state == 1
                            && !virtual_device(e)
                            && !matches(e, paired)
                            && !e.instance.to_ascii_uppercase().contains("BTH")
                    })
                };
                if let Some(target) = target {
                    if routes.select(flow, role, &target.id) {
                        self.direct.remove(&key);
                        if flow == eRender {
                            self.previous.remove(&role.0);
                        }
                    }
                }
            }
        }
    }
}

pub struct ConnectionRoutes(Vec<(EDataFlow, ERole, String)>);
impl ConnectionRoutes {
    pub fn capture(paired: &Paired) -> Self {
        let outputs = super::outputs(1).unwrap_or_default();
        let inputs = super::inputs(1).unwrap_or_default();
        Self::capture_with(paired, &outputs, &inputs, &WindowsRoutes)
    }
    fn capture_with(
        paired: &Paired,
        outputs: &[Endpoint],
        inputs: &[Endpoint],
        routes: &impl Routes,
    ) -> Self {
        let mut saved = vec![];
        for (flow, devices) in [(eRender, outputs), (eCapture, inputs)] {
            for role in [eConsole, eMultimedia, eCommunications] {
                if let Some(id) = routes.current(flow, role) {
                    if devices
                        .iter()
                        .any(|e| e.id == id && e.state == 1 && !matches(e, paired))
                    {
                        saved.push((flow, role, id));
                    }
                }
            }
        }
        Self(saved)
    }
    pub fn restore(&mut self, paired: &Paired, outputs: &[Endpoint], inputs: &[Endpoint]) {
        self.restore_with(paired, outputs, inputs, &mut WindowsRoutes);
    }
    fn restore_with(
        &mut self,
        paired: &Paired,
        outputs: &[Endpoint],
        inputs: &[Endpoint],
        routes: &mut impl Routes,
    ) {
        self.0.retain(|(flow, role, saved)| {
            let devices = if *flow == eRender { outputs } else { inputs };
            let current = routes.current(*flow, *role);
            if !devices.iter().any(|e| e.id == *saved && e.state == 1) {
                return false;
            }
            if current.as_deref().is_none_or(|id| id == saved) {
                return true;
            }
            if devices.iter().any(|e| {
                Some(e.id.as_str()) == current.as_deref()
                    && !virtual_device(e)
                    && matches(e, paired)
            }) {
                return !routes.select(*flow, *role, saved);
            }
            false
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::GUID;
    #[derive(Default)]
    struct FakeRoutes {
        defaults: HashMap<(i32, i32), String>,
        writes: Vec<(i32, i32, String)>,
    }
    impl Routes for FakeRoutes {
        fn current(&self, flow: EDataFlow, role: ERole) -> Option<String> {
            self.defaults.get(&(flow.0, role.0)).cloned()
        }
        fn select(&mut self, flow: EDataFlow, role: ERole, id: &str) -> bool {
            self.defaults.insert((flow.0, role.0), id.into());
            self.writes.push((flow.0, role.0, id.into()));
            true
        }
    }
    fn fixture() -> (Paired, Vec<Endpoint>, Vec<Endpoint>, FakeRoutes) {
        let container = GUID::from_u128(1);
        let paired = Paired {
            address: 1,
            connected: true,
            container: Some(container),
            ..Default::default()
        };
        let outputs = vec![
            Endpoint {
                id: "sonar".into(),
                name: "SteelSeries Sonar - Gaming (SteelSeries Sonar Virtual Audio Device)".into(),
                speakers: true,
                state: 1,
                ..Default::default()
            },
            Endpoint {
                id: "pods".into(),
                name: "AirPods".into(),
                container: Some(container),
                state: 1,
                ..Default::default()
            },
            Endpoint {
                id: "speakers".into(),
                name: "Speakers".into(),
                speakers: true,
                state: 1,
                ..Default::default()
            },
        ];
        let inputs = vec![
            Endpoint {
                id: "sonar-mic".into(),
                name: "SteelSeries Sonar - Microphone".into(),
                state: 1,
                ..Default::default()
            },
            Endpoint {
                id: "pods-mic".into(),
                container: Some(container),
                state: 1,
                ..Default::default()
            },
            Endpoint {
                id: "mic".into(),
                name: "Microphone Array".into(),
                state: 1,
                ..Default::default()
            },
        ];
        let mut routes = FakeRoutes::default();
        for role in [eConsole, eMultimedia, eCommunications] {
            routes.defaults.insert((eRender.0, role.0), "sonar".into());
            routes
                .defaults
                .insert((eCapture.0, role.0), "sonar-mic".into());
        }
        (paired, outputs, inputs, routes)
    }
    #[test]
    fn sonar_defaults_are_untouched_by_ears_and_disconnection() {
        let (paired, mut outputs, mut inputs, mut routes) = fixture();
        let before = routes.defaults.clone();
        let mut router = Router::default();
        router.update_with([Some(false); 2], &paired, &outputs, None, &mut routes);
        outputs[1].state = 8;
        inputs[1].state = 8;
        router.disconnected_with(&paired, &outputs, &inputs, None, &mut routes);
        router.update_with([Some(true); 2], &paired, &outputs, None, &mut routes);
        assert_eq!(routes.defaults, before);
        assert!(routes.writes.is_empty());
    }
    #[test]
    fn one_worn_earbud_or_an_unknown_sensor_keeps_the_airpods_output() {
        let (paired, outputs, _, mut routes) = fixture();
        routes
            .defaults
            .insert((eRender.0, eConsole.0), "pods".into());
        let mut router = Router::default();
        for state in [
            [Some(true), Some(false)],
            [Some(false), Some(true)],
            [None, Some(false)],
            [Some(false), None],
            [None; 2],
        ] {
            router.update_with(state, &paired, &outputs, None, &mut routes);
            assert_eq!(routes.current(eRender, eConsole).as_deref(), Some("pods"));
            assert!(routes.writes.is_empty());
        }
        router.update_with([Some(false); 2], &paired, &outputs, None, &mut routes);
        assert_eq!(
            routes.current(eRender, eConsole).as_deref(),
            Some("speakers")
        );
        assert_eq!(routes.writes.len(), 1);
    }
    #[test]
    fn direct_airpods_switch_to_physical_speakers_and_restore_only_owned_routes() {
        let (paired, outputs, _, mut routes) = fixture();
        routes
            .defaults
            .insert((eRender.0, eConsole.0), "pods".into());
        let mut router = Router::default();
        router.update_with(
            [Some(false); 2],
            &paired,
            &outputs,
            Some("sonar"),
            &mut routes,
        );
        assert_eq!(
            routes.current(eRender, eConsole).as_deref(),
            Some("speakers")
        );
        assert_eq!(
            routes.current(eRender, eMultimedia).as_deref(),
            Some("sonar")
        );
        routes
            .defaults
            .insert((eRender.0, eConsole.0), "sonar".into());
        router.update_with([Some(true); 2], &paired, &outputs, None, &mut routes);
        assert_eq!(routes.current(eRender, eConsole).as_deref(), Some("sonar"));
        assert_eq!(routes.writes.len(), 1);
    }
    #[test]
    fn an_owned_physical_speaker_route_can_resume_on_airpods() {
        let (paired, outputs, _, mut routes) = fixture();
        routes
            .defaults
            .insert((eRender.0, eConsole.0), "pods".into());
        let mut router = Router::default();
        router.update_with([Some(false); 2], &paired, &outputs, None, &mut routes);
        router.update_with([Some(true), None], &paired, &outputs, None, &mut routes);
        assert_eq!(routes.current(eRender, eConsole).as_deref(), Some("pods"));
        assert_eq!(routes.writes.len(), 2);
    }
    #[test]
    fn disconnected_direct_microphone_falls_back_without_changing_sonar_output() {
        let (paired, outputs, mut inputs, mut routes) = fixture();
        routes
            .defaults
            .insert((eCapture.0, eConsole.0), "pods-mic".into());
        inputs[1].state = 8;
        Router::default().disconnected_with(&paired, &outputs, &inputs, None, &mut routes);
        assert_eq!(routes.current(eCapture, eConsole).as_deref(), Some("mic"));
        assert_eq!(routes.current(eRender, eConsole).as_deref(), Some("sonar"));
        assert_eq!(routes.writes, vec![(eCapture.0, eConsole.0, "mic".into())]);
    }
    #[test]
    fn microphone_disconnection_preserves_an_owned_output_route() {
        let (paired, outputs, mut inputs, mut routes) = fixture();
        routes
            .defaults
            .insert((eRender.0, eConsole.0), "pods".into());
        routes
            .defaults
            .insert((eCapture.0, eConsole.0), "pods-mic".into());
        let mut router = Router::default();
        router.update_with([Some(false); 2], &paired, &outputs, None, &mut routes);
        inputs[1].state = 8;
        router.disconnected_with(&paired, &outputs, &inputs, None, &mut routes);
        router.update_with([Some(true); 2], &paired, &outputs, None, &mut routes);
        assert_eq!(routes.current(eRender, eConsole).as_deref(), Some("pods"));
        assert_eq!(routes.current(eCapture, eConsole).as_deref(), Some("mic"));
        assert_eq!(
            routes.current(eRender, eMultimedia).as_deref(),
            Some("sonar")
        );
    }
    #[test]
    fn missing_default_after_disconnect_falls_back_only_if_airpods_were_selected() {
        let (paired, mut outputs, inputs, mut routes) = fixture();
        routes
            .defaults
            .insert((eRender.0, eConsole.0), "pods".into());
        let mut router = Router::default();
        router.disconnected_with(&paired, &outputs, &inputs, None, &mut routes);
        outputs[1].state = 8;
        routes.defaults.remove(&(eRender.0, eConsole.0));
        router.disconnected_with(&paired, &outputs, &inputs, None, &mut routes);
        assert_eq!(
            routes.current(eRender, eConsole).as_deref(),
            Some("speakers")
        );
        assert_eq!(routes.writes.len(), 1);
    }
    #[test]
    fn connection_restores_sonar_only_when_windows_switched_to_airpods() {
        let (paired, outputs, inputs, mut routes) = fixture();
        let mut saved = ConnectionRoutes::capture_with(&paired, &outputs, &inputs, &routes);
        routes
            .defaults
            .insert((eRender.0, eConsole.0), "pods".into());
        routes
            .defaults
            .insert((eCapture.0, eConsole.0), "pods-mic".into());
        routes
            .defaults
            .insert((eRender.0, eMultimedia.0), "speakers".into());
        saved.restore_with(&paired, &outputs, &inputs, &mut routes);
        assert_eq!(routes.current(eRender, eConsole).as_deref(), Some("sonar"));
        assert_eq!(
            routes.current(eCapture, eConsole).as_deref(),
            Some("sonar-mic")
        );
        assert_eq!(
            routes.current(eRender, eMultimedia).as_deref(),
            Some("speakers")
        );
        assert_eq!(routes.writes.len(), 2);
    }
    #[test]
    fn keepalive_does_not_bypass_a_virtual_output() {
        let (paired, outputs, _, _) = fixture();
        assert!(selected_headset(&outputs, &paired, Some("sonar")).is_none());
        assert!(selected_headset(&outputs, &paired, Some("speakers")).is_none());
        assert_eq!(
            selected_headset(&outputs, &paired, Some("pods"))
                .unwrap()
                .id,
            "pods"
        );
    }
    #[test]
    fn a_delayed_windows_default_change_still_preserves_the_mixer() {
        let (paired, outputs, inputs, mut routes) = fixture();
        let mut saved = ConnectionRoutes::capture_with(&paired, &outputs, &inputs, &routes);
        saved.restore_with(&paired, &outputs, &inputs, &mut routes);
        assert!(routes.writes.is_empty());
        routes
            .defaults
            .insert((eRender.0, eConsole.0), "pods".into());
        saved.restore_with(&paired, &outputs, &inputs, &mut routes);
        assert_eq!(routes.current(eRender, eConsole).as_deref(), Some("sonar"));
    }
}
