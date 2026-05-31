//! Shared byte-diff core for the verify-diff view (GUI) and the `diff`
//! / `verify --diff` CLI commands. Pure logic plus a terminal renderer
//! — no hardware, no GUI types — so both front-ends build on one source
//! of truth for how mismatches are grouped into regions.

use std::fmt::Write;

/// Bytes shown per diff row.
pub const DIFF_ROW_BYTES: usize = 16;
/// Context lines kept on each side of a differing run.
pub const DIFF_CONTEXT_ROWS: usize = 2;

/// One row of a side-by-side diff: a region header or a 16-byte data
/// row (rendered as left-hex / right-hex with differing bytes lit in
/// the standard diff colours).
#[derive(Clone, Copy)]
pub enum DiffRow {
    Header {
        first_diff: usize,
        diff_count: usize,
    },
    Bytes {
        offset: usize,
    },
}

/// Byte indices where `a` and `b` differ. Compares the full length of
/// the longer side: a byte present on one side but past the end of the
/// other counts as a difference, so a length mismatch shows up rather
/// than being silently ignored. For equal-length inputs (the GUI's
/// file-vs-chip case) this is a plain pairwise compare.
pub fn diff_offsets(a: &[u8], b: &[u8]) -> Vec<usize> {
    (0..a.len().max(b.len()))
        .filter(|&i| a.get(i) != b.get(i))
        .collect()
}

/// Group `offsets` (sorted differing byte indices) into row-aligned
/// display regions — each a run of nearby diffs padded by
/// `DIFF_CONTEXT_ROWS` lines, merged when the padding overlaps — and
/// flatten to header + data rows. Returns the rows plus the header-row
/// index of each region (for Prev/Next nav). Only the differing
/// neighbourhoods are emitted, so matching stretches are skipped.
pub fn diff_regions(offsets: &[usize], total_len: usize) -> (Vec<DiffRow>, Vec<usize>) {
    let ctx = DIFF_CONTEXT_ROWS * DIFF_ROW_BYTES;
    // (start_aligned, end_clamped, first_diff, diff_count)
    let mut regions: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < offsets.len() {
        let first = offsets[i];
        let mut last = offsets[i];
        let mut count = 1usize;
        let mut j = i + 1;
        while j < offsets.len() && offsets[j].saturating_sub(last) <= 2 * ctx {
            last = offsets[j];
            count += 1;
            j += 1;
        }
        let start = first.saturating_sub(ctx) / DIFF_ROW_BYTES * DIFF_ROW_BYTES;
        let end = (last + ctx + 1).min(total_len);
        match regions.last_mut() {
            // Row-alignment can make adjacent regions touch — fold them.
            Some(r) if start <= r.1 => {
                r.1 = r.1.max(end);
                r.3 += count;
            }
            _ => regions.push((start, end, first, count)),
        }
        i = j;
    }
    let mut rows = Vec::new();
    let mut region_rows = Vec::new();
    for &(start, end, first_diff, count) in &regions {
        region_rows.push(rows.len());
        rows.push(DiffRow::Header {
            first_diff,
            diff_count: count,
        });
        let mut off = start;
        while off < end {
            rows.push(DiffRow::Bytes { offset: off });
            off += DIFF_ROW_BYTES;
        }
    }
    (rows, region_rows)
}

/// Wrap `body` in an ANSI SGR colour when `color`, else append plain.
fn paint(out: &mut String, code: &str, color: bool, body: &str) {
    if color {
        let _ = write!(out, "\x1b[{code}m{body}\x1b[0m");
    } else {
        out.push_str(body);
    }
}

/// One side's 16-byte hex group starting at `offset`. Bytes that differ
/// from `other` at the same index are painted in `code`; bytes past the
/// end of `buf` (this side is shorter) render as blank padding so the
/// columns stay aligned.
fn hex_group(out: &mut String, buf: &[u8], other: &[u8], offset: usize, color: bool, code: &str) {
    for i in 0..DIFF_ROW_BYTES {
        if i > 0 {
            out.push(' ');
        }
        let idx = offset + i;
        match buf.get(idx) {
            Some(&byte) => {
                let cell = format!("{byte:02X}");
                if buf.get(idx) != other.get(idx) {
                    paint(out, code, color, &cell);
                } else {
                    out.push_str(&cell);
                }
            }
            None => out.push_str("  "),
        }
    }
}

