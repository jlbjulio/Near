use crate::{bluetooth::wait, model::Event};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
};
use windows::{
    Devices::{
        Bluetooth::{BluetoothCacheMode, BluetoothLEDevice, GenericAttributeProfile::*},
        Enumeration::DeviceInformation,
    },
    Storage::Streams::DataReader,
    core::GUID,
};
static READING: AtomicBool = AtomicBool::new(false);
/// Reads the standard battery level of `address` only. Windows' record of that one LE device
/// is looked up first, without touching the radio: headphones that are not known as an LE
/// device are left alone, and no other device (a mouse, a keyboard) is ever opened.
pub fn read(tx: SyncSender<Event>, address: u64) {
    if READING.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = (|| -> windows::core::Result<()> {
        let selector = BluetoothLEDevice::GetDeviceSelectorFromBluetoothAddress(address)?;
        let known = wait(DeviceInformation::FindAllAsyncAqsFilter(&selector)?, 4)?;
        let Some(info) = known.into_iter().next() else {
            return Ok(());
        };
        let device = wait(BluetoothLEDevice::FromIdAsync(&info.Id()?)?, 4)?;
        let result = read_device(&tx, &device);
        let _ = device.Close();
        result
    })();
    READING.store(false, Ordering::Release);
}
fn read_device(tx: &SyncSender<Event>, device: &BluetoothLEDevice) -> windows::core::Result<()> {
    let services = wait(
        device.GetGattServicesForUuidWithCacheModeAsync(
            GUID::from_u128(0x0000180f_0000_1000_8000_00805f9b34fb),
            BluetoothCacheMode::Cached,
        )?,
        3,
    )?;
    if services.Status()? != GattCommunicationStatus::Success {
        return Ok(());
    }
    for service in services.Services()?.into_iter().take(1) {
        let result = read_level(tx, device, &service);
        let _ = service.Close();
        result?;
    }
    Ok(())
}
fn read_level(
    tx: &SyncSender<Event>,
    device: &BluetoothLEDevice,
    service: &GattDeviceService,
) -> windows::core::Result<()> {
    let chars = wait(
        service.GetCharacteristicsForUuidWithCacheModeAsync(
            GUID::from_u128(0x00002a19_0000_1000_8000_00805f9b34fb),
            BluetoothCacheMode::Uncached,
        )?,
        3,
    )?;
    if chars.Status()? != GattCommunicationStatus::Success {
        return Ok(());
    }
    for characteristic in chars.Characteristics()?.into_iter().take(1) {
        let value = wait(
            characteristic.ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)?,
            3,
        )?;
        if value.Status()? == GattCommunicationStatus::Success && value.Value()?.Length()? == 1 {
            let level = DataReader::FromBuffer(&value.Value()?)?.ReadByte()?;
            if level <= 100 {
                let _ = tx.try_send(Event::Standard(
                    device.BluetoothAddress()?,
                    device.Name()?.to_string_lossy(),
                    level,
                ));
            }
        }
    }
    Ok(())
}
