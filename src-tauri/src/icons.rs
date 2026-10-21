//! App icons exactly as Explorer and the taskbar draw them: IShellItemImageFactory,
//! full alpha, rendered at 64px so they stay sharp on high-DPI screens.
use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::HashMap;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDIBits,
    GetObjectW, HGDIOBJ,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY};
use windows::core::HSTRING;

const ICON_PX: i32 = 64;

/// `"C:\App\app.exe",0` -> `C:\App\app.exe`
pub fn clean_icon_path(raw: &str) -> String {
    let s = raw.trim();
    let s = match s.rsplit_once(',') {
        Some((path, idx)) if idx.trim().trim_start_matches('-').chars().all(|c| c.is_ascii_digit()) => path,
        _ => s,
    };
    s.trim().trim_matches('"').to_string()
}

/// `%SystemRoot%\x.exe` -> `C:\Windows\x.exe`
fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                out.push_str(&std::env::var(name).unwrap_or_else(|_| format!("%{name}%")));
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn icon_png(path: &str) -> Option<Vec<u8>> {
    unsafe {
        // Harmless if this thread already initialised COM
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()?;
        let hbmp = factory
            .GetImage(SIZE { cx: ICON_PX, cy: ICON_PX }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)
            .ok()?;
        let obj = HGDIOBJ(hbmp.0);

        let mut bm = BITMAP::default();
        GetObjectW(obj, size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w.max(0) * h.max(0) * 4) as usize];
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(dc, hbmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        let _ = DeleteDC(dc);
        let _ = DeleteObject(obj);
        if lines == 0 || px.is_empty() {
            return None;
        }

        // BGRA, premultiplied -> straight RGBA. Some legacy icons carry no alpha at all.
        let has_alpha = px.chunks_exact(4).any(|p| p[3] != 0);
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
            if !has_alpha {
                p[3] = 255;
            } else if p[3] > 0 && p[3] < 255 {
                let a = p[3] as u32;
                for c in &mut p[..3] {
                    *c = ((*c as u32 * 255) / a).min(255) as u8;
                }
            }
        }

        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().ok()?.write_image_data(&px).ok()?;
        Some(out)
    }
}

/// Icons for the given paths as base64 PNG, keyed by the input path.
pub fn extract(paths: &[String]) -> HashMap<String, String> {
    paths
        .iter()
        .filter_map(|p| {
            let full = expand_env(p);
            std::path::Path::new(&full).exists().then_some(())?;
            icon_png(&full).map(|png| (p.clone(), STANDARD.encode(png)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_icon_paths() {
        assert_eq!(clean_icon_path(r#""C:\App\app.exe",0"#), r"C:\App\app.exe");
        assert_eq!(clean_icon_path(r"C:\App\app.exe,-101"), r"C:\App\app.exe");
        assert_eq!(clean_icon_path(r"C:\App\app.ico"), r"C:\App\app.ico");
        assert_eq!(clean_icon_path(r"C:\Weird, Inc\app.exe"), r"C:\Weird, Inc\app.exe");
    }

    #[test]
    fn expands_env_vars() {
        let root = std::env::var("SystemRoot").unwrap();
        assert_eq!(expand_env(r"%SystemRoot%\x.exe"), format!(r"{root}\x.exe"));
        assert_eq!(expand_env("100% sure"), "100% sure");
    }

    #[test]
    fn renders_a_real_icon() {
        let explorer = expand_env(r"%SystemRoot%\explorer.exe");
        let png = icon_png(&explorer).expect("explorer.exe has an icon");
        assert_eq!(&png[1..4], b"PNG");
    }
}
