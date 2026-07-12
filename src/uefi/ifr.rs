//! IFR (Internal Form Representation) opcode parser — the compiled
//! Setup-form bytecode. Extracts variable stores and the questions
//! that back them (OneOf / CheckBox / Numeric), with option values
//! and the string-IDs for their labels. String text lives in a
//! separate HII string package (see `hii.rs`); this module only
//! resolves the *structure*.
//!
//! Opcode layouts follow the UEFI spec (vol. 2, HII/IFR); the
//! subset here is what a read-only Setup explorer needs, everything
//! else is skipped by its self-describing length. Cross-reference:
//! LongSoft/IFRExtractor-RS.

use std::collections::HashMap;

// --- opcodes we care about ---
const OP_FORM_SET: u8 = 0x0E;
const OP_FORM: u8 = 0x01;
const OP_ONE_OF: u8 = 0x05;
const OP_CHECKBOX: u8 = 0x06;
const OP_NUMERIC: u8 = 0x07;
const OP_ONE_OF_OPTION: u8 = 0x09;
const OP_VARSTORE: u8 = 0x24;
const OP_VARSTORE_EFI: u8 = 0x26;
const OP_END: u8 = 0x29;
const OP_SUPPRESS_IF: u8 = 0x0A;
const OP_GRAY_OUT_IF: u8 = 0x19;
const OP_DISABLE_IF: u8 = 0x1E;
/// Cross-form link (a "goto"); how the menu tree is nested.
const OP_REF: u8 = 0x0F;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QKind {
    OneOf,
    CheckBox,
    Numeric,
}

#[derive(Debug, Clone)]
pub struct Question {
    pub prompt_id: u16,
    pub help_id: u16,
    pub varstore_id: u16,
    pub var_offset: u16,
    pub width: u8,
    pub kind: QKind,
    /// (value, option-label string-id) — empty for CheckBox/Numeric.
    pub options: Vec<(u64, u16)>,
    /// True when nested inside a suppress_if / grayout_if / disable_if
    /// scope, i.e. the firmware may hide or lock it at runtime. We
    /// record the flag but do not evaluate the condition.
    pub conditional: bool,
    /// String-ids of the enclosing form and form set titles, for
    /// grouping the flat question list back into its menu structure.
    pub form_title_id: u16,
    pub formset_title_id: u16,
}

#[derive(Debug, Clone)]
pub struct VarStore {
    pub id: u16,
    pub guid: [u8; 16],
    pub name: String,
}

/// A Setup menu page.
#[derive(Debug, Clone)]
pub struct Form {
    pub id: u16,
    pub title_id: u16,
    pub formset_title_id: u16,
}

#[derive(Debug, Default)]
pub struct FormData {
    pub varstores: HashMap<u16, VarStore>,
    pub questions: Vec<Question>,
    /// Every form encountered, for the menu tree.
    pub forms: Vec<Form>,
    /// `(parent form id, child form id)` links from REF opcodes —
    /// the edges of the menu tree, scoped within a form set.
    pub links: Vec<(u16, u16)>,
}

fn u16le(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
}

/// Width in bytes from a question's flags size field (low 2 bits).
fn width_from_flags(flags: u8) -> u8 {
    match flags & 0x03 {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => 8,
    }
}

fn read_value(b: &[u8], off: usize, ty: u8) -> Option<u64> {
    let width = match ty {
        0 | 4 => 1, // u8 / bool
        1 => 2,     // u16
        2 => 4,     // u32
        3 => 8,     // u64
        _ => return None,
    };
    let s = b.get(off..off + width)?;
    let mut v = [0u8; 8];
    v[..width].copy_from_slice(s);
    Some(u64::from_le_bytes(v))
}

/// Parse every form set found in `buf` (an IFR/form-package payload).
/// Scans for FORM_SET anchors so it works whether or not the buffer
/// still carries its HII package header.
pub fn parse(buf: &[u8]) -> FormData {
    let mut data = FormData::default();
    let mut i = 0;
    while i + 2 <= buf.len() {
        if buf[i] == OP_FORM_SET && plausible_formset(&buf[i..]) {
            let consumed = parse_stream(&buf[i..], &mut data);
            if consumed >= 2 {
                i += consumed;
                continue;
            }
        }
        i += 1;
    }
    data
}

/// A FORM_SET is a real form set (not a coincidental 0x0E byte) if its
/// length is sane and a 16-byte GUID fits after the header.
fn plausible_formset(b: &[u8]) -> bool {
    let len = (b[1] & 0x7F) as usize;
    len >= 2 && b.len() >= 2 + 16 && len <= b.len()
}

