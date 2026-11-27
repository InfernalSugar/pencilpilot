//! The keyboard through libxkbcommon (loaded at run time): the compositor's keymap, so
//! every layout, its modifiers and what each key types are as the person set them up.

use std::ffi::{CStr, CString, c_char};
use std::ptr::null_mut;

use xkbcommon_dl::{
    XKB_MOD_NAME_ALT, XKB_MOD_NAME_CAPS, XKB_MOD_NAME_CTRL, XKB_MOD_NAME_LOGO, XKB_MOD_NAME_SHIFT,
    XkbCommon, xkb_context, xkb_context_flags, xkb_keymap, xkb_keymap_compile_flags,
    xkb_keymap_format, xkb_state, xkb_state_component,
};

use crate::input::Mods;

/// A keymap and the state of its modifiers.
pub struct Keyboard {
    x: &'static XkbCommon,
    ctx: *mut xkb_context,
    keymap: *mut xkb_keymap,
    state: *mut xkb_state,
}

// the xkb objects are used from one thread at a time (the window's)
unsafe impl Send for Keyboard {}

impl Keyboard {
    /// From a keymap in xkb's text format (what the compositor sends). None without
    /// libxkbcommon, or for a keymap it can't read.
    pub fn from_text(text: &str) -> Option<Keyboard> {
        let x = xkbcommon_dl::xkbcommon_option()?;
        let text = CString::new(text.trim_end_matches('\0')).ok()?;
        // SAFETY: the libxkbcommon calls, each object freed once in Drop
        unsafe {
            let ctx = (x.xkb_context_new)(xkb_context_flags::XKB_CONTEXT_NO_FLAGS);
            if ctx.is_null() {
                return None;
            }
            let keymap = (x.xkb_keymap_new_from_string)(
                ctx,
                text.as_ptr(),
                xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1,
                xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
            );
            if keymap.is_null() {
                (x.xkb_context_unref)(ctx);
                return None;
            }
            let state = (x.xkb_state_new)(keymap);
            if state.is_null() {
                (x.xkb_keymap_unref)(keymap);
                (x.xkb_context_unref)(ctx);
                return None;
            }
            Some(Keyboard {
                x,
                ctx,
                keymap,
                state,
            })
        }
    }

    /// The modifiers as the compositor reports them.
    pub fn set_mods(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        // SAFETY: a live state
        unsafe {
            (self.x.xkb_state_update_mask)(self.state, depressed, latched, locked, 0, 0, group)
        };
    }

    pub fn mods(&self) -> Mods {
        let on = |name: &[u8]| {
            // SAFETY: a live state; the names are NUL-terminated constants
            unsafe {
                (self.x.xkb_state_mod_name_is_active)(
                    self.state,
                    name.as_ptr() as *const c_char,
                    xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
                ) > 0
            }
        };
        Mods {
            shift: on(XKB_MOD_NAME_SHIFT),
            ctrl: on(XKB_MOD_NAME_CTRL),
            alt: on(XKB_MOD_NAME_ALT),
            logo: on(XKB_MOD_NAME_LOGO),
            caps_lock: on(XKB_MOD_NAME_CAPS),
        }
    }

    /// A key (evdev code) in the current state: its keysym, the keysym's name, and what it
    /// types.
    pub fn key(&self, code: u32) -> (u32, String, Option<String>) {
        let kc = code + 8; // xkb keycodes are evdev's + 8
        // SAFETY: a live state; the buffers are big enough for what they're asked
        unsafe {
            let sym = (self.x.xkb_state_key_get_one_sym)(self.state, kc);
            let mut name = [0 as c_char; 64];
            (self.x.xkb_keysym_get_name)(sym, name.as_mut_ptr(), name.len());
            let name = CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned();
            let mut buf = [0 as c_char; 64];
            let n = (self.x.xkb_state_key_get_utf8)(self.state, kc, buf.as_mut_ptr(), buf.len());
            let text = (n > 0)
                .then(|| CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
                // control characters (Escape, Backspace, Ctrl+C…) type nothing visible
                .filter(|t| !t.chars().any(char::is_control));
            (sym, name, text)
        }
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        // SAFETY: each made once in from_text
        unsafe {
            (self.x.xkb_state_unref)(self.state);
            (self.x.xkb_keymap_unref)(self.keymap);
            (self.x.xkb_context_unref)(self.ctx);
        }
        self.state = null_mut();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A US keymap from the machine's own xkb data (skips without libxkbcommon).
    fn us() -> Option<Keyboard> {
        let x = xkbcommon_dl::xkbcommon_option()?;
        // SAFETY: plain libxkbcommon calls; the string is copied out before freeing
        unsafe {
            let ctx = (x.xkb_context_new)(xkb_context_flags::XKB_CONTEXT_NO_FLAGS);
            let names = xkbcommon_dl::xkb_rule_names {
                rules: std::ptr::null(),
                model: std::ptr::null(),
                layout: c"us".as_ptr(),
                variant: std::ptr::null(),
                options: std::ptr::null(),
            };
            let km = (x.xkb_keymap_new_from_names)(
                ctx,
                &names,
                xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
            );
            if km.is_null() {
                return None;
            }
            let s = (x.xkb_keymap_get_as_string)(km, xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1);
            let text = CStr::from_ptr(s).to_string_lossy().into_owned();
            libc_free(s as *mut _);
            (x.xkb_keymap_unref)(km);
            (x.xkb_context_unref)(ctx);
            Keyboard::from_text(&text)
        }
    }

    unsafe extern "C" {
        #[link_name = "free"]
        fn libc_free(p: *mut std::ffi::c_void);
    }

    #[test]
    fn keys_type_what_the_layout_says() {
        let Some(mut kb) = us() else {
            eprintln!("skipped: no libxkbcommon or xkb data");
            return;
        };
        // evdev: KEY_A = 30, KEY_ESC = 1, KEY_1 = 2
        let (_, name, text) = kb.key(30);
        assert_eq!((name.as_str(), text.as_deref()), ("a", Some("a")));
        let (_, name, text) = kb.key(1);
        assert_eq!((name.as_str(), text), ("Escape", None));
        // Shift held (mod index 0 in xkb's standard keymaps)
        kb.set_mods(1, 0, 0, 0);
        assert!(kb.mods().shift);
        let (_, name, text) = kb.key(2);
        assert_eq!((name.as_str(), text.as_deref()), ("exclam", Some("!")));
        // Ctrl (index 2): Ctrl+A types nothing
        kb.set_mods(4, 0, 0, 0);
        assert!(kb.mods().ctrl && !kb.mods().shift);
        assert_eq!(kb.key(30).2, None);
    }
}
