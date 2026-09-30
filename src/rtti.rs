struct Section {
    rva: usize,
    size: usize,
    raw: usize,
    raw_size: usize,
    flags: u32,
}

struct Image<'a> {
    bytes: &'a [u8],
    base: u32,
    sections: Vec<Section>,
}

fn word(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

impl<'a> Image<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.get(..2)? != b"MZ" {
            return None;
        }
        let pe = word(bytes, 0x3c)? as usize;
        let header = bytes.get(pe..pe.checked_add(24)?)?;
        if header.get(..6)? != b"PE\0\0\x4c\x01" {
            return None;
        }
        let count = u16::from_le_bytes(header[6..8].try_into().ok()?) as usize;
        let optional_size = u16::from_le_bytes(header[20..22].try_into().ok()?) as usize;
        let optional = bytes.get(pe + 24..(pe + 24).checked_add(optional_size)?)?;
        if optional.get(..2)? != b"\x0b\x01" || count == 0 || count > 96 {
            return None;
        }
        let base = word(optional, 28)?;
        let image_size = word(optional, 56)? as usize;
        let table = pe + 24 + optional_size;
        let mut sections: Vec<Section> = Vec::new();
        for index in 0..count {
            let start = table.checked_add(index * 40)?;
            let header = bytes.get(start..start.checked_add(40)?)?;
            let rva = word(header, 12)? as usize;
            let virtual_size = word(header, 8)? as usize;
            let raw_size = word(header, 16)? as usize;
            let raw = word(header, 20)? as usize;
            let end = rva.checked_add(virtual_size.max(raw_size))?;
            if end > image_size || u64::from(base) + end as u64 > 0x1_0000_0000 {
                return None;
            }
            if sections
                .iter()
                .any(|section| rva < section.rva + section.size && section.rva < end)
            {
                return None;
            }
            bytes.get(raw..raw.checked_add(raw_size)?)?;
            sections.push(Section {
                rva,
                size: virtual_size.max(raw_size),
                raw,
                raw_size,
                flags: word(header, 36)?,
            });
        }
        Some(Self {
            bytes,
            base,
            sections,
        })
    }

    fn data(&self, rva: usize, size: usize) -> Option<&'a [u8]> {
        if !rva.is_multiple_of(4) {
            return None;
        }
        let section = self.sections.iter().find(|section| {
            section.flags & 0x6000_0000 == 0x4000_0000
                && rva >= section.rva
                && rva
                    .checked_add(size)
                    .is_some_and(|end| end <= section.rva + section.size)
        })?;
        let offset = section.raw.checked_add(rva - section.rva)?;
        if (rva - section.rva).checked_add(size)? > section.raw_size {
            return None;
        }
        self.bytes.get(offset..offset + size)
    }

    fn rva(&self, address: u32) -> Option<usize> {
        Some(address.checked_sub(self.base)? as usize)
    }

    fn pointer(&self, bytes: &[u8], offset: usize) -> Option<usize> {
        self.rva(word(bytes, offset)?)
    }

    fn executable(&self, rva: usize) -> bool {
        self.sections.iter().any(|section| {
            section.flags & 0x6000_0000 == 0x6000_0000
                && rva >= section.rva
                && rva < section.rva + section.raw_size
        })
    }

    fn type_name(&self, rva: usize) -> Option<&'a [u8]> {
        let descriptor = self.data(rva, 8)?;
        if word(descriptor, 4)? != 0 {
            return None;
        }
        self.data(self.pointer(descriptor, 0)?, 4)?;
        for size in 9..=264 {
            let descriptor = self.data(rva, size)?;
            if descriptor[size - 1] == 0 {
                let name = &descriptor[8..];
                return (name.starts_with(b".?A")
                    && name[..name.len() - 1].iter().all(|c| c.is_ascii_graphic()))
                .then_some(name);
            }
        }
        None
    }

    fn type_is(&self, rva: usize, name: &[u8]) -> bool {
        self.type_name(rva) == Some(name)
    }

    fn hierarchy(&self, rva: usize, type_rva: usize) -> Option<()> {
        let header = self.data(rva, 16)?;
        if word(header, 0)? != 0 || word(header, 4)? & !7 != 0 {
            return None;
        }
        let count = word(header, 8)? as usize;
        if !(3..=64).contains(&count) {
            return None;
        }
        let bases = self.data(self.pointer(header, 12)?, count * 4)?;
        let mut actor = false;
        let mut object = false;
        for index in 0..count {
            let descriptor = self.data(self.pointer(bases, index * 4)?, 24)?;
            let base_type = self.pointer(descriptor, 0)?;
            self.type_name(base_type)?;
            let contained = word(descriptor, 4)? as usize;
            if contained >= count - index || (index == 0 && contained != count - 1) {
                return None;
            }
            if index == 0
                && (base_type != type_rva
                    || word(descriptor, 8)? != 0
                    || word(descriptor, 12)? != u32::MAX
                    || word(descriptor, 16)? != 0)
            {
                return None;
            }
            actor |= self.type_is(base_type, b".?AVAActor@@\0");
            object |= self.type_is(base_type, b".?AVUObject@@\0");
        }
        (actor && object).then_some(())
    }
}

