//! Chip-database browser window. Opened from the Detect pane's
//! "Browse chip database" button. Self-contained pop-out window (same
//! `open_window` + `observe_release` lifecycle as the activity log)
//! that lists every bundled SPI + I²C chip with a vendor dropdown and
//! a live text filter. Read-only: the catalogue is embedded at build
//! time, so the browser never touches hardware or disk. Mirrors the
//! CLI `chips` command (name/JEDEC search across both buses); the
//! vendor dropdown is the GUI's richer take on the CLI `--bus` flag.

use super::{AppView, theme};
use crate::chipdb::{Chip, ChipDb, I2cChip, I2cChipDb};
use gpui::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Subscription, WeakEntity,
    Window, div, px,
};
use gpui_component::TitleBar;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement;
use gpui_component::select::{Select, SelectState};

/// Pseudo-vendor labels that aren't real JEDEC manufacturers. `ALL`
/// is the default (everything); `I2C` is the catch-all for the 24Cxx
/// EEPROMs, which don't carry a JEDEC manufacturer byte.
const ALL: &str = "All vendors";
const I2C: &str = "I²C EEPROM (24Cxx)";

/// Map a JEDEC id's manufacturer byte (the first two hex chars) to a
/// human vendor name. The TOML is grouped by these, so this also
/// drives the dropdown's option list and the in-list section headers.
fn vendor_of(jedec_id: &str) -> &'static str {
    match jedec_id
        .get(0..2)
        .unwrap_or("")
        .to_ascii_uppercase()
        .as_str()
    {
        "EF" => "Winbond",
        "C2" => "Macronix",
        "C8" => "GigaDevice",
        "BF" => "SST",
        "1F" => "Adesto/Atmel",
        "1C" => "EON",
        "85" => "PUYA",
        "9D" => "ISSI",
        _ => "Other",
    }
}

/// Human-readable byte count using exact binary multiples, matching
/// the `detect` pane / CLI `chips` style ("16 MB", "4 KB", "256 B").
// `is_multiple_of` would read cleaner but only stabilized in Rust
// 1.87; the project advertises a 1.85 MSRV, so keep the modulo.
#[allow(clippy::manual_is_multiple_of)]
fn human_bytes(n: u64) -> String {
    const K: u64 = 1 << 10;
    const M: u64 = 1 << 20;
    if n >= M && n % M == 0 {
        format!("{} MB", n / M)
    } else if n >= K && n % K == 0 {
        format!("{} KB", n / K)
    } else {
        format!("{n} B")
    }
}

pub struct ChipDbBrowser {
    spi: Vec<Chip>,
    i2c: Vec<I2cChip>,
    vendor_select: Entity<SelectState<Vec<SharedString>>>,
    search_state: Entity<InputState>,
    /// Live text filter, compared case-insensitively against name +
    /// JEDEC. Synced from the Input via the subscription in `_subs`.
    query: String,
    scroll: ScrollHandle,
    _subs: Vec<Subscription>,
    /// Weak handle back to the main view, used only to clear
    /// `chip_db_window` when this window's close button is clicked.
    app: WeakEntity<AppView>,
}

