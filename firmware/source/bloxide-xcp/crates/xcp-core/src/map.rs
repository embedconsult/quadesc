/// Access policy for one logical virtual-address region.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    ReadOnly,
    /// Readable calibration scalar; writes must cover the complete region.
    Calibration,
}

/// One descriptor-backed logical region. Addresses are never converted to
/// pointers; providers receive only descriptor IDs and bounded offsets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Region {
    pub descriptor_id: u32,
    pub address: u32,
    pub length: u32,
    pub access: Access,
}

impl Region {
    pub const fn read_only(descriptor_id: u32, address: u32, length: u32) -> Self {
        Self {
            descriptor_id,
            address,
            length,
            access: Access::ReadOnly,
        }
    }

    pub const fn calibration(descriptor_id: u32, address: u32, length: u32) -> Self {
        Self {
            descriptor_id,
            address,
            length,
            access: Access::Calibration,
        }
    }

    fn end(self) -> Option<u32> {
        self.address.checked_add(self.length)
    }
}

/// A checked view over immutable application-composed regions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VirtualMap<'a> {
    regions: &'a [Region],
}

impl<'a> VirtualMap<'a> {
    pub fn new(regions: &'a [Region]) -> Result<Self, MapDefinitionError> {
        let mut previous: Option<Region> = None;
        for (index, region) in regions.iter().copied().enumerate() {
            if region.length == 0 || region.end().is_none() {
                return Err(MapDefinitionError::InvalidRange { index });
            }
            if regions[..index]
                .iter()
                .any(|prior| prior.descriptor_id == region.descriptor_id)
            {
                return Err(MapDefinitionError::DuplicateDescriptor {
                    descriptor_id: region.descriptor_id,
                });
            }
            if let Some(prior) = previous {
                if region.address < prior.address {
                    return Err(MapDefinitionError::NotSorted { index });
                }
                if region.address < prior.end().unwrap_or(prior.address) {
                    return Err(MapDefinitionError::Overlap { index });
                }
            }
            previous = Some(region);
        }
        Ok(Self { regions })
    }

    pub const fn regions(&self) -> &[Region] {
        self.regions
    }

    pub fn resolve_read(&self, address: u32, length: u8) -> Result<ResolvedRead, ReadResolveError> {
        if length == 0 || length > 7 {
            return Err(ReadResolveError::OutOfRange);
        }
        let request_end = address
            .checked_add(u32::from(length))
            .ok_or(ReadResolveError::OutOfRange)?;
        let region =
            self.regions.iter().copied().find(|candidate| {
                address >= candidate.address && address < candidate.end().unwrap()
            });
        let Some(region) = region else {
            return Err(ReadResolveError::AccessDenied);
        };
        if request_end > region.end().unwrap() {
            return Err(ReadResolveError::AccessDenied);
        }
        Ok(ResolvedRead {
            descriptor_id: region.descriptor_id,
            offset: address - region.address,
            length,
        })
    }

    pub fn resolve_write(
        &self,
        address: u32,
        length: u8,
    ) -> Result<ResolvedWrite, WriteResolveError> {
        if length == 0 {
            return Err(WriteResolveError::OutOfRange);
        }
        let request_end = address
            .checked_add(u32::from(length))
            .ok_or(WriteResolveError::OutOfRange)?;
        let region =
            self.regions.iter().copied().find(|candidate| {
                address >= candidate.address && address < candidate.end().unwrap()
            });
        let Some(region) = region else {
            return Err(WriteResolveError::AccessDenied);
        };
        if request_end > region.end().unwrap() {
            return Err(WriteResolveError::AccessDenied);
        }
        if region.access == Access::ReadOnly {
            return Err(WriteResolveError::WriteProtected);
        }
        if address != region.address || u32::from(length) != region.length {
            return Err(WriteResolveError::OutOfRange);
        }
        Ok(ResolvedWrite {
            descriptor_id: region.descriptor_id,
            length,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MapDefinitionError {
    InvalidRange { index: usize },
    NotSorted { index: usize },
    Overlap { index: usize },
    DuplicateDescriptor { descriptor_id: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadResolveError {
    OutOfRange,
    AccessDenied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteResolveError {
    OutOfRange,
    WriteProtected,
    AccessDenied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedRead {
    pub descriptor_id: u32,
    pub offset: u32,
    pub length: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedWrite {
    pub descriptor_id: u32,
    pub length: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGIONS: [Region; 4] = [
        Region::read_only(1, 0x0000, 32),
        Region::calibration(2, 0x1000, 2),
        Region::calibration(3, 0x1002, 2),
        Region::read_only(4, 0x2000, 4),
    ];

    #[test]
    fn map_rejects_invalid_order_overlap_and_duplicate_ids() {
        assert_eq!(
            VirtualMap::new(&[Region::read_only(1, 4, 2), Region::read_only(2, 3, 1)]),
            Err(MapDefinitionError::NotSorted { index: 1 })
        );
        assert_eq!(
            VirtualMap::new(&[Region::read_only(1, 4, 2), Region::read_only(2, 5, 1)]),
            Err(MapDefinitionError::Overlap { index: 1 })
        );
        assert_eq!(
            VirtualMap::new(&[Region::read_only(1, 4, 1), Region::read_only(1, 6, 1)]),
            Err(MapDefinitionError::DuplicateDescriptor { descriptor_id: 1 })
        );
        assert_eq!(
            VirtualMap::new(&[Region::read_only(1, u32::MAX, 1)]),
            Err(MapDefinitionError::InvalidRange { index: 0 })
        );
    }

    #[test]
    fn reads_are_subranges_of_exactly_one_region() {
        let map = VirtualMap::new(&REGIONS).unwrap();
        assert_eq!(
            map.resolve_read(7, 7),
            Ok(ResolvedRead {
                descriptor_id: 1,
                offset: 7,
                length: 7
            })
        );
        assert_eq!(map.resolve_read(31, 2), Err(ReadResolveError::AccessDenied));
        assert_eq!(
            map.resolve_read(0x1001, 2),
            Err(ReadResolveError::AccessDenied)
        );
        assert_eq!(
            map.resolve_read(u32::MAX, 1),
            Err(ReadResolveError::OutOfRange)
        );
    }

    #[test]
    fn writes_require_one_complete_writable_scalar() {
        let map = VirtualMap::new(&REGIONS).unwrap();
        assert_eq!(
            map.resolve_write(0x1000, 2),
            Ok(ResolvedWrite {
                descriptor_id: 2,
                length: 2
            })
        );
        assert_eq!(
            map.resolve_write(0x1000, 1),
            Err(WriteResolveError::OutOfRange)
        );
        assert_eq!(
            map.resolve_write(0x1001, 2),
            Err(WriteResolveError::AccessDenied)
        );
        assert_eq!(
            map.resolve_write(0x2000, 4),
            Err(WriteResolveError::WriteProtected)
        );
        assert_eq!(
            map.resolve_write(0x9999, 2),
            Err(WriteResolveError::AccessDenied)
        );
    }
}