/// Render the side-by-side diff body for `rows` (from [`diff_regions`]):
/// a one-line legend, then per region a header line and 16-byte data
/// lines with `a` on the left (red / removed) and `b` on the right
/// (green / added). Displayed addresses are offset by `base`. `labels`
/// names the two sides in the legend. `color` toggles ANSI SGR codes —
/// the caller decides based on TTY / `NO_COLOR`.
pub fn render_regions(
    a: &[u8],
    b: &[u8],
    rows: &[DiffRow],
    base: u32,
    color: bool,
    labels: (&str, &str),
) -> String {
    let mut out = String::new();
    paint(&mut out, "31", color, &format!("- {}", labels.0));
    out.push_str("    ");
    paint(&mut out, "32", color, &format!("+ {}", labels.1));
    out.push('\n');
    for row in rows {
        match *row {
            DiffRow::Header {
                first_diff,
                diff_count,
            } => {
                let _ = writeln!(
                    out,
                    "@@ 0x{:08X}  {diff_count} byte(s) differ @@",
                    base as usize + first_diff
                );
            }
            DiffRow::Bytes { offset } => {
                let _ = write!(out, "0x{:08X}  ", base as usize + offset);
                hex_group(&mut out, a, b, offset, color, "31");
                out.push_str("  \u{2502}  ");
                hex_group(&mut out, b, a, offset, color, "32");
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_pairwise() {
        assert_eq!(diff_offsets(&[1, 2, 3], &[1, 9, 3]), vec![1]);
        assert!(diff_offsets(&[1, 2, 3], &[1, 2, 3]).is_empty());
    }

    #[test]
    fn offsets_count_trailing_length_mismatch() {
        // Bytes past the shorter side's end are differences either way.
        assert_eq!(diff_offsets(&[1, 2, 3, 4], &[1, 2]), vec![2, 3]);
        assert_eq!(diff_offsets(&[1, 2], &[1, 2, 3, 4]), vec![2, 3]);
    }

    #[test]
    fn regions_merge_nearby_and_align_to_row() {
        // Two diffs 2 bytes apart → one region; start aligned to 0.
        let (rows, region_rows) = diff_regions(&[0x10, 0x12], 0x100);
        assert_eq!(region_rows.len(), 1);
        assert!(matches!(rows[0], DiffRow::Header { diff_count: 2, .. }));
    }

    #[test]
    fn regions_split_when_far_apart() {
        let (_, region_rows) = diff_regions(&[0x00, 0x800], 0x1000);
        assert_eq!(region_rows.len(), 2);
    }

    #[test]
    fn render_plain_has_no_escapes() {
        let a = vec![0u8; 32];
        let mut b = a.clone();
        b[3] = 0xFF;
        let offsets = diff_offsets(&a, &b);
        let (rows, _) = diff_regions(&offsets, 32);
        let text = render_regions(&a, &b, &rows, 0, false, ("file", "chip"));
        assert!(!text.contains('\x1b'));
        assert!(text.contains("byte(s) differ"));
        assert!(text.contains("file") && text.contains("chip"));
    }

    #[test]
    fn render_color_marks_only_the_differing_byte() {
        let a = vec![0u8; 16];
        let mut b = a.clone();
        b[1] = 0xAB;
        let offsets = diff_offsets(&a, &b);
        let (rows, _) = diff_regions(&offsets, 16);
        let text = render_regions(&a, &b, &rows, 0, true, ("a", "b"));
        // The chip/right byte is green (32); the matching 00s are plain.
        assert!(text.contains("\x1b[32mAB\x1b[0m"));
        assert!(!text.contains("\x1b[32m00\x1b[0m"));
    }

    #[test]
    fn render_base_offsets_addresses() {
        let a = vec![0u8; 16];
        let mut b = a.clone();
        b[0] = 1;
        let offsets = diff_offsets(&a, &b);
        let (rows, _) = diff_regions(&offsets, 16);
        let text = render_regions(&a, &b, &rows, 0x1000, false, ("a", "b"));
        assert!(text.contains("0x00001000"));
    }
}
