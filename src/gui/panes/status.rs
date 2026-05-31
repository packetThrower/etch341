//! Diagnostic panes: status registers, SFDP, OTP security registers.

// The parent module is this submodule's prelude (see panes.rs):
// `use super::*` pulls its imports + shared widget helpers in.
use super::*;

pub(super) fn status_pane(
    regs: Option<crate::spi::StatusRegisters>,
    result: Option<&(bool, String)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let mut col = op_pane(
        "Status registers",
        "Reads SR1 / SR2 / SR3 and decodes the standard bits. Useful \
         for diagnosing writes that silently fail (block-protect set) \
         or quad-mode issues (QE clear).",
    );
    col = col.child(action_button_for(
        "Read status",
        "read-status",
        cx,
        |this, cx| this.start_read_status(cx),
    ));
    col = col.children(result_block(result));
    if let Some(r) = regs {
        // Group the three register blocks inside a single card so
        // "decoded result" is visually distinct from the pane's
        // heading + body + button stack above. Each SR block keeps
        // its own header (SR1 / SR2 / SR3) inside the card. The
        // Copy button on the card writes a plain-text dump of all
        // three registers to the clipboard for paste-into-issue
        // / share-with-a-coworker workflows.
        let copy_text = format_status_for_copy(&r);
        col = col.child(
            card_with_copy(copy_text, "copy-status", cx)
                .child(status_register_block(
                    "SR1",
                    r.sr1,
                    true,
                    &[
                        ("WIP", r.wip().to_string()),
                        ("WEL", r.wel().to_string()),
                        ("BP", r.bp().to_string()),
                        ("TB", r.tb().to_string()),
                        ("SEC/BP3", r.sec_or_bp3().to_string()),
                        ("SRP0", r.srp0().to_string()),
                    ],
                ))
                .child(status_register_block(
                    "SR2",
                    r.sr2,
                    r.sr2_present(),
                    &[
                        ("SRP1", r.srp1().to_string()),
                        ("QE", r.qe().to_string()),
                        ("LB", r.lb().to_string()),
                        ("CMP", r.cmp().to_string()),
                        ("SUS", r.sus().to_string()),
                    ],
                ))
                .child(status_register_block(
                    "SR3",
                    r.sr3,
                    r.sr3_present(),
                    &[
                        ("ADP", r.adp().to_string()),
                        ("WPS", r.wps().to_string()),
                        ("DRV", r.drv().to_string()),
                        ("HOLD/RST", r.hold_rst().to_string()),
                    ],
                )),
        );
        // Same gotcha-surfacing note the CLI prints — most common
        // reason a user is on this pane is "writes silently failing"
        // due to BP bits, so call it out directly.
        if r.bp() != 0 || r.sec_or_bp3() {
            col = col.child(armed_warning(
                "SR1 has block-protect bits set: writes and erases to the protected range \
                 will silently fail. Clear BP[2:0] (and SEC/BP3 if set) via WRSR before \
                 programming.",
            ));
        }
    }
    col
}

/// Render the same SR1/SR2/SR3 decode the CLI's `etch341 sr` prints,
/// as a plain-text block suitable for clipboard. Lives in panes.rs
/// so the GUI Copy button doesn't need to round-trip through
/// `ops::print_status` (which writes to stdout).
pub(super) fn format_status_for_copy(r: &crate::spi::StatusRegisters) -> String {
    let bit = |b: bool| if b { '1' } else { '0' };
    let mut s = String::new();
    s.push_str(&format!("SR1 : 0x{:02X}  (0b{:08b})\n", r.sr1, r.sr1));
    s.push_str(&format!(
        "        WIP={} WEL={} BP={} TB={} SEC/BP3={} SRP0={}\n",
        bit(r.wip()),
        bit(r.wel()),
        r.bp(),
        bit(r.tb()),
        bit(r.sec_or_bp3()),
        bit(r.srp0()),
    ));
    if r.sr2_present() {
        s.push_str(&format!("SR2 : 0x{:02X}  (0b{:08b})\n", r.sr2, r.sr2));
        s.push_str(&format!(
            "        SRP1={} QE={} LB={} CMP={} SUS={}\n",
            bit(r.srp1()),
            bit(r.qe()),
            r.lb(),
            bit(r.cmp()),
            bit(r.sus()),
        ));
    } else {
        s.push_str("SR2 : 0xFF    (chip didn't respond)\n");
    }
    if r.sr3_present() {
        s.push_str(&format!("SR3 : 0x{:02X}  (0b{:08b})\n", r.sr3, r.sr3));
        s.push_str(&format!(
            "        ADP={} WPS={} DRV={} HOLD/RST={}\n",
            bit(r.adp()),
            bit(r.wps()),
            r.drv(),
            bit(r.hold_rst()),
        ));
    } else {
        s.push_str("SR3 : 0xFF    (chip didn't respond)\n");
    }
    s
}

