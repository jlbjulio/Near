use crate::{
    audio::{
        self,
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
    let mut selected_batteries = Batteries::default();
    let mut auto_connect = crate::connection::AutoConnect::default();
    let mut lids: HashMap<u64, (bool, u64)> = HashMap::new();
    let mut alerts = crate::alerts::LowBattery::default();
    let mut paired = vec![];
    let mut endpoints = vec![];
    let mut chosen: Option<Paired> = None;
    let mut last_source: Option<Reading> = None;
    let mut transport = Transport::default();
    transport.initialize();
    let mut ears = EarAutomation::default();
    let mut router = audio::Router::default();
    let mut galaxy: Option<bluetooth::galaxy::Listener> = None;
    let mut next_probe = 0;
    let mut next_media = 0;
    let mut next_battery = 0;
    let mut next_scanner = 0;
    let mut next_galaxy = 0;
    let mut last_popup = 0;
    let mut popup_revision = 0;
    let mut alert_revision = 0;
    let mut alert = String::new();
    let mut message = String::new();
    let mut connect_until = 0;
    let mut disconnect_until = 0;
    let mut message_until = 0;
    let mut connection_routes: Option<audio::ConnectionRoutes> = None;
    let mut preserve_until = 0;
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
                    if let Some(p) = &chosen {
                        auto_connect.reset(p.address);
                    }
                }
                Event::Command(Command::Restart) => {
                    connection_routes = None;
                    connect_until = 0;
                    disconnect_until = 0;
                    message.clear();
                    if let Some(p) = &chosen {
                        auto_connect.reset(p.address);
                    }
                    scanner = None;
                    next_scanner = 0;
                    refresh = true;
                    ears.reset(&mut transport);
                    router.reset();
                }
                Event::Command(Command::Select(address)) => {
                    connection_routes = None;
                    prefs.preferred_address = address;
                    let _ = prefs.save();
                    refresh = true;
                    last_source = None;
                    selected_batteries = Batteries::default();
                    connect_until = 0;
                    disconnect_until = 0;
                    message.clear();
                    ears.reset(&mut transport);
                    router.reset();
                }
                Event::Command(Command::Preferences(change)) => {
                    let previous_detection = prefs.ear_detection;
                    let mut updated = prefs.clone();
                    updated.apply(change);
                    if let Err(e) = updated.save() {
                        crate::shell::log(&e.to_string());
                        message = "Couldn’t save settings.".into();
                        message_until = t + 30_000;
                        continue;
                    }
                    prefs = updated;
                    if prefs.ear_detection != previous_detection {
                        ears.reset(&mut transport);
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
                        if p.connected || connect_until > t || disconnect_until > t {
                            continue;
                        }
                        prefs.disconnected_addresses.retain(|a| *a != p.address);
                        let _ = prefs.save();
                        auto_connect.attempted(p.address, t);
                        let routes = audio::ConnectionRoutes::capture(p);
                        match audio::request_connection(p) {
                            Ok(()) => {
                                connection_routes = Some(routes);
                                preserve_until = t + 18_000;
                                message = "Connecting…".into();
                                message_until = t + 15_000;
                                connect_until = t + 15_000;
                                next_probe = 0;
                            }
                            Err(e) => {
                                crate::shell::log(&e.to_string());
                                message = "Couldn’t connect. Open the case and try again.".into();
                                message_until = t + 30_000;
                            }
                        }
                    } else {
                        message = "Pair your headphones in Bluetooth settings first.".into();
                        message_until = t + 30_000;
                    }
                }
                Event::Command(Command::Disconnect) => {
                    if let Some(p) = &chosen {
                        if !p.connected || disconnect_until > t {
                            continue;
                        }
                        if !prefs.disconnected_addresses.contains(&p.address) {
                            if prefs.disconnected_addresses.len() >= 32 {
                                prefs.disconnected_addresses.remove(0);
                            }
                            prefs.disconnected_addresses.push(p.address);
                            let _ = prefs.save();
                        }
                        connection_routes = None;
                        connect_until = 0;
                        ears.reset(&mut transport);
                        match audio::request_disconnection(p) {
                            Ok(()) => {
                                disconnect_until = t + 10_000;
                                message.clear();
                                next_probe = 0;
                            }
                            Err(e) => {
                                crate::shell::log(&e.to_string());
                                message = "Couldn’t disconnect. Try Bluetooth settings.".into();
                                message_until = t + 30_000;
                            }
                        }
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
                    nearby.insert(r.address, r);
                }
            }
        }
        if refresh || t >= next_probe {
            next_probe =
                t + if connect_until > t || disconnect_until > t || connection_routes.is_some() {
                    1_000
                } else {
                    5_000
                };
            refresh = false;
            paired = bluetooth::identity::catalog();
            endpoints = audio::outputs(9).unwrap_or_default();
            let input_endpoints = audio::inputs(9).unwrap_or_default();
            // Control-channel connections alone are not audio connections.
            for p in &mut paired {
                p.connected = endpoints
                    .iter()
                    .chain(input_endpoints.iter())
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
                selected_batteries = Batteries::default();
                connection_routes = None;
                connect_until = 0;
                disconnect_until = 0;
                preserve_until = 0;
                message.clear();
            }
            if let Some(p) = &chosen {
                if prefs.speaker_switch || connection_routes.is_some() {
                    if p.connected {
                        if let Some(routes) = connection_routes.as_mut() {
                            routes.restore(p, &endpoints, &input_endpoints);
                        }
                    }
                    if prefs.speaker_switch {
                        router.disconnected(
                            p,
                            &endpoints,
                            &input_endpoints,
                            prefs.speakers_id.as_deref(),
                        );
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
        selected_batteries.expire(t);
        let mut fresh: Vec<_> = nearby
            .values()
            .filter(|r| t.saturating_sub(r.at) < 15_000)
            .cloned()
            .collect();
        fresh.sort_by_key(|r| std::cmp::Reverse(r.signal));
        let selection = bluetooth::selection::select(chosen.as_ref(), &fresh);
        let available = chosen.as_ref().is_some_and(|p| p.available)
            || selection
                .reading
                .as_ref()
                .is_some_and(|r| t.saturating_sub(r.at) < 5_000 && r.signal >= -75);
        let source = selection.reading;
        if let Some(r) = &source {
            last_source = Some(r.clone());
            selected_batteries.remember(r);
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
            if connected && selection.sensor_reliable && disconnect_until == 0 {
                if let Some(state) = ears.update(r, &mut transport, prefs.ear_detection) {
                    if prefs.speaker_switch {
                        router.update(state, p, &endpoints, prefs.speakers_id.as_deref());
                    }
                }
            }
        }
        if let Some(p) = &chosen {
            if scanner.is_some()
                && auto_connect.ready(
                    p,
                    available,
                    &prefs.disconnected_addresses,
                    connect_until != 0 || disconnect_until != 0,
                    t,
                )
            {
                let routes = audio::ConnectionRoutes::capture(p);
                if audio::request_connection(p).is_ok() {
                    message.clear();
                    connection_routes = Some(routes);
                    preserve_until = t + 18_000;
                    connect_until = t + 15_000;
                    next_probe = 0;
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
        let batteries = selected_batteries.clone();
        if let Some(p) = chosen.as_ref().filter(|_| !selection.uncertain) {
            if let Some(notice) = alerts.update(
                p.address,
                &batteries,
                prefs.low_battery,
                advertisement::is_max(p.model.unwrap_or(0)),
                t,
            ) {
                alert = notice;
                alert_revision += 1;
            }
        } else if !prefs.low_battery || chosen.is_none() {
            alerts.reset();
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
            preserve_until = t + 3_000;
            message.clear();
        }
        if t >= preserve_until {
            connection_routes = None;
        }
        if disconnect_until != 0 && !connected {
            disconnect_until = 0;
            message.clear();
        }
        if disconnect_until != 0 && t >= disconnect_until {
            disconnect_until = 0;
            message = "Still connected. Try Bluetooth settings.".into();
            message_until = t + 30_000;
        }
        if connect_until != 0 && t >= connect_until {
            connect_until = 0;
            connection_routes = None;
            message = "Couldn’t connect. Open the case and try again.".into();
            message_until = t + 30_000;
        }
        if message_until != 0 && t >= message_until {
            message.clear();
            message_until = 0;
        }
        let media = if t >= next_media {
            next_media = t + 2_000;
            transport.read()
        } else {
            shared.lock().map(|v| v.media.clone()).unwrap_or_default()
        };
        let last_time = batteries.parts.iter().flatten().map(|s| s.at).max();
        let note = if selection.uncertain && last_time.is_none() {
            "Waiting for a clear battery signal.".into()
        } else if last_time.is_none() {
            "Open your case to see battery levels.".into()
        } else if last_time.is_some_and(|at| t.saturating_sub(at) > 15_000) {
            "Last known battery · open your case to update.".into()
        } else if batteries.approximate {
            "Estimated battery · updates nearby.".into()
        } else {
            "Battery reported by your headphones.".into()
        };
        let state = if disconnect_until > t {
            "Disconnecting…"
        } else if connected {
            "Connected"
        } else if scanner.is_none() {
            "Bluetooth unavailable"
        } else if connect_until > t {
            "Connecting…"
        } else if available {
            "Nearby"
        } else if chosen.is_some() {
            "Not connected"
        } else if source.is_some() {
            "Nearby · pair once with Windows"
        } else {
            "Not connected"
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
            disconnecting: disconnect_until > t,
            available,
        };
        if let Ok(mut vm) = shared.lock() {
            if *vm != next {
                *vm = next;
                crate::shell::notify(window);
            }
        }
    }
}
