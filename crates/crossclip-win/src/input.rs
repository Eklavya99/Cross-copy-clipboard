//! Synthetic keyboard input and foreground-window inspection.

use std::mem::size_of;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow};

use crate::keys::Keystroke;

const MODIFIERS: [VIRTUAL_KEY; 5] = [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN];

/// Window classes of terminal emulators, which need `Ctrl+Shift+C/V`.
const TERMINAL_CLASSES: [&str; 5] = [
    "ConsoleWindowClass",            // classic console (conhost)
    "CASCADIA_HOSTING_WINDOW_CLASS", // Windows Terminal
    "mintty",                        // Git Bash, Cygwin, MSYS2
    "VirtualConsoleClass",           // ConEmu / Cmder
    "PuTTY",
];

fn is_down(vk: VIRTUAL_KEY) -> bool {
    // SAFETY: no preconditions. The high bit is set while the key is down.
    unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
}

/// The hotkey fires while the user still holds `Ctrl+Alt`. If we sent `Ctrl+C`
/// now, the app would see `Ctrl+Alt+C`. Wait until the physical modifiers are
/// released; if they are still held after `timeout`, release them synthetically.
pub fn wait_for_modifiers_released(timeout: Duration) {
    let start = Instant::now();
    loop {
        let held: Vec<VIRTUAL_KEY> = MODIFIERS.into_iter().filter(|&vk| is_down(vk)).collect();
        if held.is_empty() {
            return;
        }
        if start.elapsed() >= timeout {
            let inputs: Vec<INPUT> = held.into_iter().map(|vk| key(vk, false)).collect();
            let _ = send(&inputs);
            return;
        }
        sleep(Duration::from_millis(10));
    }
}

/// Sends `Ctrl` (+ `Shift`) + key to the focused window.
pub fn send_keystroke(keystroke: Keystroke) -> Result<()> {
    let letter = keystroke.key as VIRTUAL_KEY;
    let mut inputs = vec![key(VK_CONTROL, true)];
    if keystroke.shift {
        inputs.push(key(VK_SHIFT, true));
    }
    inputs.extend([key(letter, true), key(letter, false)]);
    if keystroke.shift {
        inputs.push(key(VK_SHIFT, false));
    }
    inputs.push(key(VK_CONTROL, false));
    send(&inputs)
}

/// Whether the focused window is a terminal emulator.
pub fn foreground_is_terminal() -> bool {
    foreground_class().is_some_and(|class| TERMINAL_CLASSES.contains(&class.as_str()))
}

/// Window class name of the focused window, e.g. `Notepad`.
pub fn foreground_class() -> Option<String> {
    let mut buf = [0u16; 256];
    // SAFETY: `buf` is writable for the length we pass.
    let len = unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return None;
        }
        GetClassNameW(window, buf.as_mut_ptr(), buf.len() as i32)
    };
    (len > 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

fn key(vk: VIRTUAL_KEY, down: bool) -> INPUT {
    // Include the scan code as well: some apps (and remote-desktop clients)
    // look only at scan codes.
    // SAFETY: no preconditions.
    let scan = unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) } as u16;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: if down { 0 } else { KEYEVENTF_KEYUP },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<()> {
    // SAFETY: `inputs` is a valid slice of INPUT structs of the size we pass.
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent as usize != inputs.len() {
        bail!("SendInput delivered {sent} of {} key events", inputs.len());
    }
    Ok(())
}
