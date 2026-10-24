## v0.1.0 – 2026-10-24

### Features

- Added windows on macOS 14+ (AppKit), Windows 10 1809+ (Win32) and Linux (GTK 3 loaded at run time, Wayland and X11), drawn by a CPU renderer that repaints only what changed and following the system's light or dark appearance and accent (#2, #3, #9, #10)
- Added input: mouse, trackpad scrolling and pinch, keys named as in `keyboard-types`, modifiers, cursors and the clipboard (#4, #11)
- Added text in each system's own fonts and shaping (CoreText, DirectWrite, FreeType with HarfBuzz), with font fallback and colour emoji (#5)
- Added the UI core: an element tree built every frame, flex and grid layout, focus and Tab order, animations and a theme in the macOS System Settings look (#6)
- Added virtualized lists, menus, pop-up buttons, tabs, split views and scroll areas with overlay scrollbars (#12)
- Added text fields (one line, many lines, password) and selectable text, with undo and the system's input methods (#13)
- Added widgets: buttons, check boxes, switches, sliders, selects, segmented controls, progress bars and spinners, links, badges, tooltips, icons and a shortcut recorder, plus a settings form and a modal dialog (#14)
- Added the app platform: an app without a window that opens windows on demand, a tray icon with a menu, global hotkeys, the app's directories and launch at login (#15)
- Added a search field, filled icons and more icons, images from pixels, a window that fits its height to its content and tray icons from SVG files (#20, #21)
- Added overlay windows: borderless, transparent, never taking the focus, the mouse going through them, placed at an anchor on the screen with the pointer (#22)
- Added opening URLs and files and revealing them in the file manager, macOS privacy permissions, posting a shortcut to the focused app, the system clipboard, and running an app with its own arguments (#19, #23)
- Added where the user types in other apps (the app and window in focus, a password field), clipboard snapshots put back after a paste and text offered until it's read, the microphone permission on macOS and alerts (#24)

**Full Changelog**: https://github.com/vladkens/nimbo/commits/v0.1.0
