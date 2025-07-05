use std::ops::{Index, IndexMut};

use crate::omf_vec::OmfVec;

//
// A collection of LNAMES.
//

pub struct LNames {
    names: OmfVec<String>,
}

impl LNames {
    pub fn new() -> Self {
        LNames{ names: OmfVec::new() }
    }   

    pub fn add(&mut self, name: &str) -> usize {
        self.names.add(name.to_owned())
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn get(&self, index: usize) -> &str {
        if index == 0 { "" } else { &self.names[index] } 
    }
}


#[cfg(test)]
mod test {
    use super::LNames;

    #[test]
    fn basics() {
        let mut lnames = LNames::new();

        assert_eq!(lnames.len(), 0);
        assert_eq!(lnames.add("ABC"), 1);
        assert_eq!(lnames.len(), 1);

        assert_eq!(lnames.get(0), "");
        assert_eq!(lnames.get(1), "ABC");
    }

    #[test]
    #[should_panic]
    fn out_of_range() {
        let mut lnames = LNames::new();

        assert_eq!(lnames.len(), 0);
        assert_eq!(lnames.add("ABC"), 1);
        assert_eq!(lnames.len(), 1);

        assert_eq!(lnames.get(0), "");
        assert_eq!(lnames.get(1), "ABC");

        // panic!
        lnames.get(2);
    }

}
