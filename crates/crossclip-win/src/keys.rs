//! Shortcut parsing and the keystrokes each Cross Clipboard action sends.
//! Plain logic with no Win32 calls, so it is tested on every platform.

use std::fmt;
use std::str::FromStr;

use anyhow::{bail, Context};

/// What the user asked for with a global shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    CrossCopy,
    CrossCut,
    CrossPaste,
}

impl Action {
    pub const ALL: [Action; 3] = [Action::CrossCopy, Action::CrossCut, Action::CrossPaste];

    /// The default global shortcut for this action.
    pub fn default_hotkey(self) -> Hotkey {
        let key = match self {
            Action::CrossCopy => 'C',
            Action::CrossCut => 'X',
            Action::CrossPaste => 'V',
        };
        Hotkey {
            ctrl: true,
            alt: true,
            shift: false,
            win: false,
            key,
        }
    }

    /// The keystroke to send to the focused window to perform the normal
    /// clipboard operation underneath this action.
    ///
    /// Terminals treat `Ctrl+C` as "interrupt", so they get `Ctrl+Shift+C/V`
    /// instead. Terminals can't cut, so cross-cut copies there.
    pub fn keystroke(self, in_terminal: bool) -> Keystroke {
        let key = match (self, in_terminal) {
            (Action::CrossCopy, _) | (Action::CrossCut, true) => 'C',
            (Action::CrossCut, false) => 'X',
            (Action::CrossPaste, _) => 'V',
        };
        Keystroke {
            shift: in_terminal,
            key,
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Action::CrossCopy => "cross-copy",
            Action::CrossCut => "cross-cut",
            Action::CrossPaste => "cross-paste",
        })
    }
}

/// A `Ctrl` (+ optional `Shift`) + letter keystroke sent to another app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keystroke {
    pub shift: bool,
    /// Uppercase ASCII letter; equal to its Windows virtual-key code.
    pub key: char,
}

/// A global shortcut such as `Ctrl+Alt+C`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    /// Uppercase ASCII letter or digit; equal to its Windows virtual-key code.
    pub key: char,
}

impl FromStr for Hotkey {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let mut hotkey = Hotkey {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            key: '\0',
        };
        let mut parts = s.split('+').map(str::trim).peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                let mut chars = part.chars();
                let key = chars.next().context("missing key")?.to_ascii_uppercase();
                if chars.next().is_some() || !key.is_ascii_alphanumeric() {
                    bail!("unsupported key {part:?} in shortcut {s:?}; use a letter or digit");
                }
                hotkey.key = key;
            } else {
                match part.to_ascii_lowercase().as_str() {
                    "ctrl" | "control" => hotkey.ctrl = true,
                    "alt" => hotkey.alt = true,
                    "shift" => hotkey.shift = true,
                    "win" | "super" | "meta" => hotkey.win = true,
                    other => bail!("unknown modifier {other:?} in shortcut {s:?}"),
                }
            }
        }
        if !(hotkey.ctrl || hotkey.alt || hotkey.win) {
            bail!("shortcut {s:?} needs Ctrl, Alt or Win so it doesn't steal normal typing");
        }
        Ok(hotkey)
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mods = [
            (self.ctrl, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
            (self.win, "Win+"),
        ];
        for (_, name) in mods.iter().filter(|(on, _)| *on) {
            f.write_str(name)?;
        }
        write!(f, "{}", self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shortcuts() {
        let hk: Hotkey = "Ctrl+Alt+C".parse().unwrap();
        assert_eq!(hk, Action::CrossCopy.default_hotkey());
        let hk: Hotkey = " ctrl + shift + win + 7 ".parse().unwrap();
        assert!(hk.ctrl && hk.shift && hk.win && !hk.alt);
        assert_eq!(hk.key, '7');
        assert_eq!("super+v".parse::<Hotkey>().unwrap().to_string(), "Win+V");
    }

    #[test]
    fn rejects_bad_shortcuts() {
        for bad in [
            "",
            "C",
            "Shift+C",
            "Ctrl+",
            "Ctrl+Alt+F5",
            "Hyper+C",
            "Ctrl+é",
        ] {
            assert!(bad.parse::<Hotkey>().is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn display_round_trips() {
        for action in Action::ALL {
            let hk = action.default_hotkey();
            assert_eq!(hk.to_string().parse::<Hotkey>().unwrap(), hk);
        }
    }

    #[test]
    fn keystrokes_for_apps_and_terminals() {
        let ks = |a: Action, t| {
            let k = a.keystroke(t);
            (k.shift, k.key)
        };
        assert_eq!(ks(Action::CrossCopy, false), (false, 'C'));
        assert_eq!(ks(Action::CrossCut, false), (false, 'X'));
        assert_eq!(ks(Action::CrossPaste, false), (false, 'V'));
        assert_eq!(ks(Action::CrossCopy, true), (true, 'C'));
        assert_eq!(ks(Action::CrossCut, true), (true, 'C'));
        assert_eq!(ks(Action::CrossPaste, true), (true, 'V'));
    }
}
