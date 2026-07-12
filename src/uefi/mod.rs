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

use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// One resolved Setup setting: a human label joined to the variable
/// byte that backs it and, when found, its current value.
#[derive(Clone, Serialize)]
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
    /// The form's declared standard/factory default, if any.
    pub default_value: Option<u64>,
    /// Option label matching `default_value`.
    pub default_label: Option<String>,
    /// `Some(true)` when the current value differs from the default,
    /// `Some(false)` when it matches, `None` when either is unknown.
    pub changed: Option<bool>,
    /// True when the firmware may hide/lock it (suppress/grayout scope).
    pub conditional: bool,
}

/// A node in the Setup menu tree (form → sub-forms via IFR REF links).
pub struct FormNode {
    pub title: String,
    /// Settings that live directly on this form (not its children).
    pub setting_count: usize,
    pub children: Vec<FormNode>,
}

/// One boot menu entry, decoded from a `Boot####` load-option variable
/// in the order given by `BootOrder`.
#[derive(Clone, Serialize)]
pub struct BootEntry {
    /// The variable slot, e.g. "Boot0001".
    pub slot: String,
    /// Human description ("UEFI: Kingston…", "Windows Boot Manager").
    pub description: String,
    /// LOAD_OPTION_ACTIVE — whether the entry is enabled in the menu.
    pub active: bool,
}

/// Firmware identity recovered from the flash image. All fields are
/// best-effort — SMBIOS tables are assembled at boot, not stored, so
/// this scrapes the vendor's own identity blocks and strings instead.
#[derive(Clone, Default, Serialize)]
pub struct BiosId {
    /// AMI `$FID` project code (e.g. "NK21B010").
    pub fid: Option<String>,
    /// Firmware family: "AMI Aptio", "Insyde", "Phoenix".
    pub vendor: Option<String>,
    /// Intel platform codename, when a build tag reveals it.
    pub platform: Option<String>,
}

impl BiosId {
    /// True when nothing was recovered.
    pub fn is_empty(&self) -> bool {
        self.fid.is_none() && self.vendor.is_none() && self.platform.is_none()
    }
}

/// Everything resolved from an image: the flat settings, the menu tree
/// that organises them, the decoded boot order, and firmware identity.
pub struct Model {
    pub settings: Vec<Setting>,
    pub tree: Vec<FormNode>,
    pub boot: Vec<BootEntry>,
    pub bios_id: BiosId,
}

