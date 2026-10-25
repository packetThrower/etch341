//! Chip-database browser window. Opened from the Detect pane's
//! "Browse chip database" button. Self-contained pop-out window (same
//! `open_window` + `observe_release` lifecycle as the activity log)
//! that lists every bundled SPI + I²C chip in a GPUI Kit `DataTable`
//! (sortable, resizable columns) with a vendor dropdown and a live
//! text filter. Read-only: the catalogue is embedded at build
//! time, so the browser never touches hardware or disk. Mirrors the
//! CLI `chips` command (name/JEDEC search across both buses); the
//! vendor dropdown is the GUI's richer take on the CLI `--bus` flag.

use super::{AppView, theme};
use crate::chipdb::{Chip, ChipDb, I2cChip, I2cChipDb};
use gpui::{
    AppContext, Context, Entity, Hsla, IntoElement, ParentElement, Render, SharedString, Styled,
    Subscription, WeakEntity, Window, div, px,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::table::{Column, ColumnSort, DataTable, TableDelegate, TableState};
use gpui_kit::component::{Sizable as _, TitleBar};

/// Pseudo-vendor labels that aren't real JEDEC manufacturers. `ALL`
/// is the default (everything); `I2C` is the catch-all for the 24Cxx
/// EEPROMs, which don't carry a JEDEC manufacturer byte.
const ALL: &str = "All vendors";
const I2C: &str = "I²C EEPROM (24Cxx)";

/// Map a JEDEC id's manufacturer byte (the first two hex chars) to a
/// human vendor name. The TOML is grouped by these, so this also
/// drives the dropdown's option list and the Vendor column.
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

/// One table row, flattened from either bus so both share columns.
/// Numeric fields keep raw values for sorting; `None` renders "—".
struct Row {
    name: String,
    vendor: &'static str,
    jedec: Option<String>,
    size: u64,
    voltage: &'static str,
    voltage_color: Hsla,
    page: u64,
    sector: Option<u64>,
    addr: Option<u8>,
    notes: String,
}

impl Row {
    fn spi(c: &Chip) -> Self {
        // Colour-code voltage as a traffic light keyed to how much care a
        // part needs on a stock 3.3V rig: green 3.3V (the common default,
        // safe as-is), amber 2.3V (low-voltage but 2.3–3.6V tolerant, so
        // still fine at 3.3V), red 1.8V (strict — a 3.3V rig over-volts
        // every pin).
        let voltage = c.voltage();
        Self {
            name: c.name.clone(),
            vendor: vendor_of(&c.jedec_id),
            jedec: Some(c.jedec_id.clone()),
            size: c.size_kb as u64 * 1024,
            voltage,
            voltage_color: match voltage {
                "1.8V" => theme::caution_red(),
                "2.3V" => theme::warning_amber(),
                _ => theme::success_green(),
            },
            page: c.page_size as u64,
            sector: Some(c.sector_size as u64),
            addr: None,
            notes: c.notes.clone(),
        }
    }

    fn i2c(c: &I2cChip) -> Self {
        Self {
            name: c.name.clone(),
            vendor: I2C,
            jedec: None,
            size: c.size_bytes as u64,
            voltage: c.voltage(),
            // 24Cxx are a wide-range / 5V-capable family — a different
            // kind of value from the SPI single-rail traffic light. Use
            // the dedicated fixed `info_blue` (NOT the user accent, which
            // would change with the accent setting and could collide with
            // the 3.3V green).
            voltage_color: theme::info_blue(),
            page: c.page_size as u64,
            sector: None,
            addr: Some(c.addr_width),
            notes: String::new(),
        }
    }
}

const COLUMNS: [(&str, &str, f32); 9] = [
    ("name", "Name", 150.0),
    ("vendor", "Vendor", 150.0),
    ("jedec", "JEDEC", 80.0),
    ("size", "Size", 80.0),
    ("voltage", "Voltage", 80.0),
    ("page", "Page", 70.0),
    ("sector", "Sector", 75.0),
    ("addr", "Addr", 60.0),
    ("notes", "Notes", 360.0),
];

/// `TableDelegate` over every chip; `visible` holds the indices that
/// pass the current filter, in the current sort order.
struct ChipTable {
    rows: Vec<Row>,
    visible: Vec<usize>,
    sort: Option<(usize, ColumnSort)>,
}

impl ChipTable {
    fn filter(&mut self, vendor: &str, query: &str) {
        let q = query.to_ascii_lowercase();
        self.visible = (0..self.rows.len())
            .filter(|&i| {
                let r = &self.rows[i];
                let vendor_ok = vendor == ALL || r.vendor == vendor;
                let query_ok = q.is_empty()
                    || r.name.to_ascii_lowercase().contains(&q)
                    || r.jedec
                        .as_ref()
                        .is_some_and(|j| j.to_ascii_lowercase().contains(&q));
                vendor_ok && query_ok
            })
            .collect();
        self.apply_sort();
    }

    fn apply_sort(&mut self) {
        let Some((col, sort)) = self.sort else {
            return;
        };
        if matches!(sort, ColumnSort::Default) {
            // Back to catalogue (TOML) order.
            self.visible.sort();
            return;
        }
        let rows = &self.rows;
        self.visible.sort_by(|&a, &b| {
            let (a, b) = (&rows[a], &rows[b]);
            let ord = match col {
                0 => a.name.cmp(&b.name),
                1 => a.vendor.cmp(b.vendor),
                2 => a.jedec.cmp(&b.jedec),
                3 => a.size.cmp(&b.size),
                4 => a.voltage.cmp(b.voltage),
                5 => a.page.cmp(&b.page),
                6 => a.sector.cmp(&b.sector),
                7 => a.addr.cmp(&b.addr),
                _ => a.notes.cmp(&b.notes),
            };
            if matches!(sort, ColumnSort::Descending) {
                ord.reverse()
            } else {
                ord
            }
        });
    }
}

impl TableDelegate for ChipTable {
    fn columns_count(&self, _: &gpui::App) -> usize {
        COLUMNS.len()
    }

    fn rows_count(&self, _: &gpui::App) -> usize {
        self.visible.len()
    }

    fn column(&self, col_ix: usize, _: &gpui::App) -> Column {
        let (key, name, width) = COLUMNS[col_ix];
        Column::new(key, name).width(px(width)).sortable()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        self.sort = Some((col_ix, sort));
        self.apply_sort();
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let r = &self.rows[self.visible[row_ix]];
        let dash = || "—".to_string();
        let (text, color) = match col_ix {
            0 => (r.name.clone(), theme::text_primary()),
            1 => (r.vendor.to_string(), theme::text_secondary()),
            2 => (
                r.jedec.clone().unwrap_or_else(dash),
                theme::text_secondary(),
            ),
            3 => (human_bytes(r.size), theme::text_secondary()),
            4 => (r.voltage.to_string(), r.voltage_color),
            5 => (human_bytes(r.page), theme::text_secondary()),
            6 => (
                r.sector.map(human_bytes).unwrap_or_else(dash),
                theme::text_secondary(),
            ),
            7 => (
                r.addr.map(|a| format!("{a} B")).unwrap_or_else(dash),
                theme::text_secondary(),
            ),
            _ => (r.notes.clone(), theme::text_tertiary()),
        };
        div().text_color(color).child(text)
    }
}

pub struct ChipDbBrowser {
    table: Entity<TableState<ChipTable>>,
    vendor_select: Entity<SelectState<Vec<SharedString>>>,
    search_state: Entity<InputState>,
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

        let mut rows: Vec<Row> = spi.iter().map(Row::spi).collect();
        rows.extend(i2c.iter().map(Row::i2c));
        let visible = (0..rows.len()).collect();
        let table = cx.new(|cx| {
            TableState::new(
                ChipTable {
                    rows,
                    visible,
                    sort: None,
                },
                window,
                cx,
            )
            .col_selectable(false)
        });

        let vendor_select = cx.new(|cx| SelectState::new(vendors, None, window, cx));
        let search_state =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter by name or JEDEC id…"));

        let mut subs = Vec::new();
        // Re-filter whenever the dropdown's selection changes. `observe`
        // (rather than subscribing to SelectEvent) fires on any
        // SelectState notify, which covers selection without depending
        // on the widget's event surface.
        subs.push(cx.observe(&vendor_select, |this, _, cx| this.refilter(cx)));
        // Re-filter on every search Input change.
        // `subscribe_in` (not plain `subscribe`) is required for widget
        // events — plain `subscribe` panics with "RefCell already
        // borrowed" the first time an Input event fires.
        subs.push(cx.subscribe_in(
            &search_state,
            window,
            |this: &mut Self, _, event: &InputEvent, _, cx| {
                if let InputEvent::Change = event {
                    this.refilter(cx);
                }
            },
        ));

        Self {
            table,
            vendor_select,
            search_state,
            _subs: subs,
            app,
        }
    }

    /// Push the dropdown + search state into the table's row filter.
    fn refilter(&mut self, cx: &mut Context<Self>) {
        let vendor = self
            .vendor_select
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_else(|| SharedString::from(ALL));
        let query = self.search_state.read(cx).value().to_string();
        self.table.update(cx, |t, cx| {
            t.delegate_mut().filter(&vendor, &query);
            t.refresh(cx);
        });
        cx.notify();
    }
}

impl Render for ChipDbBrowser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.table.read(cx).delegate().visible.len();

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
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(DataTable::new(&self.table).stripe(true).small()),
            )
    }
}
