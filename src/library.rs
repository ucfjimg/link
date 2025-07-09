use std::path::PathBuf;
use crate::linker_error::LinkerError;
use crate::object::Object;
use crate::record::{Record, RecordType};

#[cfg(test)]
use std::collections::{HashSet, VecDeque};

///
/// Operations on an OMF library file
///
/// A library file is a collection of objects, plus some metadata. Key
/// concepts:
///
/// - Every library has a power-of-2 page size, which is stored in the
///   library header. All object modules start on a page boundary.
///   In the other metdadata the modules are referred to by their
///   page number. The page size is picked so that the page number fits
///   in 16 bits.
///
/// - The optional dictionary maps symbol names to a module. It is a
///   2-level hash table that starts after the last object module.
///
/// - The extended dictionary is really a graph. The nodes are object
///   modules, and the edges are dependencies; so each node has edges
///   to modules it needs. This is faster than searching for inter-
///   library dependencies by symbol name, which would require parsring
///   every object module.
///

const DICT_BLOCK_SIZE: usize = 512;
const BLOCK_BUCKETS: usize = 37;

/// If the library contains an optional dictionary (almost all surviving
/// libraries do), the bounds of the dictionary in the library file.
///
#[derive(Copy, Clone, Debug)]
pub struct Dictionary {
    offset: usize,
    block: usize,
}

/// The location, if it exists, of the extended dictionary.
///
#[derive(Copy, Clone, Debug)]
pub struct ExtDict {
    _offset: usize,
    _nodecount: usize,
}

/// An OMF library file.
///
#[derive(Debug)]
pub struct Library {
    pub name: String,
    pub page_size: usize,
    pub dictionary: Option<Dictionary>,
    pub _extdict: Option<ExtDict>,
    pub case_sensitive: bool,
    pub data: Vec<u8>,
}

/// The results of hashing a symbol name.
///
#[derive(Debug)]
struct DictHash {
    block: usize,
    bucket: usize,
    block_delta: usize,
    bucket_delta: usize
}

impl Library {
    /// Read a library from a file.
    ///
    pub fn new(name: &str, path: PathBuf) -> Result<Self, LinkerError> {
        //
        // 16-bit libraries are tiny compared to modern memory, so it's more
        // efficient to just read the entire thing in and parse it in memory.
        //
        let data = std::fs::read(&path)?;
        Self::from_data(data, name)
    }

    pub fn from_data(data: Vec<u8>, name: &str) -> Result<Self, LinkerError> {
        //
        // Check the header record.
        //
        let mut rec = Record::new(&data)?;

        if rec.rectype != RecordType::LIBHDR {
            return Err(LinkerError::new(&format!("`{}` is not a library file.", name)));
        }

        let page_size = rec.total_length();
        let dict_offset = rec.dword()? as usize;
        let dict_blocks = rec.word()? as usize;

        let dictionary = if dict_offset != 0 { Some(Dictionary{offset: dict_offset, block: dict_blocks}) } else { None };

        //
        // If the optional dictionary exists, try for the extended dicationary.
        //
        let mut extdict = None;

        if let Some(dict) = dictionary.as_ref() {
            let offset = dict.offset + dict.block * DICT_BLOCK_SIZE;

            if offset + 3 <= data.len() && data[offset] == 0xF2 {
                let enclen = &data[offset + 1..offset + 3];
                let extlen = u16::from_le_bytes([enclen[0], enclen[1]]) as usize;

                if offset + 3 + extlen <= data.len() && extlen > 2 {
                    let encnodes = &data[offset + 3..offset + 5];
                    let nodecount = u16::from_le_bytes([encnodes[0], encnodes[1]]) as usize;
                    let offset = offset + 5;

                    extdict = Some(ExtDict{_offset: offset, _nodecount: nodecount});
                }
            }
        }

        Ok(Library{
            name: name.to_owned(),
            page_size,
            dictionary,
            _extdict: extdict,
            case_sensitive: false,
            data
        })
    }

    /// Extract an object module, by module page, from the library.
    ///
    pub fn extract_module(&self, modpage: usize) -> Result<Object, LinkerError> {
        //
        // NB there are no doubt more efficient ways to do this. But for now this works.
        //
        let modstart = modpage * self.page_size;
        let mut modend = modstart;

        let rec = &self.data[modend..];
        let mut header = match Record::new(rec) {
            Ok(header) => header,
            Err(err) => return Err(LinkerError::new(&format!("page {} in library {} is not a module: {}", modpage, self.name, err))),
        };

        if header.rectype != RecordType::THEADR {
            return Err(LinkerError::new(&format!("page {} in library {} is not a module (missing THEADR)", modpage, self.name)));
        }

        let _modname = match header.counted_string() {
            Ok(name) => name,
            Err(err) => return Err(LinkerError::new(&format!("page {} in library {} is not a module: {}", modpage, self.name, err))),
        };

        modend += header.total_length();

        loop {
            let rec = &self.data[modend..];
            let rec = match Record::new(rec) {
                Ok(rec) => rec,
                Err(err) => return Err(LinkerError::new(&format!("page {} in library {} is not a module: {}", modpage, self.name, err))),
            };

            modend += rec.total_length();

            if rec.rectype == RecordType::MODEND {
                break;
            }
        };

        let contents = self.data[modstart..modend].to_vec();

        Ok(Object::from_bytes(contents))
    }

