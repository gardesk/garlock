//! Keyboard input handling for garlock
//!
//! Uses xkbcommon for proper keycode-to-character mapping and
//! modifier state tracking (Caps Lock, etc.).

use anyhow::{Context, Result};
use xkbcommon::xkb::{
    self,
    keysyms::{
        KEY_Alt_L, KEY_Alt_R, KEY_BackSpace, KEY_Caps_Lock, KEY_Control_L, KEY_Control_R,
        KEY_Delete, KEY_Down, KEY_End, KEY_Escape, KEY_F1, KEY_F12, KEY_Home, KEY_Insert,
        KEY_KP_Enter, KEY_Left, KEY_Menu, KEY_Meta_L, KEY_Meta_R, KEY_Num_Lock, KEY_Page_Down,
        KEY_Page_Up, KEY_Pause, KEY_Print, KEY_Return, KEY_Right, KEY_Scroll_Lock, KEY_Shift_L,
        KEY_Shift_R, KEY_Super_L, KEY_Super_R, KEY_Up, KEY_c, KEY_u,
    },
    Keycode, Keysym, MOD_NAME_CAPS, MOD_NAME_CTRL, MOD_NAME_SHIFT, STATE_MODS_EFFECTIVE,
    STATE_MODS_LOCKED,
};

/// Result of processing a key event
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyResult {
    /// A printable character was typed
    Char(char),
    /// Backspace was pressed
    Backspace,
    /// Enter/Return was pressed (submit password)
    Enter,
    /// Escape was pressed (dev mode exit)
    Escape,
    /// Clear buffer (Ctrl+U)
    Clear,
    /// A modifier key was pressed (no visual feedback)
    Modifier,
    /// Key should be ignored (function keys, etc.)
    Ignored,
}

/// Keyboard state handler using XKB
pub struct Keyboard {
    #[allow(dead_code)]
    context: xkb::Context,
    #[allow(dead_code)]
    keymap: xkb::Keymap,
    state: xkb::State,
}

impl Keyboard {
    /// Create a new keyboard handler with default keymap
    pub fn new() -> Result<Self> {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);

        // Load keymap from system defaults (respects XKBLAYOUT, etc.)
        let keymap = xkb::Keymap::new_from_names(
            &context,
            "", // rules (empty = default)
            "", // model
            "", // layout
            "", // variant
            None, // options
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .context("Failed to create XKB keymap")?;

        let state = xkb::State::new(&keymap);

        tracing::debug!("XKB keyboard initialized");
        Ok(Self {
            context,
            keymap,
            state,
        })
    }

    /// Process a key event and return the result
    ///
    /// `keycode` is the X11 keycode (8-based).
    /// `pressed` is true for key press, false for key release.
    pub fn process_key(&mut self, keycode: u8, pressed: bool) -> KeyResult {
        // X11 keycodes map directly to XKB keycodes
        let xkb_keycode: Keycode = keycode.into();

        // Update XKB state
        let direction = if pressed {
            xkb::KeyDirection::Down
        } else {
            xkb::KeyDirection::Up
        };
        self.state.update_key(xkb_keycode, direction);

        // Only process key presses, not releases
        if !pressed {
            return KeyResult::Ignored;
        }

        // Get the keysym for this key
        let keysym: Keysym = self.state.key_get_one_sym(xkb_keycode);
        let keysym_raw = keysym.raw();

        // Check for special keys first
        if keysym_raw == KEY_Escape {
            return KeyResult::Escape;
        }

        if keysym_raw == KEY_Return || keysym_raw == KEY_KP_Enter {
            return KeyResult::Enter;
        }

        if keysym_raw == KEY_BackSpace {
            return KeyResult::Backspace;
        }

        // Modifier keys - no visual feedback
        if matches!(
            keysym_raw,
            KEY_Shift_L
                | KEY_Shift_R
                | KEY_Control_L
                | KEY_Control_R
                | KEY_Alt_L
                | KEY_Alt_R
                | KEY_Super_L
                | KEY_Super_R
                | KEY_Meta_L
                | KEY_Meta_R
                | KEY_Caps_Lock
                | KEY_Num_Lock
                | KEY_Scroll_Lock
        ) {
            return KeyResult::Modifier;
        }

        // Function keys, navigation, etc. - ignore
        if (KEY_F1..=KEY_F12).contains(&keysym_raw)
            || matches!(
                keysym_raw,
                KEY_Home
                    | KEY_End
                    | KEY_Page_Up
                    | KEY_Page_Down
                    | KEY_Left
                    | KEY_Right
                    | KEY_Up
                    | KEY_Down
                    | KEY_Insert
                    | KEY_Delete
                    | KEY_Print
                    | KEY_Pause
                    | KEY_Menu
            )
        {
            return KeyResult::Ignored;
        }

        // Check for Ctrl+U (clear buffer)
        if self.ctrl_active() && keysym_raw == KEY_u {
            return KeyResult::Clear;
        }

        // Check for Ctrl+C (also clear, common convention)
        if self.ctrl_active() && keysym_raw == KEY_c {
            return KeyResult::Clear;
        }

        // Ignore other Ctrl combinations
        if self.ctrl_active() {
            return KeyResult::Ignored;
        }

        // Try to get a character from the key
        let utf8_string = self.state.key_get_utf8(xkb_keycode);

        if !utf8_string.is_empty() {
            if let Some(c) = utf8_string.chars().next() {
                // Filter out control characters
                if c.is_control() {
                    return KeyResult::Ignored;
                }
                return KeyResult::Char(c);
            }
        }

        KeyResult::Ignored
    }

    /// Check if Caps Lock is currently active
    pub fn caps_lock_active(&self) -> bool {
        self.state.mod_name_is_active(MOD_NAME_CAPS, STATE_MODS_LOCKED)
    }

    /// Check if Ctrl is currently held
    pub fn ctrl_active(&self) -> bool {
        self.state
            .mod_name_is_active(MOD_NAME_CTRL, STATE_MODS_EFFECTIVE)
    }

    /// Check if Shift is currently held
    #[allow(dead_code)]
    pub fn shift_active(&self) -> bool {
        self.state
            .mod_name_is_active(MOD_NAME_SHIFT, STATE_MODS_EFFECTIVE)
    }
}

impl Default for Keyboard {
    fn default() -> Self {
        Self::new().expect("Failed to initialize keyboard")
    }
}
