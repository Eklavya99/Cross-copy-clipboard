// Clipboard access for the Cross Clipboard: reading and writing items, and
// snapshotting / restoring whatever the user had on the normal clipboard.

import Meta from 'gi://Meta';
import St from 'gi://St';

import {delay} from './util.js';

const CLIPBOARD = St.ClipboardType.CLIPBOARD;
const IMAGE_PNG = 'image/png';
/// Nautilus' "copied files" format; restoring it keeps a file copy pasteable.
const COPIED_FILES = 'x-special/gnome-copied-files';
/// Larger normal-clipboard contents are not snapshotted (and not restored).
const MAX_SNAPSHOT_BYTES = 64 * 1024 * 1024;

function isTextType(mime) {
    return mime.startsWith('text/plain') || ['UTF8_STRING', 'STRING', 'TEXT'].includes(mime);
}

/// Describes an item for notifications, e.g. "3 lines of text".
export function describe(item) {
    if (item.kind === 'text') {
        const lines = item.text.split('\n').length;
        return lines > 1 ? `${lines} lines of text` : `text (${item.text.length} chars)`;
    }
    return `image (${Math.ceil(item.bytes.get_size() / 1024)} KB)`;
}

export class ClipboardAccess {
    constructor() {
        this._clipboard = St.Clipboard.get_default();
        this._selection = global.display.get_selection();
        this._changes = 0;
        this._waiters = new Set();
        this._ownerChangedId = this._selection.connect('owner-changed', (_selection, type) => {
            if (type !== Meta.SelectionType.SELECTION_CLIPBOARD)
                return;
            this._changes++;
            for (const wake of this._waiters)
                wake();
        });
    }

    destroy() {
        this._selection.disconnect(this._ownerChangedId);
        // Pending waits simply never resolve; their callers are being torn down.
        this._waiters.clear();
    }

    /// Increments every time the clipboard changes owner.
    get changeCount() {
        return this._changes;
    }

    /// Resolves to true once the clipboard changes after `since`
    /// (a previous `changeCount`), or to false after `timeoutMs`.
    async waitForChange(since, timeoutMs) {
        if (this._changes !== since)
            return true;
        let wake;
        const changed = new Promise(resolve => {
            wake = () => resolve(true);
            this._waiters.add(wake);
        });
        const result = await Promise.race([changed, delay(timeoutMs).then(() => false)]);
        this._waiters.delete(wake);
        return result;
    }

    /// Reads the clipboard as a Cross Clipboard item: text if there is any,
    /// otherwise a PNG image. Returns null for anything else.
    async read() {
        const types = this._mimetypes();
        if (types.some(isTextType)) {
            const text = await this._getText();
            if (text !== null)
                return {kind: 'text', text};
        }
        if (types.includes(IMAGE_PNG)) {
            const bytes = await this._getBytes(IMAGE_PNG);
            if (bytes && bytes.get_size() > 0)
                return {kind: 'image', bytes};
        }
        return null;
    }

    /// Replaces the clipboard with a Cross Clipboard item.
    write(item) {
        if (item.kind === 'text')
            this._clipboard.set_text(CLIPBOARD, item.text);
        else
            this._clipboard.set_content(CLIPBOARD, IMAGE_PNG, item.bytes);
    }

    /// Saves the normal clipboard so it can be restored after an action.
    ///
    /// GNOME lets us put back only one format, so we keep the most useful one:
    /// copied files, then text, then a PNG image, then whatever comes first.
    /// Returns null when there is nothing restorable or it is too large.
    async snapshot() {
        const types = this._mimetypes();
        if (types.length === 0)
            return {kind: 'empty'};
        if (!types.includes(COPIED_FILES) && types.some(isTextType)) {
            const text = await this._getText();
            return text === null ? null : {kind: 'text', text};
        }
        const mime = [COPIED_FILES, IMAGE_PNG].find(t => types.includes(t)) ??
            types.find(t => t.includes('/'));
        if (!mime)
            return null;
        const bytes = await this._getBytes(mime);
        if (!bytes || bytes.get_size() > MAX_SNAPSHOT_BYTES)
            return null;
        return {kind: 'raw', mime, bytes};
    }

    restore(snapshot) {
        switch (snapshot.kind) {
        case 'empty':
            this._clipboard.set_text(CLIPBOARD, '');
            break;
        case 'text':
            this._clipboard.set_text(CLIPBOARD, snapshot.text);
            break;
        case 'raw':
            this._clipboard.set_content(CLIPBOARD, snapshot.mime, snapshot.bytes);
            break;
        }
    }

    _mimetypes() {
        return this._clipboard.get_mimetypes(CLIPBOARD) ?? [];
    }

    _getText() {
        return new Promise(resolve => {
            this._clipboard.get_text(CLIPBOARD, (_clipboard, text) => resolve(text));
        });
    }

    _getBytes(mime) {
        return new Promise(resolve => {
            this._clipboard.get_content(CLIPBOARD, mime, (_clipboard, bytes) => resolve(bytes));
        });
    }
}
