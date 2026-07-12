//! `AppView` methods — BIOS Setup explorer: pick an image, parse it.

// `use crate::gui::*` pulls the parent prelude (gpui, AppView, the
// `push_log` infra) into scope, same as the other app/ submodules.
use crate::gui::*;

impl AppView {
    /// Open the file picker, load a BIOS image, and parse it into
    /// resolved Setup settings for the BIOS pane to render.
    ///
    /// **Deferred via cx.spawn** for the same reason as `pick_hex_file`:
    /// the native file dialog pumps its own modal loop, and opening it
    /// synchronously from a click handler while an Input is focused
    /// panics gpui with "RefCell already borrowed". The spawn lets the
    /// click-handler borrow drop first.
    ///
    /// Parsing a full flash image walks every firmware volume and
    /// decompresses sections, so it runs in a background task off the
    /// UI thread; only the finished `Vec<Setting>` is handed back.
    pub fn pick_bios_file(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_hex_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("BIOS images", &["bin", "rom", "fd"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();

            // Read + parse off the UI thread: a 16 MB image with LZMA /
            // Tiano volumes is too heavy to block the frame on.
            let parsed = cx
                .background_spawn(async move {
                    std::fs::read(&path).map(|bytes| {
                        let model = crate::uefi::extract_model(&bytes);
                        let ifd = crate::ifd::parse(&bytes);
                        (path, model, ifd)
                    })
                })
                .await;

            weak.update(cx, |this, cx| {
                match parsed {
                    Ok((path, model, ifd)) => {
                        this.push_log(format!(
                            "Parsed BIOS image: {} ({} Setup settings, {} menu pages)",
                            path.display(),
                            model.settings.len(),
                            model.tree.len()
                        ));
                        if let Some(parent) = path.parent() {
                            this.prefs.last_hex_dir = Some(parent.to_path_buf());
                            let _ = this.prefs.save();
                        }
                        this.bios_input_path = Some(path);
                        this.bios_settings = Some(Arc::new(model.settings));
                        this.bios_tree = Some(Arc::new(model.tree));
                        this.bios_boot = Some(Arc::new(model.boot));
                        this.bios_id = Some(model.bios_id);
                        this.bios_ifd = ifd;
                        this.bios_selected_form = None;
                    }
                    Err(e) => this.push_log(format!("BIOS parse failed: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Select a form in the BIOS navigator (or `None` for "all
    /// settings"), scrolling the settings list back to the top.
    pub fn select_bios_form(&mut self, form: Option<String>, cx: &mut Context<Self>) {
        self.bios_selected_form = form;
        self.bios_scroll
            .scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
    }

    /// Pick a second BIOS image, diff its settings against the loaded
    /// one, and open the result in the diff window. Deferred dialog +
    /// background parse, like the open picker.
    pub fn pick_bios_compare(&mut self, cx: &mut Context<Self>) {
        let Some(a_settings) = self.bios_settings.clone() else {
            return;
        };
        let a_name = self
            .bios_input_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "(loaded image)".into());
        let start_dir = self.prefs.last_hex_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("BIOS images", &["bin", "rom", "fd"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let bpath = handle.path().to_path_buf();
            let b_name = bpath
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "(image B)".into());

            let parsed = cx
                .background_spawn(async move {
                    std::fs::read(&bpath).map(|bytes| crate::uefi::extract_settings(&bytes, None))
                })
                .await;

            weak.update(cx, |this, cx| {
                match parsed {
                    Ok(b_settings) => {
                        let diffs = crate::uefi::diff_settings(&a_settings, &b_settings);
                        this.push_log(format!(
                            "BIOS diff vs {b_name}: {} setting(s) differ",
                            diffs.len()
                        ));
                        this.open_bios_diff(diffs, a_name, b_name, cx);
                    }
                    Err(e) => this.push_log(format!("Compare load failed: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Toggle the "changed from default only" filter.
    pub fn toggle_bios_changed_only(&mut self, cx: &mut Context<Self>) {
        self.bios_changed_only = !self.bios_changed_only;
        self.bios_scroll
            .scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
    }

    /// Save the loaded Setup settings as JSON via a file dialog. The
    /// dialog is deferred (cx.spawn) for the same RefCell-borrow reason
    /// as the open picker.
    pub fn export_bios_json(&mut self, cx: &mut Context<Self>) {
        let Some(settings) = self.bios_settings.clone() else {
            return;
        };
        let default_name = self
            .bios_input_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| format!("{}-settings.json", s.to_string_lossy()))
            .unwrap_or_else(|| "bios-settings.json".to_string());
        let start_dir = self.prefs.last_hex_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new().set_file_name(default_name);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.save_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            let result = serde_json::to_string_pretty(&*settings)
                .map_err(|e| e.to_string())
                .and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string()));
            weak.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.push_log(format!(
                        "Exported {} settings → {}",
                        settings.len(),
                        path.display()
                    )),
                    Err(e) => this.push_log(format!("BIOS export failed: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
