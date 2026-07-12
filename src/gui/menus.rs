//! Application menus. On macOS these become the native menu bar; on
//! Windows/Linux the same definitions drive an in-window menu bar
//! (`gpui_component::menu::AppMenuBar`) rendered in the title bar,
//! mirroring how Zed surfaces menus on those platforms.
//!
//! `install` is called once at startup: it binds the menu shortcuts,
//! registers the app-global Quit handler, and publishes the menu tree
//! both to the OS (`set_menus`) and to gpui-component's `GlobalState`
//! (which the `AppMenuBar` widget reads). Per-window actions (Open,
//! About) are handled on the root view in `AppView::render`.

use super::{About, CopyHexSelection, FindNextAction, FindPrevAction, FocusFind, OpenBios, Quit};
use gpui::{App, KeyBinding, Menu, MenuItem};
use gpui_component::GlobalState;

/// Bind menu shortcuts, register the global Quit handler, and publish
/// the menu tree to both the OS and the in-window menu bar. Call once,
/// after `gpui_component::init` and after the other key bindings so the
/// menu shows every accelerator.
pub fn install(cx: &mut App) {
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-o", OpenBios, None),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-q", Quit, None),
        KeyBinding::new("ctrl-o", OpenBios, None),
    ]);

    // Quit needs only the App, so it lives as an app-global handler.
    cx.on_action(|_: &Quit, cx: &mut App| cx.quit());

    cx.set_menus(build_menus());
    let owned = build_menus().into_iter().map(|m| m.owned()).collect();
    GlobalState::global_mut(cx).set_app_menus(owned);
}

fn build_menus() -> Vec<Menu> {
    vec![
        // The first menu is the macOS application menu (bold, app name).
        Menu {
            name: "etch341".into(),
            items: vec![
                MenuItem::action("About etch341", About),
                MenuItem::separator(),
                MenuItem::action("Quit etch341", Quit),
            ],
            disabled: false,
        },
        Menu {
            name: "File".into(),
            items: vec![MenuItem::action("Open BIOS Image…", OpenBios)],
            disabled: false,
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::action("Copy Selection", CopyHexSelection),
                MenuItem::separator(),
                MenuItem::action("Find", FocusFind),
                MenuItem::action("Find Next", FindNextAction),
                MenuItem::action("Find Previous", FindPrevAction),
            ],
            disabled: false,
        },
    ]
}
