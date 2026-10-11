use crate::model::Paired;
use std::collections::HashMap;
use windows::{
    Win32::{
        Devices::{Bluetooth::*, DeviceAndDriverInstallation::*},
        Foundation::DEVPROPKEY,
    },
    core::{GUID, PCWSTR},
};

pub fn catalog() -> Vec<Paired> {
    unsafe {
        let search = BLUETOOTH_DEVICE_SEARCH_PARAMS {
            dwSize: std::mem::size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
            fReturnAuthenticated: true.into(),
            fReturnRemembered: true.into(),
            fReturnConnected: true.into(),
            ..Default::default()
        };
        let mut info = BLUETOOTH_DEVICE_INFO {
            dwSize: std::mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
            ..Default::default()
        };
        let Ok(find) = BluetoothFindFirstDevice(&search, &mut info) else {
            return vec![];
        };
        let metadata = metadata();
        let mut result = vec![];
        let mut visited = 0;
        loop {
            let address = info.Address.Anonymous.ullLong;
            let name = String::from_utf16_lossy(
                &info.szName[..info
                    .szName
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(info.szName.len())],
            );
            let (model, container) = metadata.get(&address).copied().unwrap_or_default();
            let lower = name.to_lowercase();
            if (info.fRemembered.as_bool() || info.fAuthenticated.as_bool())
                && (model.is_some()
                    || info.ulClassofDevice & 0x1f00 == 0x400
                    || [
                        "airpods",
                        "beats",
                        "buds",
                        "headphone",
                        "headset",
                        "wh-",
                        "wf-",
                        "auricular",
                    ]
                    .iter()
                    .any(|n| lower.contains(n)))
            {
                result.push(Paired {
                    address,
                    name,
                    connected: info.fConnected.as_bool(),
                    model,
                    container,
                });
            }
            visited += 1;
            if result.len() >= 32
                || visited >= 128
                || BluetoothFindNextDevice(find, &mut info).is_err()
            {
                break;
            }
        }
        let _ = BluetoothFindDeviceClose(find);
        result
    }
}
fn hex_address(id: &str) -> Option<u64> {
    let upper = id.to_ascii_uppercase();
    upper.split(['&', '_', '\\']).find_map(|s| {
        (s.len() == 12 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| u64::from_str_radix(s, 16).ok())
            .flatten()
    })
}
fn metadata() -> HashMap<u64, (Option<u16>, Option<GUID>)> {
    let mut result = HashMap::new();
    unsafe {
        let mut size = 0;
        if CM_Get_Device_ID_List_SizeW(
            &mut size,
            windows::core::w!("BTHENUM"),
            CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT,
        ) != CR_SUCCESS
            || size == 0
            || size > 1_048_576
        {
            return result;
        }
        let mut buffer = vec![0u16; size as usize];
        if CM_Get_Device_ID_ListW(
            windows::core::w!("BTHENUM"),
            &mut buffer,
            CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT,
        ) != CR_SUCCESS
        {
            return result;
        }
        for raw in buffer
            .split(|n| *n == 0)
            .filter(|s| !s.is_empty())
            .take(256)
        {
            let id = String::from_utf16_lossy(raw);
            let Some(address) = hex_address(&id) else {
                continue;
            };
            let entry = result.entry(address).or_insert((None, None));
            if let Some((_, suffix)) = id.to_ascii_uppercase().split_once("VID&0001004C_PID&") {
                if suffix.len() >= 4 {
                    entry.0 = u16::from_str_radix(&suffix[..4], 16).ok();
                }
            }
            let mut node = 0;
            let text: Vec<u16> = raw.iter().copied().chain([0]).collect();
            if CM_Locate_DevNodeW(&mut node, PCWSTR(text.as_ptr()), CM_LOCATE_DEVNODE_NORMAL)
                != CR_SUCCESS
            {
                continue;
            }
            let key = DEVPROPKEY {
                fmtid: GUID::from_u128(0x8c7ed206_3f8a_4827_b3ab_ae9e1faefc6c),
                pid: 2,
            };
            let mut property_type = windows::Win32::Devices::Properties::DEVPROPTYPE(0);
            let mut bytes = [0u8; 16];
            let mut length = 16;
            if CM_Get_DevNode_PropertyW(
                node,
                &key,
                &mut property_type,
                Some(bytes.as_mut_ptr()),
                &mut length,
                0,
            ) == CR_SUCCESS
                && length == 16
                && property_type.0 == 13
            {
                let guid = std::ptr::read_unaligned(bytes.as_ptr().cast::<GUID>());
                if guid != GUID::zeroed() {
                    entry.1 = Some(guid);
                }
            }
        }
    }
    result
}
