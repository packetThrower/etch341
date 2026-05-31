//! `AppView` methods — I²C bus operations: scan / read / write / verify / erase / blank check.

// `impl AppView` blocks may live in any module of the crate; this
// submodule adds one. `use crate::gui::*` pulls the parent module's
// prelude (gpui, AppView, shared types + the `push_log`/`set_op_result`
// infra these methods call) into scope.
use crate::gui::*;

impl AppView {
    /// I²C clock for ops. Standard mode (100 kHz) — the safe default
    /// every 24Cxx supports; a settings-driven I²C speed control is a
    /// follow-up. Never exceeds the 400 kHz the transport enforces.
    fn i2c_speed(&self) -> u32 {
        100
    }

    /// I²C bus scan — probes 0x08..0x77 and records the ACKing
    /// addresses. Mirrors `start_read`'s spawn → background → open →
    /// op → update shape, but opens the CH341 in I²C mode.
    pub fn start_i2c_scan(&mut self, cx: &mut Context<Self>) {
        self.op_result = None;
        self.push_log("→ i2c scan (0x08..0x77)".into());
        cx.notify();
        let speed = self.i2c_speed();
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut ch = Programmer::open_i2c(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    crate::i2c::scan(&mut ch, 0x08..=0x77).map_err(|e| format!("scan: {e}"))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok(hits) => {
                        if hits.is_empty() {
                            this.push_log(
                                "scan: no devices responded — note a blank EEPROM (all 0xFF) \
                                 won't show up; pick its chip and read it directly"
                                    .into(),
                            );
                        } else {
                            let list = hits
                                .iter()
                                .map(|a| format!("0x{a:02X}"))
                                .collect::<Vec<_>>()
                                .join(" ");
                            this.push_log(format!("scan: {} ACK → {list}", hits.len()));
                        }
                        this.i2c_scan_results = Some(hits);
                    }
                    Err(err) => {
                        this.i2c_scan_results = None;
                        this.set_op_result(false, format!("Scan failed: {err}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// I²C read — dumps the selected chip to a timestamped file (same
    /// naming + save dir as the SPI Read pane). Requires a chip
    /// selection; logs a hint and bails if none is picked.
    pub fn start_i2c_read(&mut self, cx: &mut Context<Self>) {
        let Some(chip_name) = self.i2c_chip_select.read(cx).selected_value().cloned() else {
            self.set_op_result(false, "Pick a chip first — I²C has no auto-detect".into());
            cx.notify();
            return;
        };
        let chip_name = chip_name.to_string();
        let path = read_output_path(&self.prefs);
        self.push_log(format!("→ i2c read ({chip_name}) → {}", path.display()));
        cx.notify();
        self.spawn_progress_poller(cx);

        let path_for_task = path.clone();
        let progress = self.progress.clone();
        let speed = self.i2c_speed();
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let chip = crate::i2c_ops::resolve_chip(&chip_name)
                        .map_err(|e| format!("chip: {e}"))?;
                    let mut ch = Programmer::open_i2c(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let mut sink = GuiSink::new(progress, "i2c-read");
                    crate::i2c_ops::read(
                        &mut ch,
                        &chip,
                        0,
                        chip.size_bytes,
                        0,
                        &path_for_task,
                        &mut sink,
                    )
                    .map_err(|e| format!("read: {e}"))?;
                    Ok::<_, String>((chip.name, chip.size_bytes))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, size)) => this.set_op_result(
                        true,
                        format!("Read {size} bytes from {name} → {}", path.display()),
                    ),
                    Err(err) => this.set_op_result(false, format!("Read failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Currently-selected I²C chip name, or `None` if the dropdown is
    /// still on its placeholder.
    fn i2c_chip_name(&self, cx: &mut Context<Self>) -> Option<String> {
        self.i2c_chip_select
            .read(cx)
            .selected_value()
            .map(|v| v.to_string())
    }

    /// File picker for the I²C Write pane (mirrors `pick_write_file`).
    pub fn pick_i2c_write_file(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_write_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("EEPROM dumps", &["bin", "rom", "eep"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            weak.update(cx, |this, cx| {
                this.push_log(format!("Picked for i2c write: {}", path.display()));
                if let Some(parent) = path.parent() {
                    this.prefs.last_write_dir = Some(parent.to_path_buf());
                    let _ = this.prefs.save();
                }
                this.i2c_write_path = Some(path);
                this.i2c_write_armed = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// File picker for the I²C Verify pane.
    pub fn pick_i2c_verify_file(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_write_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("EEPROM dumps", &["bin", "rom", "eep"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            weak.update(cx, |this, cx| {
                this.push_log(format!("Picked for i2c verify: {}", path.display()));
                this.i2c_verify_path = Some(path);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// I²C Write — two-stage arm/confirm (destructive). Needs both a
    /// file and a chip selection.
    pub fn arm_or_fire_i2c_write(&mut self, cx: &mut Context<Self>) {
        if self.i2c_write_path.is_none() {
            self.set_op_result(false, "Pick an input file first".into());
            cx.notify();
            return;
        }
        if self.i2c_chip_name(cx).is_none() {
            self.set_op_result(false, "Pick a chip first".into());
            cx.notify();
            return;
        }
        if self.i2c_write_armed {
            self.i2c_write_armed = false;
            self.start_i2c_write(cx);
        } else {
            self.i2c_write_armed = true;
            self.push_log("⚠ i2c write armed: click again to confirm".into());
            cx.notify();
        }
    }

    fn start_i2c_write(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some(chip_name)) = (self.i2c_write_path.clone(), self.i2c_chip_name(cx))
        else {
            self.set_op_result(false, "Need a chip and a file".into());
            cx.notify();
            return;
        };
        self.push_log(format!("→ i2c write ({chip_name}) ← {}", path.display()));
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.i2c_speed();
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let data = std::fs::read(&path).map_err(|e| format!("read input: {e}"))?;
                    let chip = crate::i2c_ops::resolve_chip(&chip_name)
                        .map_err(|e| format!("chip: {e}"))?;
                    if data.len() as u32 > chip.size_bytes {
                        return Err(format!(
                            "file is {} bytes but {} only holds {}",
                            data.len(),
                            chip.name,
                            chip.size_bytes
                        ));
                    }
                    let mut ch = Programmer::open_i2c(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let mut wsink = GuiSink::new(progress.clone(), "i2c-wr");
                    crate::i2c_ops::write(&mut ch, &chip, 0, &data, 0, &mut wsink)
                        .map_err(|e| format!("write: {e}"))?;
                    // Verify-after-write, matching the SPI Write pane.
                    let mut vsink = GuiSink::new(progress, "i2c-vfy");
                    let mismatches =
                        crate::i2c_ops::verify(&mut ch, &chip, &data, 0, 0, &mut vsink)
                            .map_err(|e| format!("verify: {e}"))?;
                    Ok::<_, String>((chip.name, data.len(), mismatches))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, n, 0)) => {
                        this.set_op_result(true, format!("Wrote {n} bytes to {name} (verified)"))
                    }
                    Ok((name, n, m)) => this.set_op_result(
                        false,
                        format!("Wrote {n} bytes to {name} but verify found {m} mismatch(es)"),
                    ),
                    Err(err) => this.set_op_result(false, format!("Write failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// I²C Verify — read-only, no confirmation. Needs a file + chip.
    pub fn start_i2c_verify(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some(chip_name)) = (self.i2c_verify_path.clone(), self.i2c_chip_name(cx))
        else {
            self.set_op_result(false, "Need a chip and a file".into());
            cx.notify();
            return;
        };
        self.push_log(format!("→ i2c verify ({chip_name}) vs {}", path.display()));
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.i2c_speed();
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let data = std::fs::read(&path).map_err(|e| format!("read input: {e}"))?;
                    let chip = crate::i2c_ops::resolve_chip(&chip_name)
                        .map_err(|e| format!("chip: {e}"))?;
                    let mut ch = Programmer::open_i2c(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let mut sink = GuiSink::new(progress, "i2c-vfy");
                    crate::i2c_ops::verify(&mut ch, &chip, &data, 0, 0, &mut sink)
                        .map_err(|e| format!("verify: {e}"))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok(0) => this.set_op_result(true, "Chip matches the file".into()),
                    Ok(m) => this.set_op_result(false, format!("{m} byte(s) differ")),
                    Err(err) => this.set_op_result(false, format!("Verify failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// I²C Erase — two-stage arm/confirm (destructive: writes 0xFF
    /// over the whole chip).
    pub fn arm_or_fire_i2c_erase(&mut self, cx: &mut Context<Self>) {
        if self.i2c_chip_name(cx).is_none() {
            self.set_op_result(false, "Pick a chip first".into());
            cx.notify();
            return;
        }
        if self.i2c_erase_armed {
            self.i2c_erase_armed = false;
            self.start_i2c_erase(cx);
        } else {
            self.i2c_erase_armed = true;
            self.push_log("⚠ i2c erase armed: click again to confirm".into());
            cx.notify();
        }
    }

    fn start_i2c_erase(&mut self, cx: &mut Context<Self>) {
        let Some(chip_name) = self.i2c_chip_name(cx) else {
            self.set_op_result(false, "Pick a chip first".into());
            cx.notify();
            return;
        };
        self.push_log(format!(
            "→ i2c erase ({chip_name}) — writing 0xFF everywhere"
        ));
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.i2c_speed();
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let chip = crate::i2c_ops::resolve_chip(&chip_name)
                        .map_err(|e| format!("chip: {e}"))?;
                    let mut ch = Programmer::open_i2c(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let mut sink = GuiSink::new(progress, "i2c-erase");
                    crate::i2c_ops::erase(&mut ch, &chip, 0, &mut sink)
                        .map_err(|e| format!("erase: {e}"))?;
                    Ok::<_, String>((chip.name, chip.size_bytes))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, size)) => {
                        this.set_op_result(true, format!("Erased {name} to 0xFF ({size} bytes)"))
                    }
                    Err(err) => this.set_op_result(false, format!("Erase failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// I²C Blank check — confirm every byte reads 0xFF. Read-only.
    pub fn start_i2c_blank_check(&mut self, cx: &mut Context<Self>) {
        let Some(chip_name) = self.i2c_chip_name(cx) else {
            self.set_op_result(false, "Pick a chip first".into());
            cx.notify();
            return;
        };
        self.push_log(format!("→ i2c blank check ({chip_name})"));
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.i2c_speed();
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let chip = crate::i2c_ops::resolve_chip(&chip_name)
                        .map_err(|e| format!("chip: {e}"))?;
                    let mut ch = Programmer::open_i2c(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let mut sink = GuiSink::new(progress, "i2c-blank");
                    crate::i2c_ops::blank_check(&mut ch, &chip, 0, &mut sink)
                        .map_err(|e| format!("{e}"))?;
                    Ok::<_, String>((chip.name, chip.size_bytes))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, size)) => this
                        .set_op_result(true, format!("{name} is blank — all 0xFF ({size} bytes)")),
                    Err(err) => this.set_op_result(false, format!("Blank check: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
