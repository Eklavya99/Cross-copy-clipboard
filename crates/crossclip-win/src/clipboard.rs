//! Win32 clipboard access: reading and writing Cross Clipboard content, and
//! snapshotting / restoring whatever the user had on the normal clipboard.

use std::ptr;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use crossclip_core::ClipContent;
use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows_sys::Win32::System::Ole::{
    CF_BITMAP, CF_DIB, CF_DIBV5, CF_DSPBITMAP, CF_DSPENHMETAFILE, CF_DSPMETAFILEPICT,
    CF_ENHMETAFILE, CF_METAFILEPICT, CF_OWNERDISPLAY, CF_PALETTE, CF_UNICODETEXT,
};

use crate::dib;

/// Formats whose clipboard handle is not an `HGLOBAL` (GDI objects and
/// private handles), so their bytes can't be copied for a snapshot.
fn is_handle_format(format: u32) -> bool {
    const CF_PRIVATEFIRST: u32 = 0x0200;
    const CF_GDIOBJLAST: u32 = 0x03FF;
    [
        CF_BITMAP,
        CF_METAFILEPICT,
        CF_PALETTE,
        CF_ENHMETAFILE,
        CF_OWNERDISPLAY,
    ]
    .into_iter()
    .chain([CF_DSPBITMAP, CF_DSPMETAFILEPICT, CF_DSPENHMETAFILE])
    .any(|f| u32::from(f) == format)
        || (CF_PRIVATEFIRST..=CF_GDIOBJLAST).contains(&format)
}

/// Clipboard formats registered by name at runtime.
pub struct Formats {
    png: u32,
    /// Windows clipboard history and cloud clipboard skip content carrying these.
    exclude_from_monitoring: u32,
    can_include_in_history: u32,
    can_upload_to_cloud: u32,
}

impl Formats {
    pub fn register() -> Result<Self> {
        Ok(Formats {
            png: register_format("PNG")?,
            exclude_from_monitoring: register_format(
                "ExcludeClipboardContentFromMonitorProcessing",
            )?,
            can_include_in_history: register_format("CanIncludeInClipboardHistory")?,
            can_upload_to_cloud: register_format("CanUploadToCloudClipboard")?,
        })
    }
}

fn register_format(name: &str) -> Result<u32> {
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call.
    match unsafe { RegisterClipboardFormatW(wide.as_ptr()) } {
        0 => Err(std::io::Error::last_os_error()).context(format!("registering {name}")),
        id => Ok(id),
    }
}

/// Everything that was on the clipboard, copied into our own memory.
pub struct Snapshot(Vec<(u32, Vec<u8>)>);

/// Increments on every clipboard change, system-wide.
pub fn sequence_number() -> u32 {
    // SAFETY: no preconditions.
    unsafe { GetClipboardSequenceNumber() }
}

/// Waits up to `timeout` for the clipboard to change from `before`.
pub fn wait_for_change(before: u32, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if sequence_number() != before {
            // The source app may still be adding formats; give it a moment.
            sleep(Duration::from_millis(30));
            return true;
        }
        sleep(Duration::from_millis(10));
    }
    false
}

/// An open clipboard; closed on drop. Only one app can have it open at a time.
struct Open;

impl Open {
    fn new(owner: HWND) -> Result<Self> {
        for _ in 0..25 {
            // SAFETY: `owner` is our window handle (or null).
            if unsafe { OpenClipboard(owner) } != 0 {
                return Ok(Open);
            }
            sleep(Duration::from_millis(20));
        }
        Err(std::io::Error::last_os_error()).context("clipboard is busy (held by another app)")
    }

    fn has(&self, format: u32) -> bool {
        // SAFETY: the clipboard is open.
        unsafe { IsClipboardFormatAvailable(format) != 0 }
    }

    /// Copies the bytes of an `HGLOBAL`-backed format.
    fn get(&self, format: u32) -> Option<Vec<u8>> {
        // SAFETY: the clipboard is open; the handle stays valid until it closes.
        // We lock it, copy `GlobalSize` bytes and unlock again.
        unsafe {
            let handle = GetClipboardData(format);
            if handle.is_null() {
                return None;
            }
            let data = GlobalLock(handle) as *const u8;
            if data.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(data, GlobalSize(handle)).to_vec();
            GlobalUnlock(handle);
            Some(bytes)
        }
    }

