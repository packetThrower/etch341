use super::{AppView, DiffRow, DiffSide, DiffView, Pane, theme};
use gpui::{
    AnyElement, ClickEvent, Context, Entity, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, UniformListScrollHandle, WeakEntity, div, prelude::FluentBuilder, px, uniform_list,
};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::tooltip::Tooltip;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

// Pane families live in submodules; this file keeps the dispatch
// (`render`) and the shared widget toolkit they build on. `use X::*`
// pulls each family's `pub(super)` pane fns back into scope so `render`
// calls them unqualified.
mod bios;
mod diff;
mod hex;
mod i2c;
mod settings;
mod spi;
mod status;
use bios::*;
use diff::*;
use hex::*;
use i2c::*;
use settings::*;
use spi::*;
use status::*;

/// Bundle of per-pane state passed from `AppView::render` to keep
/// `render()`'s signature from growing per added pane.
pub struct PaneInputs<'a> {
    pub erase_armed: bool,
    pub write_armed: bool,
    pub write_path: Option<&'a Path>,
    pub verify_path: Option<&'a Path>,
    /// True when the last verify left a stored diff — shows the Verify
    /// pane's "View diff in Hex" button.
    pub verify_has_diff: bool,
    pub hex_path: Option<&'a Path>,
    pub hex_bytes: Option<Arc<Vec<u8>>>,
    pub hex_strings: Option<Arc<Vec<(usize, String)>>>,
    pub hex_byte_matches: Arc<HashSet<usize>>,
    pub hex_match_total: usize,
    pub hex_current_match: Option<usize>,
    pub hex_scroll: UniformListScrollHandle,
    pub strings_scroll: UniformListScrollHandle,
    /// When `Some`, the Hex pane renders this side-by-side verify diff
    /// instead of its normal hex/strings viewer.
    pub hex_diff: Option<&'a DiffView>,
    pub diff_scroll: UniformListScrollHandle,
    pub diff_selection: Option<(DiffSide, usize, usize)>,
    pub hex_highlight_line: Option<usize>,
    pub hex_show_strings: bool,
    /// Whether the Hex pane's byte-colour legend is expanded.
    pub hex_show_legend: bool,
    /// Normalized `(lo, hi)` inclusive byte range. `None` = no
    /// selection. Already normalized by the caller (AppView::selection_range).
    pub hex_selection: Option<(usize, usize)>,
    pub hex_search_term: &'a str,
    pub hex_search_state: &'a Entity<InputState>,
    pub bios_path: Option<&'a Path>,
    pub bios_settings: Option<Arc<Vec<crate::uefi::Setting>>>,
    pub bios_tree: Option<Arc<Vec<crate::uefi::FormNode>>>,
    pub bios_boot: Option<Arc<Vec<crate::uefi::BootEntry>>>,
    pub bios_id: Option<&'a crate::uefi::BiosId>,
    pub bios_ifd: Option<&'a crate::ifd::Ifd>,
    pub bios_selected_form: Option<&'a str>,
    pub bios_changed_only: bool,
    pub bios_scroll: UniformListScrollHandle,
    pub bios_nav_scroll: UniformListScrollHandle,
    pub bios_search_term: &'a str,
    pub bios_search_state: &'a Entity<InputState>,
    pub prefs_path: Option<&'a Path>,
    /// Last-read SR1/SR2/SR3 bytes for the Status pane. `None`
    /// before the user clicks "Read"; populated by
    /// `AppView::start_read_status` once the chip has responded.
    pub status_regs: Option<crate::spi::StatusRegisters>,
    /// Security (OTP) registers from the most recent OTP-pane read.
    /// `None` before the user clicks "Read security registers".
    pub otp_regs: Option<&'a [crate::ops::OtpRegister]>,
    /// Target register (1/2/3) for the OTP pane's erase / write
    /// controls, the selected file, and the two arm flags.
    pub otp_target_register: u8,
    pub otp_write_path: Option<&'a Path>,
    pub otp_erase_armed: bool,
    pub otp_write_armed: bool,
    /// Most recent Detect result for the Detect pane's chip-info
    /// card. `None` until the user clicks Detect chip.
    pub detect_result: Option<&'a crate::gui::DetectInfo>,
    /// Parsed SFDP table from the most recent Detect run, shown
    /// as a second card inside the Detect pane when present.
    /// Decoupled from `detect_result` because some chips have a
    /// valid SFDP magic but a BFPT we can't decode (or vice
    /// versa).
    pub detect_sfdp: Option<&'a crate::sfdp::Sfdp>,
    /// Font size (px) for the hex+ASCII view inside the Hex pane.
    /// Persisted in `prefs.toml` and adjustable on the fly with
    /// Cmd/Ctrl + / - / 0.
    pub hex_font_size: f32,
    /// Same idea for the strings list inside the Hex pane.
    pub strings_font_size: f32,
    /// I²C bus-scan result (the 7-bit addresses that ACKed) for the
    /// I²C Scan pane. `None` before the first scan this session.
    pub i2c_scan_results: Option<&'a [u8]>,
    /// Shared dropdown state for picking the I²C chip; rendered in each
    /// I²C op pane (Read/Write/Verify/Erase/Blank check).
    pub i2c_chip_select: &'a Entity<SelectState<Vec<SharedString>>>,
    /// I²C Write / Verify file selections and the Write / Erase arm
    /// flags (the I²C analogues of `write_path` / `*_armed`).
    pub i2c_write_path: Option<&'a Path>,
    pub i2c_verify_path: Option<&'a Path>,
    pub i2c_write_armed: bool,
    pub i2c_erase_armed: bool,
    /// Last op outcome (either bus) — `(ok, message)` — rendered as a
    /// colored result line in the active op pane (green ✓ / red ✗).
    /// `None` before any op runs or after navigating away.
    pub op_result: Option<&'a (bool, String)>,
}