pub fn compass_vtable(bytes: &[u8]) -> Result<usize, String> {
    let image = Image::parse(bytes).ok_or("Compass RTTI: invalid PE32 image")?;
    let mut locators = Vec::new();
    for section in &image.sections {
        if section.flags & 0x6000_0000 != 0x4000_0000 {
            continue;
        }
        for offset in (0..section.size.saturating_sub(19)).step_by(4) {
            let rva = section.rva + offset;
            let Some(locator) = image.data(rva, 20) else {
                continue;
            };
            if locator[..12] != [0; 12] {
                continue;
            }
            let Some(type_rva) = image.pointer(locator, 12) else {
                continue;
            };
            if !image.type_is(type_rva, b".?AVAGPSArrow@@\0") {
                continue;
            }
            let hierarchy = image
                .pointer(locator, 16)
                .and_then(|rva| image.hierarchy(rva, type_rva));
            locators.push((rva, hierarchy.is_some()));
        }
    }
    let mut result = None;
    for section in &image.sections {
        if section.flags & 0x6000_0000 != 0x4000_0000 {
            continue;
        }
        for offset in (0..section.size.saturating_sub(15)).step_by(4) {
            let rva = section.rva + offset;
            let Some(table) = image.data(rva, 16) else {
                continue;
            };
            let Some(locator_rva) = image.pointer(table, 0) else {
                continue;
            };
            let Some((_, valid_hierarchy)) = locators.iter().find(|(rva, _)| *rva == locator_rva)
            else {
                continue;
            };
            if !valid_hierarchy {
                return Err("Compass RTTI: invalid AGPSArrow hierarchy".into());
            }
            if !(1..=3).all(|index| {
                image
                    .pointer(table, index * 4)
                    .is_some_and(|rva| image.executable(rva))
            }) {
                return Err("Compass RTTI: non-executable AGPSArrow virtual method".into());
            }
            if result.replace(rva + 4).is_some() {
                return Err("Compass RTTI: ambiguous AGPSArrow primary vtable".into());
            }
        }
    }
    result.ok_or_else(|| "Compass RTTI: AGPSArrow primary vtable not found".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn fixture(base: u32, shift: u32) -> Vec<u8> {
        let mut bytes = vec![0; 0x800];
        bytes[..2].copy_from_slice(b"MZ");
        put(&mut bytes, 0x3c, 0x80);
        bytes[0x80..0x86].copy_from_slice(b"PE\0\0\x4c\x01");
        bytes[0x86] = 2;
        bytes[0x94] = 0xe0;
        bytes[0x98..0x9a].copy_from_slice(b"\x0b\x01");
        put(&mut bytes, 0xb4, base);
        put(&mut bytes, 0xd0, shift + 0x3000);
        for (index, (rva, size, raw, flags)) in [
            (shift + 0x1000, 0x200, 0x200, 0x6000_0020),
            (shift + 0x2000, 0x400, 0x400, 0x4000_0040),
        ]
        .into_iter()
        .enumerate()
        {
            let header = 0x178 + index * 40;
            put(&mut bytes, header + 8, size);
            put(&mut bytes, header + 12, rva);
            put(&mut bytes, header + 16, size);
            put(&mut bytes, header + 20, raw);
            put(&mut bytes, header + 36, flags);
        }
        let address = |raw: u32| base + shift + 0x2000 + raw - 0x400;
        for (raw, name) in [
            (0x400, b".?AVAGPSArrow@@\0".as_slice()),
            (0x440, b".?AVAActor@@\0".as_slice()),
            (0x480, b".?AVUObject@@\0".as_slice()),
        ] {
            put(&mut bytes, raw, address(0x7e0));
            bytes[raw + 8..raw + 8 + name.len()].copy_from_slice(name);
        }
        put(&mut bytes, 0x50c, address(0x400));
        put(&mut bytes, 0x510, address(0x540));
        put(&mut bytes, 0x548, 3);
        put(&mut bytes, 0x54c, address(0x580));
        for (index, raw) in [0x600, 0x620, 0x640].into_iter().enumerate() {
            put(&mut bytes, 0x580 + index * 4, address(raw));
            put(
                &mut bytes,
                raw as usize,
                address(0x400 + index as u32 * 0x40),
            );
            put(&mut bytes, raw as usize + 4, 2 - index as u32);
            put(&mut bytes, raw as usize + 12, u32::MAX);
        }
        put(&mut bytes, 0x700, address(0x500));
        for index in 1..=3 {
            put(
                &mut bytes,
                0x700 + index * 4,
                base + shift + 0x1000 + index as u32 * 0x10,
            );
        }
        bytes
    }

    #[test]
    fn finds_relocated_vtables_including_above_two_gigabytes() {
        for (base, shift) in [
            (0x0040_0000, 0),
            (0xa000_0000, 0x5000),
            (0x1000_0000, 0x19000),
        ] {
            assert_eq!(
                compass_vtable(&fixture(base, shift)).unwrap(),
                shift as usize + 0x2304
            );
        }
    }

    #[test]
    fn rejects_missing_duplicate_and_wrong_class_anchors() {
        let mut bytes = fixture(0x0040_0000, 0);
        bytes[0x40e] = b'X';
        assert!(compass_vtable(&bytes).unwrap_err().contains("not found"));
        bytes = fixture(0x0040_0000, 0);
        let table = bytes[0x700..0x710].to_vec();
        bytes[0x720..0x730].copy_from_slice(&table);
        assert!(compass_vtable(&bytes).unwrap_err().contains("ambiguous"));
        for offset in [0x448, 0x488, 0x608] {
            let mut bytes = fixture(0x0040_0000, 0);
            bytes[offset] ^= 1;
            assert!(compass_vtable(&bytes).is_err());
        }
    }

    #[test]
    fn rejects_invalid_bounds_and_non_executable_methods() {
        for (offset, value) in [
            (0x3c, u32::MAX),
            (0xd0, 0x2100),
            (0x178 + 40 + 12, 0x1000),
            (0x178 + 40 + 20, u32::MAX),
            (0x548, u32::MAX),
            (0x54c, 0x0040_23ff),
            (0x704, 0x0040_2100),
            (0x704, 0xffff_ffff),
            (0x510, 0x0040_23ff),
            (0x580, 0x0040_23ff),
            (0x604, 0),
            (0x644, 1),
            (0x98, 0x20b),
        ] {
            let mut bytes = fixture(0x0040_0000, 0);
            put(&mut bytes, offset, value);
            assert!(compass_vtable(&bytes).is_err(), "offset={offset:#x}");
        }
        let bytes = fixture(0x0040_0000, 0);
        for end in 0..bytes.len() {
            assert!(compass_vtable(&bytes[..end]).is_err(), "length={end:#x}");
        }
    }

    #[test]
    fn failure_after_success_has_no_cached_selection() {
        let bytes = fixture(0x0040_0000, 0);
        assert!(compass_vtable(&bytes).is_ok());
        assert!(compass_vtable(&[]).is_err());
        assert!(compass_vtable(&bytes).is_ok());
    }

    #[test]
    #[ignore = "requires the installed Steam executable through BIOSHOCK_DISCOVERY_EXE"]
    fn installed_steam_image_matches_independent_measurement() {
        let path = std::env::var_os("BIOSHOCK_DISCOVERY_EXE").unwrap();
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(compass_vtable(&bytes).unwrap(), 0xd8992c);
    }
}
