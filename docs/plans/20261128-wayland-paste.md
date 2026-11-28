# Paste on Hyprland and Sway

Goal: `paste::text` pastes on Wayland compositors whose portals have no RemoteDesktop (Hyprland, Sway and the other wlroots ones), and tells where the user types there (#125). wavo-app logs `Ctrl+V was not sent: CreateSession: … No such interface "org.freedesktop.portal.RemoteDesktop"` for every dictation on Hyprland today.

Today (main at aa73ed0): Wayland keys only through the RemoteDesktop portal (`remote.rs`); the clipboard through the portal's, else XWayland's CLIPBOARD, which Hyprland and Sway don't hand to their Wayland apps (the offer is `UNREACHED`); `focus::current()` is `None` on Wayland.

## Decisions

- One Wayland client of the app's own on GTK's `wl_display` and its default queue (`wl.rs`): `libwayland-client` loaded at run time, interfaces built at run time, so no scanner and no build-time C. GDK's loop dispatches its events, so a reader's `send` comes any time.
- Keys through `zwp_virtual_keyboard_manager_v1` where the compositor has it, before the portal (`vkbd.rs`). Its keymap is one key with the shortcut's keysym, as `wtype`'s, so a Russian layout still types Ctrl+V, plus the four modifier keys with their `modifier_map`: GTK 3 ignores a Ctrl+V whose keymap maps no Control (checked in Sway).
- The clipboard through data control (`ext_data_control_manager_v1`, else `zwlr_data_control_manager_v1`) where the portal has no clipboard, before XWayland (`datactl.rs`): offer, snapshot, restore and a plain write without the focus. A snapshot waits at most 500 ms for all the owner's types, so an app that hangs costs half a second; the bytes the app serves are written on a thread, since the reader may be the app itself.

## Tasks

- [x] 1. Keys and clipboard (this PR): `wl.rs`, `vkbd.rs`, `datactl.rs`, the hooks in `post.rs`, `paste.rs` and `Clip::write`. Unit tests of the mask and the keymap. 2026-11-28: `make check` passes; `make test` too but `ink_far_from_the_pen_is_drawn` and `ink_above_its_box_paints`, which fail on main here as well. Headless Sway 1.12 (`WLR_BACKENDS=headless`, its own session bus), the tray example's "Paste a Line in 3 Seconds" into a GTK 3 entry: the text lands, the clipboard holds what `wl-copy` put there before; the same into an XWayland GTK 3 entry (`GDK_BACKEND=x11`); with the clipboard's owner stopped (`kill -STOP` on `wl-copy`) the paste lands half a second later and the text stays on the clipboard. Not run: Hyprland (only the developer's own session here), KDE and GNOME (their paths are unchanged where the portal has RemoteDesktop and a clipboard; GNOME has neither protocol).
- [ ] 2. The focus from `zwlr_foreign_toplevel_manager_v1`: the activated toplevel's `app_id` and title for `focus::current`, so a moved focus is caught, no window is a refusal, and a terminal gets Ctrl+Shift+V.

## Checks

- `make check`, `make test`.
- Headless Sway with the tray example and a GTK 3 entry, Wayland and XWayland; wavo-app built against the branch on Hyprland.