/// Parse one form set's opcode stream starting at a FORM_SET opcode.
/// Returns the number of bytes consumed (through the form set's
/// matching END, or end of buffer).
fn parse_stream(buf: &[u8], data: &mut FormData) -> usize {
    let mut off = 0;
    // Scope stack holds the opcode that opened each scope, so we can
    // tell how deep we are inside conditionals.
    let mut scope: Vec<u8> = Vec::new();
    let mut cond_depth = 0usize;
    // The most recently opened question, so ONE_OF_OPTIONs attach to it.
    let mut cur_q: Option<usize> = None;
    // Enclosing form-set title, and the current form title (cleared
    // when the form's scope ends), so each question can be grouped
    // back under its menu page.
    let mut formset_title = 0u16;
    let mut form_title = 0u16;
    let mut cur_form_id = 0u16;

    while off + 2 <= buf.len() {
        let opcode = buf[off];
        let len = (buf[off + 1] & 0x7F) as usize;
        let has_scope = buf[off + 1] & 0x80 != 0;
        if len < 2 || off + len > buf.len() {
            break;
        }
        let body = &buf[off..off + len];

        match opcode {
            // FORM_SET title sits after the 16-byte GUID.
            OP_FORM_SET => formset_title = u16le(body, 18).unwrap_or(0),
            // FORM: FormId(u16) then Title(u16 string-id).
            OP_FORM => {
                cur_form_id = u16le(body, 2).unwrap_or(0);
                form_title = u16le(body, 4).unwrap_or(0);
                data.forms.push(Form {
                    id: cur_form_id,
                    title_id: form_title,
                    formset_title_id: formset_title,
                });
            }
            // REF: a goto to another form. The target FormId sits at
            // body offset 13 — after the 10-byte question header plus a
            // one-byte field this build emits (empirically verified:
            // offset 13 resolves 97% of targets to real form ids, vs
            // ~2% at the spec's nominal offset 12). Trailing REF2-5
            // fields come after, so the offset is fixed across variants.
            OP_REF => {
                if len >= 15
                    && let Some(target) = u16le(body, 13)
                    && target != 0
                    && cur_form_id != 0
                {
                    data.links.push((cur_form_id, target));
                }
            }
            OP_VARSTORE => {
                if let Some(vs) = parse_varstore(body) {
                    data.varstores.insert(vs.id, vs);
                }
            }
            OP_VARSTORE_EFI => {
                if let Some(vs) = parse_varstore_efi(body) {
                    data.varstores.insert(vs.id, vs);
                }
            }
            OP_ONE_OF | OP_CHECKBOX | OP_NUMERIC => {
                if let Some(mut q) = parse_question(body, opcode, cond_depth > 0) {
                    q.form_title_id = form_title;
                    q.formset_title_id = formset_title;
                    data.questions.push(q);
                    cur_q = Some(data.questions.len() - 1);
                }
            }
            OP_ONE_OF_OPTION => {
                if let (Some(idx), Some(opt)) = (cur_q, parse_option(body)) {
                    data.questions[idx].options.push(opt);
                }
            }
            _ => {}
        }

        if has_scope {
            scope.push(opcode);
            if matches!(opcode, OP_SUPPRESS_IF | OP_GRAY_OUT_IF | OP_DISABLE_IF) {
                cond_depth += 1;
            }
        }
        if opcode == OP_END {
            if let Some(open) = scope.pop() {
                if matches!(open, OP_SUPPRESS_IF | OP_GRAY_OUT_IF | OP_DISABLE_IF) {
                    cond_depth = cond_depth.saturating_sub(1);
                }
                // Leaving a question's own scope clears the option target.
                if matches!(open, OP_ONE_OF | OP_CHECKBOX | OP_NUMERIC) {
                    cur_q = None;
                }
                // Leaving a form clears its title/id for the next form.
                if open == OP_FORM {
                    form_title = 0;
                    cur_form_id = 0;
                }
            }
            // Form set closed and we're back at the top: done.
            if scope.is_empty() {
                return off + len;
            }
        }
        off += len;
    }
    off
}

/// EFI_IFR_QUESTION_HEADER sits right after the 2-byte op header:
/// prompt(u16) help(u16) qid(u16) varstoreid(u16) varoffset(u16),
/// then a Flags byte. Returns None if truncated.
fn parse_question(body: &[u8], opcode: u8, conditional: bool) -> Option<Question> {
    let prompt_id = u16le(body, 2)?;
    let help_id = u16le(body, 4)?;
    let varstore_id = u16le(body, 8)?;
    let var_offset = u16le(body, 10)?;
    let (kind, width) = match opcode {
        OP_CHECKBOX => (QKind::CheckBox, 1),
        OP_ONE_OF => (QKind::OneOf, width_from_flags(*body.get(12)?)),
        OP_NUMERIC => (QKind::Numeric, width_from_flags(*body.get(12)?)),
        _ => return None,
    };
    Some(Question {
        prompt_id,
        help_id,
        varstore_id,
        var_offset,
        width,
        kind,
        options: Vec::new(),
        conditional,
        form_title_id: 0,
        formset_title_id: 0,
    })
}

