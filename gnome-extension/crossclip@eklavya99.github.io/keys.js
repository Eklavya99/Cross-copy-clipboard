// Synthetic keystrokes (via a Clutter virtual keyboard) and focused-window checks.

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';

import {delay} from './util.js';

const MODIFIER_MASK =
    Clutter.ModifierType.CONTROL_MASK |
    Clutter.ModifierType.SHIFT_MASK |
    Clutter.ModifierType.MOD1_MASK | // Alt
    Clutter.ModifierType.MOD4_MASK | // Super
    Clutter.ModifierType.SUPER_MASK;

/// The keystroke that performs the normal clipboard operation underneath an
/// action. Terminals treat Ctrl+C as "interrupt", so they get Ctrl+Shift+C/V;
/// terminals can't cut, so cross-cut copies there.
export function keystrokeFor(action, inTerminal) {
    let key = Clutter.KEY_c;
    if (action === 'paste')
        key = Clutter.KEY_v;
    else if (action === 'cut' && !inTerminal)
        key = Clutter.KEY_x;
    return {shift: inTerminal, key};
}

/// Whether the focused window's WM class or app id is in `terminalClasses`.
export function focusedIsTerminal(terminalClasses) {
    const window = global.display.focus_window;
    if (!window)
        return false;
    const names = [window.get_wm_class(), window.get_gtk_application_id()]
        .filter(Boolean)
        .map(n => n.toLowerCase());
    return terminalClasses.some(c => names.includes(c.toLowerCase()));
}

export class VirtualKeyboard {
    constructor() {
        const seat = Clutter.get_default_backend().get_default_seat();
        this._device = seat.create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
    }

    destroy() {
        this._device = null;
    }

    /// The shortcut fires while the user still holds Ctrl+Alt. If we sent
    /// Ctrl+C now, the app would see Ctrl+Alt+C. Wait (up to `timeoutMs`)
    /// until the physical modifiers are released.
    async waitForModifiersReleased(timeoutMs) {
        const start = GLib.get_monotonic_time();
        for (;;) {
            const [, , mods] = global.get_pointer();
            if ((mods & MODIFIER_MASK) === 0)
                return true;
            if (GLib.get_monotonic_time() - start >= timeoutMs * 1000)
                return false;
            await delay(10);
        }
    }

    /// Sends Ctrl (+ Shift) + key to the focused window.
    send({shift, key}) {
        const keys = [Clutter.KEY_Control_L, ...(shift ? [Clutter.KEY_Shift_L] : []), key];
        const time = GLib.get_monotonic_time();
        for (const keyval of keys)
            this._device.notify_keyval(time, keyval, Clutter.KeyState.PRESSED);
        for (const keyval of keys.reverse())
            this._device.notify_keyval(time, keyval, Clutter.KeyState.RELEASED);
    }
}