/// One register row inside `status_pane` — section label, raw hex
/// (+ binary), and a wrapped grid of decoded bit name → value pairs.
/// `present == false` means the chip returned `0xFF` (no response)
/// and we hide the decoded bits since they'd just be noise.
pub(super) fn status_register_block(
    name: &'static str,
    value: u8,
    present: bool,
    bits: &[(&'static str, String)],
) -> gpui::Div {
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .child(
            div()
                .text_size(px(11.0))
                .text_color(theme::text_tertiary())
                .child(name),
        )
        .child(
            div()
                .text_size(px(12.0))
                .font_family(theme::MONO_FONT)
                .text_color(if present {
                    theme::text_primary()
                } else {
                    theme::text_tertiary()
                })
                .child(format!("0x{value:02X}  0b{value:08b}")),
        );
    if !present {
        return div().flex().flex_col().gap_1().child(header).child(
            div()
                .text_size(px(12.0))
                .text_color(theme::text_tertiary())
                .whitespace_normal()
                .child(format!(
                    "(chip didn't respond, likely doesn't implement {name})"
                )),
        );
    }
    // Bit-grid: each entry is "NAME: value" in Menlo at 12px, laid
    // out as flex-wrap so narrow windows wrap rather than overflow.
    let mut grid = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_x_4()
        .gap_y_1()
        .text_size(px(12.0))
        .font_family(theme::MONO_FONT)
        .text_color(theme::text_secondary());
    for (label, v) in bits {
        grid = grid.child(div().child(format!("{label}={v}")));
    }
    div().flex().flex_col().gap_1().child(header).child(grid)
}

/// OTP / security-register pane. Mirrors `etch341 otp read` /
/// `otp erase` / `otp write`: read dumps the three registers as a
/// copyable hexdump card; erase / write target one register behind
/// the same two-stage arm/confirm the Erase and Write panes use.
pub(super) fn otp_pane(
    regs: Option<&[crate::ops::OtpRegister]>,
    target_register: u8,
    write_path: Option<&Path>,
    erase_armed: bool,
    write_armed: bool,
    result: Option<&(bool, String)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let mut col = op_pane(
        "Security registers",
        "The chip's security registers (Winbond / GigaDevice 0x48 \
         convention: three 256-byte registers) commonly hold serial \
         numbers, MAC addresses, or vendor keys. Read dumps all \
         three. Erase / write target one register — programming only \
         clears bits, so erase first for a clean write. Macronix \
         parts use a different opcode and aren't covered.",
    );
    col = col.child(action_button_for(
        "Read security registers",
        "read-otp",
        cx,
        |this, cx| this.start_read_otp(cx),
    ));
    col = col.children(result_block(result));
    if let Some(regs) = regs {
        let mut card = card_with_copy(format_otp_for_copy(regs), "copy-otp", cx);
        for reg in regs {
            card = card.child(otp_register_block(reg));
        }
        col = col.child(card);
    }

    // Destructive controls live below a divider, split into two
    // separate outlined boxes so erase and write don't read as one
    // cluster. The target register lives with erase (you pick a
    // register and can wipe it there); write gets its own box. Both
    // act on the same selected register; both are capped at 680px to
    // match the Settings pane's content width.
    col = col.child(otp_divider());

    let selected_idx = (target_register as usize).checked_sub(1);
    let mut target_box = GroupBox::new()
        .id("otp-target-box")
        .outline()
        .max_w(px(680.0))
        .title("Target register")
        .child(
            RadioGroup::horizontal("otp-target")
                .selected_index(selected_idx)
                .on_click(cx.listener(|this: &mut AppView, ix: &usize, _, cx| {
                    this.set_otp_target_register(*ix as u8 + 1, cx);
                }))
                .children(
                    crate::ops::OTP_REGISTER_INDICES
                        .iter()
                        .map(|n| format!("Register {n}")),
                ),
        );
    if erase_armed {
        target_box = target_box.child(armed_warning(
            "Armed: next click erases the selected register to 0xFF.",
        ));
    }
    target_box = target_box.child(armable_button(
        "Erase selected register",
        "Click again to erase",
        "otp-erase",
        erase_armed,
        cx,
        |this, cx| this.arm_or_fire_otp_erase(cx),
    ));

    let mut write_box = GroupBox::new()
        .id("otp-write-box")
        .outline()
        .max_w(px(680.0))
        .title("Write from file")
        .child(bordered_file_row(write_path, "pick-otp", cx, |this, cx| {
            this.pick_otp_file(cx)
        }));
    if write_armed && write_path.is_some() {
        write_box = write_box.child(armed_warning(
            "Armed: next click programs the selected register from the file (offset 0).",
        ));
    }
    write_box = write_box.child(armable_button(
        "Write selected register from file",
        "Click again to write",
        "otp-write",
        write_armed,
        cx,
        |this, cx| this.arm_or_fire_otp_write(cx),
    ));

    col = col.child(target_box).child(write_box);
    col
}

/// Hairline divider separating the read output from the destructive
/// modify boxes below it. `my_2` adds margin on top of the pane's
/// 16px flex gap so the read↔modify boundary reads as a real
/// section break rather than just another row.
///
/// `flex_shrink_0` is load-bearing: the pane is a scrolling flex
/// column, and at the default window height the content overflows.
/// Flexbox then shrinks children to fit — a 1px divider with no
/// content shrinks straight to 0 and vanishes (the buttons / boxes
/// survive because they have a min-content height). Pinning shrink
/// to 0 keeps the line at its 1px height regardless of overflow.
pub(super) fn otp_divider() -> impl IntoElement {
    div()
        .flex_shrink_0()
        .my_2()
        .h(px(1.0))
        .bg(theme::workshop_glass_strong())
}

/// Hexdump one register as offset / hex / ASCII text lines, matching
/// the CLI `otp read` layout. Blank (all-0xFF) registers collapse to
/// a single note line instead of 16 identical rows. Shared by the
/// visual block and the clipboard text so the two never drift.
pub(super) fn otp_hexdump_lines(reg: &crate::ops::OtpRegister) -> Vec<String> {
    if reg.is_blank() {
        return vec!["all 0xFF (blank / unprogrammed)".to_string()];
    }
    reg.data
        .chunks(16)
        .enumerate()
        .map(|(i, chunk)| {
            let off = reg.addr as usize + i * 16;
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02X}")).collect();
            let ascii: String = chunk
                .iter()
                .map(|&b| {
                    if (0x20..0x7F).contains(&b) {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            format!("{off:06X}  {:<47}  |{ascii}|", hex.join(" "))
        })
        .collect()
}

/// One register block inside the OTP card: a small-caps header plus
/// the monospace hexdump lines.
pub(super) fn otp_register_block(reg: &crate::ops::OtpRegister) -> gpui::Div {
    let header = div()
        .text_size(px(11.0))
        .text_color(theme::text_tertiary())
        .child(format!("REGISTER {} @ 0x{:06X}", reg.index, reg.addr));
    let mut body = div()
        .flex()
        .flex_col()
        .text_size(px(12.0))
        .font_family(theme::MONO_FONT)
        .text_color(if reg.is_blank() {
            theme::text_tertiary()
        } else {
            theme::text_secondary()
        });
    for line in otp_hexdump_lines(reg) {
        body = body.child(div().whitespace_nowrap().child(line));
    }
    div().flex().flex_col().gap_1().child(header).child(body)
}

/// Plain-text hexdump of all registers for the card's Copy button —
/// matches the CLI `otp read` output so a paste-into-issue reads the
/// same as a terminal session.
pub(super) fn format_otp_for_copy(regs: &[crate::ops::OtpRegister]) -> String {
    let mut s = String::new();
    for reg in regs {
        s.push_str(&format!("Register {} @ 0x{:06X}:\n", reg.index, reg.addr));
        for line in otp_hexdump_lines(reg) {
            s.push_str("  ");
            s.push_str(&line);
            s.push('\n');
        }
        s.push('\n');
    }
    s
}

/// Build the card that renders a decoded SFDP table: header line,
/// parameter-header list, decoded BFPT, and erase-type table. Used
/// by the Detect pane after a Detect run that picked up SFDP. Lives
/// here (not in the Detect pane render) so the layout is reusable
/// if we ever want a standalone SFDP view back.
pub(super) fn sfdp_card(parsed: &crate::sfdp::Sfdp, cx: &mut Context<AppView>) -> gpui::Div {
    let mut hdr_grid = mono_block();
    for (i, ph) in parsed.parameter_headers.iter().enumerate() {
        let tag = if ph.id == crate::sfdp::ParameterHeader::BFPT_ID {
            "JEDEC BFPT"
        } else {
            "vendor"
        };
        hdr_grid = hdr_grid.child(div().child(format!(
            "[{i}] id=0x{:04X} ({tag})  rev {}.{}  len={} dwords  @ 0x{:06X}",
            ph.id, ph.major_rev, ph.minor_rev, ph.length_dwords, ph.ptr,
        )));
    }

    let copy_text = format_sfdp_for_copy(parsed);
    let mut output = card_with_copy(copy_text, "copy-sfdp", cx)
        .child(section_block_with_text(
            "SFDP HEADER",
            format!(
                "rev {}.{}  ·  {} parameter header(s)",
                parsed.header.major_rev,
                parsed.header.minor_rev,
                parsed.parameter_headers.len(),
            ),
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_label("PARAMETER HEADERS"))
                .child(hdr_grid),
        );

    if let Some(bfpt) = &parsed.bfpt {
        let addr = match bfpt.addressing {
            crate::sfdp::Addressing::ThreeByteOnly => "3-byte only",
            crate::sfdp::Addressing::Either => "3- or 4-byte (default 3)",
            crate::sfdp::Addressing::FourByteOnly => "4-byte only",
            crate::sfdp::Addressing::Reserved => "reserved encoding",
        };
        let mut grid = mono_block()
            .child(div().child(format!(
                "size      : {} bytes ({} KB, {} Mbit)",
                bfpt.size_bytes,
                bfpt.size_bytes / 1024,
                bfpt.size_bytes * 8 / 1_000_000,
            )))
            .child(div().child(format!("page size : {} bytes", bfpt.page_size)))
            .child(div().child(format!("address   : {addr}")));
        if bfpt.erase_4k_opcode != 0xFF {
            grid = grid
                .child(div().child(format!("4K erase  : opcode 0x{:02X}", bfpt.erase_4k_opcode)));
        } else {
            grid = grid.child(div().child("4K erase  : not supported".to_string()));
        }
        output = output.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_label("BFPT"))
                .child(grid),
        );

        let mut etype_grid = mono_block();
        let mut any = false;
        for (i, e) in bfpt.erase_types.iter().enumerate() {
            if e.size_bytes == 0 {
                continue;
            }
            any = true;
            let size = if e.size_bytes >= (1 << 20) {
                format!("{} MB", e.size_bytes >> 20)
            } else if e.size_bytes >= (1 << 10) {
                format!("{} KB", e.size_bytes >> 10)
            } else {
                format!("{} B", e.size_bytes)
            };
            etype_grid = etype_grid.child(div().child(format!(
                "[{i}] opcode 0x{:02X}  {} bytes ({size})",
                e.opcode, e.size_bytes,
            )));
        }
        if !any {
            etype_grid = etype_grid.child(div().child("(none advertised)".to_string()));
        }
        output = output.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_label("ERASE TYPES"))
                .child(etype_grid),
        );
    } else {
        output = output.child(
            div()
                .text_color(theme::text_tertiary())
                .child("(BFPT body wasn't reachable inside the 256-byte read window)"),
        );
    }

    output
}

