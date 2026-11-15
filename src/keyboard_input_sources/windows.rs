//! Keyboard layouts (HKLs) as input sources. Windows keeps a layout per
//! thread, so the current source is the foreground window's. Hex reads them
//! only while keyboard model switching is on.

use windows_sys::Win32::Globalization::{
    GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SLOCALIZEDDISPLAYNAME,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardLayoutList, HKL,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use crate::transcription::keyboard_model_switching::InputSource;

const LOCALE_NAME_MAX_LENGTH: usize = 85;
const LAYOUTS_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Keyboard Layouts";

/// Every Windows session has keyboard layouts.
pub(crate) fn available() -> bool {
    true
}

/// The HKL's 32 bits (x64 sign-extends them) as its stable ID, `04190419`.
fn id(layout: HKL) -> String {
    format!("{:08X}", layout as usize as u32)
}

/// The foreground window's layout; `None` without one, as on the lock
/// screen, or for a console that reports none.
pub(crate) fn current() -> Option<String> {
    // SAFETY: plain Win32 queries; a null window yields thread 0.
    let layout = unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return None;
        }
        GetKeyboardLayout(GetWindowThreadProcessId(window, std::ptr::null_mut()))
    };
    (!layout.is_null()).then(|| id(layout))
}

/// The layouts in the user's language bar.
pub(crate) fn enabled() -> Vec<InputSource> {
    // SAFETY: the first call sizes the buffer the second fills.
    let layouts = unsafe {
        let count = GetKeyboardLayoutList(0, std::ptr::null_mut());
        let mut layouts = vec![std::ptr::null_mut(); count.max(0) as usize];
        let filled = GetKeyboardLayoutList(count, layouts.as_mut_ptr());
        layouts.truncate(filled.max(0) as usize);
        layouts
    };
    let mut sources: Vec<InputSource> = layouts
        .into_iter()
        .map(|layout| {
            let bits = layout as usize as u32;
            let locale = locale_name(bits & 0xFFFF);
            let language = locale.as_deref().and_then(display_name);
            let keyboard = layout_text(bits);
            let name = match (language, keyboard) {
                (Some(language), Some(keyboard)) => format!("{language} – {keyboard}"),
                (Some(name), None) | (None, Some(name)) => name,
                (None, None) => id(layout),
            };
            InputSource {
                id: id(layout),
                name,
                languages: layout_languages(bits)
                    .into_iter()
                    .filter_map(locale_name)
                    .collect(),
            }
        })
        .collect();
    sources.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    sources.dedup_by(|a, b| a.id == b.id);
    sources
}

/// The language IDs a layout is typed in, most specific first. The low word
/// is the language the layout was added under; a plain keyboard's high word
/// is the keyboard's own language, which says more about what is dictated:
/// the Kazakh keyboard added under Russian (`043F0419`) types Kazakh.
fn layout_languages(layout: u32) -> Vec<u32> {
    let language = layout & 0xFFFF;
    let keyboard = layout >> 16;
    let mut languages = Vec::with_capacity(2);
    if keyboard & 0xF000 == 0 && keyboard != 0 && keyboard != language {
        languages.push(keyboard);
    }
    languages.push(language);
    languages
}

/// BCP 47 for a language ID, `ru-RU`.
fn locale_name(language: u32) -> Option<String> {
    let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH];
    // SAFETY: the buffer holds LOCALE_NAME_MAX_LENGTH characters.
    let length = unsafe { LCIDToLocaleName(language, buffer.as_mut_ptr(), buffer.len() as i32, 0) };
    wide(&buffer, length)
}

/// The locale's name in the display language, `Russian (Russia)`.
fn display_name(locale: &str) -> Option<String> {
    let locale: Vec<u16> = locale.encode_utf16().chain([0]).collect();
    let mut buffer = [0u16; 256];
    // SAFETY: both buffers are NUL-terminated or sized as passed.
    let length = unsafe {
        GetLocaleInfoEx(
            locale.as_ptr(),
            LOCALE_SLOCALIZEDDISPLAYNAME,
            buffer.as_mut_ptr(),
            buffer.len() as i32,
        )
    };
    wide(&buffer, length)
}

/// A written length that counts the NUL.
fn wide(buffer: &[u16], length: i32) -> Option<String> {
    let length = usize::try_from(length).ok()?.checked_sub(1)?;
    let text = String::from_utf16_lossy(buffer.get(..length)?);
    (!text.is_empty()).then_some(text)
}

/// The keyboard's own name, `Russian` or `United States-Dvorak`, from the
/// registry entry its HKL's high word selects: an IME's whole HKL, a
/// `Layout Id` for `0xFnnn`, or else the layout's language ID.
fn layout_text(layout: u32) -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let layouts = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(LAYOUTS_KEY)
        .ok()?;
    let device = layout >> 16;
    let klid = match device & 0xF000 {
        0xE000 => format!("{layout:08X}"),
        0xF000 => {
            let wanted = device & 0x0FFF;
            layouts.enum_keys().flatten().find(|klid| {
                layouts
                    .open_subkey(klid)
                    .and_then(|key| key.get_value::<String, _>("Layout Id"))
                    .is_ok_and(|id| u32::from_str_radix(id.trim(), 16) == Ok(wanted))
            })?
        }
        _ => format!("{device:08X}"),
    };
    layouts
        .open_subkey(klid)
        .and_then(|key| key.get_value::<String, _>("Layout Text"))
        .ok()
        .filter(|text| !text.is_empty())
}

/// Polled by the runtime while switching is on; reports the first source
/// once, like the macOS observer, then only changes.
#[derive(Default)]
pub(crate) struct Watcher {
    last: Option<String>,
}

impl Watcher {
    pub(crate) fn poll(&mut self) -> Option<Option<String>> {
        let current = current()?;
        (self.last.as_ref() != Some(&current)).then(|| {
            self.last = Some(current.clone());
            Some(current)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keyboard_added_under_another_language_types_its_own_first() {
        // Kazakh keyboard under Russian, plain Russian, US, and a variant
        // (Dvorak, 0xF002) and an IME (0xE0010411) whose high word is no language.
        assert_eq!(layout_languages(0x043F_0419), [0x043F, 0x0419]);
        assert_eq!(layout_languages(0x0419_0419), [0x0419]);
        assert_eq!(layout_languages(0x0409_0409), [0x0409]);
        assert_eq!(layout_languages(0xF002_0409), [0x0409]);
        assert_eq!(layout_languages(0xE001_0411), [0x0411]);
        assert_eq!(
            layout_languages(0x043F_0419)
                .into_iter()
                .filter_map(locale_name)
                .collect::<Vec<_>>(),
            ["kk-KZ", "ru-RU"]
        );
    }
}
