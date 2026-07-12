//! Application menus. On macOS these become the native menu bar; on
//! Windows/Linux the same definitions drive an in-window menu bar
//! (`gpui_component::menu::AppMenuBar`) rendered in the title bar,
//! mirroring how Zed surfaces menus on those platforms.
//!
//! The menus are *bus-aware*: the Actions menu lists the current bus's
//! operations and the View menu's SPI/I²C radio reflects the active
//! bus. Whenever the bus changes, `AppView::refresh_menus` calls
//! [`publish`] to rebuild both the OS menu and the in-window bar.
//!
//! `install` runs once at startup: it binds the menu shortcuts,
//! registers the app-global Quit handler, and publishes the initial
//! (default-bus) menu. Per-window actions (Navigate, SetBus, Identify,
//! Open, About) are handled on the root view in `AppView::render`.

use super::{
    About, Bus, CopyHexSelection, FindNextAction, FindPrevAction, FocusFind, Identify, Navigate,
    OpenBios, Pane, Quit, SetBus,
};
use gpui::{App, KeyBinding, Menu, MenuItem};
use gpui_component::GlobalState;

/// Bind menu shortcuts, register the global Quit handler, and publish
/// the initial menu. Call once, after `gpui_component::init` and after
/// the other key bindings so every item shows its accelerator.
pub fn install(cx: &mut App) {
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-o", OpenBios, None),
        KeyBinding::new("cmd-d", Identify, None),
        KeyBinding::new("cmd-,", Navigate(Pane::Settings), None),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-q", Quit, None),
        KeyBinding::new("ctrl-o", OpenBios, None),
        KeyBinding::new("ctrl-d", Identify, None),
        KeyBinding::new("ctrl-,", Navigate(Pane::Settings), None),
    ]);

    // Quit needs only the App, so it lives as an app-global handler.
    cx.on_action(|_: &Quit, cx: &mut App| cx.quit());

    publish(cx, Bus::default());
}

/// Rebuild the menu tree for `bus` and push it to both the OS menu bar
/// (macOS) and gpui-component's `GlobalState` (which the in-window
/// `AppMenuBar` reads on Windows/Linux). The caller reloads the bar.
pub fn publish(cx: &mut App, bus: Bus) {
    cx.set_menus(build_menus(bus));
    let owned = build_menus(bus).into_iter().map(|m| m.owned()).collect();
    GlobalState::global_mut(cx).set_app_menus(owned);
}

fn build_menus(bus: Bus) -> Vec<Menu> {
    vec![
        // The first menu is the macOS application menu (bold, app name).
        Menu {
            name: "etch341".into(),
            items: vec![
                MenuItem::action("About etch341", About),
                MenuItem::separator(),
                MenuItem::action("Settings…", Navigate(Pane::Settings)),
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
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("SPI Mode", SetBus(Bus::Spi)).checked(bus == Bus::Spi),
                MenuItem::action("I²C Mode", SetBus(Bus::I2c)).checked(bus == Bus::I2c),
                MenuItem::separator(),
                MenuItem::action("Hex Viewer", Navigate(Pane::Hex)),
                MenuItem::action("BIOS Explorer", Navigate(Pane::Bios)),
            ],
            disabled: false,
        },
        actions_menu(bus),
    ]
}

/// The bus-aware Actions menu: the current bus's operations. Destructive
/// ops navigate to their pane (which holds the file pickers + confirm
/// step); the identify op fires directly.
fn actions_menu(bus: Bus) -> Menu {
    let items = match bus {
        Bus::Spi => vec![
            MenuItem::action("Detect Chip", Identify),
            MenuItem::separator(),
            MenuItem::action("Read…", Navigate(Pane::Read)),
            MenuItem::action("Erase…", Navigate(Pane::Erase)),
            MenuItem::action("Write…", Navigate(Pane::Write)),
            MenuItem::action("Verify…", Navigate(Pane::Verify)),
            MenuItem::separator(),
            MenuItem::action("Blank Check", Navigate(Pane::Blank)),
            MenuItem::action("Status Registers", Navigate(Pane::Status)),
            MenuItem::action("Security / OTP", Navigate(Pane::Otp)),
        ],
        Bus::I2c => vec![
            MenuItem::action("Scan Bus", Identify),
            MenuItem::separator(),
            MenuItem::action("Read…", Navigate(Pane::I2cRead)),
            MenuItem::action("Erase…", Navigate(Pane::I2cErase)),
            MenuItem::action("Write…", Navigate(Pane::I2cWrite)),
            MenuItem::action("Verify…", Navigate(Pane::I2cVerify)),
            MenuItem::separator(),
            MenuItem::action("Blank Check", Navigate(Pane::I2cBlank)),
        ],
    };
    Menu {
        name: "Actions".into(),
        items,
        disabled: false,
    }
}