/// EFI_IFR_ONE_OF_OPTION: option(u16) flags(u8) type(u8) value(...).
fn parse_option(body: &[u8]) -> Option<(u64, u16)> {
    let label_id = u16le(body, 2)?;
    let ty = *body.get(5)?;
    let value = read_value(body, 6, ty)?;
    Some((value, label_id))
}

/// EFI_IFR_VARSTORE: guid(16) id(u16) size(u16) name(ascii,nul).
fn parse_varstore(body: &[u8]) -> Option<VarStore> {
    let guid: [u8; 16] = body.get(2..18)?.try_into().ok()?;
    let id = u16le(body, 18)?;
    let name = ascii_z(body.get(22..)?);
    Some(VarStore { id, guid, name })
}

/// EFI_IFR_VARSTORE_EFI: id(u16) guid(16) attrs(u32) size(u16)
/// name(ascii,nul). Older builds emit a shorter form with no
/// attrs/size/name — those carry no usable name, so skip them.
fn parse_varstore_efi(body: &[u8]) -> Option<VarStore> {
    let id = u16le(body, 2)?;
    let guid: [u8; 16] = body.get(4..20)?.try_into().ok()?;
    // name only present in the long form (>= 26 header bytes).
    let name = body.get(26..).map(ascii_z).unwrap_or_default();
    Some(VarStore { id, guid, name })
}

