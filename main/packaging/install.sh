#!/bin/sh
# Installs srvera-emoji for the current user. Only the udev rule needs sudo.
set -e
cd "$(dirname "$0")"
[ "$(id -u)" -ne 0 ] || { echo "Run this as your normal user, not root."; exit 1; }
B="$HOME/.local/bin/srvera-emoji"

./srvera-emoji --quit 2>/dev/null || true
install -Dm755 srvera-emoji "$B"
install -Dm644 srvera-emoji.svg "$HOME/.local/share/icons/hicolor/scalable/apps/srvera-emoji.svg"
# .desktop files can't expand $HOME, so write the full path in.
mkdir -p "$HOME/.local/share/applications" "$HOME/.config/autostart"
sed "s|^Exec=srvera-emoji|Exec=$B|" srvera-emoji.desktop >"$HOME/.local/share/applications/srvera-emoji.desktop"
sed "s|^Exec=srvera-emoji|Exec=$B|" srvera-emoji-autostart.desktop >"$HOME/.config/autostart/srvera-emoji.desktop"

# Lets the picker type the paste key. Skipped if a package or an earlier run already added it.
if [ ! -e /usr/lib/udev/rules.d/60-srvera-uinput.rules ] && [ ! -e /etc/udev/rules.d/60-srvera-uinput.rules ]; then
  echo "Adding the udev rule for /dev/uinput (needs sudo):"
  sudo install -Dm644 60-srvera-uinput.rules /etc/udev/rules.d/60-srvera-uinput.rules
  sudo udevadm control --reload-rules
  sudo udevadm trigger --subsystem-match=misc --sysname-match=uinput
fi

setsid -f "$B" >/dev/null 2>&1
echo "Installed. Press Super+. to open it."
echo "On KDE, move Plasma's own Emoji Selector off Super+. in System Settings -> Shortcuts."
echo "Remember that you can always change Srvera Emoji's shortcut in System Settings -> Shortcuts -> Srvera Emoji!"
