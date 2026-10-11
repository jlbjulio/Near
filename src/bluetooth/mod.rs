pub mod advertisement;
pub mod galaxy;
pub mod identity;
pub mod standard;
use crate::model::{Event, now};
use std::{
    sync::mpsc::SyncSender,
    time::{Duration, Instant},
};
use windows::{
    Devices::Bluetooth::Advertisement::*,
    Foundation::TypedEventHandler,
    Storage::Streams::DataReader,
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
                        let _ = tx.try_send(Event::Reading(reading));
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
