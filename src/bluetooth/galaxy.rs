use crate::{
    bluetooth::wait,
    model::{Event, Paired, Reading, now},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    thread::JoinHandle,
};
use windows::{
    Devices::Bluetooth::{BluetoothCacheMode, BluetoothDevice},
    Networking::Sockets::{SocketProtectionLevel, StreamSocket},
    Storage::Streams::{DataReader, InputStreamOptions},
    core::GUID,
};
pub fn parse(frame: &[u8], legacy: bool, address: u64, name: &str) -> Option<Reading> {
    if frame.len() < 7 {
        return None;
    }
    let n = if legacy {
        frame[2] as usize
    } else {
        u16::from_le_bytes([frame[1], frame[2]]) as usize & 0x3ff
    };
    if n < 3
        || n + 4 != frame.len()
        || frame[frame.len() - 1] != if legacy { 0xee } else { 0xdd }
        || !legacy && frame[2] & 0x20 != 0
    {
        return None;
    }
    let mut crc = 0u16;
    for value in &frame[3..n + 1] {
        crc ^= (*value as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                crc.wrapping_shl(1) ^ 0x1021
            } else {
                crc.wrapping_shl(1)
            };
        }
    }
    if crc != u16::from_le_bytes([frame[n + 1], frame[n + 2]]) || !matches!(frame[3], 0x60 | 0x61) {
        return None;
    }
    let d = &frame[4..n + 1];
    let extended = frame[3] == 0x61;
    let b = if extended { 2 } else { 1 };
    let p = if extended { 6 } else { 5 };
    if d.len() <= p + usize::from(!legacy) {
        return None;
    }
    let level = |n: u8| (n <= 100).then_some(n);
    let worn = |n: u8| match n {
        1 => Some(true),
        2..=4 => Some(false),
        _ => None,
    };
    let ears = if legacy {
        [Some(matches!(d[p], 1 | 17)), Some(matches!(d[p], 16 | 17))]
    } else {
        [worn(d[p] >> 4), worn(d[p] & 15)]
    };
    let charging = if !legacy && !extended && d.len() >= 8 {
        [
            Some(d[7] & 16 != 0),
            Some(d[7] & 4 != 0),
            Some(d[7] & 1 != 0),
        ]
    } else {
        [None; 3]
    };
    Some(Reading {
        address,
        model: 0,
        name: name.into(),
        signal: 0,
        at: now(),
        battery: [
            level(d[b]),
            level(d[b + 1]),
            if legacy { None } else { level(d[p + 1]) },
        ],
        charging,
        ears,
        lid: None,
        approximate: false,
    })
}
pub struct Listener {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub address: u64,
    socket: Arc<std::sync::Mutex<Option<StreamSocket>>>,
}
impl Listener {
    pub fn start(p: Paired, tx: SyncSender<Event>) -> Option<Self> {
        let address = p.address;
        let stop = Arc::new(AtomicBool::new(false));
        let end = stop.clone();
        let socket = Arc::new(std::sync::Mutex::new(None));
        let shared = socket.clone();
        let thread = std::thread::Builder::new()
            .name("Near Buds".into())
            .spawn(move || {
                let _apartment = crate::shell::Apartment::mta();
                let _ = (|| -> windows::core::Result<()> {
                    let device = wait(BluetoothDevice::FromBluetoothAddressAsync(address)?, 5)?;
                    let services = wait(
                        device.GetRfcommServicesWithCacheModeAsync(BluetoothCacheMode::Uncached)?,
                        5,
                    )?;
                    let services: Vec<_> = services.Services()?.into_iter().collect();
                    let identifiers = [
                        GUID::from_u128(0x00001101_0000_1000_8000_00805f9b34fb),
                        GUID::from_u128(0x2e73a4ad_332d_41fc_90e2_16bef06523f2),
                        GUID::from_u128(0x00001102_0000_1000_8000_00805f9b34fd),
                    ];
                    let selected = identifiers
                        .iter()
                        .find_map(|id| {
                            services
                                .iter()
                                .find(|s| s.ServiceId().and_then(|i| i.Uuid()).ok() == Some(*id))
                        })
                        .cloned();
                    let Some(service) = selected else {
                        let _ = device.Close();
                        return Ok(());
                    };
                    let legacy = service.ServiceId()?.Uuid()? == identifiers[2];
                    let sock = StreamSocket::new()?;
                    if let Ok(mut slot) = shared.lock() {
                        *slot = Some(sock.clone());
                    }
                    let op = sock.ConnectWithProtectionLevelAsync(
                        &service.ConnectionHostName()?,
                        &service.ConnectionServiceName()?,
                        SocketProtectionLevel::BluetoothEncryptionAllowNullAuthentication,
                    )?;
                    let start = std::time::Instant::now();
                    while op.Status()? == windows_future::AsyncStatus::Started {
                        if start.elapsed().as_secs() >= 8 || end.load(Ordering::Relaxed) {
                            let _ = op.Cancel();
                            let _ = sock.Close();
                            return Ok(());
                        }
                        std::thread::sleep(std::time::Duration::from_millis(30));
                    }
                    op.GetResults()?;
                    let reader = DataReader::CreateDataReader(&sock.InputStream()?)?;
                    reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
                    let mut buffer = Vec::with_capacity(1024);
                    while !end.load(Ordering::Relaxed) {
                        let op = reader.LoadAsync(256)?;
                        while op.Status()? == windows_future::AsyncStatus::Started {
                            if end.load(Ordering::Relaxed) {
                                let _ = op.Cancel();
                                return Ok(());
                            }
                            std::thread::sleep(std::time::Duration::from_millis(50));
                        }
                        let n = op.GetResults()?;
                        if n == 0 {
                            break;
                        }
                        let mut chunk = vec![0u8; n as usize];
                        reader.ReadBytes(&mut chunk)?;
                        buffer.extend_from_slice(&chunk);
                        while buffer.len() >= 4 {
                            if buffer[0] != if legacy { 0xfe } else { 0xfd } {
                                buffer.remove(0);
                                continue;
                            }
                            let n = if legacy {
                                buffer[2] as usize
                            } else {
                                u16::from_le_bytes([buffer[1], buffer[2]]) as usize & 0x3ff
                            };
                            if !(3..=512).contains(&n) {
                                buffer.remove(0);
                                continue;
                            }
                            if buffer.len() < n + 4 {
                                break;
                            }
                            if let Some(r) = parse(&buffer[..n + 4], legacy, address, &p.name) {
                                let _ = tx.try_send(Event::Reading(r));
                            }
                            buffer.drain(..n + 4);
                        }
                        if buffer.len() > 1024 {
                            buffer.clear();
                        }
                    }
                    let _ = reader.Close();
                    let _ = sock.Close();
                    let _ = service.Close();
                    let _ = device.Close();
                    Ok(())
                })();
                end.store(true, Ordering::Relaxed);
            })
            .ok()?;
        Some(Self {
            stop,
            thread: Some(thread),
            address,
            socket,
        })
    }
    pub fn finished(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut sock) = self.socket.lock() {
            if let Some(s) = sock.take() {
                let _ = s.Close();
            }
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