    /// Compute the hash for a symbol. Libraries use a two-level hashing scheme -
    /// one level to select the block, and another to select the bucket within
    /// the block. So the hash algorithm must produce four values: a starting block
    /// and bucket, and a delta for each. The number of blocks and the number of
    /// buckets are both always prime, so any non-zero delta will walk a cycle
    /// containing all blocks/buckets before repeating.
    ///
    fn hash_symbol(&self, symbol: &str) -> Option<DictHash> {
        match &self.dictionary {
            None => None,
            Some(dict) => {
                const BLANK: u16 = 0x20;

                //
                // NB in general, this is a bad idea in Rust, but in OMF files all
                // the identifiers are ASCII.
                //
                let bytes = symbol.as_bytes();
                let mut len = bytes.len() as u16;
                let mut forward = 0;
                let mut backward = len as usize;

                let mut block = (len | BLANK) as u16;
                let mut bucket_delta = block;
                let mut block_delta = 0;
                let mut bucket = 0;

                let rotr2 = |x: u16| (x >> 2) | (x << 14);
                let rotl2 = |x: u16| (x << 2) | (x >> 14);

                loop {
                    backward -= 1;
                    let cback = bytes[backward] as u16 | BLANK;
                    bucket = rotr2(bucket) ^ cback;
                    block_delta = rotl2(block_delta) ^ cback;

                    len -= 1;
                    if len == 0 {
                        break;
                    }

                    let cfront = bytes[forward] as u16 | BLANK;
                    block = rotl2(block) ^ cfront;
                    bucket_delta = rotr2(bucket_delta) ^ cfront;

                    forward += 1;
                }

                block %= dict.block as u16;
                block_delta %= dict.block as u16;
                bucket %= BLOCK_BUCKETS as u16;
                bucket_delta %= BLOCK_BUCKETS as u16;

                block_delta = if block_delta == 0 { 1 } else { block_delta };
                bucket_delta = if bucket_delta == 0 { 1 } else { bucket_delta };

                Some(DictHash{
                    block: block as usize,
                    bucket: bucket as usize,
                    block_delta: block_delta as usize,
                    bucket_delta: bucket_delta as usize,
                })
            }
        }
    }

    /// Find a symbol, by name, in the library. Currently the library must contain
    /// an optional dictionary. On success returns the page number of the start of
    /// the library which provides the given symbol.
    ///
    pub fn find_symbol_in_dictionary(&self, symbol: &str) -> Result<Option<usize>, LinkerError> {
        let hash = self.hash_symbol(symbol).unwrap_or_else(|| panic!("library {} does not have a dictionary", self.name));
        let dict = self.dictionary.unwrap();

        let mut block = hash.block;

        loop {
            let block_offset = dict.offset + block * DICT_BLOCK_SIZE;

            let mut bucket = hash.bucket;
            loop {
                let index = self.data[block_offset + bucket] as usize * 2;
                if index != 0 {
                    if index >= DICT_BLOCK_SIZE {
                        return Err(LinkerError::new("invalid linker dict block: index out of bounds"));
                    }

                    let count = self.data[block_offset + index] as usize;
                    let index = index + 1;

                    //
                    // String + word of offset
                    //
                    if index + count + 2  > DICT_BLOCK_SIZE {
                        return Err(LinkerError::new("invalid linker dict block: index out of bounds"));
                    }

                    let text = &self.data[block_offset + index..block_offset + index + count];

                    let key =match std::str::from_utf8(text) {
                        Ok(text) => text.to_string(),
                        Err(err) => return Err(LinkerError::new(&format!("invalid counted string in linker dict: {}", err))),
                    };

                    let success = if self.case_sensitive {
                        key == symbol
                    } else {
                        key.to_lowercase() == symbol.to_lowercase()
                    };

                    if success {
                        let index = index + count;
                        let page = &self.data[block_offset + index..block_offset + index + 2];
                        let page = u16::from_le_bytes([page[0], page[1]]) as usize;
                        return Ok(Some(page));
                    }
                }

                bucket = (bucket + hash.bucket_delta) % BLOCK_BUCKETS;
                if bucket == hash.bucket {
                    break;
                }
            }

            //
            // If the block wasn't full, then we can safely stop with failure.
            //
            let next_free = self.data[block_offset + BLOCK_BUCKETS];
            if next_free != 0xff {
                break;
            }

            block = (block + hash.block_delta) % dict.block;
            if block == hash.block {
                break;
            }
        }

        Ok(None)
    }

