use std::ops::{Index, IndexMut};

//
// Collections in OMF are stored as 1-based, with index 0 meaning 
// "no such element". This collection acts like a normal std::Vec
// but panics if the index is zero as well as out of range.
//

pub struct OmfVec<T> where T: Sized {
    data: Vec<T>,
}

pub struct Iter<'a, T> {
    vec: &'a OmfVec<T>,
    index: usize
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.vec.len() {
            self.index += 1;
            let value = &self.vec[self.index];
            Some(value)
        } else {
            None
        }
    }
}

impl<T> OmfVec<T> {
    /// Construct
    /// 
    pub fn new() -> OmfVec<T> {
        OmfVec{ data: Vec::new() }
    }

    /// Push a new element
    /// 
    pub fn push(&mut self, value: T) {
        self.data.push(value);
    }

    /// Like `push` but returns the index of the just-added element.
    /// 
    pub fn add(&mut self, value: T) -> usize {
        self.data.push(value);
        self.data.len()
    }

    /// Number of elements in the vector
    ///
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check that an index is valid
    /// 
    pub fn is_valid_index(&self, index: usize) -> bool {
        index > 0 && index <= self.len()
    }

    /// Return a non-mutable iterator to the vector.
    ///
    pub fn iter(&self) -> Iter<T> {
        Iter{ vec: self, index: 0 }
    }
}

impl<T> Index<usize> for OmfVec<T> 
    where T: Sized {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        if index == 0 {
            panic!("index of 0 used in OMF collection");
        }

        &self.data[index - 1]
    }
}

impl<T> IndexMut<usize> for OmfVec<T> 
    where T: Sized {

    fn index_mut(&mut self, index: usize) -> &mut T {
        if index == 0 {
            panic!("index of 0 used in OMF collection");
        }

        &mut self.data[index - 1]
    }
}

#[cfg(test)]
mod test {
    use super::OmfVec;

    #[test]
    fn basics() {
        let mut v = OmfVec::new();

        assert_eq!(v.len(), 0);

        v.push(1);
        assert_eq!(v.len(), 1);
        assert_eq!(v[1], 1);

        v[1] = 2;

        assert_eq!(v.len(), 1);
        assert_eq!(v[1], 2);

        assert_eq!(v.add(42), 2);
    }

    #[test]
    #[should_panic]
    fn index_zero_panics() {
        let mut v = OmfVec::new();

        assert_eq!(v.len(), 0);

        v.push(1);
        assert_eq!(v.len(), 1);

        v[0];
    }

    #[test]
    #[should_panic]
    fn out_of_range_index_panics() {
        let mut v = OmfVec::new();

        assert_eq!(v.len(), 0);

        v.push(1);
        assert_eq!(v.len(), 1);

        v[2];
    }

    #[test]
    fn iterate() {
        let mut v = OmfVec::new();

        v.push(1);
        v.push(2);
        v.push(3);

        let mut iter = v.iter();

        assert_eq!(iter.next(), Some(&1));
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), None);

    }
}