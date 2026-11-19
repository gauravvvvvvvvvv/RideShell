//! The real icon of an installed program, as PNG.
//!
//! Launchers and the tools list show each program's own icon, read from its
//! executable with Windows' icon APIs. Programs without one (scripts such as
//! npm's `.cmd` wrappers, store aliases) return `None`, and the UI shows a
//! neutral placeholder instead. Results are cached for the app's lifetime.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const SIZE: i32 = 64;

fn cache() -> &'static Mutex<HashMap<String, Option<Vec<u8>>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Vec<u8>>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

pub fn png_for(path: &str) -> Option<Vec<u8>> {
    let key = path.to_ascii_lowercase();
    if let Some(hit) = cache().lock().ok()?.get(&key) {
        return hit.clone();
    }
    let lower = key.as_str();
    let result = if lower.ends_with(".exe") || lower.ends_with(".com") { extract(path) } else { None };
    if let Ok(mut c) = cache().lock() {
        c.insert(key, result.clone());
    }
    result
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().ok()?;
        w.write_image_data(rgba).ok()?;
    }
    Some(out)
}

#[cfg(windows)]
fn extract(path: &str) -> Option<Vec<u8>> {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, GetIconInfo, PrivateExtractIconsW, HICON, ICONINFO,
    };

    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; 260];
    let n = wide.len().min(259);
    buf[..n].copy_from_slice(&wide[..n]);
    let mut icon: HICON = std::ptr::null_mut();
    let mut id = 0u32;
    let got = unsafe { PrivateExtractIconsW(buf.as_ptr(), 0, SIZE, SIZE, &mut icon, &mut id, 1, 0) };
    if got == 0 || got == u32::MAX || icon.is_null() {
        return None;
    }

    let result = unsafe {
        let mut info: ICONINFO = std::mem::zeroed();
        if GetIconInfo(icon, &mut info) == 0 {
            DestroyIcon(icon);
            return None;
        }
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        let read = |bitmap| -> Option<Vec<u8>> {
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE,
                biHeight: -SIZE, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
            let lines = GetDIBits(dc, bitmap, 0, SIZE as u32, px.as_mut_ptr().cast(), &mut bmi, DIB_RGB_COLORS);
            (lines != 0).then_some(px)
        };
        let color = if info.hbmColor.is_null() { None } else { read(info.hbmColor) };
        let mask = if info.hbmMask.is_null() { None } else { read(info.hbmMask) };
        DeleteDC(dc);
        if !info.hbmColor.is_null() {
            DeleteObject(info.hbmColor);
        }
        if !info.hbmMask.is_null() {
            DeleteObject(info.hbmMask);
        }
        DestroyIcon(icon);

        let mut px = color?;
        // Old-style icons have no alpha channel: take transparency from the mask.
        if px.chunks_exact(4).all(|p| p[3] == 0) {
            let mask = mask?;
            for (p, m) in px.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
                p[3] = if m[0] == 0 { 255 } else { 0 };
            }
        }
        // BGRA -> RGBA
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        if px.chunks_exact(4).all(|p| p[3] == 0) {
            return None;
        }
        encode_png(SIZE as u32, SIZE as u32, &px)
    };
    result
}

#[cfg(not(windows))]
fn extract(_path: &str) -> Option<Vec<u8>> {
    let _ = encode_png;
    None
}

/// Decode `%XX` escapes (the UI sends paths with encodeURIComponent).
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
