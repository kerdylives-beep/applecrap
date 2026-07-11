//! Dev tool: prints every render audio session's PID, process-derived state,
//! and display name, to verify the volume-mixer labeling works.
//! Run: cargo run --example dump_audio_sessions

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    use windows::Win32::Media::Audio::{
        eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
        MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        for device_index in 0..devices.GetCount()? {
            let device = devices.Item(device_index)?;
            let manager = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)?;
            let sessions = manager.GetSessionEnumerator()?;
            for session_index in 0..sessions.GetCount()? {
                let control = sessions.GetSession(session_index)?;
                let control2 = windows::core::Interface::cast::<IAudioSessionControl2>(&control)?;
                let pid = control2.GetProcessId().unwrap_or(0);
                let state = control.GetState().map(|s| s.0).unwrap_or(-1);
                let name = control2
                    .GetDisplayName()
                    .map(|value| {
                        let text = value.to_string().unwrap_or_default();
                        CoTaskMemFree(Some(value.0 as _));
                        text
                    })
                    .unwrap_or_default();
                println!("device {device_index} session {session_index}: pid={pid} state={state} name=\"{name}\"");
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    println!("windows only");
}
