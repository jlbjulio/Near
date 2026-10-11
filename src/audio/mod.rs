pub mod keep_alive;
pub mod media;
mod routing;
use crate::model::{Endpoint, Paired};
pub use routing::{ConnectionRoutes, Router, selected_headset};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::{
    PropVariantToGUID, PropVariantToStringAlloc, PropVariantToUInt32,
};
use windows::{
    Win32::{
        Media::{Audio::*, KernelStreaming::*},
        System::{Com::StructuredStorage::PropVariantClear, Com::*},
        UI::Shell::PropertiesSystem::*,
    },
    core::{GUID, Interface, PCWSTR, define_interface},
};

const NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 14,
};
const INSTANCE: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x78c34fc8_104a_4aca_9ea4_524d52996e57),
    pid: 256,
};
const CONTAINER: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x8c7ed206_3f8a_4827_b3ab_ae9e1faefc6c),
    pid: 2,
};
const FORM: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffe28d1d),
    pid: 0,
};
fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}
unsafe fn string(store: &IPropertyStore, key: &PROPERTYKEY) -> String {
    unsafe {
        let Ok(mut value) = store.GetValue(key) else {
            return String::new();
        };
        let text = PropVariantToStringAlloc(&value)
            .ok()
            .map(|p| {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0.cast()));
                s
            })
            .unwrap_or_default();
        let _ = PropVariantClear(&mut value);
        text
    }
}
pub fn outputs(states: u32) -> windows::core::Result<Vec<Endpoint>> {
    endpoints(eRender, states)
}
pub fn inputs(states: u32) -> windows::core::Result<Vec<Endpoint>> {
    endpoints(eCapture, states)
}
fn endpoints(flow: EDataFlow, states: u32) -> windows::core::Result<Vec<Endpoint>> {
    unsafe {
        let devices = enumerator()?.EnumAudioEndpoints(flow, DEVICE_STATE(states))?;
        let mut list = vec![];
        for i in 0..devices.GetCount()?.min(64) {
            let device = devices.Item(i)?;
            let props = device.OpenPropertyStore(STGM_READ)?;
            let id = device.GetId()?;
            let id_text = id.to_string().unwrap_or_default();
            CoTaskMemFree(Some(id.0.cast()));
            let container = props.GetValue(&CONTAINER).ok().and_then(|mut value| {
                let guid = PropVariantToGUID(&value).ok();
                let _ = PropVariantClear(&mut value);
                guid
            });
            let mut form = props.GetValue(&FORM)?;
            let speakers = PropVariantToUInt32(&form).ok() == Some(1);
            let _ = PropVariantClear(&mut form);
            list.push(Endpoint {
                id: id_text,
                name: string(&props, &NAME),
                instance: string(&props, &INSTANCE),
                container,
                speakers,
                state: device.GetState()?.0,
            });
        }
        Ok(list)
    }
}
pub fn matches(e: &Endpoint, p: &Paired) -> bool {
    p.container.is_some() && e.container == p.container
        || e.instance
            .to_ascii_uppercase()
            .split(['&', '_', '\\', '{', '}'])
            .any(|part| part == format!("{:012X}", p.address))
}
pub fn request_connection(p: &Paired) -> windows::core::Result<()> {
    unsafe {
        let candidates: Vec<_> = outputs(9)?.into_iter().filter(|e| matches(e, p)).collect();
        if candidates.iter().any(|e| e.state == 1) {
            return Ok(());
        }
        let mut error = windows::core::Error::new(
            windows::core::HRESULT(0x80070490u32 as i32),
            "Windows has no paired audio endpoint. Open Bluetooth settings.",
        );
        for endpoint in candidates {
            let result = (|| {
                let devices = enumerator()?;
                let id: Vec<u16> = endpoint.id.encode_utf16().chain([0]).collect();
                let device = devices.GetDevice(PCWSTR(id.as_ptr()))?;
                let topology: IDeviceTopology = device.Activate(CLSCTX_ALL, None)?;
                let adapter_id = topology.GetConnector(0)?.GetDeviceIdConnectedTo()?;
                let adapter = devices.GetDevice(PCWSTR(adapter_id.0));
                CoTaskMemFree(Some(adapter_id.0.cast()));
                let ks: IKsControl = adapter?.Activate(CLSCTX_ALL, None)?;
                let property = KSIDENTIFIER {
                    Anonymous: KSIDENTIFIER_0 {
                        Anonymous: KSIDENTIFIER_0_0 {
                            Set: GUID::from_u128(0x7fa06c40_b8f6_4c7e_8556_e8c33a12e54d),
                            Id: 0,
                            Flags: 1,
                        },
                    },
                };
                let mut returned = 0;
                // ONESHOT_RECONNECT asks the existing Windows driver; endpoint state confirms success.
                ks.KsProperty(
                    &property,
                    std::mem::size_of::<KSIDENTIFIER>() as u32,
                    std::ptr::null_mut(),
                    0,
                    &mut returned,
                )
            })();
            match result {
                Ok(()) => return Ok(()),
                Err(e) => error = e,
            }
        }
        Err(error)
    }
}
pub fn default_output(role: ERole) -> windows::core::Result<String> {
    default_device(eRender, role)
}
fn default_device(flow: EDataFlow, role: ERole) -> windows::core::Result<String> {
    unsafe {
        let p = enumerator()?.GetDefaultAudioEndpoint(flow, role)?.GetId()?;
        let s = p.to_string()?;
        CoTaskMemFree(Some(p.0.cast()));
        Ok(s)
    }
}

define_interface!(
    IPolicyConfig,
    IPolicyConfig_Vtbl,
    0xf8679f50_850a_41cf_9c72_430f290290c8
);
#[repr(C)]
pub struct IPolicyConfig_Vtbl {
    base: windows::core::IUnknown_Vtbl,
    preceding: [usize; 10],
    set_default:
        unsafe extern "system" fn(*mut std::ffi::c_void, PCWSTR, i32) -> windows::core::HRESULT,
    visibility: usize,
}
pub fn set_default(id: &str, role: ERole) -> windows::core::Result<()> {
    unsafe {
        let policy: IPolicyConfig = CoCreateInstance(
            &GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9),
            None,
            CLSCTX_ALL,
        )?;
        let name: Vec<u16> = id.encode_utf16().chain([0]).collect();
        (policy.vtable().set_default)(policy.as_raw(), PCWSTR(name.as_ptr()), role.0).ok()
    }
}
