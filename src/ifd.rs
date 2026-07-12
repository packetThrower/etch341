//! Intel Flash Descriptor (IFD) parser: the small structure at the
//! start of an Intel-chipset SPI flash that carves the chip into named
//! regions (Descriptor / BIOS / ME / GbE / …) and encodes per-master
//! read/write access. Read-only, and static: it parses the image on
//! disk, not the chipset's runtime SPI lock registers (those live in
//! the PCH, not the flash).
//!
//! Layout: FLVALSIG (`0x0FF0A55A`) at offset 0x10, then FLMAP0/FLMAP1
//! give the base of the region table (FRBA) and master table (FMBA).
//! Reference: Intel SPI Programming Guide; flashrom/ifdtool.
//!
//! ponytail: the master read/write masks are decoded with the classic
//! (pre-Skylake) 8-bit-per-region layout, which is what the region
//! table here uses. Skylake+ descriptors widened the masks and moved
//! regions to 16 slots — the region map still parses, but `masters`
//! would need the newer layout. Upgrade there if a Skylake+ dump shows
//! up.

/// Descriptor validity signature, little-endian at image offset 0x10.
const FLVALSIG: u32 = 0x0FF0_A55A;
const SIG_OFFSET: usize = 0x10;
const DESC_LEN: usize = 0x1000; // the descriptor occupies the first 4 KiB

const REGION_NAMES: [&str; 8] = [
    "Flash Descriptor",
    "BIOS",
    "Intel ME",
    "GbE",
    "Platform Data",
    "Region 5",
    "Region 6",
    "EC",
];
const MASTER_NAMES: [&str; 5] = ["BIOS", "Intel ME", "GbE", "EC", "Master 5"];

/// A present flash region: `base..=limit` (inclusive last byte).
pub struct Region {
    pub index: usize,
    pub name: &'static str,
    pub base: u32,
    pub limit: u32,
}

impl Region {
    pub fn size(&self) -> u32 {
        self.limit - self.base + 1
    }
}

/// One flash master and the regions it may write (bit N = region N).
pub struct Master {
    pub name: &'static str,
    pub write: u8,
}

pub struct Ifd {
    /// Regions actually present (absent/empty slots dropped).
    pub regions: Vec<Region>,
    pub masters: Vec<Master>,
    /// Capacity of component 0, from its density code.
    pub density_bytes: Option<u64>,
    pub components: u8,
}

impl Ifd {
    /// May the host/BIOS master write region `index`? Used to report
    /// whether the Descriptor and ME regions are locked against the OS.
    pub fn bios_can_write(&self, index: usize) -> bool {
        self.masters
            .first()
            .is_some_and(|m| m.write & (1 << index) != 0)
    }
}

/// Parse the descriptor at the start of `image`. Returns `None` if the
/// signature is absent (not an Intel-descriptor flash).
pub fn parse(image: &[u8]) -> Option<Ifd> {
    if image.len() < DESC_LEN || u32le(image, SIG_OFFSET)? != FLVALSIG {
        return None;
    }
    let flmap0 = u32le(image, 0x14)?;
    let flmap1 = u32le(image, 0x18)?;
    let fcba = ((flmap0 & 0xff) << 4) as usize;
    let components = (((flmap0 >> 8) & 0x3) + 1) as u8;
    let frba = (((flmap0 >> 16) & 0xff) << 4) as usize;
    let nr = (((flmap0 >> 24) & 0x7) + 1) as usize; // region count
    let nm = (((flmap1 >> 8) & 0x3) + 1) as usize; // master count
    let fmba = ((flmap1 & 0xff) << 4) as usize;

    // Regions live at fixed slots; walk the NR the descriptor declares
    // and keep the present ones (absent slots use the sentinels below).
    let mut regions = Vec::new();
    for (i, &name) in REGION_NAMES.iter().enumerate().take(nr) {
        let Some(reg) = u32le(image, frba + i * 4) else {
            break;
        };
        if reg == 0xFFFF_FFFF {
            continue; // absent slot
        }
        let base_f = reg & 0x7FFF;
        let limit_f = (reg >> 16) & 0x7FFF;
        if limit_f < base_f {
            continue; // empty region (e.g. base=0x7FFF, limit=0)
        }
        regions.push(Region {
            index: i,
            name,
            base: base_f << 12,
            limit: (limit_f << 12) | 0xFFF,
        });
    }

    let mut masters = Vec::new();
    for (i, &name) in MASTER_NAMES.iter().enumerate().take(nm) {
        let Some(m) = u32le(image, fmba + i * 4) else {
            break;
        };
        masters.push(Master {
            name,
            write: ((m >> 24) & 0xff) as u8,
        });
    }

    let density_bytes =
        u32le(image, fcba).and_then(|flcomp| density_to_bytes((flcomp & 0x7) as u8));

    Some(Ifd {
        regions,
        masters,
        density_bytes,
        components,
    })
}

