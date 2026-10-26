#!/bin/sh
# Removes what install.sh added. Favorites and recent lists stay in ~/.local/share/srvera-emoji/.
B="$HOME/.local/bin/srvera-emoji"
"$B" --quit 2>/dev/null
rm -f "$B" "$HOME/.local/share/icons/hicolor/scalable/apps/srvera-emoji.svg" \
  "$HOME/.local/share/applications/srvera-emoji.desktop" "$HOME/.config/autostart/srvera-emoji.desktop"
if [ -e /etc/udev/rules.d/60-srvera-uinput.rules ]; then
  echo "Removing the udev rule (needs sudo):"
  sudo rm -f /etc/udev/rules.d/60-srvera-uinput.rules && sudo udevadm control --reload-rules
fi
echo "Uninstalled. Delete ~/.local/share/srvera-emoji/ too if you want your favorites gone."