    /// Retrieve a list of depdenent modules for a module. Both `modpage` and the returned
    /// modules are by module page number.
    ///
    #[cfg(test)]
    pub fn get_module_dependencies(&self, modpage: usize) -> Result<Vec<usize>, LinkerError> {
        let mut dependencies = Vec::new();

        if let Some(extdict) = self._extdict.as_ref() {
            let mut low = 0;
            let mut high = extdict._nodecount - 1;

            let depsoffs = loop {
                if low > high {
                    break None;
                }
                let mid = (low + high) / 2;
                let offset = extdict._offset + mid * 4;
                let node = &self.data[offset..offset + 4];

                let midmodpage = u16::from_le_bytes([node[0], node[1]]) as usize;

                if midmodpage == modpage {
                    let depsoffs = u16::from_le_bytes([node[2], node[3]]) as usize;
                    break Some(depsoffs);
                }

                if modpage > midmodpage {
                    low = mid + 1;
                } else {
                    high = mid - 1;
                }
            };

            if let Some(depsoffs) = depsoffs {
                let depsoffs = depsoffs + extdict._offset;

                if depsoffs + 2 > self.data.len() {
                    return Err(LinkerError::new("invalid linker extdict: index out of bounds"));
                }

                let count = &self.data[depsoffs..depsoffs + 2];
                let count = u16::from_le_bytes([count[0], count[1]]) as usize;

                let depsoffs = depsoffs + 2;
                if depsoffs + count * 2 > self.data.len() {
                    return Err(LinkerError::new("invalid linker extdict: index out of bounds"));
                }

                for i in 0..count {
                    let nodeoffs = depsoffs + i * 2;
                    let depsnode = &self.data[nodeoffs..nodeoffs + 2];
                    let depsnode = u16::from_le_bytes([depsnode[0], depsnode[1]]) as usize;

                    if depsnode >= extdict._nodecount {
                        return Err(LinkerError::new("invalid linker extdict: node out of bounds"));
                    }

                    let nodeoffs = extdict._offset + depsnode * 4;
                    let node = &self.data[nodeoffs..nodeoffs + 2];
                    let modpage = u16::from_le_bytes([node[0], node[1]]) as usize;

                    dependencies.push(modpage);
                }
            }
        }

        Ok(dependencies)
    }

    /// Retrieve a list of all module dependencies for a module. `get_module_dependencies`
    /// fetches the direct dependencies of a module from the extended dictionary graph;
    /// however, that data does not include transitive dependencies. This routine will
    /// search the graph to find all (even indirect) dependencies.
    ///
    #[cfg(test)]
    pub fn get_all_module_dependencies(&self, modstart: usize) -> Result<Vec<usize>, LinkerError> {
        let mut alldeps = HashSet::new();
        let mut depqueue = VecDeque::new();
        depqueue.push_back(modstart);

        loop {
            let dep = match depqueue.pop_front() {
                Some(d) => d,
                None => break,
            };

            if alldeps.contains(&dep) {
                continue;
            }

            alldeps.insert(dep);

            let deps = self.get_module_dependencies(dep)?;
            for dep in deps {
                depqueue.push_back(dep);
            }
        };

        let mut alldeps = alldeps.into_iter().collect::<Vec<usize>>();
        alldeps.sort();

        Ok(alldeps)
    }
}

#[cfg(test)] 
mod test {
    use super::Library;
    use crate::linker_error::LinkerError;
    use crate::testlib::{get_testlib, MOD1_PAGE, MOD2_PAGE};
    use crate::record::{Record, RecordType};

    #[test]
    fn basic_parsing() -> Result<(), LinkerError> {
        Library::from_data(get_testlib(), "testlib")?;
        Ok(())
    }

    #[test]
    fn find_dependencies() -> Result<(), LinkerError> {
        let lib = Library::from_data(get_testlib(), "testlib")?;

        assert_eq!(lib.get_all_module_dependencies(MOD1_PAGE)?, Vec::from([MOD1_PAGE]));
        assert_eq!(lib.get_all_module_dependencies(MOD2_PAGE)?, Vec::from([MOD1_PAGE, MOD2_PAGE]));

        Ok(())
    }

    #[test]
    fn find_symbols() -> Result<(), LinkerError> {
        let lib = Library::from_data(get_testlib(), "testlib")?;

        assert_eq!(lib.find_symbol_in_dictionary("FOO")?, Some(MOD1_PAGE));
        assert_eq!(lib.find_symbol_in_dictionary("BAR")?, None);

        Ok(())
    }

    #[test]
    fn extract_object() -> Result<(), LinkerError> {
        let lib = Library::from_data(get_testlib(), "testlib")?;

        let mut obj = lib.extract_module(MOD1_PAGE)?;
        let data = obj.data.take().unwrap();
        let mut rec = Record::new(&data[..])?;

        assert_eq!(rec.rectype, RecordType::THEADR);
        assert_eq!(rec.counted_string()?, "mod1.ASM");
        
        Ok(())
    } 
}