    fn set(&self, format: u32, bytes: &[u8]) -> Result<()> {
        // SAFETY: the clipboard is open and was emptied by us, so we own it. The
        // allocation is at least `bytes.len()` long. On success the system owns
        // the memory; on failure we free it.
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
            if handle.is_null() {
                bail!("out of memory for clipboard data");
            }
            let dest = GlobalLock(handle) as *mut u8;
            if dest.is_null() {
                GlobalFree(handle);
                bail!("could not lock clipboard memory");
            }
            ptr::copy_nonoverlapping(bytes.as_ptr(), dest, bytes.len());
            GlobalUnlock(handle);
            if SetClipboardData(format, handle).is_null() {
                let err = std::io::Error::last_os_error();
                GlobalFree(handle);
                return Err(err).context(format!("setting clipboard format {format}"));
            }
        }
        Ok(())
    }

    fn empty(&self) -> Result<()> {
        // SAFETY: the clipboard is open.
        if unsafe { EmptyClipboard() } == 0 {
            return Err(std::io::Error::last_os_error()).context("emptying clipboard");
        }
        Ok(())
    }

    /// Keeps our temporary writes out of Win+V history and cloud clipboard.
    fn hide_from_history(&self, formats: &Formats) -> Result<()> {
        let zero = 0u32.to_le_bytes();
        self.set(formats.exclude_from_monitoring, &zero)?;
        self.set(formats.can_include_in_history, &zero)?;
        self.set(formats.can_upload_to_cloud, &zero)
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: we opened the clipboard in `Open::new`.
        unsafe { CloseClipboard() };
    }
}

/// Reads the clipboard as Cross Clipboard content: text if there is any,
/// otherwise an image. Returns `None` for anything else (e.g. files).
pub fn read(owner: HWND, formats: &Formats) -> Result<Option<ClipContent>> {
    let clip = Open::new(owner)?;
    let text_format = u32::from(CF_UNICODETEXT);
    if clip.has(text_format) {
        if let Some(bytes) = clip.get(text_format) {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&u| u != 0)
                .collect();
            return Ok(Some(ClipContent::Text(String::from_utf16_lossy(&units))));
        }
    }
    if clip.has(formats.png) {
        if let Some(png) = clip.get(formats.png) {
            // Some apps pad HGLOBALs; the PNG decoder ignores trailing bytes.
            if let Ok((width, height)) = dib::png_dimensions(&png) {
                return Ok(Some(ClipContent::Image { png, width, height }));
            }
        }
    }
    for format in [u32::from(CF_DIBV5), u32::from(CF_DIB)] {
        if let Some(bytes) = clip.has(format).then(|| clip.get(format)).flatten() {
            let image = dib::decode_dib(&bytes).context("reading clipboard bitmap")?;
            let png = dib::encode_png(&image)?;
            return Ok(Some(ClipContent::Image {
                png,
                width: image.width,
                height: image.height,
            }));
        }
    }
    Ok(None)
}

/// Replaces the clipboard with `content`, hidden from clipboard history.
/// Returns the clipboard sequence number right after the write.
pub fn write(owner: HWND, formats: &Formats, content: &ClipContent) -> Result<u32> {
    // Encode before opening so the clipboard is held as briefly as possible.
    let data: Vec<(u32, Vec<u8>)> = match content {
        ClipContent::Text(text) => {
            let wide: Vec<u8> = text
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            vec![(u32::from(CF_UNICODETEXT), wide)]
        }
        ClipContent::Image { png, .. } => {
            let dibv5 = dib::encode_dibv5(&dib::decode_png(png)?);
            // Windows synthesizes CF_DIB and CF_BITMAP from CF_DIBV5 for older apps.
            vec![(formats.png, png.clone()), (u32::from(CF_DIBV5), dibv5)]
        }
    };
    let clip = Open::new(owner)?;
    clip.empty()?;
    for (format, bytes) in &data {
        clip.set(*format, bytes)?;
    }
    clip.hide_from_history(formats)?;
    drop(clip);
    Ok(sequence_number())
}

/// Copies every restorable format on the clipboard. Returns `None` when the
/// content is larger than `max_bytes` (for example a huge spreadsheet range);
/// the caller then leaves the clipboard as it is instead of restoring.
pub fn snapshot(owner: HWND, max_bytes: usize) -> Result<Option<Snapshot>> {
    let clip = Open::new(owner)?;
    let mut saved = Vec::new();
    let mut total = 0;
    let mut format = 0;
    loop {
        // SAFETY: the clipboard is open.
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }
        if is_handle_format(format) {
            continue;
        }
        if let Some(bytes) = clip.get(format) {
            total += bytes.len();
            if total > max_bytes {
                return Ok(None);
            }
            saved.push((format, bytes));
        }
    }
    Ok(Some(Snapshot(saved)))
}

/// Puts a snapshot back on the clipboard, hidden from clipboard history
/// (the original copy is already there).
pub fn restore(owner: HWND, formats: &Formats, snapshot: &Snapshot) -> Result<()> {
    let clip = Open::new(owner)?;
    clip.empty()?;
    for (format, bytes) in &snapshot.0 {
        // A single odd format must not prevent restoring the rest.
        if let Err(err) = clip.set(*format, bytes) {
            eprintln!("crossclip: could not restore clipboard format {format}: {err:#}");
        }
    }
    if !snapshot.0.is_empty() {
        clip.hide_from_history(formats)?;
    }
    Ok(())
}