impl ChipDbBrowser {
    pub fn new(app: WeakEntity<AppView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let spi: Vec<Chip> = ChipDb::load_embedded().iter().cloned().collect();
        let i2c: Vec<I2cChip> = I2cChipDb::load_embedded().iter().cloned().collect();

        // Dropdown options: "All", then each SPI vendor in first-seen
        // (TOML) order, then the I²C catch-all. Built off the data so a
        // newly added vendor shows up automatically.
        let mut vendors: Vec<SharedString> = vec![ALL.into()];
        for c in &spi {
            let v = vendor_of(&c.jedec_id);
            if !vendors.iter().any(|x| x.as_ref() == v) {
                vendors.push(v.into());
            }
        }
        if !i2c.is_empty() {
            vendors.push(I2C.into());
        }

        let vendor_select = cx.new(|cx| SelectState::new(vendors, None, window, cx));
        let search_state =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter by name or JEDEC id…"));

        let mut subs = Vec::new();
        // Re-render the list whenever the dropdown's selection changes.
        // `observe` (rather than subscribing to SelectEvent) re-renders
        // on any SelectState notify, which covers selection without
        // depending on the widget's event surface; render reads the
        // selected value straight off the state.
        subs.push(cx.observe(&vendor_select, |_, _, cx| cx.notify()));
        // Bridge the search Input's Change events into `query`.
        // `subscribe_in` (not plain `subscribe`) is required for widget
        // events — plain `subscribe` panics with "RefCell already
        // borrowed" the first time an Input event fires.
        subs.push(cx.subscribe_in(
            &search_state,
            window,
            |this: &mut Self, state, event: &InputEvent, _, cx| {
                if let InputEvent::Change = event {
                    this.query = state.read(cx).value().to_string();
                    cx.notify();
                }
            },
        ));

        Self {
            spi,
            i2c,
            vendor_select,
            search_state,
            query: String::new(),
            scroll: ScrollHandle::new(),
            _subs: subs,
            app,
        }
    }
}

impl Render for ChipDbBrowser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vendor = self
            .vendor_select
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_else(|| SharedString::from(ALL));
        let v = vendor.as_ref();
        let q = self.query.to_ascii_lowercase();

        let show_spi = v != I2C;
        let show_i2c = v == ALL || v == I2C;

        let spi_rows: Vec<&Chip> = if show_spi {
            self.spi
                .iter()
                .filter(|c| {
                    (v == ALL || vendor_of(&c.jedec_id) == v)
                        && (q.is_empty()
                            || c.name.to_ascii_lowercase().contains(&q)
                            || c.jedec_id.to_ascii_lowercase().contains(&q))
                })
                .collect()
        } else {
            Vec::new()
        };

        let i2c_rows: Vec<&I2cChip> = if show_i2c {
            self.i2c
                .iter()
                .filter(|c| q.is_empty() || c.name.to_ascii_lowercase().contains(&q))
                .collect()
        } else {
            Vec::new()
        };

        let count = spi_rows.len() + i2c_rows.len();

        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .px_5()
            .py_3()
            .font_family(theme::MONO_FONT)
            .text_size(px(12.0))
            .text_color(theme::text_secondary());

        if !spi_rows.is_empty() {
            body = body.child(spi_cols());
            if v == ALL {
                // Group rows under per-vendor dividers in the "All" view.
                let mut last = "";
                for c in spi_rows {
                    let cv = vendor_of(&c.jedec_id);
                    if cv != last {
                        last = cv;
                        body = body.child(group_header(cv));
                    }
                    body = body.child(spi_row(c));
                }
            } else {
                for c in spi_rows {
                    body = body.child(spi_row(c));
                }
            }
        }

        if !i2c_rows.is_empty() {
            body = body.child(group_header(I2C)).child(i2c_cols());
            for c in i2c_rows {
                body = body.child(i2c_row(c));
            }
        }

        if count == 0 {
            body = body.child(
                div()
                    .py_4()
                    .text_color(theme::text_tertiary())
                    .child("No chips match the filter."),
            );
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bench_black())
            .text_color(theme::text_primary())
            .child(
                TitleBar::new()
                    // Same Linux close-button hook as the activity-log
                    // pop-out (see `log.rs`): the title bar's X calls
                    // `remove_window()` directly, and the entity teardown is
                    // deferred on Wayland, so clear the handle synchronously
                    // here or a re-open would activate a dead window.
                    .on_close_window({
                        let app = self.app.clone();
                        move |_, window, cx| {
                            let _ = app.update(cx, |this, cx| {
                                this.chip_db_window = None;
                                cx.notify();
                            });
                            window.remove_window();
                        }
                    })
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(theme::text_secondary())
                            .child("Chip Database"),
                    ),
            )
            .child(
                // Controls: vendor dropdown · search · live count.
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .px_5()
                    .py_3()
                    .border_b_1()
                    .border_color(theme::workshop_glass_strong())
                    .child(
                        div()
                            .w(px(190.0))
                            .flex_shrink_0()
                            .child(Select::new(&self.vendor_select).placeholder(ALL)),
                    )
                    .child(div().flex_1().child(Input::new(&self.search_state)))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(12.0))
                            .text_color(theme::text_tertiary())
                            .child(format!("{count} chip{}", if count == 1 { "" } else { "s" })),
                    ),
            )
            .child(
                // Scrollable table. The scrollable element and the
                // scrollbar are SIBLINGS inside the `.relative()` box —
                // a scrollbar nested inside the scrolling child would
                // scroll with the content.
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(
                        div()
                            .id("chipdb-list")
                            .size_full()
                            .track_scroll(&self.scroll)
                            .overflow_y_scroll()
                            .child(body),
                    )
                    .vertical_scrollbar(&self.scroll),
            )
    }
}

