use std::fs;
use std::path::PathBuf;

use crate::index_map::IndexMap;
use crate::linker_error::LinkerError;
use crate::omf_vec::OmfVec;
use crate::segment::SegDef;

//
// Holds collections of data parsed from each object file.
//
pub struct Object {
    pub data: Option<Vec<u8>>,
    pub name: String,
    pub lnames: IndexMap,
    pub segdefs: OmfVec<SegDef>,
}

impl Object {
    //
    // Empty
    //
    pub fn new() -> Self {
        Object {
            data: None,
            name: "".to_owned(),
            lnames: IndexMap::new(),
            segdefs: OmfVec::new(),
        }
    }

    //
    // Construct around a filename.
    //
    pub fn from_filename(name: &PathBuf) -> Result<Self, LinkerError> {
        Ok(Object {
            data: Some(fs::read(name)?),
            name: "".to_owned(),
            lnames: IndexMap::new(),
            segdefs: OmfVec::new(),
        })
    }
}
