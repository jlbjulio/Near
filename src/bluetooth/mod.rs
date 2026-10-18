pub mod advertisement;
pub mod galaxy;
pub mod identity;
pub mod selection;
pub mod standard;
use crate::model::{Event, now};
use std::{
    sync::mpsc::SyncSender,
    time::{Duration, Instant},
};
use windows::{
    Devices::Bluetooth::Advertisement::*,
    Foundation::TypedEventHandler,
    Storage::Streams::{DataReader, DataWriter},
    core::{Error, HRESULT, Result, RuntimeType},
};
use windows_future::{AsyncStatus, IAsyncOperation};
pub fn wait<T: RuntimeType>(op: IAsyncOperation<T>, seconds: u64) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        if op.Status()? != AsyncStatus::Started {
            return op.GetResults();
        }
        if Instant::now() >= deadline {
            let _ = op.Cancel();
            return Err(Error::new(
                HRESULT(0x800705b4u32 as i32),
                "Bluetooth operation timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
pub struct Scanner {
    watcher: BluetoothLEAdvertisementWatcher,
    received: i64,
    stopped: i64,
    pub failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Scanner {
    pub fn start(tx: SyncSender<Event>) -> Result<Self> {
        let watcher = BluetoothLEAdvertisementWatcher::new()?;
        watcher.SetScanningMode(BluetoothLEScanningMode::Passive)?;
        // Share the radio with Wi-Fi: Bluetooth and Wi-Fi are often one chip, and games feel
        // scan time as ping spikes. Windows 11 24H2+; older versions keep the default.
        if let Ok(coexist) = BluetoothLEAdvertisementScanParameters::CoexistenceOptimized() {
            let _ = watcher.SetScanParameters(&coexist);
        }
        // Let the Bluetooth stack drop everything except Apple proximity-pairing
        // broadcasts (type 0x07) before they wake this process.
        let prefix = DataWriter::new()?;
        prefix.WriteBytes(&[7])?;
        let apple = BluetoothLEManufacturerData::new()?;
        apple.SetCompanyId(0x4c)?;
        apple.SetData(&prefix.DetachBuffer()?)?;
        watcher
            .AdvertisementFilter()?
            .Advertisement()?
            .ManufacturerData()?
            .Append(&apple)?;
        // Headphones repeat the same status several times a second, with a slightly different
        // signal strength each time. Pass a reading on when its content changes, or every 2 s
        // to keep the signal and "nearby" fresh; ear and lid changes still arrive at once.
        let last: std::sync::Mutex<std::collections::HashMap<u64, (crate::model::Reading, u64)>> =
            Default::default();
        let received = watcher.Received(&TypedEventHandler::new(
            move |_: windows::core::Ref<BluetoothLEAdvertisementWatcher>,
                  args: windows::core::Ref<BluetoothLEAdvertisementReceivedEventArgs>| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                for section in args.Advertisement()?.ManufacturerData()? {
                    if section.CompanyId()? != 0x4c {
                        continue;
                    }
                    let buffer = section.Data()?;
                    let length = buffer.Length()?;
                    if !(2..=255).contains(&length) {
                        continue;
                    }
                    let reader = DataReader::FromBuffer(&buffer)?;
                    let mut bytes = vec![0u8; length as usize];
                    reader.ReadBytes(&mut bytes)?;
                    let _ = reader.Close();
                    if let Some(reading) = advertisement::parse(
                        &bytes,
                        args.BluetoothAddress()?,
                        args.RawSignalStrengthInDBm()?,
                        now(),
                    ) {
                        let fresh = last.lock().map_or(true, |mut seen| {
                            let same = |old: &crate::model::Reading| {
                                crate::model::Reading {
                                    signal: reading.signal,
                                    at: reading.at,
                                    ..old.clone()
                                } == reading
                            };
                            let repeat = seen.get(&reading.address).is_some_and(|(old, sent)| {
                                same(old) && reading.at.saturating_sub(*sent) < 2_000
                            });
                            if !repeat {
                                if seen.len() > 64 {
                                    seen.clear();
                                }
                                seen.insert(reading.address, (reading.clone(), reading.at));
                            }
                            !repeat
                        });
                        if fresh {
                            let _ = tx.try_send(Event::Reading(reading));
                        }
                    }
                }
                Ok(())
            },
        ))?;
        let failed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = failed.clone();
        let stopped =
            watcher.Stopped(
                &TypedEventHandler::new(
                    move |_: windows::core::Ref<BluetoothLEAdvertisementWatcher>,
                          _: windows::core::Ref<
                        BluetoothLEAdvertisementWatcherStoppedEventArgs,
                    >| {
                        flag.store(true, std::sync::atomic::Ordering::Relaxed);
                        Ok(())
                    },
                ),
            )?;
        watcher.Start()?;
        Ok(Self {
            watcher,
            received,
            stopped,
            failed,
        })
    }
}
impl Drop for Scanner {
    fn drop(&mut self) {
        let _ = self.watcher.RemoveReceived(self.received);
        let _ = self.watcher.RemoveStopped(self.stopped);
        let _ = self.watcher.Stop();
    }
}
