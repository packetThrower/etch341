//! `AppView` methods — SPI flash operations: detect / read / write / erase / verify / blank / status.

// `impl AppView` blocks may live in any module of the crate; this
// submodule adds one. `use crate::gui::*` pulls the parent module's
// prelude (gpui, AppView, shared types + the `push_log`/`set_op_result`
// infra these methods call) into scope.
use crate::gui::*;

impl AppView {
    /// Open the OS folder picker to choose where Read pane dumps
    /// should land. Saved to `prefs.read_output_dir`; cleared back
    /// to `None` (use `$HOME` fallback) by passing nothing the
    /// picker rejects. Deferred via `cx.spawn` so the dialog
    /// doesn't block the foreground render — same panic-avoidance
    /// reason as `pick_hex_file`.
    pub fn pick_read_output_dir(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.read_output_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new();
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_folder().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            weak.update(cx, |this, cx| {
                this.push_log(format!("Read save location: {}", path.display()));
                this.prefs.read_output_dir = Some(path);
                let _ = this.prefs.save();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Open the OS file picker to choose a binary to write to the chip.
    /// Deferred via cx.spawn — see `pick_hex_file` for the panic-avoidance
    /// rationale. Remembers the parent dir as `last_write_dir` so the
    /// next pick lands in the same place.
    pub fn pick_write_file(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_write_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("Flash dumps", &["bin", "rom"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            weak.update(cx, |this, cx| {
                this.push_log(format!("Picked for write: {}", path.display()));
                if let Some(parent) = path.parent() {
                    this.prefs.last_write_dir = Some(parent.to_path_buf());
                    let _ = this.prefs.save();
                }
                this.write_input_path = Some(path);
                this.write_armed = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn pick_verify_file(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_verify_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("Flash dumps", &["bin", "rom"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            weak.update(cx, |this, cx| {
                this.push_log(format!("Picked for verify: {}", path.display()));
                if let Some(parent) = path.parent() {
                    this.prefs.last_verify_dir = Some(parent.to_path_buf());
                    let _ = this.prefs.save();
                }
                this.verify_input_path = Some(path);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Two-stage trigger for write (same shape as `arm_or_fire_erase`).
    pub fn arm_or_fire_write(&mut self, cx: &mut Context<Self>) {
        if self.write_input_path.is_none() {
            self.set_op_result(false, "Pick an input file first".into());
            cx.notify();
            return;
        }
        if self.write_armed {
            self.write_armed = false;
            self.start_write(cx);
        } else {
            self.write_armed = true;
            self.push_log("⚠ Write armed: click again to confirm".into());
            cx.notify();
        }
    }

    /// Background-spawn ops::write with erase-first + verify-after
    /// (matches the CLI's default behaviour). Path must already be set.
    fn start_write(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.write_input_path.clone() else {
            self.set_op_result(false, "Pick an input file first".into());
            cx.notify();
            return;
        };
        self.push_log(format!(
            "→ write {} (erase + program + verify)",
            path.display()
        ));
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut sink = GuiSink::new(progress, "write");
                    let data = std::fs::read(&path).map_err(|e| format!("read input: {e}"))?;
                    let mut ch = Programmer::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let detect = ops::run_detect(&mut ch).map_err(|e| format!("detect: {e}"))?;
                    let chip = match detect.diagnosis {
                        Diagnosis::Known(c) => c,
                        Diagnosis::UnknownChip => {
                            // JEDEC isn't in the bundled DB; fall back to
                            // SFDP so a brand-new chip can still be
                            // read/written/erased/verified using its
                            // self-described parameters. If SFDP isn't
                            // available either, surface the
                            // ChipNotRecognized condition with both
                            // escape hatches in the message.
                            let jedec = detect.jedec_string();
                            match ops::synthesize_from_sfdp(&mut ch, &jedec) {
                                Ok(Some(c)) => c,
                                _ => {
                                    return Err(format!(
                                        "unknown JEDEC 0x{jedec} and chip has no SFDP; add to chips.toml or pass --chip"
                                    ));
                                }
                            }
                        }
                        Diagnosis::MisoStuckLow => {
                            return Err("MISO stuck low (target board contention)".into());
                        }
                        Diagnosis::MisoFloatsHigh => {
                            return Err("MISO floats high (no chip / HOLD# grounded)".into());
                        }
                    };
                    ops::write(&mut ch, &chip, &data, 0, true, true, &mut sink)
                        .map_err(|e| format!("write: {e}"))?;
                    Ok::<_, String>((chip.name, data.len()))
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, n)) => {
                        this.set_op_result(true, format!("Wrote {n} bytes to {name} (verified)"))
                    }
                    Err(err) => this.set_op_result(false, format!("Write failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Background-spawn ops::verify. Read-only, no confirmation needed.
    pub fn start_verify(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.verify_input_path.clone() else {
            self.set_op_result(false, "Pick an input file first".into());
            cx.notify();
            return;
        };
        self.verify_diff = None;
        self.push_log(format!("→ verify against {}", path.display()));
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut sink = GuiSink::new(progress, "verify");
                    let data = std::fs::read(&path).map_err(|e| format!("read input: {e}"))?;
                    let mut ch = Programmer::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let detect = ops::run_detect(&mut ch).map_err(|e| format!("detect: {e}"))?;
                    let chip = match detect.diagnosis {
                        Diagnosis::Known(c) => c,
                        Diagnosis::UnknownChip => {
                            // JEDEC isn't in the bundled DB; fall back to
                            // SFDP so a brand-new chip can still be
                            // read/written/erased/verified using its
                            // self-described parameters. If SFDP isn't
                            // available either, surface the
                            // ChipNotRecognized condition with both
                            // escape hatches in the message.
                            let jedec = detect.jedec_string();
                            match ops::synthesize_from_sfdp(&mut ch, &jedec) {
                                Ok(Some(c)) => c,
                                _ => {
                                    return Err(format!(
                                        "unknown JEDEC 0x{jedec} and chip has no SFDP; add to chips.toml or pass --chip"
                                    ));
                                }
                            }
                        }
                        Diagnosis::MisoStuckLow => return Err("MISO stuck low".into()),
                        Diagnosis::MisoFloatsHigh => return Err("MISO floats high".into()),
                    };
                    let chip_bytes =
                        ops::read_bytes(&mut ch, &chip, 0, data.len() as u32, &mut sink)
                            .map_err(|e| format!("verify: {e}"))?;
                    let offsets = crate::diff::diff_offsets(&data, &chip_bytes);
                    Ok::<_, String>((chip.name, chip_bytes, offsets, data))
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, _chip, offs, file)) if offs.is_empty() => {
                        let n = file.len();
                        this.set_op_result(true, format!("All {n} bytes match {name}"))
                    }
                    Ok((name, chip, offs, file)) => {
                        let (n, m) = (file.len(), offs.len());
                        this.verify_diff = Some(VerifyDiff {
                            file_bytes: Arc::new(file),
                            chip_bytes: Arc::new(chip),
                            offsets: offs,
                        });
                        this.set_op_result(false, format!("{m} of {n} bytes differ ({name})"));
                    }
                    Err(err) => this.set_op_result(false, format!("Verify failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Run `ops::run_detect` synchronously on the UI thread, also
    /// read the chip's SFDP table, and fold both into the session
    /// header / activity log / Detect pane state. USB enumeration,
    /// JEDEC, and SFDP together total roughly 60 ms in practice,
    /// which is acceptable on the UI thread for this command (long
    /// ops use background tasks). Stashing the parsed SFDP into
    /// `self.detect_sfdp` lets the Detect pane render the rich
    /// JESD216 view without a separate "Read SFDP" button.
    pub fn refresh_detect(&mut self, cx: &mut Context<Self>) {
        self.push_log("→ detect".to_string());
        self.op_result = None;
        // `outcome` carries the resolved chip-info plus the SFDP
        // parse (if any). MISO-stuck states short-circuit before
        // the SFDP read so we don't waste time decoding 256 bytes
        // of `0xFF` on a disconnected bus.
        let outcome: Result<(DetectInfo, Option<crate::sfdp::Sfdp>, ConnState), _> =
            Programmer::open(false).and_then(|mut ch| {
                let result = ops::run_detect(&mut ch)?;
                let jedec = result.jedec_string();
                let (chip, source, conn, sfdp) = match result.diagnosis {
                    Diagnosis::Known(c) => {
                        // Even for in-DB chips, read SFDP so the
                        // pane can show the rich table.
                        let sfdp = read_sfdp_best_effort(&mut ch);
                        let conn = ConnState::Ready {
                            name: c.name.clone(),
                            size_kb: c.size_kb,
                        };
                        (Some(c), ChipSource::Database, conn, sfdp)
                    }
                    Diagnosis::UnknownChip => {
                        // Try SFDP as fallback. If it provides a
                        // BFPT, synthesise a chip; either way keep
                        // the parsed SFDP for the pane.
                        let synth = ops::synthesize_from_sfdp(&mut ch, &jedec)?;
                        let sfdp = read_sfdp_best_effort(&mut ch);
                        match synth {
                            Some(c) => {
                                let conn = ConnState::Ready {
                                    name: c.name.clone(),
                                    size_kb: c.size_kb,
                                };
                                (Some(c), ChipSource::Sfdp, conn, sfdp)
                            }
                            None => (None, ChipSource::Unknown, ConnState::NoChip, sfdp),
                        }
                    }
                    Diagnosis::MisoStuckLow | Diagnosis::MisoFloatsHigh => {
                        (None, ChipSource::NoChip, ConnState::NoChip, None)
                    }
                };
                let info = DetectInfo {
                    jedec,
                    chip,
                    source,
                };
                Ok((info, sfdp, conn))
            });
        match outcome {
            Ok((info, sfdp, conn)) => {
                self.push_log(format!("JEDEC 0x{}", info.jedec));
                match (&info.source, info.chip.as_ref()) {
                    (ChipSource::Database, Some(c)) => {
                        self.push_log(format!("Detected {} ({} KB)", c.name, c.size_kb));
                    }
                    (ChipSource::Sfdp, Some(c)) => {
                        self.push_log(format!(
                            "Detected {} ({} KB, parameters from SFDP)",
                            c.name, c.size_kb
                        ));
                    }
                    (ChipSource::Unknown, _) => {
                        self.push_log(format!(
                            "Unknown JEDEC 0x{}: chip has no SFDP either; add to chips.toml or pass --chip",
                            info.jedec
                        ));
                    }
                    (ChipSource::NoChip, _) => {
                        // Surfaced via run_detect's diagnosis path
                        // — synthesize the right log line from the
                        // jedec ID, which encodes which condition
                        // we're in (000000 vs FFFFFF).
                        if info.jedec == "000000" {
                            self.push_log(
                                "MISO stuck low: target board contention (lift chip or pin 8)"
                                    .into(),
                            );
                        } else {
                            self.push_log(
                                "MISO floats high: no chip detected (check clip, VCC, pin 1)"
                                    .into(),
                            );
                        }
                    }
                    _ => {}
                }
                self.connection = match conn {
                    ConnState::Ready { name, size_kb } => Connection::Ready {
                        chip_name: name,
                        size_kb,
                    },
                    ConnState::NoChip => Connection::NoChip,
                };
                self.detect_result = Some(info);
                self.detect_sfdp = sfdp;
            }
            Err(err) => {
                self.set_op_result(false, format!("Detect failed: {err}"));
                self.connection = Connection::Disconnected;
                self.detect_result = None;
                self.detect_sfdp = None;
            }
        }
        cx.notify();
    }

    /// Fire a background read of the whole chip to a timestamped file
    /// in $HOME. The blocking USB+SPI work runs on
    /// `cx.background_executor()` so the GUI stays responsive; on
    /// completion the foreground updates the log + connection state.
    pub fn start_read(&mut self, cx: &mut Context<Self>) {
        let path = read_output_path(&self.prefs);
        self.push_log(format!("→ read → {}", path.display()));
        cx.notify();
        self.spawn_progress_poller(cx);

        let path_for_task = path.clone();
        let progress = self.progress.clone();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut sink = GuiSink::new(progress, "read");
                    let mut ch = Programmer::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let detect = ops::run_detect(&mut ch).map_err(|e| format!("detect: {e}"))?;
                    let chip = match detect.diagnosis {
                        Diagnosis::Known(c) => c,
                        Diagnosis::UnknownChip => {
                            // JEDEC isn't in the bundled DB; fall back to
                            // SFDP so a brand-new chip can still be
                            // read/written/erased/verified using its
                            // self-described parameters. If SFDP isn't
                            // available either, surface the
                            // ChipNotRecognized condition with both
                            // escape hatches in the message.
                            let jedec = detect.jedec_string();
                            match ops::synthesize_from_sfdp(&mut ch, &jedec) {
                                Ok(Some(c)) => c,
                                _ => {
                                    return Err(format!(
                                        "unknown JEDEC 0x{jedec} and chip has no SFDP; add to chips.toml or pass --chip"
                                    ));
                                }
                            }
                        }
                        Diagnosis::MisoStuckLow => {
                            return Err("MISO stuck low (target board contention)".into());
                        }
                        Diagnosis::MisoFloatsHigh => {
                            return Err("MISO floats high (no chip / HOLD# grounded)".into());
                        }
                    };
                    let size = chip.size_kb.saturating_mul(1024);
                    ops::read(&mut ch, &chip, 0, size, &path_for_task, &mut sink)
                        .map_err(|e| format!("read: {e}"))?;
                    Ok::<_, String>((chip.name, size))
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

    /// Two-stage destructive trigger for full-chip erase. First click
    /// flips `erase_armed`; the button visually re-renders (red text,
    /// new label). Second click within the same pane visit fires the
    /// real erase. Navigating away resets the arm state via the
    /// sidebar's pane-change handler.
    pub fn arm_or_fire_erase(&mut self, cx: &mut Context<Self>) {
        if self.erase_armed {
            self.erase_armed = false;
            self.start_erase(cx);
        } else {
            self.erase_armed = true;
            self.push_log("⚠ Erase armed: click again to confirm".into());
            cx.notify();
        }
    }

    /// Background-spawn the actual full-chip erase. Same shape as
    /// `start_read` / `start_blank_check`; ops::erase_chip handles
    /// the WREN → 0xC7 → poll WIP loop. Typical durations: ~30s for
    /// a 4 MB chip, several minutes for 16 MB+.
    fn start_erase(&mut self, cx: &mut Context<Self>) {
        self.push_log("→ erase chip starting (may take 30s–minutes)".into());
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut sink = GuiSink::new(progress, "erase");
                    let mut ch = Programmer::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let detect = ops::run_detect(&mut ch).map_err(|e| format!("detect: {e}"))?;
                    let chip = match detect.diagnosis {
                        Diagnosis::Known(c) => c,
                        Diagnosis::UnknownChip => {
                            // JEDEC isn't in the bundled DB; fall back to
                            // SFDP so a brand-new chip can still be
                            // read/written/erased/verified using its
                            // self-described parameters. If SFDP isn't
                            // available either, surface the
                            // ChipNotRecognized condition with both
                            // escape hatches in the message.
                            let jedec = detect.jedec_string();
                            match ops::synthesize_from_sfdp(&mut ch, &jedec) {
                                Ok(Some(c)) => c,
                                _ => {
                                    return Err(format!(
                                        "unknown JEDEC 0x{jedec} and chip has no SFDP; add to chips.toml or pass --chip"
                                    ));
                                }
                            }
                        }
                        Diagnosis::MisoStuckLow => {
                            return Err("MISO stuck low (target board contention)".into());
                        }
                        Diagnosis::MisoFloatsHigh => {
                            return Err("MISO floats high (no chip / HOLD# grounded)".into());
                        }
                    };
                    ops::erase_chip(&mut ch, &chip, &mut sink)
                        .map_err(|e| format!("erase: {e}"))?;
                    Ok::<_, String>(chip.name)
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok(name) => {
                        this.set_op_result(true, format!("Erased {name} — chip is now blank"))
                    }
                    Err(err) => this.set_op_result(false, format!("Erase failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Background-spawn a full-chip blank check. Useful for verifying
    /// that an erase succeeded (`ops::blank_check` returns
    /// `Error::NotBlank { addr, value }` on the first non-FF byte;
    /// the location is included in the error message).
    /// Read SR1/SR2/SR3 in the background and stash the result in
    /// `self.status_regs` for the Status pane to render. Mirrors
    /// the `etch341 sr` CLI subcommand. No progress bar — the read
    /// is three single-byte SPI ops, much faster than the polling
    /// interval that drives `SharedProgress`.
    pub fn start_read_status(&mut self, cx: &mut Context<Self>) {
        self.push_log("→ status regs".into());
        self.op_result = None;
        cx.notify();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut ch = Programmer::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    // Same JEDEC-first guard as `ops::status` —
                    // bail with a friendly message instead of
                    // showing a "decoded" 0xFF as protected state.
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
                    crate::spi::read_all_status(&mut ch).map_err(|e| format!("{e}"))
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok(regs) => {
                        this.status_regs = Some(regs);
                        this.push_log(format!(
                            "Status OK: SR1=0x{:02X} SR2=0x{:02X} SR3=0x{:02X}",
                            regs.sr1, regs.sr2, regs.sr3
                        ));
                    }
                    Err(err) => this.set_op_result(false, format!("Status read failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn start_blank_check(&mut self, cx: &mut Context<Self>) {
        self.push_log("→ blank check".into());
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut sink = GuiSink::new(progress, "blank");
                    let mut ch = Programmer::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let detect = ops::run_detect(&mut ch).map_err(|e| format!("detect: {e}"))?;
                    let chip = match detect.diagnosis {
                        Diagnosis::Known(c) => c,
                        Diagnosis::UnknownChip => {
                            // JEDEC isn't in the bundled DB; fall back to
                            // SFDP so a brand-new chip can still be
                            // read/written/erased/verified using its
                            // self-described parameters. If SFDP isn't
                            // available either, surface the
                            // ChipNotRecognized condition with both
                            // escape hatches in the message.
                            let jedec = detect.jedec_string();
                            match ops::synthesize_from_sfdp(&mut ch, &jedec) {
                                Ok(Some(c)) => c,
                                _ => {
                                    return Err(format!(
                                        "unknown JEDEC 0x{jedec} and chip has no SFDP; add to chips.toml or pass --chip"
                                    ));
                                }
                            }
                        }
                        Diagnosis::MisoStuckLow => {
                            return Err("MISO stuck low (target board contention)".into());
                        }
                        Diagnosis::MisoFloatsHigh => {
                            return Err("MISO floats high (no chip / HOLD# grounded)".into());
                        }
                    };
                    let size = chip.size_kb.saturating_mul(1024);
                    ops::blank_check(&mut ch, &chip, &mut sink).map_err(|e| format!("{e}"))?;
                    Ok::<_, String>((chip.name, size))
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, size)) => this.set_op_result(
                        true,
                        format!("{name} is blank — all {size} bytes are 0xFF"),
                    ),
                    Err(err) => this.set_op_result(false, format!("Blank check failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
