//! Read-only UEFI image parsing: firmware volumes → FFS files →
//! sections (with decompression) → HII/IFR forms joined against the
//! NVRAM variable store, yielding human-readable Setup settings.
//!
//! Crate discipline: this module is destined to be extracted into a
//! standalone MIT-licensed crate, so nothing in `uefi::` may import
//! from the rest of etch341 (no `crate::error`, no `crate::gui`) and
//! everything operates on plain `&[u8]`.

// Some fields/helpers are only read by the CLI/GUI surface added in a
// later phase; keep the walker warning-clean until then.
#![allow(dead_code)]

pub mod fv;
pub mod hii;
pub mod ifr;
pub mod nvram;

use std::collections::HashMap;

/// One resolved Setup setting: a human label joined to the variable
/// byte that backs it and, when found, its current value.
#[derive(Clone)]
pub struct Setting {
    pub name: String,
    pub help: String,
    /// Menu page (IFR form) this setting lives on, e.g. "CPU
    /// Configuration". Empty when the form title didn't resolve.
    pub form: String,
    /// Enclosing form set title (usually "Setup"). Empty when unresolved.
    pub formset: String,
    pub varstore: String,
    pub offset: u16,
    pub width: u8,
    pub kind: ifr::QKind,
    /// (value, label) option choices; empty for numeric/checkbox.
    pub options: Vec<(u64, String)>,
    /// Current value read from NVRAM, if the variable was found.
    pub value: Option<u64>,
    /// Option label matching `value`, or a synthesized one for checkbox.
    pub value_label: Option<String>,
    /// True when the firmware may hide/lock it (suppress/grayout scope).
    pub conditional: bool,
}

/// Parse an image and return every Setup setting we can resolve.
/// `filter`, when set, keeps only settings whose label contains it
/// (case-insensitive).
pub fn extract_settings(image: &[u8], filter: Option<&str>) -> Vec<Setting> {
    let walk = fv::walk_image(image);
    let nvram = nvram::parse(image);
    let filter = filter.map(|f| f.to_lowercase());

    // A driver (one file GUID) owns one HII package list, so its forms
    // and strings share a string-ID space. Group leaves by file GUID
    // and resolve within each group.
    let mut by_file: HashMap<[u8; 16], Vec<&fv::Leaf>> = HashMap::new();
    for leaf in &walk.leaves {
        by_file.entry(leaf.file_guid).or_default().push(leaf);
    }

    let mut settings = Vec::new();
    for leaves in by_file.values() {
        let mut forms = ifr::FormData::default();
        let mut string_pkgs = Vec::new();
        for leaf in leaves {
            let f = ifr::parse(&leaf.data);
            forms.varstores.extend(f.varstores);
            forms.questions.extend(f.questions);
            string_pkgs.extend(hii::parse(&leaf.data));
        }
        if forms.questions.is_empty() {
            continue;
        }
        let strings = hii::english_strings(&string_pkgs);

        for q in &forms.questions {
            let name = strings.get(&q.prompt_id).cloned().unwrap_or_default();
            if name.is_empty() {
                continue; // unresolved label — nothing useful to show
            }
            if let Some(f) = &filter
                && !name.to_lowercase().contains(f.as_str())
            {
                continue;
            }
            let varstore = forms
                .varstores
                .get(&q.varstore_id)
                .map(|v| v.name.clone())
                .unwrap_or_default();

            let options: Vec<(u64, String)> = q
                .options
                .iter()
                .map(|(v, sid)| (*v, strings.get(sid).cloned().unwrap_or_default()))
                .collect();

            let value = nvram
                .get(&varstore)
                .and_then(|data| nvram::read_at(data, q.var_offset as usize, q.width));

            let value_label = value.and_then(|v| label_for(q.kind, &options, v));

            settings.push(Setting {
                name,
                help: strings.get(&q.help_id).cloned().unwrap_or_default(),
                form: strings
                    .get(&q.form_title_id)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default(),
                formset: strings
                    .get(&q.formset_title_id)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default(),
                varstore,
                offset: q.var_offset,
                width: q.width,
                kind: q.kind,
                options,
                value,
                value_label,
                conditional: q.conditional,
            });
        }
    }

    // Group by menu page: form set, then form, then a stable order
    // within the form (variable + offset) so output is deterministic
    // despite the HashMap-ordered driver walk above.
    settings.sort_by(|a, b| {
        (&a.formset, &a.form, &a.varstore, a.offset).cmp(&(
            &b.formset,
            &b.form,
            &b.varstore,
            b.offset,
        ))
    });
    // A driver often appears more than once (e.g. an uncompressed copy
    // plus a compressed one), yielding identical questions. Collapse
    // exact duplicates now that they're adjacent.
    settings.dedup_by(|a, b| {
        a.name == b.name
            && a.form == b.form
            && a.varstore == b.varstore
            && a.offset == b.offset
            && a.width == b.width
    });
    settings
}

fn label_for(kind: ifr::QKind, options: &[(u64, String)], value: u64) -> Option<String> {
    if let Some((_, label)) = options.iter().find(|(v, _)| *v == value) {
        return Some(label.clone());
    }
    match kind {
        ifr::QKind::CheckBox => Some(if value != 0 { "Enabled" } else { "Disabled" }.into()),
        _ => None,
    }
}
