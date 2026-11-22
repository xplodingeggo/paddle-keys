#!/bin/sh
# Installs paddle-keys for the current user from the extracted release folder.
#   ./install.sh              install to ~/.local/opt/paddle-keys (PREFIX=... to change ~/.local)
#   ./install.sh --no-udev    skip the udev rule
#   ./install.sh --uninstall  remove it again (also takes the Steam hook out of steam.sh)
set -eu

here=$(cd "$(dirname "$0")" && pwd)
prefix=${PREFIX:-$HOME/.local}
dest=$prefix/opt/paddle-keys
rule=/etc/udev/rules.d/70-paddle-keys.rules
asroot() { if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo "$@"; fi; }

if [ "${1:-}" = "--uninstall" ]; then
    [ -x "$dest/paddle-keys" ] && "$dest/paddle-keys" --remove-steam-hook || true
    pkill -x paddle-keys 2>/dev/null || true
    rm -rf "$dest" "$prefix/bin/paddle-keys" "$prefix/share/applications/paddle-keys.desktop" \
        "${XDG_DATA_HOME:-$HOME/.local/share}/paddle-keys" "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/paddle-keys.desktop"
    if [ -f "$rule" ]; then
        echo "Removing $rule (needs root)"
        asroot rm -f "$rule" && asroot udevadm control --reload
    fi
    echo "Removed. Your settings in ~/.config/paddle-keys were kept."
    exit 0
fi

for f in paddle-keys libpaddle_keys_steam32.so libpaddle_keys_steam64.so; do
    [ -f "$here/$f" ] || { echo "missing $f next to install.sh" >&2; exit 1; }
done

install -d "$dest" "$prefix/bin" "$prefix/share/applications"
install -m755 "$here/paddle-keys" "$dest/paddle-keys"
install -m644 "$here/libpaddle_keys_steam32.so" "$here/libpaddle_keys_steam64.so" "$dest/"
ln -sf "$dest/paddle-keys" "$prefix/bin/paddle-keys"
sed "s|^Exec=.*|Exec=$dest/paddle-keys|" "$here/paddle-keys.desktop" > "$prefix/share/applications/paddle-keys.desktop"
echo "Installed to $dest (command: $prefix/bin/paddle-keys)"

if [ "${1:-}" != "--no-udev" ]; then
    if cmp -s "$here/70-paddle-keys.rules" "$rule" 2>/dev/null; then
        echo "udev rule already installed"
    elif [ -t 0 ]; then
        printf "Install the udev rule (hidraw and uinput access) to %s? Needs root. [Y/n] " "$rule"
        read -r a
        case "$a" in
            n|N) echo "Skipped. Install it later: sudo install -m644 70-paddle-keys.rules $rule" ;;
            *) asroot install -m644 "$here/70-paddle-keys.rules" "$rule"
               asroot udevadm control --reload && asroot udevadm trigger
               echo "udev rule installed" ;;
        esac
    else
        echo "Not interactive, so the udev rule is not installed. If paddle-keys --probe 5 cannot find the controller:"
        echo "  sudo install -m644 $here/70-paddle-keys.rules $rule && sudo udevadm control --reload && sudo udevadm trigger"
    fi
fi

case ":$PATH:" in *":$prefix/bin:"*) ;; *) echo "Note: $prefix/bin is not in your PATH." ;; esac
echo "Next: paddle-keys --probe 5, then run paddle-keys. Steam Elite mode: see the README."
