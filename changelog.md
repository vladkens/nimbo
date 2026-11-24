## v0.1.0 – 2026-11-24

### Features

- Added nimbo, a desktop UI toolkit that draws every widget itself on the CPU over a thin native layer: AppKit and CoreText on macOS, Win32 and DirectWrite on Windows, GTK 3 with FreeType and HarfBuzz on Linux (Wayland and X11); frames are paced to the display and an idle window sleeps (#3, #9, #60)
- Added a look measured from macOS System Settings, in light and dark, in the system's accent colour, which turns grey in an inactive window (#10, #84)
- Added three layers to build on: the core (windows, layout, drawing, text, input), headless behaviours for a design system of your own, and the styled controls over them; cargo features compile only what an app uses: `styled` by default, `headless`, `tray`, `hotkey`, `paste`, `permissions`, `instance`, `autostart`, `system`, `png`, `bitmap`, `full` and `harness` (#89, #92)
- Added an immediate-mode API in SwiftUI's shape: an app builds each frame in `view` from its own state, with stacks, grids, spacers, dividers, overlays, scroll views and lazy columns of hundreds of thousands of rows, and modifiers scoped to what a container holds (`ui.disabled`, `ui.control_size`, `ui.help`) (#64, #69, #93)
- Added the controls: buttons and icon buttons with roles, switches, check boxes, sliders with steps, pickers and segmented pickers, text, multi-line, secure and search fields, progress bars, spinners, badges, a hotkey recorder, tooltips and help marks (#13, #14, #20, #71)
- Added custom looks over the same behaviour: `ButtonStyle`, `ToggleStyle`, `SliderStyle`, `TextFieldStyle`, `PickerStyle`, `ListStyle` and `HelpStyle`, and the `custom-look` example, a whole look built on public API (#94)
- Added lists with sections, selection and an inset grouped style, tabs, splits and a navigation split, and settings forms with sections, rows, footers and disclosures (#12, #14, #32, #67, #68, #70, #71)
- Added the window's parts: a unified title bar with toolbar tabs and items, a standard menu bar with the app's own commands, the system's context menu on any element, alerts, the system's open and save panels, files dropped onto a window, and a window icon (#28, #29, #35, #56, #73, #77, #91)
- Added text through the system's stack: shaping, subpixel positioning, colour emoji, input methods, selectable text and links, a text measured as it wraps, and the desktop's text scale on Linux (#13, #62, #76, #81)
- Added translations: strings marked with `t!` and `tn!` come from gettext catalogs with each language's plural forms, in the system's language or the app's choice, before any window opens too; a pseudo language finds the strings left unmarked, and nimbo's own strings come in 12 languages (#75, #85, #86, #87)
- Added tray and HUD apps: a tray icon with a title and a menu that changes while open, several global hotkeys at once, overlays that never take the focus, one instance that takes commands from a second launch, launch at login, a launcher entry on Linux and an uninstall that undoes what nimbo set up (#15, #22, #38, #42, #78, #80, #90)
- Added ways to reach other apps: keys posted to the app in focus, a paste that gives the clipboard back, the system clipboard, where the user types and with which keyboard layout, links and files opened or shown, and the system's permissions (Accessibility, Secure Input, the microphone) (#23, #24, #42, #74, #82)
- Added the system's changes as they come, such as a microphone plugged in or the laptop's lid closed (#49, #58)
- Added photos decoded by the system at the size they show (JPEG, PNG, GIF; HEIC and WebP where the system decodes them), PNG images and SVG tray icons (#21, #47, #72)
- Added accessibility on macOS: VoiceOver reads and operates every widget, in a custom look too (#41)
- Added a headless harness: an app renders to a PNG without a window, and tests click, type and check the pixels and what a screen reader reads, in parallel (#16)
- Added macOS 11 as the minimum, with newer system features used where the system has them, so an app sets no deployment target (#61)
- Added an API made to grow without breaking apps: what an app reads and enums that may grow are `#[non_exhaustive]`, what it passes in is built by constructors and builders, every public item has one path, and errors are `nimbo::Error`

**Full Changelog**: https://github.com/vladkens/nimbo/commits/v0.1.0