/// Plain-text rendering of the same decoded SFDP view the pane
/// shows visually. Used by the card's Copy button to put the
/// output on the clipboard.
pub(super) fn format_sfdp_for_copy(parsed: &crate::sfdp::Sfdp) -> String {
    use crate::sfdp::{Addressing, ParameterHeader};
    let mut s = String::new();
    s.push_str(&format!(
        "SFDP rev {}.{}, {} parameter header(s)\n",
        parsed.header.major_rev,
        parsed.header.minor_rev,
        parsed.parameter_headers.len(),
    ));
    for (i, ph) in parsed.parameter_headers.iter().enumerate() {
        let tag = if ph.id == ParameterHeader::BFPT_ID {
            "JEDEC BFPT"
        } else {
            "vendor"
        };
        s.push_str(&format!(
            "  [{i}] id=0x{:04X} ({tag}) rev {}.{} len={} dwords @ 0x{:06X}\n",
            ph.id, ph.major_rev, ph.minor_rev, ph.length_dwords, ph.ptr,
        ));
    }
    if let Some(b) = &parsed.bfpt {
        let addr = match b.addressing {
            Addressing::ThreeByteOnly => "3-byte only",
            Addressing::Either => "3- or 4-byte (default 3)",
            Addressing::FourByteOnly => "4-byte only",
            Addressing::Reserved => "reserved encoding",
        };
        s.push_str("\nBFPT:\n");
        s.push_str(&format!(
            "  size      : {} bytes ({} KB, {} Mbit)\n",
            b.size_bytes,
            b.size_bytes / 1024,
            b.size_bytes * 8 / 1_000_000,
        ));
        s.push_str(&format!("  page size : {} bytes\n", b.page_size));
        s.push_str(&format!("  address   : {addr}\n"));
        if b.erase_4k_opcode != 0xFF {
            s.push_str(&format!(
                "  4K erase  : opcode 0x{:02X}\n",
                b.erase_4k_opcode
            ));
        } else {
            s.push_str("  4K erase  : not supported\n");
        }
        s.push_str("  erase types:\n");
        let mut any = false;
        for (i, e) in b.erase_types.iter().enumerate() {
            if e.size_bytes == 0 {
                continue;
            }
            any = true;
            s.push_str(&format!(
                "    [{i}] opcode 0x{:02X}  {} bytes\n",
                e.opcode, e.size_bytes,
            ));
        }
        if !any {
            s.push_str("    (none advertised)\n");
        }
    }
    s
}