pub fn render(
    selected: Pane,
    inputs: PaneInputs<'_>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    match selected {
        Pane::Detect => detect_pane(
            inputs.detect_result,
            inputs.detect_sfdp,
            inputs.op_result,
            cx,
        )
        .into_any_element(),
        Pane::Read => read_pane(inputs.op_result, cx).into_any_element(),
        Pane::Erase => erase_pane(inputs.erase_armed, inputs.op_result, cx).into_any_element(),
        Pane::Write => write_pane(inputs.write_path, inputs.write_armed, inputs.op_result, cx)
            .into_any_element(),
        Pane::Verify => verify_pane(
            inputs.verify_path,
            inputs.op_result,
            inputs.verify_has_diff,
            cx,
        )
        .into_any_element(),
        Pane::Blank => blank_pane(inputs.op_result, cx).into_any_element(),
        Pane::Status => status_pane(inputs.status_regs, inputs.op_result, cx).into_any_element(),
        Pane::Otp => otp_pane(
            inputs.otp_regs,
            inputs.otp_target_register,
            inputs.otp_write_path,
            inputs.otp_erase_armed,
            inputs.otp_write_armed,
            inputs.op_result,
            cx,
        )
        .into_any_element(),
        Pane::Hex if inputs.hex_diff.is_some() => diff_pane(
            inputs.hex_diff.unwrap(),
            inputs.diff_selection,
            inputs.diff_scroll,
            inputs.hex_font_size,
            cx,
        )
        .into_any_element(),
        Pane::Hex => hex_pane(
            inputs.hex_path,
            inputs.hex_bytes,
            inputs.hex_strings,
            inputs.hex_byte_matches,
            inputs.hex_match_total,
            inputs.hex_current_match,
            inputs.hex_scroll,
            inputs.strings_scroll,
            inputs.hex_highlight_line,
            inputs.hex_show_strings,
            inputs.hex_show_legend,
            inputs.hex_selection,
            inputs.hex_search_term,
            inputs.hex_search_state,
            inputs.hex_font_size,
            inputs.strings_font_size,
            cx,
        )
        .into_any_element(),
        Pane::Bios => bios_pane(
            inputs.bios_path,
            inputs.bios_settings,
            inputs.bios_tree,
            inputs.bios_boot,
            inputs.bios_id,
            inputs.bios_ifd,
            inputs.bios_selected_form,
            inputs.bios_changed_only,
            inputs.bios_scroll,
            inputs.bios_nav_scroll,
            inputs.bios_search_term,
            inputs.bios_search_state,
            cx,
        )
        .into_any_element(),
        Pane::Settings => settings_pane(inputs.prefs_path, cx).into_any_element(),
        Pane::I2cScan => {
            i2c_scan_pane(inputs.i2c_scan_results, inputs.op_result, cx).into_any_element()
        }
        Pane::I2cRead => {
            i2c_read_pane(inputs.i2c_chip_select, inputs.op_result, cx).into_any_element()
        }
        Pane::I2cWrite => i2c_write_pane(
            inputs.i2c_chip_select,
            inputs.i2c_write_path,
            inputs.i2c_write_armed,
            inputs.op_result,
            cx,
        )
        .into_any_element(),
        Pane::I2cVerify => i2c_verify_pane(
            inputs.i2c_chip_select,
            inputs.i2c_verify_path,
            inputs.op_result,
            inputs.verify_has_diff,
            cx,
        )
        .into_any_element(),
        Pane::I2cErase => i2c_erase_pane(
            inputs.i2c_chip_select,
            inputs.i2c_erase_armed,
            inputs.op_result,
            cx,
        )
        .into_any_element(),
        Pane::I2cBlank => {
            i2c_blank_pane(inputs.i2c_chip_select, inputs.op_result, cx).into_any_element()
        }
    }
}

