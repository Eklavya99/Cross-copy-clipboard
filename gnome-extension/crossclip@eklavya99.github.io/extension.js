// CrossClip GNOME Shell extension.
//
// Milestone 0 (desktop spike): the Cross Clipboard lives inside this extension,
// so Ctrl+Alt+C then Ctrl+Alt+V works on this machine only. The next milestone
// hands items to the CrossClip agent over D-Bus so they reach the other machine.

import Meta from 'gi://Meta';
import Shell from 'gi://Shell';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {ClipboardAccess, describe} from './clipboard.js';
import {VirtualKeyboard, focusedIsTerminal, keystrokeFor} from './keys.js';
import {cancelPendingTimeouts, delay} from './util.js';

const SHORTCUTS = [
    ['cross-copy', 'copy'],
    ['cross-cut', 'cut'],
    ['cross-paste', 'paste'],
];
const MODIFIER_RELEASE_TIMEOUT_MS = 1000;

export default class CrossClipExtension extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._clipboard = new ClipboardAccess();
        this._keyboard = new VirtualKeyboard();
        this._item = null;
        this._busy = false;

        for (const [name, action] of SHORTCUTS) {
            Main.wm.addKeybinding(name, this._settings,
                Meta.KeyBindingFlags.IGNORE_AUTOREPEAT, Shell.ActionMode.NORMAL,
                () => this._run(action));
        }
    }

    disable() {
        for (const [name] of SHORTCUTS)
            Main.wm.removeKeybinding(name);
        cancelPendingTimeouts();
        this._clipboard.destroy();
        this._keyboard.destroy();
        this._clipboard = null;
        this._keyboard = null;
        this._settings = null;
        this._item = null;
    }

    async _run(action) {
        if (this._busy)
            return;
        this._busy = true;
        try {
            if (action === 'paste')
                await this._crossPaste();
            else
                await this._crossCopy(action);
        } catch (e) {
            console.error(`CrossClip: ${action} failed`, e);
            this._notify(`Cross-${action} failed`, e.message);
        } finally {
            this._busy = false;
        }
    }

    async _prepare() {
        if (!await this._keyboard.waitForModifiersReleased(MODIFIER_RELEASE_TIMEOUT_MS))
            throw new Error('Release Ctrl and Alt right after the shortcut.');
        return focusedIsTerminal(this._settings.get_strv('terminal-wm-classes'));
    }

    async _crossCopy(action) {
        const inTerminal = await this._prepare();
        const snapshot = await this._clipboard.snapshot();
        const before = this._clipboard.changeCount;

        this._keyboard.send(keystrokeFor(action, inTerminal));
        const copied = await this._clipboard.waitForChange(
            before, this._settings.get_int('copy-wait-ms'));
        // If nothing was selected the clipboard didn't change: use what is
        // already on it (e.g. a screenshot).
        const item = await this._clipboard.read();
        if (copied && snapshot)
            this._clipboard.restore(snapshot);

        if (!item)
            throw new Error('Nothing to copy: the clipboard has no text or image.');
        this._item = item;
        this._notify(`Cross-${action}: ${describe(item)}`,
            copied ? 'From your selection.' : 'From the current clipboard.');
    }

    async _crossPaste() {
        if (!this._item)
            throw new Error('The Cross Clipboard is empty. Use Ctrl+Alt+C first.');
        const inTerminal = await this._prepare();
        const snapshot = await this._clipboard.snapshot();

        this._clipboard.write(this._item);
        // Let the focused app receive the new clipboard offer before the keystroke.
        await delay(30);
        const ours = this._clipboard.changeCount;
        this._keyboard.send(keystrokeFor('paste', inTerminal));
        await delay(this._settings.get_int('paste-restore-delay-ms'));

        // Don't clobber something the user copied in the meantime.
        if (snapshot && this._clipboard.changeCount === ours)
            this._clipboard.restore(snapshot);
    }

    _notify(title, body) {
        Main.notify(title, body);
    }
}