/// Component density code → capacity in bytes. 0 = 512 KiB, doubling up.
fn density_to_bytes(code: u8) -> Option<u64> {
    (code <= 7).then(|| (512 * 1024u64) << code)
}

fn u32le(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal descriptor: signature, FLMAP0/1, and the region +
    /// master tables at FRBA=0x40 / FMBA=0x60.
    fn descriptor(regs: &[u32], masters: &[u32], flcomp: u32) -> Vec<u8> {
        let mut d = vec![0u8; DESC_LEN];
        d[0x10..0x14].copy_from_slice(&FLVALSIG.to_le_bytes());
        // FCBA=0x30, NC=1, FRBA=0x40, NR=regs-1
        let flmap0 = 0x03 | (0x04 << 16) | (((regs.len() - 1) as u32) << 24);
        d[0x14..0x18].copy_from_slice(&flmap0.to_le_bytes());
        // FMBA=0x60, NM=masters-1
        let flmap1 = 0x06 | (((masters.len() - 1) as u32) << 8);
        d[0x18..0x1c].copy_from_slice(&flmap1.to_le_bytes());
        d[0x30..0x34].copy_from_slice(&flcomp.to_le_bytes());
        for (i, r) in regs.iter().enumerate() {
            d[0x40 + i * 4..0x44 + i * 4].copy_from_slice(&r.to_le_bytes());
        }
        for (i, m) in masters.iter().enumerate() {
            d[0x60 + i * 4..0x64 + i * 4].copy_from_slice(&m.to_le_bytes());
        }
        d
    }

    #[test]
    fn parses_regions_masters_and_density() {
        // Descriptor 0x0-0xFFF, BIOS 0x400000-0x7FFFFF, ME 0x1000-0x3FFFFF,
        // GbE + PD unused. FLCOMP density 4 = 8 MiB. Real bytes from an
        // Acer V5WE2217 Insyde dump.
        let d = descriptor(
            &[0x0000_0000, 0x07ff_0400, 0x03ff_0001, 0x0000_7fff],
            &[0x0a0b_0000, 0x0c0d_0000, 0x0808_0118],
            0x6490_0044,
        );
        let ifd = parse(&d).unwrap();
        assert_eq!(ifd.regions.len(), 3);
        assert_eq!(ifd.regions[0].name, "Flash Descriptor");
        assert_eq!(ifd.regions[1].name, "BIOS");
        assert_eq!(
            (ifd.regions[1].base, ifd.regions[1].limit),
            (0x40_0000, 0x7f_ffff)
        );
        assert_eq!(ifd.regions[1].size(), 0x40_0000);
        assert_eq!(ifd.regions[2].name, "Intel ME");
        assert_eq!(ifd.density_bytes, Some(8 << 20));
        // BIOS master can write BIOS (bit 1) but not Descriptor (0) or ME (2).
        assert!(ifd.bios_can_write(1));
        assert!(!ifd.bios_can_write(0));
        assert!(!ifd.bios_can_write(2));
    }

    #[test]
    fn rejects_non_descriptor_image() {
        assert!(parse(&vec![0xFF; DESC_LEN]).is_none());
        assert!(parse(b"too short").is_none());
    }
}
