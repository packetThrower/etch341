//! SPI flash operation panes: detect / read / erase / write / verify / blank.

// The parent module is this submodule's prelude (see panes.rs):
// `use super::*` pulls its imports + shared widget helpers in.
use super::*;

pub(super) fn read_pane(
    result: Option<&(bool, String)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    op_pane(
        "Read",
        "Dumps the chip to a timestamped file. Pick the save directory \
         in Settings → Read save location.",
    )
    .child(action_button_for(
        "Start read",
        "start-read",
        cx,
        |this, cx| this.start_read(cx),
    ))
    .children(result_block(result))
}

pub(super) fn erase_pane(
    armed: bool,
    result: Option<&(bool, String)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    op_pane(
        "Erase",
        "Erases the entire chip back to 0xFF. DESTRUCTIVE and not \
         undoable. Make sure you have a Read backup first. Click \
         the button to arm, then click again to actually erase. \
         Switching panes resets the arm state.",
    )
    .when(armed, |this| {
        this.child(armed_warning(
            "Armed: next click will erase the entire chip.",
        ))
    })
    .child(armable_button(
        "Erase chip",
        "Click again to confirm",
        "start-erase",
        armed,
        cx,
        |this, cx| this.arm_or_fire_erase(cx),
    ))
    .children(result_block(result))
}

pub(super) fn write_pane(
    path: Option<&Path>,
    armed: bool,
    result: Option<&(bool, String)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let mut group = GroupBox::new()
        .id("write-box")
        .outline()
        .max_w(px(680.0))
        .title("Program from file")
        .child(bordered_file_row(path, "pick-write", cx, |this, cx| {
            this.pick_write_file(cx)
        }));
    if armed && path.is_some() {
        group = group.child(armed_warning(
            "Armed: next click will erase and overwrite the chip.",
        ));
    }
    group = group.child(armable_button(
        "Write chip",
        "Click again to confirm",
        "start-write",
        armed,
        cx,
        |this, cx| this.arm_or_fire_write(cx),
    ));
    op_pane(
        "Write",
        "Programs the chip from a file. Erases first, then writes \
         page-by-page, then verifies. DESTRUCTIVE: same arm/confirm \
         protection as Erase. Switching panes resets the arm state.",
    )
    .child(group)
    .children(result_block(result))
}

pub(super) fn verify_pane(
    path: Option<&Path>,
    result: Option<&(bool, String)>,
    has_diff: bool,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let group = GroupBox::new()
        .id("verify-box")
        .outline()
        .max_w(px(680.0))
        .title("Compare against a file")
        .child(bordered_file_row(path, "pick-verify", cx, |this, cx| {
            this.pick_verify_file(cx)
        }))
        .child(action_button_for(
            "Verify",
            "start-verify",
            cx,
            |this, cx| this.start_verify(cx),
        ));
    op_pane(
        "Verify",
        "Reads the chip and compares against a file byte-by-byte. \
         Non-destructive. When bytes differ, \"View diff in Hex\" opens \
         the chip's read-back in the Hex pane with the mismatches \
         highlighted — step through them with Cmd/Ctrl+G.",
    )
    .child(group)
    .children(result_block(result))
    .when(has_diff, |this| {
        this.child(action_button_for(
            "View diff in Hex",
            "view-diff",
            cx,
            |this, cx| this.show_verify_diff(cx),
        ))
    })
}

pub(super) fn blank_pane(
    result: Option<&(bool, String)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    op_pane(
        "Blank check",
        "Reads the entire chip and confirms every byte is 0xFF. \
         Most useful after an erase — fails with the address of the \
         first non-FF byte if the chip isn't actually blank. A \
         programmed chip (e.g. a VBIOS) will fail at offset 0x0.",
    )
    .child(action_button_for(
        "Run blank check",
        "start-blank",
        cx,
        |this, cx| this.start_blank_check(cx),
    ))
    .children(result_block(result))
}

