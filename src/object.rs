use std::fs;
use std::path::PathBuf;

use crate::linker_error::LinkerError;
use crate::omf_vec::OmfVec;

//
// Holds collections of data parsed from each object file.
//
pub struct Object {
    pub data: Option<Vec<u8>>,
    pub name: String,
    pub lnames: OmfVec<usize>,
}

impl Object {
    //
    // Empty
    //
    pub fn new() -> Self {
        Object {
            data: None,
            name: "".to_owned(),
            lnames: OmfVec::new(),
        }
    }

    //
    // Construct around a filename.
    //
    pub fn from_filename(name: &PathBuf) -> Result<Self, LinkerError> {
        Ok(Object {
            data: Some(fs::read(name)?),
            name: "".to_owned(),
            lnames: OmfVec::new(),
        })
    }
}
