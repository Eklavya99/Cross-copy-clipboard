//! The Windows agent's desktop side: global hotkeys and the cross-copy /
//! cross-cut / cross-paste flows.
//!
//! Milestone 0 (desktop spike): the Cross Clipboard is local to this machine,
//! so `Ctrl+Alt+C` then `Ctrl+Alt+V` works within one computer. Networking
//! comes in the next milestone.

use std::ptr;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use crossclip_core::ClipContent;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MB_ICONWARNING, MSG, WM_HOTKEY,
};

use crate::clipboard::{self, Formats};
use crate::input;
use crate::keys::{Action, Hotkey};

/// How long to wait for the user to let go of `Ctrl+Alt` before we type.
const MODIFIER_RELEASE_TIMEOUT: Duration = Duration::from_secs(1);
/// How long the app gets to put its selection on the clipboard after `Ctrl+C`.
const COPY_WAIT: Duration = Duration::from_millis(400);
/// How long the app gets to read the clipboard after `Ctrl+V` before we restore it.
const PASTE_RESTORE_DELAY: Duration = Duration::from_millis(500);
/// Larger normal-clipboard contents are not snapshotted (and not restored).
const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

struct Agent {
    window: HWND,
    formats: Formats,
    item: Option<ClipContent>,
}

/// Runs the agent until the process is stopped.
pub fn run() -> Result<()> {
    let window = create_message_window()?;
    let hotkeys: Vec<(Action, Hotkey)> = Action::ALL
        .iter()
        .map(|&a| (a, a.default_hotkey()))
        .collect();
    for (id, (action, hotkey)) in hotkeys.iter().enumerate() {
        register_hotkey(window, id as i32 + 1, hotkey)
            .with_context(|| format!("registering {hotkey} for {action}"))?;
    }
    let mut agent = Agent {
        window,
        formats: Formats::register()?,
        item: None,
    };

    println!(
        "CrossClip {} is running (local-only desktop spike).",
        crossclip_core::VERSION
    );
    for (action, hotkey) in &hotkeys {
        println!("  {hotkey:<12} {action}");
    }
    println!("Close this window to stop.");

    // SAFETY: MSG is plain data; GetMessageW fills it in.
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: `msg` is a valid out-pointer; null HWND means "any window of this thread".
        match unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } {
            0 => return Ok(()),
            -1 => return Err(std::io::Error::last_os_error()).context("message loop"),
            _ => {}
        }
        if msg.message == WM_HOTKEY {
            let Some(&(action, _)) = hotkeys.get(msg.wParam.wrapping_sub(1)) else {
                continue;
            };
            if let Err(err) = agent.handle(action) {
                eprintln!("{action} failed: {err:#}");
                beep();
            }
        } else {
            // SAFETY: `msg` came from GetMessageW.
            unsafe { DispatchMessageW(&msg) };
        }
    }
}

impl Agent {
    fn handle(&mut self, action: Action) -> Result<()> {
        match action {
            Action::CrossCopy | Action::CrossCut => self.cross_copy(action),
            Action::CrossPaste => self.cross_paste(),
        }
    }

    fn cross_copy(&mut self, action: Action) -> Result<()> {
        input::wait_for_modifiers_released(MODIFIER_RELEASE_TIMEOUT);
        let terminal = input::foreground_is_terminal();
        let snapshot = clipboard::snapshot(self.window, MAX_SNAPSHOT_BYTES)?;
        let before = clipboard::sequence_number();

        input::send_keystroke(action.keystroke(terminal))?;
        let copied = clipboard::wait_for_change(before, COPY_WAIT);
        // If nothing was selected the clipboard didn't change: use what is
        // already on it (e.g. a screenshot taken with Win+Shift+S).
        let content = clipboard::read(self.window, &self.formats)?;
        if copied {
            match &snapshot {
                Some(snapshot) => clipboard::restore(self.window, &self.formats, snapshot)?,
                None => eprintln!("  (normal clipboard was too large to restore)"),
            }
        }

        let Some(content) = content else {
            bail!("nothing to copy: the clipboard has no text or image");
        };
        let source = if copied {
            "selection"
        } else {
            "current clipboard"
        };
        println!("{action}: {} from {source}", content.summary());
        self.item = Some(content);
        Ok(())
    }

    fn cross_paste(&mut self) -> Result<()> {
        let Some(item) = &self.item else {
            bail!("the Cross Clipboard is empty; use Ctrl+Alt+C first");
        };
        input::wait_for_modifiers_released(MODIFIER_RELEASE_TIMEOUT);
        let terminal = input::foreground_is_terminal();
        let snapshot = clipboard::snapshot(self.window, MAX_SNAPSHOT_BYTES)?;

        let ours = clipboard::write(self.window, &self.formats, item)?;
        input::send_keystroke(Action::CrossPaste.keystroke(terminal))?;
        sleep(PASTE_RESTORE_DELAY);

        // Don't clobber something the user copied in the meantime.
        match snapshot {
            Some(snapshot) if clipboard::sequence_number() == ours => {
                clipboard::restore(self.window, &self.formats, &snapshot)?
            }
            Some(_) => eprintln!("  (clipboard changed meanwhile; not restoring it)"),
            None => eprintln!("  (normal clipboard was too large to restore)"),
        }
        println!("cross-paste: {}", item.summary());
        Ok(())
    }
}

/// A hidden message-only window: it receives hotkey messages and owns the
/// clipboard when we write to it.
fn create_message_window() -> Result<HWND> {
    let class: Vec<u16> = "STATIC".encode_utf16().chain([0]).collect();
    let title: Vec<u16> = "CrossClip".encode_utf16().chain([0]).collect();
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let window = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            ptr::null_mut(),
            GetModuleHandleW(ptr::null()),
            ptr::null(),
        )
    };
    if window.is_null() {
        return Err(std::io::Error::last_os_error()).context("creating message window");
    }
    Ok(window)
}

fn register_hotkey(window: HWND, id: i32, hotkey: &Hotkey) -> Result<()> {
    let mut modifiers = MOD_NOREPEAT;
    for (on, flag) in [
        (hotkey.ctrl, MOD_CONTROL),
        (hotkey.alt, MOD_ALT),
        (hotkey.shift, MOD_SHIFT),
        (hotkey.win, MOD_WIN),
    ] {
        if on {
            modifiers |= flag;
        }
    }
    // SAFETY: `window` is our live window. The key is an uppercase ASCII
    // letter or digit, which equals its virtual-key code.
    if unsafe { RegisterHotKey(window, id, modifiers, hotkey.key as u32) } == 0 {
        return Err(std::io::Error::last_os_error())
            .context("the shortcut is probably already used by another app");
    }
    Ok(())
}

fn beep() {
    // SAFETY: no preconditions.
    unsafe { MessageBeep(MB_ICONWARNING) };
}