pub(super) fn detect_pane(
    info: Option<&crate::gui::DetectInfo>,
    sfdp: Option<&crate::sfdp::Sfdp>,
    result: Option<&(bool, String)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let mut col = op_pane(
        "Detect",
        "Reads the JEDEC ID, identifies the chip, and pulls the chip's \
         SFDP table if it has one. The other steps detect internally too, \
         so this is optional. Useful as a sanity check before anything \
         destructive.",
    )
    .child(
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .child(action_button("Detect chip", cx))
            .child(
                styled_button("browse-chipdb")
                    .bg(theme::workshop_glass())
                    .text_color(theme::text_secondary())
                    .hover(|d| {
                        d.bg(theme::workshop_glass_strong())
                            .text_color(theme::text_primary())
                    })
                    .child("Browse chip database")
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _, cx| {
                        this.open_chip_db(cx);
                    })),
            ),
    );

    // A failed Detect (no chip / bus error) clears the card and
    // surfaces a red ✗ line here instead; on success this is empty and
    // the chip-info card below is the result.
    col = col.children(result_block(result));

    let Some(info) = info else {
        return col;
    };

    // Chip-identification card (JEDEC + chip name + size + source +
    // notes). Always shown after a Detect run; the source field
    // makes it obvious whether the chip name came from the bundled
    // DB or from an SFDP synthesis.
    col = col.child(chip_info_card(info, cx));

    // Second card: the raw decoded SFDP table, if the chip carried
    // one. Rendered for both DB-hit and SFDP-fallback chips so power
    // users can inspect the table regardless of catalogue status.
    if let Some(s) = sfdp {
        col = col.child(sfdp_card(s, cx));
    }
    col
}

pub(super) fn chip_info_card(
    info: &crate::gui::DetectInfo,
    cx: &mut Context<AppView>,
) -> gpui::Div {
    use crate::gui::ChipSource;
    let mut grid = mono_block().child(div().child(format!("JEDEC : 0x{}", info.jedec)));
    let mut copy_text = format!("JEDEC : 0x{}\n", info.jedec);
    match (&info.source, info.chip.as_ref()) {
        (ChipSource::Database, Some(c)) => {
            grid = grid
                .child(div().child(format!("Chip  : {}", c.name)))
                .child(div().child("Source: chips.toml".to_string()))
                .child(div().child(format!(
                    "Size  : {} KB  ·  page {} B  ·  sector {} B",
                    c.size_kb, c.page_size, c.sector_size,
                )));
            copy_text.push_str(&format!(
                "Chip  : {}\nSource: chips.toml\nSize  : {} KB, page {} B, sector {} B\n",
                c.name, c.size_kb, c.page_size, c.sector_size
            ));
            if !c.notes.is_empty() {
                grid = grid.child(div().child(format!("Notes : {}", c.notes)));
                copy_text.push_str(&format!("Notes : {}\n", c.notes));
            }
        }
        (ChipSource::Sfdp, Some(c)) => {
            grid = grid
                .child(div().child(format!("Chip  : {}", c.name)))
                .child(div().child("Source: SFDP (no chips.toml entry)".to_string()))
                .child(div().child(format!(
                    "Size  : {} KB  ·  page {} B  ·  sector {} B",
                    c.size_kb, c.page_size, c.sector_size,
                )));
            copy_text.push_str(&format!(
                "Chip  : {}\nSource: SFDP (no chips.toml entry)\nSize  : {} KB, page {} B, sector {} B\n",
                c.name, c.size_kb, c.page_size, c.sector_size,
            ));
        }
        (ChipSource::Unknown, _) => {
            grid = grid
                .child(div().child("Chip  : unknown".to_string()))
                .child(div().child("Source: neither chips.toml nor SFDP".to_string()));
            copy_text.push_str("Chip  : unknown\nSource: neither chips.toml nor SFDP\n");
        }
        (ChipSource::NoChip, _) => {
            grid = grid.child(
                div()
                    .text_color(theme::text_tertiary())
                    .child("Chip  : (no chip on the bus)".to_string()),
            );
            copy_text.push_str("Chip  : (no chip on the bus)\n");
        }
        _ => {}
    }
    card_with_copy(copy_text, "copy-detect", cx).child(grid)
}
