//! I²C EEPROM panes: scan / read / write / verify / erase / blank.

// The parent module is this submodule's prelude (see panes.rs):
// `use super::*` pulls its imports + shared widget helpers in.
use super::*;

/// I²C bus scan: probe every 7-bit address and list the ones that
/// ACK. The result card renders the hits; the body notes the
/// blank-EEPROM blind spot so an empty scan isn't mistaken for a
/// missing chip.
pub(super) fn i2c_scan_pane(
    hits: Option<&[u8]>,
    result: Option<&(bool, String)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let mut col = op_pane(
        "I²C Scan",
        "Probes 0x08..0x77 and lists the addresses that ACK. A 24Cxx \
         with its address pins grounded shows at 0x50. Note: a blank \
         EEPROM (all 0xFF) can't be detected — pick its chip and read \
         it directly.",
    )
    .child(action_button_for("Scan bus", "i2c-scan", cx, |this, cx| {
        this.start_i2c_scan(cx)
    }));

    if let Some(hits) = hits {
        let body = if hits.is_empty() {
            "No devices responded.".to_string()
        } else {
            hits.iter()
                .map(|a| format!("0x{a:02X}"))
                .collect::<Vec<_>>()
                .join("   ")
        };
        col = col.child(
            card_with_copy(body.clone(), "copy-i2c-scan", cx).child(
                mono_block()
                    .child(
                        div()
                            .text_color(theme::text_tertiary())
                            .child(format!("{} address(es) ACKing:", hits.len())),
                    )
                    .child(div().child(body)),
            ),
        );
    }
    col.children(result_block(result))
}

/// The shared I²C chip dropdown, rendered at the top of every I²C op
/// pane. All panes read/write the same `SelectState`, so the choice
/// persists as you move between Read / Write / Verify / etc.
pub(super) fn i2c_chip_picker(
    chip_select: &Entity<SelectState<Vec<SharedString>>>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .child(div().text_color(theme::text_secondary()).child("Chip"))
        .child(
            div()
                .w(px(160.0))
                .child(Select::new(chip_select).placeholder("Pick a 24Cxx…")),
        )
}

/// I²C read pane: chip picker + a button that dumps the whole chip to
/// a timestamped file (same save dir as the SPI Read pane).
pub(super) fn i2c_read_pane(
    chip_select: &Entity<SelectState<Vec<SharedString>>>,
    result: Option<&(bool, String)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    op_pane(
        "I²C Read",
        "Dumps the entire EEPROM to a timestamped file in your Read save \
         directory. Pick the chip first — I²C has no JEDEC auto-detect.",
    )
    .child(i2c_chip_picker(chip_select))
    .child(action_button_for(
        "Read chip",
        "i2c-read",
        cx,
        |this, cx| this.start_i2c_read(cx),
    ))
    .children(result_block(result))
}

/// I²C write pane: chip picker + file row + arm/confirm. Programs the
/// EEPROM from a file, then verifies (mirrors the SPI Write pane).
pub(super) fn i2c_write_pane(
    chip_select: &Entity<SelectState<Vec<SharedString>>>,
    path: Option<&Path>,
    armed: bool,
    result: Option<&(bool, String)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let mut group = GroupBox::new()
        .id("i2c-write-box")
        .outline()
        .max_w(px(680.0))
        .title("Program from file")
        .child(i2c_chip_picker(chip_select))
        .child(bordered_file_row(path, "pick-i2c-write", cx, |this, cx| {
            this.pick_i2c_write_file(cx)
        }));
    if armed && path.is_some() {
        group = group.child(armed_warning(
            "Armed: next click will overwrite the EEPROM.",
        ));
    }
    group = group.child(armable_button(
        "Write chip",
        "Click again to confirm",
        "start-i2c-write",
        armed,
        cx,
        |this, cx| this.arm_or_fire_i2c_write(cx),
    ));
    op_pane(
        "I²C Write",
        "Programs the EEPROM from a file, then verifies the result. \
         DESTRUCTIVE — same arm/confirm protection as Erase. Pick the \
         chip first.",
    )
    .child(group)
    .children(result_block(result))
}

/// I²C verify pane: chip picker + file row + a Verify button.
/// Read-only; reports how many bytes differ.
pub(super) fn i2c_verify_pane(
    chip_select: &Entity<SelectState<Vec<SharedString>>>,
    path: Option<&Path>,
    result: Option<&(bool, String)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let group = GroupBox::new()
        .id("i2c-verify-box")
        .outline()
        .max_w(px(680.0))
        .title("Compare against a file")
        .child(i2c_chip_picker(chip_select))
        .child(bordered_file_row(
            path,
            "pick-i2c-verify",
            cx,
            |this, cx| this.pick_i2c_verify_file(cx),
        ))
        .child(action_button_for(
            "Verify",
            "start-i2c-verify",
            cx,
            |this, cx| this.start_i2c_verify(cx),
        ));
    op_pane(
        "I²C Verify",
        "Reads the EEPROM and compares it against a file byte-by-byte. \
         Non-destructive; reports how many bytes differ.",
    )
    .child(group)
    .children(result_block(result))
}

/// I²C erase pane: chip picker + arm/confirm. Writes 0xFF everywhere.
pub(super) fn i2c_erase_pane(
    chip_select: &Entity<SelectState<Vec<SharedString>>>,
    armed: bool,
    result: Option<&(bool, String)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    op_pane(
        "I²C Erase",
        "Writes 0xFF to every byte — EEPROMs have no sector-erase, so \
         this is a full-chip overwrite. DESTRUCTIVE and not undoable; \
         arm/confirm protected. Pick the chip first.",
    )
    .child(i2c_chip_picker(chip_select))
    .when(armed, |this| {
        this.child(armed_warning(
            "Armed: next click will erase the whole EEPROM.",
        ))
    })
    .child(armable_button(
        "Erase chip",
        "Click again to confirm",
        "start-i2c-erase",
        armed,
        cx,
        |this, cx| this.arm_or_fire_i2c_erase(cx),
    ))
    .children(result_block(result))
}

/// I²C blank-check pane: chip picker + a button. Confirms all 0xFF.
pub(super) fn i2c_blank_pane(
    chip_select: &Entity<SelectState<Vec<SharedString>>>,
    result: Option<&(bool, String)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    op_pane(
        "I²C Blank check",
        "Confirms every byte on the chip reads back as 0xFF. \
         Non-destructive. Pick the chip first.",
    )
    .child(i2c_chip_picker(chip_select))
    .child(action_button_for(
        "Run blank check",
        "start-i2c-blank",
        cx,
        |this, cx| this.start_i2c_blank_check(cx),
    ))
    .children(result_block(result))
}
