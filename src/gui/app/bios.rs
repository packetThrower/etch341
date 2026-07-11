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
                        let settings = crate::uefi::extract_settings(&bytes, None);
                        (path, settings)
                    })
                })
                .await;

            weak.update(cx, |this, cx| {
                match parsed {
                    Ok((path, settings)) => {
                        this.push_log(format!(
                            "Parsed BIOS image: {} ({} Setup settings)",
                            path.display(),
                            settings.len()
                        ));
                        if let Some(parent) = path.parent() {
                            this.prefs.last_hex_dir = Some(parent.to_path_buf());
                            let _ = this.prefs.save();
                        }
                        this.bios_input_path = Some(path);
                        this.bios_settings = Some(Arc::new(settings));
                    }
                    Err(e) => this.push_log(format!("BIOS parse failed: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
