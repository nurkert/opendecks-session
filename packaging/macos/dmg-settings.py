# dmgbuild settings for the OpenDecks Session disk image.
# Used as: dmgbuild -s packaging/macos/dmg-settings.py -D app=<path.app> "OpenDecks Session" out.dmg
import os.path

app = defines["app"]  # noqa: F821 (provided by dmgbuild)
app_name = os.path.basename(app)

format = "UDZO"
filesystem = "HFS+"
files = [app]
symlinks = {"Applications": "/Applications"}
icon = defines.get("icon")  # noqa: F821
badge_icon = None

background = defines.get("background")  # noqa: F821
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
window_rect = ((200, 160), (660, 448))
default_view = "icon-view"
show_icon_preview = False
arrange_by = None
grid_spacing = 100
label_pos = "bottom"
text_size = 13
icon_size = 112
icon_locations = {app_name: (165, 214), "Applications": (495, 214)}
