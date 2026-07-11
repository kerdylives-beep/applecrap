//! Labels the app's audio sessions in the Windows volume mixer.
//!
//! Audio from the embedded player is produced by WebView2 utility processes,
//! so the mixer attributes it to "Microsoft Edge WebView2" instead of the app
//! (https://github.com/MicrosoftEdge/WebView2Feedback/issues/2236). The
//! established workaround is to find audio sessions owned by our descendant
//! processes and set their display name/icon via the Core Audio API.

#![cfg(windows)]

use std::{collections::HashMap, thread, time::Duration};

pub const MIXER_DISPLAY_NAME: &str = "AppleCrap Alpha";

/// Spawns a background thread that periodically relabels audio sessions
/// belonging to this process or its descendants (the WebView2 processes).
/// Sessions only exist while an app plays audio, so this must poll: a session
/// created when playback starts is relabeled within one cycle.
pub fn spawn_session_labeler() {
    thread::Builder::new()
        .name("audio-session-labeler".to_string())
        .spawn(|| {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
            unsafe {
                // MTA for the lifetime of this thread; never uninitialized.
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }

            let exe_name = std::env::current_exe()
                .ok()
                .and_then(|path| path.file_name().map(|name| name.to_string_lossy().to_lowercase()))
                .unwrap_or_default();
            let icon_path = std::env::current_exe()
                .ok()
                .map(|path| format!("{},0", path.display()));

            loop {
                let _ = relabel_descendant_sessions(&exe_name, icon_path.as_deref());
                thread::sleep(Duration::from_secs(4));
            }
        })
        .ok();
}

fn relabel_descendant_sessions(exe_name: &str, icon_path: Option<&str>) -> windows::core::Result<()> {
    use windows::core::HSTRING;
    use windows::Win32::Media::Audio::{
        eRender, IAudioSessionControl2, IMMDeviceEnumerator, MMDeviceEnumerator,
        DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

    let parents = snapshot_parent_map();
    let display_name = HSTRING::from(MIXER_DISPLAY_NAME);
    let icon = icon_path.map(HSTRING::from);

    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        for device_index in 0..devices.GetCount()? {
            let Ok(device) = devices.Item(device_index) else {
                continue;
            };
            let Ok(manager) = device.Activate::<windows::Win32::Media::Audio::IAudioSessionManager2>(CLSCTX_ALL, None) else {
                continue;
            };
            let Ok(sessions) = manager.GetSessionEnumerator() else {
                continue;
            };
            for session_index in 0..sessions.GetCount()? {
                let Ok(control) = sessions.GetSession(session_index) else {
                    continue;
                };
                let Ok(control2) = windows::core::Interface::cast::<IAudioSessionControl2>(&control)
                else {
                    continue;
                };
                let Ok(session_pid) = control2.GetProcessId() else {
                    continue;
                };
                if session_pid == 0 || !has_ancestor_named(&parents, session_pid, exe_name) {
                    continue;
                }

                let already_labeled = control2
                    .GetDisplayName()
                    .map(|name| {
                        let value = name.to_string().unwrap_or_default();
                        windows::Win32::System::Com::CoTaskMemFree(Some(name.0 as _));
                        value == MIXER_DISPLAY_NAME
                    })
                    .unwrap_or(false);
                if already_labeled {
                    continue;
                }

                let _ = control2.SetDisplayName(&display_name, std::ptr::null());
                if let Some(icon) = icon.as_ref() {
                    let _ = control2.SetIconPath(icon, std::ptr::null());
                }
            }
        }
    }

    Ok(())
}

/// pid -> (parent pid, lowercased exe name) for every running process.
fn snapshot_parent_map() -> HashMap<u32, (u32, String)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut map = HashMap::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return map;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let name_len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..name_len]).to_lowercase();
                map.insert(entry.th32ProcessID, (entry.th32ParentProcessID, name));
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    map
}

/// True when the process or any ancestor is our executable. Matching by exe
/// name instead of our own pid matters because WebView2 shares one browser
/// process tree between app instances using the same profile: the audio
/// process may descend from a different (even already-updated) instance of
/// the app than the one running this labeler.
fn has_ancestor_named(
    parents: &HashMap<u32, (u32, String)>,
    mut pid: u32,
    exe_name: &str,
) -> bool {
    if exe_name.is_empty() {
        return false;
    }
    // Bounded walk: parent pids can be recycled, so a stale chain could loop.
    for _ in 0..32 {
        let Some((parent, name)) = parents.get(&pid) else {
            return false;
        };
        if name == exe_name {
            return true;
        }
        if *parent == 0 || *parent == pid {
            return false;
        }
        pid = *parent;
    }
    false
}
