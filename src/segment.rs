use std::cmp::max;

use crate::linker_error::LinkerError;

//
// Structures for segments 
//

/// Segment alignments, as coded in the ACBP field of a SEGDEF.
///
#[derive(PartialEq, Eq, Debug, Copy, Clone, PartialOrd, Ord)]
pub enum Align {
    //
    // These are ordered so that greater enum value is more strict alignment.
    //
    Absolute,
    Byte,
    Word,
    Dword,
    Para,
    Page,
}

impl Align {
    /// Extract the alignment from an ACBP byte.
    ///
    pub fn from_acbp(acbp: u8) -> Result<Self, LinkerError> {
        let align = (acbp >> 5) & 7;
        Ok(match align {
            0 => Align::Absolute,
            1 => Align::Byte,
            2 => Align::Word,
            3 => Align::Para,
            4 => Align::Page,
            5 => Align::Dword,
            _ => { return Err(LinkerError::new(&format!("invalid align in SEGDEF {:02x}", align))) },
        })
    }

    /// Given an offset, adjust it upwards to the next boundary implied
    /// by the alignment.
    /// 
    pub fn align_by(self, offset: usize) -> usize {
        let align = match self {
            Align::Absolute => 1,
            Align::Byte => 1,
            Align::Word => 2,
            Align::Para => 16,
            Align::Page => 256,
            Align::Dword => 4,
        };

        (offset + align - 1) & !(align - 1)
    }
}

/// Segment combine methods, as coded in the ACBP field of a SEGDEF.
///
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum Combine {
    Private,
    Public,
    Stack,
    Common,
}

impl Combine {
    pub fn from_acbp(acbp: u8) -> Result<Self, LinkerError> {
        let combine = (acbp >> 2) & 7;
        Ok(match combine {
            0 => Combine::Private,
            2 | 4 | 7 => Combine::Public,
            5 => Combine::Stack,
            6 => Combine::Common,
            _ => { return Err(LinkerError::new(&format!("invalid combine in SEGDEF {:02x}", combine))) },
        })
    }
}

/// A `SegDef` is the representation of a segment in the object module.
/// It contains a reference back to the combined segment, as well as 
/// the base and length of the segment's data owned by the object 
/// module.
/// 
pub struct SegDef {
    pub segidx: usize,
    pub base: usize,
    pub length: usize,
    pub acbp: u8,
    pub align: Align,
    pub combine: Combine,
}

impl SegDef {
    pub fn new(segidx: usize, length: usize, acbp: u8, align: Align, combine: Combine) -> SegDef {
        SegDef {
            segidx,
            base: 0,
            length,
            acbp,
            align,
            combine
        }
    }
}

/// The name of a segment is the triple of it's name, class, and overlay lnames.
/// These are indices into the global lnames table.
/// 
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegName {
    pub nameidx: usize,
    pub classidx: usize,
    pub ovlyidx: usize,
}

impl SegName {
    pub fn new(nameidx: usize, classidx: usize, ovlyidx: usize) -> SegName {
        SegName{ nameidx, classidx, ovlyidx }
    }
}

/// A `Segment` is the collection of all combined `SegDef`'s of the same
/// name. It represents a contiguous region of memory in the final executable's
/// address space.
///
#[derive(Clone)] 
pub struct Segment {
    pub name: SegName,
    pub length: usize,
    pub align: Align,
    pub combine: Combine,
    pub base: usize,
}

/// The maximum size of a 32-bit segment.
/// 
const MAX_SEGMENT_SIZE: usize = 0x10000;

impl Segment {
    pub fn new(name: SegName, length: usize, align: Align, combine: Combine) -> Segment {
        Segment{ name, length, align, combine, base: 0 }
    }