/// Recover firmware identity by scanning for the vendor's own markers
/// (AMI `$FID`, family strings, platform build tags).
pub fn bios_id(image: &[u8]) -> BiosId {
    BiosId {
        fid: ami_fid(image),
        vendor: detect_vendor(image),
        platform: detect_platform(image),
    }
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The AMI `$FID` block is `"$FID"` + a few header bytes + a
/// NUL-terminated ASCII project code. Return the first printable run of
/// 4+ chars after the signature.
fn ami_fid(image: &[u8]) -> Option<String> {
    let pos = find_bytes(image, b"$FID")?;
    let region = image.get(pos + 4..(pos + 4 + 40).min(image.len()))?;
    let mut i = 0;
    while i < region.len() {
        if region[i].is_ascii_graphic() {
            let start = i;
            while i < region.len() && region[i].is_ascii_graphic() {
                i += 1;
            }
            if i - start >= 4 {
                return Some(String::from_utf8_lossy(&region[start..i]).into_owned());
            }
        } else {
            i += 1;
        }
    }
    None
}

fn detect_vendor(image: &[u8]) -> Option<String> {
    // "AMITSE" (Aptio's Text Setup Engine) is the most reliable AMI
    // Aptio marker; "Aptio"/"American Megatrends" as literal ASCII are
    // often absent, and "ALASKA" (AMI's SMBIOS placeholder) is a
    // weaker fallback.
    for (needle, label) in [
        (b"AMITSE".as_slice(), "AMI Aptio"),
        (b"Aptio".as_slice(), "AMI Aptio"),
        (b"American Megatrends".as_slice(), "AMI"),
        (b"InsydeH2O".as_slice(), "Insyde"),
        (b"Phoenix".as_slice(), "Phoenix"),
        (b"ALASKA".as_slice(), "AMI"),
    ] {
        if find_bytes(image, needle).is_some() {
            return Some(label.to_string());
        }
    }
    None
}

fn detect_platform(image: &[u8]) -> Option<String> {
    // Build tags AMI/Intel embed for the reference-code platform. Both
    // common casings are listed since drivers use either.
    const TAGS: &[(&[u8], &str)] = &[
        (b"KabyLake", "Kaby Lake"),
        (b"KABYLAKE", "Kaby Lake"),
        (b"SkyLake", "Sky Lake"),
        (b"SKYLAKE", "Sky Lake"),
        (b"CoffeeLake", "Coffee Lake"),
        (b"CometLake", "Comet Lake"),
        (b"WhiskeyLake", "Whiskey Lake"),
        (b"TigerLake", "Tiger Lake"),
        (b"AlderLake", "Alder Lake"),
        (b"ApolloLake", "Apollo Lake"),
        (b"GeminiLake", "Gemini Lake"),
        (b"Broadwell", "Broadwell"),
        (b"Haswell", "Haswell"),
        (b"IvyBridge", "Ivy Bridge"),
        (b"SandyBridge", "Sandy Bridge"),
    ];
    TAGS.iter()
        .find(|(needle, _)| find_bytes(image, needle).is_some())
        .map(|(_, label)| label.to_string())
}

/// Decode the UEFI boot menu: `BootOrder` (an array of u16 option
/// numbers) resolved against each `Boot####` load-option variable.
/// Empty when the image has no boot variables.
pub fn boot_order(image: &[u8]) -> Vec<BootEntry> {
    boot_from_nvram(&nvram::parse(image))
}

fn boot_from_nvram(nvram: &HashMap<String, Vec<u8>>) -> Vec<BootEntry> {
    let Some(order) = nvram.get("BootOrder") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for chunk in order.chunks_exact(2) {
        let num = u16::from_le_bytes([chunk[0], chunk[1]]);
        let slot = format!("Boot{num:04X}");
        if let Some(data) = nvram.get(&slot)
            && let Some(entry) = parse_load_option(&slot, data)
        {
            out.push(entry);
        }
    }
    out
}

/// EFI_LOAD_OPTION: Attributes(u32) FilePathListLength(u16)
/// Description(UCS-2, NUL-terminated) FilePathList[] OptionalData[].
/// We surface the description + active flag; decoding the device path
/// into a readable form is a later refinement.
fn parse_load_option(slot: &str, data: &[u8]) -> Option<BootEntry> {
    if data.len() < 6 {
        return None;
    }
    let attrs = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let active = attrs & 0x0000_0001 != 0;
    let description = ucs2_z(&data[6..]);
    Some(BootEntry {
        slot: slot.to_string(),
        description,
        active,
    })
}

/// Decode a NUL-terminated little-endian UCS-2 string.
fn ucs2_z(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Parse an image and return every Setup setting we can resolve.
/// `filter`, when set, keeps only settings whose label contains it
/// (case-insensitive).
pub fn extract_settings(image: &[u8], filter: Option<&str>) -> Vec<Setting> {
    let settings = extract_model(image).settings;
    match filter {
        None => settings,
        Some(f) => {
            let f = f.to_lowercase();
            settings
                .into_iter()
                .filter(|s| s.name.to_lowercase().contains(&f))
                .collect()
        }
    }
}

/// Parse an image into resolved settings and the menu tree.
pub fn extract_model(image: &[u8]) -> Model {
    let walk = fv::walk_image(image);
    let nvram = nvram::parse(image);

    // A driver (one file GUID) owns one HII package list, so its forms
    // and strings share a string-ID space. Group leaves by file GUID
    // and resolve within each group.
    let mut by_file: HashMap<[u8; 16], Vec<&fv::Leaf>> = HashMap::new();
    for leaf in &walk.leaves {
        by_file.entry(leaf.file_guid).or_default().push(leaf);
    }

    let mut settings = Vec::new();
    // Menu tree spans drivers: the top menu REFs to forms defined in
    // other form sets, so form-id→title must be resolved globally, not
    // per group. Drivers use distinct form-id ranges in practice, so a
    // global map links up cleanly; on the rare collision, last wins.
    let mut id_title: HashMap<u16, String> = HashMap::new();
    let mut all_links: Vec<(u16, u16)> = Vec::new();
    let mut all_forms: HashSet<String> = HashSet::new();

    for leaves in by_file.values() {
        let mut forms = ifr::FormData::default();
        let mut string_pkgs = Vec::new();
        for leaf in leaves {
            let f = ifr::parse(&leaf.data);
            forms.varstores.extend(f.varstores);
            forms.questions.extend(f.questions);
            forms.forms.extend(f.forms);
            forms.links.extend(f.links);
            string_pkgs.extend(hii::parse(&leaf.data));
        }
        if forms.questions.is_empty() && forms.forms.is_empty() {
            continue;
        }
        let strings = hii::english_strings(&string_pkgs);

        for f in &forms.forms {
            let title = strings
                .get(&f.title_id)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if !title.is_empty() {
                id_title.insert(f.id, title.clone());
                all_forms.insert(title);
            }
        }
        all_links.extend(forms.links.iter().copied());

        for q in &forms.questions {
            let name = strings.get(&q.prompt_id).cloned().unwrap_or_default();
            if name.is_empty() {
                continue; // unresolved label — nothing useful to show
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
            let default_label = q.default_value.and_then(|d| label_for(q.kind, &options, d));
            let changed = match (value, q.default_value) {
                (Some(v), Some(d)) => Some(v != d),
                _ => None,
            };

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
                default_value: q.default_value,
                default_label,
                changed,
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

    // Translate the global REF links (id→id) into title→title menu
    // edges now that every driver's forms are in `id_title`.
    let mut child_of: HashMap<String, Vec<String>> = HashMap::new();
    let mut is_child: HashSet<String> = HashSet::new();
    for (pid, cid) in &all_links {
        if let (Some(p), Some(c)) = (id_title.get(pid), id_title.get(cid))
            && p != c
        {
            let kids = child_of.entry(p.clone()).or_default();
            if !kids.contains(c) {
                kids.push(c.clone());
            }
            is_child.insert(c.clone());
        }
    }

    // Settings-per-form, for the navigator's counts.
    let mut counts: HashMap<String, usize> = HashMap::new();
    for s in &settings {
        if !s.form.is_empty() {
            *counts.entry(s.form.clone()).or_default() += 1;
        }
    }

    // Roots are forms nothing links to; build down from them. A second
    // pass adopts any form unreachable from a root (orphan / link cycle)
    // so every form still shows up somewhere.
    let mut visited: HashSet<String> = HashSet::new();
    let mut roots: Vec<&String> = all_forms
        .iter()
        .filter(|t| !is_child.contains(*t))
        .collect();
    roots.sort();
    let mut tree: Vec<FormNode> = roots
        .iter()
        .filter_map(|r| build_node(r, &child_of, &counts, &mut visited))
        .collect();
    let mut orphans: Vec<&String> = all_forms.iter().filter(|t| !visited.contains(*t)).collect();
    orphans.sort();
    for o in orphans {
        if let Some(n) = build_node(o, &child_of, &counts, &mut visited) {
            tree.push(n);
        }
    }

    let boot = boot_from_nvram(&nvram);
    Model {
        settings,
        tree,
        boot,
        bios_id: bios_id(image),
    }
}

/// Build a menu node and its descendants, guarding against cycles and
/// a form appearing under two parents (first wins).
fn build_node(
    title: &str,
    child_of: &HashMap<String, Vec<String>>,
    counts: &HashMap<String, usize>,
    visited: &mut HashSet<String>,
) -> Option<FormNode> {
    if !visited.insert(title.to_string()) {
        return None;
    }
    let mut children = Vec::new();
    if let Some(kids) = child_of.get(title) {
        let mut kids = kids.clone();
        kids.sort();
        for c in &kids {
            if let Some(n) = build_node(c, child_of, counts, visited) {
                children.push(n);
            }
        }
    }
    Some(FormNode {
        title: title.to_string(),
        setting_count: counts.get(title).copied().unwrap_or(0),
        children,
    })
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

/// Human display of a setting's current value.
pub fn display_value(s: &Setting) -> String {
    match (&s.value_label, s.value) {
        (Some(l), Some(v)) => format!("{l} (0x{v:x})"),
        (None, Some(v)) => format!("0x{v:x}"),
        _ => "<not set>".to_string(),
    }
}

/// One difference between two dumps' settings.
pub struct SettingDiff {
    pub form: String,
    pub name: String,
    pub varstore: String,
    pub offset: u16,
    /// Displayed value in each image; `None` when absent from that side.
    pub a: Option<String>,
    pub b: Option<String>,
}

/// Compare two settings lists, returning every setting whose displayed
/// value differs (or that exists in only one). Keyed by
/// `(form, name, varstore, offset)` so a setting is tracked across the
/// two images even as list order shifts.
pub fn diff_settings(a: &[Setting], b: &[Setting]) -> Vec<SettingDiff> {
    type Key = (String, String, String, u16);
    let key = |s: &Setting| (s.form.clone(), s.name.clone(), s.varstore.clone(), s.offset);
    let ma: HashMap<Key, &Setting> = a.iter().map(|s| (key(s), s)).collect();
    let mb: HashMap<Key, &Setting> = b.iter().map(|s| (key(s), s)).collect();

    let mut keys: Vec<Key> = ma.keys().chain(mb.keys()).cloned().collect();
    keys.sort();
    keys.dedup();

    let mut out = Vec::new();
    for k in keys {
        let av = ma.get(&k).map(|s| display_value(s));
        let bv = mb.get(&k).map(|s| display_value(s));
        if av != bv {
            out.push(SettingDiff {
                form: k.0,
                name: k.1,
                varstore: k.2,
                offset: k.3,
                a: av,
                b: bv,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(name: &str, form: &str, label: Option<&str>, value: Option<u64>) -> Setting {
        Setting {
            name: name.into(),
            help: String::new(),
            form: form.into(),
            formset: String::new(),
            varstore: "Setup".into(),
            offset: 0,
            width: 1,
            kind: ifr::QKind::OneOf,
            options: vec![],
            value,
            value_label: label.map(|s| s.into()),
            default_value: None,
            default_label: None,
            changed: None,
            conditional: false,
        }
    }

    #[test]
    fn diff_reports_changed_and_one_sided() {
        let a = vec![
            setting("VT-d", "Advanced", Some("Enabled"), Some(1)),
            setting("SATA", "Advanced", Some("AHCI"), Some(0)),
            setting("OnlyA", "Boot", Some("X"), Some(9)),
        ];
        let b = vec![
            setting("VT-d", "Advanced", Some("Disabled"), Some(0)), // changed
            setting("SATA", "Advanced", Some("AHCI"), Some(0)),     // unchanged
            setting("OnlyB", "Boot", Some("Y"), Some(9)),           // only in B
        ];
        let d = diff_settings(&a, &b);
        assert_eq!(d.len(), 3); // VT-d changed, OnlyA gone, OnlyB new
        let vtd = d.iter().find(|x| x.name == "VT-d").unwrap();
        assert_eq!(vtd.a.as_deref(), Some("Enabled (0x1)"));
        assert_eq!(vtd.b.as_deref(), Some("Disabled (0x0)"));
        assert!(d.iter().any(|x| x.name == "OnlyA" && x.b.is_none()));
        assert!(d.iter().any(|x| x.name == "OnlyB" && x.a.is_none()));
    }
}