/// A table row: fixed-width monospace cells so columns line up.
fn row() -> gpui::Div {
    div().flex().flex_row().gap_2()
}

fn cell(w: f32, text: impl Into<SharedString>) -> gpui::Div {
    div().w(px(w)).flex_shrink_0().child(text.into())
}

fn spi_cols() -> impl IntoElement {
    row()
        .text_color(theme::text_tertiary())
        .child(cell(148.0, "NAME"))
        .child(cell(78.0, "JEDEC"))
        .child(cell(80.0, "SIZE"))
        .child(cell(64.0, "VOLTAGE"))
        .child(cell(62.0, "PAGE"))
        .child(cell(72.0, "SECTOR"))
        .child(div().flex_1().child("NOTES"))
}

fn spi_row(c: &Chip) -> impl IntoElement {
    // Colour-code voltage as a traffic light keyed to how much care a
    // part needs on a stock 3.3V rig: green 3.3V (the common default,
    // safe as-is), amber 2.3V (low-voltage but 2.3–3.6V tolerant, so
    // still fine at 3.3V), red 1.8V (strict — a 3.3V rig over-volts
    // every pin).
    let voltage = c.voltage();
    let voltage_color = match voltage {
        "1.8V" => theme::caution_red(),
        "2.3V" => theme::warning_amber(),
        _ => theme::success_green(),
    };
    row()
        .child(cell(148.0, c.name.clone()).text_color(theme::text_primary()))
        .child(cell(78.0, c.jedec_id.clone()))
        .child(cell(80.0, human_bytes(c.size_kb as u64 * 1024)))
        .child(cell(64.0, voltage).text_color(voltage_color))
        .child(cell(62.0, human_bytes(c.page_size as u64)))
        .child(cell(72.0, human_bytes(c.sector_size as u64)))
        .child(
            div()
                .flex_1()
                .whitespace_normal()
                .text_color(theme::text_tertiary())
                .child(c.notes.clone()),
        )
}

fn i2c_cols() -> impl IntoElement {
    row()
        .text_color(theme::text_tertiary())
        .child(cell(148.0, "NAME"))
        .child(cell(80.0, "SIZE"))
        .child(cell(84.0, "VOLTAGE"))
        .child(cell(62.0, "PAGE"))
        .child(cell(72.0, "ADDR"))
}

fn i2c_row(c: &I2cChip) -> impl IntoElement {
    row()
        .child(cell(148.0, c.name.clone()).text_color(theme::text_primary()))
        .child(cell(80.0, human_bytes(c.size_bytes as u64)))
        // 24Cxx are a wide-range / 5V-capable family — a different kind
        // of value from the SPI single-rail traffic light. Use the
        // dedicated fixed `info_blue` (NOT the user accent, which would
        // change with the accent setting and could collide with the
        // 3.3V green).
        .child(cell(84.0, c.voltage()).text_color(theme::info_blue()))
        .child(cell(62.0, human_bytes(c.page_size as u64)))
        .child(cell(72.0, format!("{} B", c.addr_width)))
}

/// Accent-tinted divider used for per-vendor groups (in the "All"
/// view) and the I²C section break.
fn group_header(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .mt(px(10.0))
        .mb(px(2.0))
        .pb(px(2.0))
        .border_b_1()
        .border_color(theme::workshop_glass_strong())
        .text_size(px(11.0))
        .text_color(theme::accent())
        .child(text.into())
}
