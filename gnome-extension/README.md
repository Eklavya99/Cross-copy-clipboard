# CrossClip GNOME Shell extension

Adds the Cross Clipboard shortcuts to GNOME (Wayland), where ordinary apps are
not allowed to register global shortcuts, read the clipboard in the background
or type into other windows. Supports GNOME 49 and 50 (Ubuntu 25.10 / 26.04).

| Shortcut | Action |
|---|---|
| `Ctrl+Alt+C` | Cross-copy the selection (or, if nothing is selected, the current clipboard) |
| `Ctrl+Alt+X` | Cross-cut the selection |
| `Ctrl+Alt+V` | Cross-paste into the focused window |

## Install from a CI build

1. Download the `crossclip-gnome-extension` artifact from the latest CI run and unzip it
   to get `crossclip@eklavya99.github.io.shell-extension.zip`.
2. Install it:
   ```sh
   gnome-extensions install --force crossclip@eklavya99.github.io.shell-extension.zip
   ```
3. **Log out and back in.** On Wayland, GNOME Shell only picks up new extensions at login.
4. Enable it:
   ```sh
   gnome-extensions enable crossclip@eklavya99.github.io
   ```

## Install from source

```sh
EXT=~/.local/share/gnome-shell/extensions/crossclip@eklavya99.github.io
mkdir -p "$EXT" && cp -r gnome-extension/crossclip@eklavya99.github.io/* "$EXT"
glib-compile-schemas "$EXT/schemas"
```

Then log out and back in, and enable the extension as above.

## Debugging

```sh
journalctl --user -f -o cat /usr/bin/gnome-shell | grep -i crossclip
```

To test changes without logging out, run a nested GNOME Shell in a window:
`dbus-run-session gnome-shell --devkit --wayland` (GNOME 49+).

## Settings

The shortcuts, the list of terminal window classes (which get `Ctrl+Shift+C/V`)
and the timing values are GSettings keys:

```sh
gsettings --schemadir "$EXT/schemas" list-recursively org.gnome.shell.extensions.crossclip
gsettings --schemadir "$EXT/schemas" set org.gnome.shell.extensions.crossclip cross-paste "['<Super><Shift>v']"
```
