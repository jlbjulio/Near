use crate::{
    audio::{
        self,
        keep_alive::KeepAlive,
        media::{EarAutomation, Transport},
    },
    bluetooth::{self, advertisement},
    model::*,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, SyncSender},
    },
    time::Duration,
};

pub fn run(
    shared: Arc<Mutex<ViewModel>>,
    rx: Receiver<Event>,
    tx: SyncSender<Event>,
    window: usize,
) {
    let _apartment = crate::shell::Apartment::mta();
    let mut prefs = shared
        .lock()
        .map(|v| v.preferences.clone())
        .unwrap_or_default();
    let mut scanner = bluetooth::Scanner::start(tx.clone()).ok();
    let mut nearby: HashMap<u64, Reading> = HashMap::new();
    let mut history: History = HashMap::new();
    let mut lids: HashMap<u64, (bool, u64)> = HashMap::new();
    let mut alerts = HashSet::new();
    let mut paired = vec![];
    let mut endpoints = vec![];
    let mut chosen: Option<Paired> = None;
    let mut last_source: Option<Reading> = None;
    let mut transport = Transport::default();
    transport.initialize();
    let mut ears = EarAutomation::default();
    let mut router = audio::Router::default();
    let mut keepalive: Option<KeepAlive> = None;
    let mut galaxy: Option<bluetooth::galaxy::Listener> = None;
    let mut next_probe = 0;
    let mut next_media = 0;
    let mut next_battery = 0;
    let mut next_scanner = 0;
    let mut next_galaxy = 0;
    let mut next_audio = 0;
    let mut last_popup = 0;
    let mut popup_revision = 0;
    let mut alert_revision = 0;
    let mut alert = String::new();
    let mut message = String::new();
    let mut connect_until = 0;
    let mut connection_routes: Option<audio::ConnectionRoutes> = None;
    let mut refresh = true;
    let mut connected_before = false;
    let mut last_publish = 0;
    loop {
        let event = rx.recv_timeout(Duration::from_millis(300));
        let t = now();
        let mut opened = HashSet::new();
        let events = event.into_iter().chain(rx.try_iter().take(255));
        for event in events {
            match event {
                Event::Command(Command::Quit) => return,
                Event::Command(Command::Refresh) => {
                    refresh = true;
                    next_battery = 0;
                }
                Event::Command(Command::Restart) => {
                    connection_routes = None;
                    scanner = None;
                    next_scanner = 0;
                    refresh = true;
                    ears.reset(&mut transport);
                    router.reset();
                    keepalive = None;
                }
                Event::Command(Command::Select(address)) => {
                    connection_routes = None;
                    prefs.preferred_address = address;
                    let _ = prefs.save();
                    refresh = true;
                    last_source = None;
                    ears.reset(&mut transport);
                    router.reset();
                }
                Event::Command(Command::Preferences(p)) => {
                    prefs = p;
                    let _ = prefs.save();
                    if !prefs.ear_detection {
                        transport.reset();
                    }
                    if !prefs.speaker_switch {
                        router.reset();
                    }
                }
                Event::Command(Command::Media(c)) => {
                    transport.command(c);
                    next_media = 0;
                }
                Event::Command(Command::Connect) => {
                    if let Some(p) = &chosen {
                        let routes = audio::ConnectionRoutes::capture(p);
                        match audio::request_connection(p) {
                            Ok(()) => {
                                connection_routes = Some(routes);
                                message = "Connecting…".into();
                                connect_until = t + 15_000;
                                next_probe = 0;
                            }
                            Err(_) => {
                                message = "Open the case, then try Bluetooth settings.".into();
                                crate::shell::open_bluetooth();
                            }
                        }
                    } else {
                        crate::shell::open_bluetooth();
                        message = "Pair once in Windows, then reconnect from Near.".into();
                    }
                }
                Event::Standard(address, name, level) => {
                    let reading = Reading {
                        address,
                        model: 0,
                        name,
                        signal: 0,
                        at: t,
                        battery: [Some(level), None, None],
                        charging: [None; 3],
                        ears: [None; 2],
                        lid: None,
                        approximate: false,
                    };
                    history.entry(address).or_default().remember(&reading);
                    nearby.insert(address, reading);
                }
                Event::Reading(r) => {
                    if r.lid == Some(true)
                        && lids
                            .get(&r.address)
                            .is_none_or(|(open, at)| !*open || r.at.saturating_sub(*at) > 30_000)
                    {
                        opened.insert(r.address);
                    }
                    if let Some(open) = r.lid {
                        lids.insert(r.address, (open, r.at));
                    }
                    history.entry(r.address).or_default().remember(&r);
                    nearby.insert(r.address, r);
                }
            }
        }
        if refresh || t >= next_probe {
            next_probe = t + if connect_until > t { 1_000 } else { 5_000 };
            refresh = false;
            paired = bluetooth::identity::catalog();
            endpoints = audio::outputs(9).unwrap_or_default();
            // Control-channel connections alone are not audio connections.
            for p in &mut paired {
                p.connected = endpoints
                    .iter()
                    .any(|e| e.state == 1 && audio::matches(e, p));
            }
            chosen = paired
                .iter()
                .find(|p| p.address == prefs.preferred_address)
                .or_else(|| paired.iter().find(|p| p.connected))
                .or_else(|| {
                    paired
                        .iter()
                        .find(|p| p.model.is_some_and(advertisement::is_airpods))
                })
                .or_else(|| paired.first())
                .cloned();
            if chosen.as_ref().map(|p| p.address)
                != shared
                    .lock()
                    .ok()
                    .and_then(|v| v.chosen.as_ref().map(|p| p.address))
            {
                ears.reset(&mut transport);
                router.reset();
                last_source = None;
            }
            if let Some(p) = &chosen {
                if prefs.speaker_switch || connection_routes.is_some() {
                    let inputs = audio::inputs(9).unwrap_or_default();
                    if p.connected {
                        if let Some(routes) = connection_routes.take() {
                            routes.restore(p, &endpoints, &inputs);
                        }
                    }
                    if prefs.speaker_switch {
                        router.disconnected(p, &endpoints, &inputs, prefs.speakers_id.as_deref());
                    }
                }
            }
        }
        if t >= next_scanner {
            next_scanner = t + 10_000;
            if scanner
                .as_ref()
                .is_none_or(|s| s.failed.load(std::sync::atomic::Ordering::Relaxed))
            {
                drop(scanner.take());
                scanner = bluetooth::Scanner::start(tx.clone()).ok();
            }
        }
        nearby.retain(|_, r| t.saturating_sub(r.at) < 120_000);
        lids.retain(|a, _| nearby.contains_key(a));
        if nearby.len() > 32 {
            let mut values: Vec<_> = nearby.values().map(|r| (r.at, r.address)).collect();
            values.sort_unstable();
            for (_, a) in values.into_iter().take(nearby.len() - 32) {
                nearby.remove(&a);
            }
        }
        if history.len() > 32 {
            let mut values: Vec<_> = history
                .iter()
                .map(|(a, h)| {
                    (
                        h.parts.iter().flatten().map(|p| p.at).max().unwrap_or(0),
                        *a,
                    )
                })
                .collect();
            values.sort_unstable();
            for (_, a) in values.into_iter().take(history.len() - 32) {
                history.remove(&a);
            }
        }
        for h in history.values_mut() {
            h.expire(t);
        }
        let mut fresh: Vec<_> = nearby
            .values()
            .filter(|r| t.saturating_sub(r.at) < 15_000)
            .cloned()
            .collect();
        fresh.sort_by_key(|r| -r.signal);
        let exact = chosen
            .as_ref()
            .and_then(|p| fresh.iter().find(|r| r.address == p.address));
        let compatible: Vec<_> = fresh
            .iter()
            .filter(|r| chosen.as_ref().is_some_and(|p| p.model == Some(r.model)))
            .collect();
        // Random BLE addresses are not pair identities. Refuse ambiguous model matches.
        let source = exact
            .or_else(|| (compatible.len() == 1).then(|| compatible[0]))
            .or_else(|| chosen.is_none().then(|| fresh.first()).flatten())
            .cloned();
        if let Some(r) = &source {
            last_source = Some(r.clone());
        }
        if last_source
            .as_ref()
            .is_some_and(|r| t.saturating_sub(r.at) > 600_000)
        {
            last_source = None;
        }
        let source = source.or_else(|| last_source.clone());
        let connected = chosen.as_ref().is_some_and(|p| p.connected);
        if !connected {
            ears.reset(&mut transport);
            router.reset();
        }
        if let (Some(p), Some(r)) = (&chosen, &source) {
            let reliable = !r.approximate
                || r.address == p.address
                || (compatible.len() == 1 && p.model == Some(r.model) && r.signal >= -75);
            if connected && reliable {
                if let Some(state) = ears.update(r, &mut transport, prefs.ear_detection) {
                    if prefs.speaker_switch {
                        router.update(state, p, &endpoints, prefs.speakers_id.as_deref());
                    }
                }
            }
        }
        if t >= next_audio {
            next_audio = t + 5_000;
            let current = audio::default_output(windows::Win32::Media::Audio::eMultimedia).ok();
            let headset = chosen
                .as_ref()
                .filter(|_| connected)
                .and_then(|p| audio::selected_headset(&endpoints, p, current.as_deref()));
            if keepalive
                .as_ref()
                .is_some_and(|k| k.failed() || headset.is_none_or(|h| h.id != k.endpoint))
            {
                keepalive = None;
            }
            if keepalive.is_none() {
                if let Some(e) = headset {
                    keepalive = KeepAlive::start(&e.id);
                }
            }
        }
        if t >= next_battery {
            next_battery = t + 60_000;
            if chosen.as_ref().is_some_and(|p| {
                p.connected && p.model.is_none() && !p.name.to_lowercase().contains("buds")
            }) {
                let tx = tx.clone();
                let _ = std::thread::Builder::new()
                    .name("Near battery".into())
                    .spawn(move || {
                        let _apartment = crate::shell::Apartment::mta();
                        bluetooth::standard::read(tx);
                    });
            }
        }
        let buds = chosen
            .as_ref()
            .filter(|p| p.connected && p.name.to_lowercase().contains("buds"));
        if galaxy
            .as_ref()
            .is_some_and(|g| buds.is_none_or(|p| p.address != g.address) || g.finished())
        {
            galaxy = None;
            next_galaxy = t + 30_000;
        }
        if galaxy.is_none() && t >= next_galaxy {
            if let Some(p) = buds {
                galaxy = bluetooth::galaxy::Listener::start(p.clone(), tx.clone());
            }
        }
        if t.saturating_sub(last_publish) < 250 {
            continue;
        }
        last_publish = t;
        let name = chosen
            .as_ref()
            .map(|p| p.name.clone())
            .or_else(|| source.as_ref().map(|r| r.name.clone()))
            .unwrap_or_else(|| "AirPods".into());
        let batteries = source
            .as_ref()
            .and_then(|r| history.get(&r.address))
            .cloned()
            .unwrap_or_default();
        if prefs.low_battery {
            for (i, part) in batteries.parts.iter().enumerate() {
                if let Some(s) = part {
                    let key = (source.as_ref().map(|r| r.address).unwrap_or(0), i);
                    if s.level >= 30 || s.charging == Some(true) {
                        alerts.remove(&key);
                    } else if s.level <= 20 && t.saturating_sub(s.at) < 15_000 && alerts.insert(key)
                    {
                        alert = format!(
                            "{} · {}: {}{}%",
                            name,
                            ["Left", "Right", "Case"][i],
                            if batteries.approximate { "about " } else { "" },
                            s.level
                        );
                        alert_revision += 1;
                    }
                }
            }
            if alerts.len() > 96 {
                alerts.clear();
            }
        }
        if prefs.popup
            && t.saturating_sub(last_popup) > 12_000
            && (connected && !connected_before
                || source.as_ref().is_some_and(|r| opened.contains(&r.address)))
        {
            popup_revision += 1;
            last_popup = t;
        }
        connected_before = connected;
        if connected && connect_until != 0 {
            connect_until = 0;
            message.clear();
        }
        if connect_until != 0 && t >= connect_until {
            connect_until = 0;
            connection_routes = None;
            message = "Could not connect. Open the case and try again.".into();
        }
        let media = if t >= next_media {
            next_media = t + 2_000;
            transport.read()
        } else {
            shared.lock().map(|v| v.media.clone()).unwrap_or_default()
        };
        let last_time = batteries.parts.iter().flatten().map(|s| s.at).max();
        let note = if compatible.len() > 1 && exact.is_none() {
            "Multiple AirPods nearby · battery cannot be matched safely".into()
        } else if last_time.is_none() {
            if connected {
                "Open the case for battery"
            } else {
                "Open your case to check battery"
            }
            .into()
        } else if last_time.is_some_and(|at| t.saturating_sub(at) > 15_000) {
            "Last reading · case updates while broadcasting".into()
        } else if batteries.approximate {
            "Bluetooth estimate · 10% steps".into()
        } else {
            "Battery reported by your headphones".into()
        };
        let state = if connected {
            "Connected"
        } else if scanner.is_none() {
            "Bluetooth unavailable"
        } else if connect_until > t {
            "Connecting…"
        } else if chosen.is_some() {
            "Ready to connect"
        } else if source.is_some() {
            "Nearby · pair once with Windows"
        } else {
            "Open your case to get started"
        };
        let next = ViewModel {
            preferences: prefs.clone(),
            paired: paired.clone(),
            chosen: chosen.clone(),
            nearby: fresh.into_iter().take(12).collect(),
            batteries,
            model: chosen
                .as_ref()
                .and_then(|p| p.model)
                .or_else(|| source.as_ref().map(|r| r.model))
                .unwrap_or(0),
            name,
            state: state.into(),
            note,
            media,
            endpoints: endpoints.iter().filter(|e| e.state == 1).cloned().collect(),
            message: message.clone(),
            bluetooth: scanner.is_some(),
            popup_revision,
            alert_revision,
            alert: alert.clone(),
            connecting: connect_until > t,
        };
        if let Ok(mut vm) = shared.lock() {
            if *vm != next {
                *vm = next;
                crate::shell::notify(window);
            }
        }
    }
}
