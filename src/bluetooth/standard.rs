use crate::{bluetooth::wait, model::Event};
use std::sync::mpsc::SyncSender;
use windows::{
    Devices::{
        Bluetooth::{BluetoothCacheMode, GenericAttributeProfile::*},
        Enumeration::DeviceInformation,
    },
    Storage::Streams::DataReader,
    core::GUID,
};
pub fn read(tx: SyncSender<Event>) {
    let result = (|| -> windows::core::Result<()> {
        let selector = GattDeviceService::GetDeviceSelectorFromUuid(GUID::from_u128(
            0x0000180f_0000_1000_8000_00805f9b34fb,
        ))?;
        let devices = wait(DeviceInformation::FindAllAsyncAqsFilter(&selector)?, 4)?;
        for info in devices.into_iter().take(8) {
            let _ = (|| -> windows::core::Result<()> {
                let service = wait(GattDeviceService::FromIdAsync(&info.Id()?)?, 3)?;
                let result = (|| -> windows::core::Result<()> {
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
                            characteristic
                                .ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)?,
                            3,
                        )?;
                        if value.Status()? == GattCommunicationStatus::Success
                            && value.Value()?.Length()? == 1
                        {
                            let level = DataReader::FromBuffer(&value.Value()?)?.ReadByte()?;
                            if level <= 100 {
                                let device = service.Device()?;
                                let _ = tx.try_send(Event::Standard(
                                    device.BluetoothAddress()?,
                                    device.Name()?.to_string_lossy(),
                                    level,
                                ));
                            }
                        }
                    }
                    Ok(())
                })();
                let _ = service.Close();
                result
            })();
        }
        Ok(())
    })();
    let _ = result;
}