/// Read a NUL-terminated ASCII string.
fn ascii_z(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op_header(opcode: u8, len: u8, scope: bool) -> [u8; 2] {
        [opcode, len | if scope { 0x80 } else { 0 }]
    }

    #[test]
    fn parses_oneof_with_options_and_varstore() {
        let mut s = Vec::new();
        // FORM_SET, scope, len = 2 + 16 guid + 2 title + 2 help + 1 flags = 23
        s.extend_from_slice(&op_header(OP_FORM_SET, 23, true));
        s.extend_from_slice(&[0x11; 16]);
        s.extend_from_slice(&[0, 0, 0, 0, 0]);

        // VARSTORE id=1 name="Setup"
        let name = b"Setup\0";
        let vlen = (2 + 16 + 2 + 2 + name.len()) as u8;
        s.extend_from_slice(&op_header(OP_VARSTORE, vlen, false));
        s.extend_from_slice(&[0x22; 16]);
        s.extend_from_slice(&1u16.to_le_bytes()); // id
        s.extend_from_slice(&64u16.to_le_bytes()); // size
        s.extend_from_slice(name);

        // ONE_OF at varstore 1 offset 0x10, width u8, scope open.
        // len = 2 op header + 10 question header + 1 flags = 13.
        s.extend_from_slice(&op_header(OP_ONE_OF, 13, true));
        s.extend_from_slice(&100u16.to_le_bytes()); // prompt
        s.extend_from_slice(&101u16.to_le_bytes()); // help
        s.extend_from_slice(&5u16.to_le_bytes()); // qid
        s.extend_from_slice(&1u16.to_le_bytes()); // varstore id
        s.extend_from_slice(&0x10u16.to_le_bytes()); // offset
        s.push(0); // flags: size = u8

        // two options; len = 2 op header + 2 label + 1 flags + 1 type + 1 value = 7.
        for (val, label) in [(0u8, 200u16), (1u8, 201u16)] {
            s.extend_from_slice(&op_header(OP_ONE_OF_OPTION, 7, false));
            s.extend_from_slice(&label.to_le_bytes());
            s.push(0); // option flags
            s.push(0); // type = u8
            s.push(val);
        }
        s.extend_from_slice(&op_header(OP_END, 2, false)); // close one_of
        s.extend_from_slice(&op_header(OP_END, 2, false)); // close form set

        let data = parse(&s);
        assert_eq!(data.varstores[&1].name, "Setup");
        assert_eq!(data.questions.len(), 1);
        let q = &data.questions[0];
        assert_eq!(q.kind, QKind::OneOf);
        assert_eq!(q.prompt_id, 100);
        assert_eq!(q.var_offset, 0x10);
        assert_eq!(q.width, 1);
        assert!(!q.conditional);
        assert_eq!(q.options, vec![(0, 200), (1, 201)]);
    }

    #[test]
    fn marks_suppressed_question_conditional() {
        let mut s = Vec::new();
        s.extend_from_slice(&op_header(OP_FORM_SET, 23, true));
        s.extend_from_slice(&[0x11; 16]);
        s.extend_from_slice(&[0, 0, 0, 0, 0]);
        // suppress_if scope
        s.extend_from_slice(&op_header(OP_SUPPRESS_IF, 2, true));
        // checkbox inside
        s.extend_from_slice(&op_header(OP_CHECKBOX, 13, false));
        s.extend_from_slice(&10u16.to_le_bytes());
        s.extend_from_slice(&11u16.to_le_bytes());
        s.extend_from_slice(&1u16.to_le_bytes());
        s.extend_from_slice(&2u16.to_le_bytes());
        s.extend_from_slice(&0x20u16.to_le_bytes());
        s.push(0);
        s.extend_from_slice(&op_header(OP_END, 2, false)); // close suppress
        s.extend_from_slice(&op_header(OP_END, 2, false)); // close form set

        let data = parse(&s);
        assert_eq!(data.questions.len(), 1);
        assert!(data.questions[0].conditional);
        assert_eq!(data.questions[0].kind, QKind::CheckBox);
    }

    fn checkbox(prompt: u16, offset: u16) -> Vec<u8> {
        let mut c = op_header(OP_CHECKBOX, 13, false).to_vec();
        c.extend_from_slice(&prompt.to_le_bytes());
        c.extend_from_slice(&(prompt + 1).to_le_bytes()); // help
        c.extend_from_slice(&1u16.to_le_bytes()); // qid
        c.extend_from_slice(&2u16.to_le_bytes()); // varstore
        c.extend_from_slice(&offset.to_le_bytes());
        c.push(0); // flags
        c
    }

    #[test]
    fn captures_form_and_formset_titles() {
        let mut s = Vec::new();
        // FORM_SET, title id 50
        s.extend_from_slice(&op_header(OP_FORM_SET, 23, true));
        s.extend_from_slice(&[0x11; 16]);
        s.extend_from_slice(&50u16.to_le_bytes()); // formset title
        s.extend_from_slice(&51u16.to_le_bytes()); // help
        s.push(0); // flags
        // FORM id 1, title id 60, with a checkbox inside
        s.extend_from_slice(&op_header(OP_FORM, 6, true));
        s.extend_from_slice(&1u16.to_le_bytes()); // form id
        s.extend_from_slice(&60u16.to_le_bytes()); // form title
        s.extend_from_slice(&checkbox(10, 0));
        s.extend_from_slice(&op_header(OP_END, 2, false)); // close form
        // A checkbox outside any form — form title must have cleared.
        s.extend_from_slice(&checkbox(20, 4));
        s.extend_from_slice(&op_header(OP_END, 2, false)); // close form set

        let data = parse(&s);
        assert_eq!(data.questions.len(), 2);
        assert_eq!(data.questions[0].form_title_id, 60);
        assert_eq!(data.questions[0].formset_title_id, 50);
        // Outside the form: form cleared to 0, form set still in scope.
        assert_eq!(data.questions[1].form_title_id, 0);
        assert_eq!(data.questions[1].formset_title_id, 50);
    }

    #[test]
    fn captures_ref_links_between_forms() {
        // REF target FormId sits at body offset 13 (after the 10-byte
        // question header + a one-byte field), not the nominal 12.
        let mut refbody = op_header(OP_REF, 15, false).to_vec();
        refbody.extend_from_slice(&0u16.to_le_bytes()); // prompt
        refbody.extend_from_slice(&0u16.to_le_bytes()); // help
        refbody.extend_from_slice(&0u16.to_le_bytes()); // qid
        refbody.extend_from_slice(&0u16.to_le_bytes()); // varstore id
        refbody.extend_from_slice(&0u16.to_le_bytes()); // varstore info
        refbody.push(0); // the extra byte at offset 12
        refbody.extend_from_slice(&2u16.to_le_bytes()); // target form id @ 13

        let mut s = Vec::new();
        s.extend_from_slice(&op_header(OP_FORM_SET, 23, true));
        s.extend_from_slice(&[0x11; 16]);
        s.extend_from_slice(&[0, 0, 0, 0, 0]);
        // FORM id 1 -> REF to form 2
        s.extend_from_slice(&op_header(OP_FORM, 6, true));
        s.extend_from_slice(&1u16.to_le_bytes());
        s.extend_from_slice(&70u16.to_le_bytes()); // form title
        s.extend_from_slice(&refbody);
        s.extend_from_slice(&op_header(OP_END, 2, false)); // close form 1
        // FORM id 2 (the target)
        s.extend_from_slice(&op_header(OP_FORM, 6, true));
        s.extend_from_slice(&2u16.to_le_bytes());
        s.extend_from_slice(&71u16.to_le_bytes());
        s.extend_from_slice(&op_header(OP_END, 2, false)); // close form 2
        s.extend_from_slice(&op_header(OP_END, 2, false)); // close form set

        let data = parse(&s);
        assert_eq!(data.forms.len(), 2);
        assert_eq!(data.links, vec![(1, 2)]);
    }
}
