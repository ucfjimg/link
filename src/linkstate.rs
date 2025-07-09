use crate::group::Group;
use crate::lnames::LNames;
use crate::omf_vec::OmfVec;
use crate::segment::{Segment, SegName};
use crate::symbols::SymbolTable;

#[cfg(test)]
use crate::segment::{Align, Combine};

//
// Linker-global data
//

///
/// A segmented 16:16 pointer
///
#[derive(Clone, Copy)]
pub struct FarPtr {
    pub seg: u16,
    pub offset: u16,
}

impl FarPtr {
    /// Create a pointer with a value
    ///
    pub fn new(seg: u16, offset: u16) -> FarPtr {
        FarPtr { seg, offset }
    }

    /// Create a far null pointer
    ///
    pub fn null() -> FarPtr {
        FarPtr::new(0, 0)
    }

    pub fn to_linear(&self) -> usize {
        (self.seg as usize) << 4 + self.offset as usize
    }
}


pub struct LinkState {
    pub lnames: LNames,
    pub segments: OmfVec<Segment>,
    pub groups: OmfVec<Group>,
    pub symbols: SymbolTable,
    pub segment_order: Vec<usize>,
    pub entry: Option<FarPtr>,
}

impl LinkState {
    pub fn new() -> Self {
        LinkState{
            lnames: LNames::new(),
            segments: OmfVec::new(),
            groups: OmfVec::new(),
            symbols: SymbolTable::new(),
            segment_order: Vec::new(),
            entry: None,
        }
    }

    /// Given segment name, turn it into a string based on the global names table.
    /// 
    pub fn segname(&self, segname: &SegName) -> String {
        format!("{}.{}.{}", 
            self.lnames.get(segname.nameidx),
            self.lnames.get(segname.classidx),
            self.lnames.get(segname.ovlyidx))
    }

    
    pub fn get_segment_named(&mut self, segname: &SegName) -> Option<usize> {
        self.segments
            .iter()
            .enumerate()
            .find(|(_, seg)| seg.name == *segname)
            .map(|(i, _)| i + 1)
    }

    pub fn get_group_named(&mut self, grpname: usize) -> Option<usize> {
        self.groups
            .iter()
            .enumerate()
            .find(|(_, grp)| grp.name == grpname)
            .map(|(i, _)| i + 1)
    }

}

#[cfg(test)]
mod test {
    use super::{LinkState, Segment, SegName, Align, Combine, Group};

    #[test]
    fn get_segment_named() {
        let mut state = LinkState::new();

        let segment = Segment::new(SegName::new(1, 2, 3), 0x200, Align::Byte, Combine::Public);
        state.segments.add(segment);

        let segment = Segment::new(SegName::new(4, 5, 6), 0x200, Align::Byte, Combine::Public);
        state.segments.add(segment);

        assert_eq!(state.get_segment_named(&SegName::new(4,5,6)), Some(2));
        assert_eq!(state.get_segment_named(&SegName::new(4,8,6)), None);
    }

    #[test]
    fn get_group_named() {
        let mut state = LinkState::new();

        let group = Group::new(1);
        state.groups.add(group);

        let group = Group::new(3);
        state.groups.add(group);


        assert_eq!(state.get_group_named(3), Some(2));
        assert_eq!(state.get_group_named(2), None);
    }
}