/// A colored result line for an op pane (either bus) — green ✓ on
/// success, red ✗ on failure (a hardware error, or a verify finding
/// mismatches). Returns `None` when no op has run, so panes
/// `.children(...)` it unconditionally. The result is cleared on
/// navigation, so it only shows in the pane that produced it rather
/// than lingering in the log.
fn result_block(result: Option<&(bool, String)>) -> Option<Alert> {
    let (ok, text) = result?;
    let alert = if *ok {
        Alert::success("op-result", text.clone())
    } else {
        Alert::error("op-result", text.clone())
    };
    // Kit's Alert is full-width; cap it so it doesn't span a wide
    // pane.
    Some(alert.mt_2().max_w(px(680.0)))
}

/// Small-caps section header inside the settings pane. Matches the
/// existing visual treatment used for "SPI CLOCK SPEED" /
/// "PREFERENCES FILE"; factored out to keep the new "WINDOW"
/// header consistent with the rest by construction.
fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .pt_3()
        .text_size(px(11.0))
        .text_color(theme::text_tertiary())
        .child(text)
}

/// File-path display + Browse button with the path shown in a
/// bordered, input-style box, so the selected file reads as a field
/// rather than loose text floating next to a button. Shared by every
/// pane with a file selection (Hex / Write / Verify / I²C / OTP), each
/// wrapped in a titled GroupBox.
fn bordered_file_row<F>(
    path: Option<&Path>,
    button_id: &'static str,
    cx: &mut Context<AppView>,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&mut AppView, &mut Context<AppView>) + 'static,
{
    let display = path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no file selected)".to_string());
    let path_color = if path.is_some() {
        theme::text_primary()
    } else {
        theme::text_tertiary()
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                // Clip a long path at the box edge instead of letting it
                // spill under the Browse button (paths have no spaces,
                // so whitespace wrapping can't break them).
                .overflow_hidden()
                .px_3()
                .py_2()
                .rounded(px(6.0))
                .border_1()
                .border_color(theme::workshop_glass_strong())
                .bg(theme::bench_black())
                .whitespace_nowrap()
                .text_color(path_color)
                .text_size(px(12.0))
                .font_family(theme::MONO_FONT)
                .child(display),
        )
        .child(action_button_for("Browse…", button_id, cx, on_click))
}

/// Mono-styled flex column used for fixed-width text blocks inside
/// the SFDP card. Pulls font + color + spacing into one place so
/// every section reads with the same rhythm.
fn mono_block() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .text_size(px(12.0))
        .font_family(theme::MONO_FONT)
        .text_color(theme::text_secondary())
}

/// Section label paired with a single line of mono-styled text.
/// Used by the SFDP card's "HEADER" section where the content is
/// a one-liner instead of a multi-row grid.
fn section_block_with_text(label: &'static str, text: String) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_label(label))
        .child(
            div()
                .text_size(px(12.0))
                .font_family(theme::MONO_FONT)
                .text_color(theme::text_secondary())
                .child(text),
        )
}

/// Shared outer shell for the operation panes (Detect / Read / Erase
/// / Write / Verify / Blank / Status / SFDP). Returns a scrollable
/// `flex_col` with the standard pane padding + gap, the heading,
/// and the body paragraph already added — callers chain on the
/// per-pane extras (file picker, armed warning, action button,
/// output cards) via `.child(...)`. Keeps "what a pane looks like"
/// in one place so future tweaks (e.g. changing the body color or
/// the gap between rows) land here instead of in eight
/// near-identical copies.
///
/// `id` is derived from the heading text so each pane gets a
/// distinct scroll handle — gpui requires a stable identifier on
/// any interactive (here, scrollable) element. The container is
/// `size_full()` + `min_h(0)` so flex shrinks it to the resizable
/// pane area rather than expanding to its content height, which
/// is what lets `overflow_y_scroll` actually clip the content
/// instead of growing the parent.
fn op_pane(heading_text: &'static str, body_text: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(heading_text)
        .size_full()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_4()
        .px_5()
        .py_5()
        .child(heading(heading_text))
        .child(body(body_text))
}

