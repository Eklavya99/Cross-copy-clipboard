// Small helpers shared by the extension modules.

import GLib from 'gi://GLib';

const pendingTimeouts = new Set();

/// Resolves after `ms` milliseconds.
export function delay(ms) {
    return new Promise(resolve => {
        const id = GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => {
            pendingTimeouts.delete(id);
            resolve();
            return GLib.SOURCE_REMOVE;
        });
        pendingTimeouts.add(id);
    });
}

/// Removes all timeouts started by `delay`; called when the extension is disabled.
export function cancelPendingTimeouts() {
    for (const id of pendingTimeouts)
        GLib.source_remove(id);
    pendingTimeouts.clear();
}
