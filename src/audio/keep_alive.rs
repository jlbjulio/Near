use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};
use windows::{
    Win32::{Foundation::*, Media::Audio::*, System::Threading::*},
    core::PCWSTR,
};
pub struct KeepAlive {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub endpoint: String,
}
impl KeepAlive {
    pub fn start(endpoint: &str) -> Option<Self> {
        let mut selected = None;
        unsafe {
            for id in 0..waveOutGetNumDevs() {
                let mut size = 0u32;
                if waveOutMessage(
                    Some(HWAVEOUT(id as usize as *mut _)),
                    0x812,
                    (&mut size as *mut u32) as usize,
                    0,
                ) != 0
                    || !(2..=8192).contains(&size)
                {
                    continue;
                }
                let mut value = vec![0u16; (size as usize).div_ceil(2)];
                if waveOutMessage(
                    Some(HWAVEOUT(id as usize as *mut _)),
                    0x811,
                    value.as_mut_ptr() as usize,
                    size as usize,
                ) == 0
                    && PCWSTR(value.as_ptr())
                        .to_string()
                        .unwrap_or_default()
                        .eq_ignore_ascii_case(endpoint)
                {
                    selected = Some(id);
                    break;
                }
            }
        }
        let id = selected?;
        let stop = Arc::new(AtomicBool::new(false));
        let end = stop.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("Near audio".into())
            .spawn(move || unsafe {
                let format = WAVEFORMATEX {
                    wFormatTag: 1,
                    nChannels: 2,
                    nSamplesPerSec: 48_000,
                    nAvgBytesPerSec: 192_000,
                    nBlockAlign: 4,
                    wBitsPerSample: 16,
                    cbSize: 0,
                };
                let Ok(event) = CreateEventW(None, false, false, None) else {
                    let _ = tx.send(false);
                    return;
                };
                let mut device = HWAVEOUT::default();
                if waveOutOpen(
                    Some(&mut device),
                    id,
                    &format,
                    Some(event.0 as usize),
                    None,
                    MIDI_WAVE_OPEN_TYPE(0x50000),
                ) != 0
                {
                    let _ = CloseHandle(event);
                    let _ = tx.send(false);
                    return;
                }
                let mut data = [Box::new([0u8; 19_200]), Box::new([0u8; 19_200])];
                let mut headers: Vec<Box<WAVEHDR>> = (0..2)
                    .map(|i| {
                        Box::new(WAVEHDR {
                            lpData: windows::core::PSTR(data[i].as_mut_ptr()),
                            dwBufferLength: 19_200,
                            ..Default::default()
                        })
                    })
                    .collect();
                let size = std::mem::size_of::<WAVEHDR>() as u32;
                let mut prepared = 0;
                let mut queued = 0;
                for header in &mut headers {
                    if waveOutPrepareHeader(device, &mut **header, size) != 0 {
                        break;
                    }
                    prepared += 1;
                    if waveOutWrite(device, &mut **header, size) != 0 {
                        break;
                    }
                    queued += 1;
                }
                let _ = tx.send(queued == 2);
                while queued == 2 && !end.load(Ordering::Relaxed) {
                    if WaitForSingleObject(event, 500) != WAIT_OBJECT_0 {
                        continue;
                    }
                    for header in &mut headers {
                        if header.dwFlags & 1 != 0 && waveOutWrite(device, &mut **header, size) != 0
                        {
                            end.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
                let _ = waveOutReset(device);
                for header in headers.iter_mut().take(prepared) {
                    let _ = waveOutUnprepareHeader(device, &mut **header, size);
                }
                let _ = waveOutClose(device);
                let _ = CloseHandle(event);
            })
            .ok()?;
        if rx.recv_timeout(std::time::Duration::from_secs(3)).ok() != Some(true) {
            stop.store(true, Ordering::Relaxed);
            let _ = thread.join();
            return None;
        }
        Some(Self {
            stop,
            thread: Some(thread),
            endpoint: endpoint.into(),
        })
    }
    pub fn failed(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}
impl Drop for KeepAlive {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
