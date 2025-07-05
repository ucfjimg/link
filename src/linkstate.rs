use crate::omf_vec::OmfVec;

//
// Linker-global data
//

pub struct LinkState {
    pub lnames: OmfVec<String>,
}

impl LinkState {
    pub fn new() -> Self {
        LinkState{
            lnames: OmfVec::new(),
        }
    }
}
