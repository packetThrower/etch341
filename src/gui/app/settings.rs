//! `AppView` methods — Settings: clock speed, accent, update check, fonts, window, prefs folder.

// `impl AppView` blocks may live in any module of the crate; this
// submodule adds one. `use crate::gui::*` pulls the parent module's
// prelude (gpui, AppView, shared types + the `push_log`/`set_op_result`
// infra these methods call) into scope.
use crate::gui::*;

impl AppView {
    /// Persist a new SPI clock setting. Saves to ~/.config/etch341/prefs.toml
    /// immediately; the next op picks up the new value when it opens the
    /// CH341A.
    pub fn set_spi_speed(&mut self, khz: u32, cx: &mut Context<Self>) {
        self.prefs.spi_speed_khz = khz;
        match self.prefs.save() {
            Ok(()) => self.push_log(format!("SPI clock set to {khz} kHz (saved)")),
            Err(e) => self.push_log(format!("SPI clock set to {khz} kHz (save failed: {e})")),
        }
        cx.notify();
    }

    /// Settings → Appearance accent swatch. Updates our palette
    /// global + the gpui-component theme primary, persists the
    /// choice, and re-renders. The pop-out log window picks it up
    /// for free via its observe subscription on this AppView.
    pub fn set_accent(&mut self, hex: u32, cx: &mut Context<Self>) {
        if self.prefs.accent_color == hex {
            return;
        }
        self.prefs.accent_color = hex;
        theme::set_accent_hex(hex);
        apply_accent_to_component_theme(cx);
        if let Err(e) = self.prefs.save() {
            self.push_log(format!("accent save failed: {e}"));
        }
        cx.notify();
    }

    /// Settings → Updates toggle. `enabled` is the user-facing
    /// "check on launch" switch; we persist its inverse
    /// (`disable_update_check`). Enabling also kicks an immediate
    /// check so the user gets feedback without relaunching.
    pub fn set_update_check_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.prefs.disable_update_check = !enabled;
        if let Err(e) = self.prefs.save() {
            self.push_log(format!("update-check pref save failed: {e}"));
        }
        if enabled {
            self.check_for_updates_now(cx);
        } else {
            cx.notify();
        }
    }

    /// Settings → Updates → "Check now". Re-runs the GitHub check in
    /// the background and updates the global; the sidebar dot +
    /// Updates row repaint when it resolves.
    pub fn check_for_updates_now(&mut self, cx: &mut Context<Self>) {
        self.push_log("→ checking for updates".into());
        cx.notify();
        cx.spawn(async move |_weak, cx_async| {
            let result = cx_async
                .background_executor()
                .spawn(async move { updater::check_for_update(env!("CARGO_PKG_VERSION")) })
                .await;
            cx_async.update(|cx| {
                let available = result.ok().flatten();
                cx.set_global(updater::UpdateState { available });
                cx.refresh_windows();
            });
        })
        .detach();
    }

    /// Open the pending release's GitHub page in the default browser.
    /// No-op (with a log line) if no update is currently pending.
    pub fn open_release_page(&mut self, cx: &mut Context<Self>) {
        let Some(update) = updater::available(cx) else {
            self.push_log("no update pending".into());
            cx.notify();
            return;
        };
        // Defense-in-depth: the URL comes from the GitHub API JSON,
        // and we hand it to the OS opener. There's no shell (it's a
        // `Command` arg, not `sh -c`), so no command injection — but
        // a tampered response (e.g. a defeated-TLS MITM) could swap
        // in a non-web scheme like `file:` / `smb:` that the opener
        // would happily launch. Require https:// before spawning.
        if !update.html_url.starts_with("https://") {
            self.push_log(format!(
                "refusing to open release URL with unexpected scheme: {}",
                update.html_url
            ));
            cx.notify();
            return;
        }
        // Same per-OS launcher as `open_prefs_folder`; all three
        // accept a URL and hand it to the default browser.
        #[cfg(target_os = "macos")]
        let cmd = "open";
        #[cfg(target_os = "windows")]
        let cmd = "explorer";
        #[cfg(target_os = "linux")]
        let cmd = "xdg-open";
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        let cmd: &str = "";
        if cmd.is_empty() {
            self.push_log("open release page: unsupported platform".into());
            cx.notify();
            return;
        }
        match std::process::Command::new(cmd)
            .arg(&update.html_url)
            .spawn()
        {
            Ok(_) => self.push_log(format!("Opened {}", update.html_url)),
            Err(e) => self.push_log(format!("open release page: {e}")),
        }
        cx.notify();
    }

    /// Settings → Log timestamps toggle. Storage stays UTC; this
    /// only flips how existing + new log lines render. Saves
    /// immediately so the next launch lands on the same display.
    pub fn set_timestamp_local(&mut self, local: bool, cx: &mut Context<Self>) {
        if self.prefs.timestamp_local == local {
            return;
        }
        self.prefs.timestamp_local = local;
        if let Err(e) = self.prefs.save() {
            self.push_log(format!("timestamp display save failed: {e}"));
        }
        cx.notify();
    }

    /// Open the directory containing `prefs.toml` in the OS file
    /// manager. Best-effort — if `$HOME` isn't set (no prefs path),
    /// or the platform helper can't be spawned, we log the failure
    /// and move on. Done as a `Command::spawn` (not `output()`) so
    /// the GUI doesn't block on file-manager startup.
    pub fn open_prefs_folder(&mut self, cx: &mut Context<Self>) {
        let Some(path) = Prefs::path() else {
            self.push_log("open prefs folder: $HOME not set".to_string());
            cx.notify();
            return;
        };
        let Some(dir) = path.parent() else {
            self.push_log(format!(
                "open prefs folder: no parent for {}",
                path.display()
            ));
            cx.notify();
            return;
        };
        // Per-OS file-manager invocation. `open` (macOS), `explorer`
        // (Windows), and `xdg-open` (Linux freedesktop spec) all
        // accept a directory and open it in the default file
        // browser. No third-party crate dep — the surface area is
        // three lines of platform-gated code.
        #[cfg(target_os = "macos")]
        let cmd = "open";
        #[cfg(target_os = "windows")]
        let cmd = "explorer";
        #[cfg(target_os = "linux")]
        let cmd = "xdg-open";
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        let cmd: &str = "";

        if cmd.is_empty() {
            self.push_log("open prefs folder: unsupported platform".to_string());
            cx.notify();
            return;
        }
        match std::process::Command::new(cmd).arg(dir).spawn() {
            Ok(_) => self.push_log(format!("Opened {}", dir.display())),
            Err(e) => self.push_log(format!("open prefs folder: {e}")),
        }
        cx.notify();
    }

    /// Flip the "restore window position on startup" toggle. The
    /// actual save happens inside the window-close handler in
    /// `gui::run` — turning it off here simply means the next close
    /// won't snapshot bounds (and any previously-saved
    /// `window_bounds` is left in the file but ignored on next
    /// launch).
    pub fn toggle_restore_window_bounds(&mut self, cx: &mut Context<Self>) {
        self.prefs.restore_window_bounds = !self.prefs.restore_window_bounds;
        let state = if self.prefs.restore_window_bounds {
            "on"
        } else {
            "off"
        };
        match self.prefs.save() {
            Ok(()) => self.push_log(format!("Restore window position: {state} (saved)")),
            Err(e) => self.push_log(format!(
                "Restore window position: {state} (save failed: {e})"
            )),
        }
        cx.notify();
    }
}