    /// Add a SEGDEF to the segment, validating the combine type and total size, and returning
    /// the offset of the SEGDEF in the segment.
    /// 
    pub fn add_segdef(&mut self, segdef: &SegDef) -> Result<usize, LinkerError> {
        if segdef.combine == Combine::Private {
            Err(LinkerError::new("cannot combine private segdef into existing segment."))
        } else if self.combine == Combine::Private {
            Err(LinkerError::new("cannot combine segdef into private segment."))
        } else if segdef.combine != self.combine {
            Err(LinkerError::new(&format!("cannot combine `{:?}` segdef into `{:?}` segment.", segdef.combine, self.combine)))
        } else if segdef.length > MAX_SEGMENT_SIZE {
            Err(LinkerError::new(&format!("segment length {:X}H is larger than the maximum size of 64k.", segdef.length)))
        } else {
            //
            // Combine types are ok, compute segdef's offset and new segment length.
            //
            let offset = match self.combine {
                Combine::Public => {
                    //
                    // Public segments combine end to end, honoring the alignment
                    // requirements.
                    //
                    let offset = segdef.align.align_by(self.length);
                    let new_length = offset + segdef.length;

                    if new_length > MAX_SEGMENT_SIZE {
                        return Err(LinkerError::new("segment overflow."));
                    }

                    self.length = new_length;
                    offset
                },
                Combine::Stack => {
                    //
                    // Stack segments combine with byte alignment (regardless of the given alignment)
                    //
                    let offset = self.length;
                    let new_length = offset + segdef.length;

                    if new_length > MAX_SEGMENT_SIZE {
                        return Err(LinkerError::new("segment overflow."));
                    }

                    self.length = new_length;
                    offset

                },
                Combine::Common => {
                    //
                    // Common segments are all overlaid. The offset is always zero, and
                    // the length is the length of the longest contributor.
                    //
                    self.length = max(self.length, segdef.length);
                    0
                },
                Combine::Private => unreachable!(),
            };

            self.align = max(self.align, segdef.align);

            Ok(offset)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn align_basics() -> Result<(), LinkerError> {
        let acbp = 0b011_000_00u8;

        let align = Align::from_acbp(acbp)?;
        assert_eq!(align, Align::Para);

        assert_eq!(align.align_by(9), 16);
        assert_eq!(align.align_by(16), 16);
        assert_eq!(align.align_by(17), 32);

        // invalid align field.
        //
        let acbp = 0b111_000_00u8;
        assert!(Align::from_acbp(acbp).is_err());

        Ok(())
    }

    #[test]
    fn segment_basic_public() -> Result<(), LinkerError> {
        let mut segment = Segment::new(
            SegName::new(0, 0, 0),
            0x3e8,
            Align::Word,
            Combine::Public
        );

        let acbp = 0x68;
        let segdef = SegDef::new(1, 0x1f4, acbp, Align::Para, Combine::Public);

        let base = segment.add_segdef(&segdef)?;

        assert_eq!(base, 0x3f0);
        assert_eq!(segment.length, 0x5e4);

        Ok(())
    }

    #[test]
    fn segment_basic_stack() -> Result<(), LinkerError> {
        let mut segment = Segment::new(
            SegName::new(0, 0, 0),
            0x3e8,
            Align::Para,
            Combine::Stack
        );

        let acbp = 0x74;
        let segdef = SegDef::new(1, 0x1f4, acbp, Align::Para, Combine::Stack);

        let base = segment.add_segdef(&segdef)?;

        assert_eq!(base, 0x3e8);
        assert_eq!(segment.length, 0x5dc);

        Ok(())
    }

    #[test]
    fn segment_basic_common() -> Result<(), LinkerError> {
        let mut segment = Segment::new(
            SegName::new(0, 0, 0),
            0x3e8,
            Align::Para,
            Combine::Common
        );

        let acbp = 0x78;
        let segdef = SegDef::new(1, 0x1f4, acbp, Align::Para, Combine::Common);

        let base = segment.add_segdef(&segdef)?;

        assert_eq!(base, 0);
        assert_eq!(segment.length, 0x3e8);

        let mut segment = Segment::new(
            SegName::new(0, 0, 0),
            0x1e8,
            Align::Para,
            Combine::Common
        );

        let segdef = SegDef::new(1, 0x1f4, acbp, Align::Para, Combine::Common);

        let base = segment.add_segdef(&segdef)?;

        assert_eq!(base, 0);
        assert_eq!(segment.length, 0x1f4);

        Ok(())
    }

   #[test]
    fn segment_too_large() -> Result<(), LinkerError> {
        let mut segment = Segment::new(
            SegName::new(0, 0, 0),
            0xfff8,
            Align::Byte,
            Combine::Public
        );

        //
        // Exactly 64k is fine.
        //
        let acbp = 0x28;
        let segdef = SegDef::new(1, 8, acbp, Align::Byte, Combine::Public);

        let base = segment.add_segdef(&segdef)?;

        assert_eq!(base, 0xfff8);
        assert_eq!(segment.length, 0x10000);

        //
        // 64k + 1 byte is not.
        //
        let segdef = SegDef::new(1, 1, acbp, Align::Byte, Combine::Public);

        assert!(segment.add_segdef(&segdef).is_err());

        Ok(())
    }

    #[test]
    fn segment_cannot_combine_privates() -> Result<(), LinkerError> {
        let mut segment = Segment::new(
            SegName::new(0, 0, 0),
            0,
            Align::Byte,
            Combine::Private
        );

        let acbp = 0x20;
        let segdef = SegDef::new(1, 8, acbp, Align::Byte, Combine::Private);

        assert!(segment.add_segdef(&segdef).is_err());

        Ok(())
    }

    #[test]
    fn segment_cannot_combine_different_combine_types() -> Result<(), LinkerError> {
        let mut segment = Segment::new(
            SegName::new(0, 0, 0),
            0,
            Align::Byte,
            Combine::Public
        );

        let acbp = 0x34;
        
        let segdef = SegDef::new(1, 8, acbp, Align::Byte, Combine::Stack);

        assert!(segment.add_segdef(&segdef).is_err());

        Ok(())
    }
}
