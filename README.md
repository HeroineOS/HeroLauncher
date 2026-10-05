# HeroLauncher

A lightweight app launcher for HeroineOS, built on [HeroUI](https://github.com/HeroineOS/HeroUI).
Type to find an installed app and start it; keep your most used apps as favorites.

- **Two ways to open it.** As a menu dropping down from a panel button, like XFCE's or
  Windows' (HeroBar's `launcher` module does this, in whichever corner the button is), or
  in the middle of the screen, like the launchers riced setups use. A keyboard shortcut and
  the button can each use their own style.
- **Toggles.** Running `herolauncher` while it's open closes it, so one shortcut opens and
  closes it.
- **Favorites** ("Start"): right-click an app to add it; favorites show first as a grid of
  icons, separate from the taskbar's pinned apps. Right-click a favorite to move or remove it.
- **Three layouts:** a list (icon, name, what it is), a grid of icons with names (like the
  taskbar's folders), or split: favorites on one side, all apps on the other.
- **Categories** like XFCE's menu (Games, Development, Internet, Office...): buttons above
  the apps show one at a time (Tab / Shift+Tab step through them).
- **Smooth:** it unrolls from the button (menu) or grows from the middle (centered), and
  rolls away when closed (instant with the theme's `animations = false`).
- **Keyboard first:** type right away, Up/Down to pick, Enter to start, Escape to close. A
  click outside closes it too. Mouse, touch scrolling and right-click menus work as well.
- **Costs nothing when closed:** it's a separate program that only runs while it's open.

It finds apps from their `.desktop` files (the freedesktop.org standard every desktop uses),
skipping hidden ones and ones meant for other desktops, and searches their names, what they
are ("web browser"), keywords and descriptions.

## Usage

```sh
herolauncher                     # in the middle of the screen
herolauncher --menu --edge top --x 40 --offset 38
                                 # as a menu: left edge at x = 40, 38 px below the top
                                 # (--edge bottom: above a bottom panel)
herolauncher --print-default-config
```

Keyboard shortcut examples (open centered; press again to close):

```toml
# HeroWM (~/.config/fht/compositor.toml, [keybinds])
Super-Space = { action = "run-command", arg = "herolauncher" }
```

```ini
# Hyprland (hyprland.conf)
bind = SUPER, SPACE, exec, herolauncher
# sway (config)
bindsym $mod+space exec herolauncher
```

## Configuration

`~/.config/hero/launcher.toml` (see [`res/launcher.toml`](res/launcher.toml)):

```toml
favorites = ["firefox-esr", "foot"]   # desktop file names; edited by the launcher too
layout = "list"                       # "list", "grid" or "split"
categories = true                     # category buttons (Games, Internet...)
width = 0                             # the panel's size (0: 480, or 720 for "split")
height = 540
terminal = ""                         # for terminal apps ("" = $TERMINAL or the first found)
```

Colors, fonts and corners come from the shared HeroUI theme (edit them in Appearance).

## How it works

On Wayland compositors with wlr-layer-shell (HeroWM, sway, Hyprland, river, labwc...) it's
an overlay over the whole screen that takes the keyboard; it's see-through except for its
panel (through the [HeroineOS fltk-sys fork](https://github.com/HeroineOS/fltk-sys)), so a
click anywhere else lands on it and closes it. On X11 it's a borderless window that grabs
the pointer, for the same effect.

## Building

Rust 1.80+ and the FLTK build dependencies (see HeroUI's README), then `cargo build --release`.
CI builds `.deb` packages for x86_64 and arm64 (`cargo deb`).

License: MIT OR Apache-2.0.
