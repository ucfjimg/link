use crate::omf_vec::OmfVec;

//
// Maps a set of object file indices to linker level indices.
// Like an OmfVec<usize> except that 0 (no such entry) is valid and maps to 0.
//

#[derive(Debug)]
pub struct IndexMap {
    indices: OmfVec<usize>,
}

impl IndexMap {
    pub fn new() -> Self {
        IndexMap{ indices: OmfVec::new() }
    }   

    //
    // Takes the linker-level index and returns the object-level index.
    //
    pub fn add(&mut self, index: usize) -> usize {
        self.indices.add(index)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.indices.len()
    }

    pub fn get(&self, index: usize) -> usize {
        if index == 0 { 0 } else { self.indices[index] } 
    }

    pub fn is_valid_index(&self, index: usize) -> bool {
        index == 0 || self.indices.is_valid_index(index)
    }
}

#[cfg(test)]
mod test {
    use super::IndexMap;

    #[test]
    fn basics() {
        let mut indices = IndexMap::new();

        assert_eq!(indices.len(), 0);
        assert_eq!(indices.add(42), 1);
        assert_eq!(indices.len(), 1);

        assert_eq!(indices.get(0), 0);
        assert_eq!(indices.get(1), 42);
    }

    #[test]
    #[should_panic]
    fn out_of_range() {
        let mut indices = IndexMap::new();

        assert_eq!(indices.len(), 0);
        assert_eq!(indices.add(42), 1);
        assert_eq!(indices.len(), 1);

        assert_eq!(indices.get(0), 0);
        assert_eq!(indices.get(1), 42);

        // panic!
        indices.get(2);
    }

}