/// Visually-distinct container for pane output (Status register
/// blocks, SFDP decoded fields, etc). The slightly lighter glass
/// background + hairline border sets "this is the result of an
/// operation" apart from the surrounding pane chrome so the user's
/// eye lands on it without having to track what's heading vs
/// description vs output. Callers append the actual content
/// children via `.child(...)`.
fn card() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_4()
        .rounded(px(8.0))
        .bg(theme::workshop_glass())
        .border_1()
        .border_color(theme::workshop_glass_strong())
}

/// `card()` with GPUI Kit's copy button anchored to the top
/// right. Clicking the button writes `copy_text` to the system
/// clipboard. Used by output panes (Status registers, SFDP) so
/// users can share / paste the decoded output without retyping —
/// GPUI's `div().child("text")` doesn't natively support
/// cursor-based text selection, so a copy-all button is the
/// pragmatic substitute. The first child of the returned div is
/// the button row; callers chain the actual content below it
/// via `.child(...)`.
fn card_with_copy(
    copy_text: String,
    copy_id: &'static str,
    cx: &mut Context<AppView>,
) -> gpui::Div {
    card().child(
        div()
            .flex()
            .flex_row()
            .justify_end()
            .child(copy_button(copy_text, copy_id, cx)),
    )
}

/// GPUI Kit copy button. Click writes `text` to the clipboard and
/// pushes a brief confirmation to the activity log.
fn copy_button(text: String, id: &'static str, cx: &mut Context<AppView>) -> impl IntoElement {
    let weak = cx.entity().downgrade();
    Clipboard::new(id)
        .value(text)
        .tooltip("Copy")
        .on_copied(move |payload, _, cx| {
            let len = payload.len();
            let _ = weak.update(cx, |this: &mut AppView, _| {
                this.push_log(format!("Copied {len} chars to clipboard"));
            });
        })
}

fn heading(text: &'static str) -> impl IntoElement {
    div()
        .text_size(px(18.0))
        .text_color(theme::text_primary())
        .child(text)
}

fn body(text: &'static str) -> impl IntoElement {
    // `whitespace_normal` overrides gpui's default `nowrap`; without
    // this, long descriptions push the pane wider than its viewport
    // and the right edge clips off-screen.
    div()
        .text_color(theme::text_secondary())
        .whitespace_normal()
        .child(text)
}

/// The amber "Armed — next click will <thing>" pill shown by the
/// two-stage destructive panes (Erase, Write) between the body and
/// the action button when the user has primed the operation.
fn armed_warning(text: &'static str) -> impl IntoElement {
    // Same 680px cap as `result_block` — Kit's Alert is full-width.
    Alert::warning("armed-warning", text).max_w(px(680.0))
}

fn action_button(label: &'static str, cx: &mut Context<AppView>) -> impl IntoElement {
    // Legacy single-purpose button used by the Detect pane.
    action_button_for(label, label, cx, |this, cx| this.refresh_detect(cx))
}

fn action_button_for<F>(
    label: &'static str,
    id: &'static str,
    cx: &mut Context<AppView>,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&mut AppView, &mut Context<AppView>) + 'static,
{
    op_button(id, label).primary().on_click(cx.listener(
        move |this: &mut AppView, _: &ClickEvent, _, cx| {
            on_click(this, cx);
        },
    ))
}

/// Two-stage destructive button used by Erase and Write. First click
/// arms (the parent sets `armed = true` and re-renders); second
/// click fires. The button flips accent → danger red as a "you're
/// about to do something irreversible" cue. The handler closure runs on
/// every click — the AppView decides whether this is an arm or a
/// fire based on its own `*_armed` flag.
fn armable_button<F>(
    idle_label: &'static str,
    armed_label: &'static str,
    id: &'static str,
    armed: bool,
    cx: &mut Context<AppView>,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&mut AppView, &mut Context<AppView>) + 'static,
{
    let button = if armed {
        op_button(id, armed_label).danger()
    } else {
        op_button(id, idle_label).primary()
    };
    button.on_click(
        cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
            on_click(this, cx);
        }),
    )
}

/// Shared op-pane button: GPUI Kit `Button`, small, with a minimum
/// width so short labels (Refresh, Start read) match while longer
/// ones grow. `self_start` opts it out of the parent `flex_col`
/// cross-axis stretch. Callers pick the variant and add `on_click`.
fn op_button(id: &'static str, label: &'static str) -> Button {
    Button::new(id)
        .label(label)
        .small()
        .min_w(px(84.0))
        .self_start()
}
