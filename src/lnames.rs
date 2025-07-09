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

    pub fn find_or_add(&mut self, name: &str) -> usize {
        match self.names.iter().enumerate().find(|(_, n)| *n == name) {
            Some((x, _)) => x+1,
            None => self.add(name)
        }
    }

    #[cfg(test)]
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

    #[test]
    fn find_or_add() {
        let mut lnames = LNames::new();

        assert_eq!(lnames.len(), 0);
        assert_eq!(lnames.add("ABC"), 1);
        assert_eq!(lnames.len(), 1);

        assert_eq!(lnames.find_or_add("ABC"), 1);
        assert_eq!(lnames.len(), 1);
        assert_eq!(lnames.find_or_add("DEF"), 2);
        assert_eq!(lnames.len(), 2);
    }
}
