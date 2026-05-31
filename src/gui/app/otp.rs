//! `AppView` methods — OTP security-register operations: read / target / write / erase.

// `impl AppView` blocks may live in any module of the crate; this
// submodule adds one. `use crate::gui::*` pulls the parent module's
// prelude (gpui, AppView, shared types + the `push_log`/`set_op_result`
// infra these methods call) into scope.
use crate::gui::*;

impl AppView {
    /// Read the three security (OTP) registers in the background and
    /// stash them in `self.otp_regs` for the OTP pane. Mirrors
    /// `start_read_status` — same JEDEC-first guard, no progress bar
    /// (three 256-byte reads finish well under the poll interval).
    pub fn start_read_otp(&mut self, cx: &mut Context<Self>) {
        self.push_log("→ security registers".into());
        self.op_result = None;
        cx.notify();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut ch = Programmer::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let detect = ops::run_detect(&mut ch).map_err(|e| format!("detect: {e}"))?;
                    match detect.diagnosis {
                        Diagnosis::MisoStuckLow => {
                            return Err("MISO stuck low (target board contention)".into());
                        }
                        Diagnosis::MisoFloatsHigh => {
                            return Err("MISO floats high (no chip / HOLD# grounded)".into());
                        }
                        _ => {}
                    }
                    ops::read_otp_registers(&mut ch).map_err(|e| format!("{e}"))
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok(regs) => {
                        let blank = regs.iter().filter(|r| r.is_blank()).count();
                        this.push_log(format!(
                            "Security registers OK: {} read, {blank} blank (0xFF)",
                            regs.len()
                        ));
                        this.otp_regs = Some(regs);
                    }
                    Err(err) => {
                        this.set_op_result(false, format!("Security registers read failed: {err}"))
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Set the target register (1/2/3) for the OTP erase / write
    /// controls. Re-disarms both since the target changed under them.
    pub fn set_otp_target_register(&mut self, register: u8, cx: &mut Context<Self>) {
        self.otp_target_register = register;
        self.otp_erase_armed = false;
        self.otp_write_armed = false;
        cx.notify();
    }

    /// File picker for the OTP write source. Reuses the Write pane's
    /// last-directory memory so the two share a starting folder.
    pub fn pick_otp_file(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_write_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("Binary", &["bin", "rom"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            weak.update(cx, |this, cx| {
                this.push_log(format!("Picked for OTP write: {}", path.display()));
                if let Some(parent) = path.parent() {
                    this.prefs.last_write_dir = Some(parent.to_path_buf());
                    let _ = this.prefs.save();
                }
                this.otp_write_path = Some(path);
                this.otp_write_armed = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Two-stage erase trigger for the OTP pane. First click arms,
    /// second fires.
    pub fn arm_or_fire_otp_erase(&mut self, cx: &mut Context<Self>) {
        if self.otp_erase_armed {
            self.otp_erase_armed = false;
            self.start_otp_erase(cx);
        } else {
            self.otp_erase_armed = true;
            self.push_log(format!(
                "⚠ OTP erase armed (register {}): click again to confirm",
                self.otp_target_register
            ));
            cx.notify();
        }
    }

    fn start_otp_erase(&mut self, cx: &mut Context<Self>) {
        let register = self.otp_target_register;
        self.push_log(format!("→ erase security register {register}"));
        cx.notify();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut ch = Programmer::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    ops::ensure_chip_present(&mut ch).map_err(|e| format!("{e}"))?;
                    ops::otp_erase(&mut ch, register).map_err(|e| format!("{e}"))?;
                    // Re-read so the result card reflects the erase.
                    ops::read_otp_registers(&mut ch).map_err(|e| format!("{e}"))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok(regs) => {
                        this.otp_regs = Some(regs);
                        this.set_op_result(
                            true,
                            format!("Security register {register} erased to 0xFF"),
                        );
                    }
                    Err(err) => this.set_op_result(false, format!("OTP erase failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Two-stage write trigger for the OTP pane. Writes from offset 0
    /// of the selected register; use the CLI `--start` for a partial
    /// write. Requires a file to be picked first.
    pub fn arm_or_fire_otp_write(&mut self, cx: &mut Context<Self>) {
        if self.otp_write_path.is_none() {
            self.set_op_result(false, "Pick an input file first".into());
            cx.notify();
            return;
        }
        if self.otp_write_armed {
            self.otp_write_armed = false;
            self.start_otp_write(cx);
        } else {
            self.otp_write_armed = true;
            self.push_log(format!(
                "⚠ OTP write armed (register {}): click again to confirm",
                self.otp_target_register
            ));
            cx.notify();
        }
    }

    fn start_otp_write(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.otp_write_path.clone() else {
            self.set_op_result(false, "Pick an input file first".into());
            cx.notify();
            return;
        };
        let register = self.otp_target_register;
        self.push_log(format!(
            "→ write security register {register} from {}",
            path.display()
        ));
        cx.notify();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let data = std::fs::read(&path).map_err(|e| format!("read input: {e}"))?;
                    let mut ch = Programmer::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    ops::ensure_chip_present(&mut ch).map_err(|e| format!("{e}"))?;
                    ops::otp_write(&mut ch, register, 0, &data).map_err(|e| format!("{e}"))?;
                    let len = data.len();
                    let regs = ops::read_otp_registers(&mut ch).map_err(|e| format!("{e}"))?;
                    Ok::<_, String>((len, regs))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((len, regs)) => {
                        this.otp_regs = Some(regs);
                        this.set_op_result(
                            true,
                            format!(
                                "Security register {register} written ({len} byte(s), verified)"
                            ),
                        );
                    }
                    Err(err) => this.set_op_result(false, format!("OTP write failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
