//
// A group of segments
//

pub struct Group {
    pub name: usize,
    members: Vec<usize>,
    pub base: usize,
}

pub struct Iter<'a> {
    vec: &'a [usize],
    index: usize
}

impl<'a> Iterator for Iter<'a> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.vec.len() {
            let value = self.vec[self.index];
            self.index += 1;
            Some(value)
        } else {
            None
        }
    }
}

impl Group {
    pub fn new(name: usize) -> Self {
        Group {
            name,
            members: Vec::new(),
            base: 0,
        }
    }

    pub fn has(&self, index: usize) -> bool {
        self.members.iter().find(|member| **member == index).is_some()
    }

    pub fn add(&mut self, index : usize) {
        if !self.has(index) {
            self.members.push(index);
        }
    }

    pub fn iter(&self) -> Iter {
        Iter{ vec: &self.members[..], index: 0 }
    }
}

#[cfg(test)]
mod test {
    use super::Group;

    #[test]
    fn basics() {
        let mut group = Group::new(1);

        assert_eq!(group.name, 1);

        group.add(1);
        group.add(2);
        group.add(2);

        assert_eq!(group.members.len(), 2);

        assert!(group.has(1));
        assert!(group.has(2));
        assert!(!group.has(3));
    }

    #[test]
    fn iterates() {
        let mut group = Group::new(1);

        assert_eq!(group.name, 1);

        group.add(1);
        group.add(2);
        group.add(2);
        group.add(3);

        let mut iter = group.iter();

        assert_eq!(iter.next(), Some(1));
        assert_eq!(iter.next(), Some(2));
        assert_eq!(iter.next(), Some(3));
        assert_eq!(iter.next(), None);
    }
}