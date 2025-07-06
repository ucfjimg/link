use crate::Args;
use crate::group::Group;
use crate::library::Library;
use crate::linker_error::LinkerError;
use crate::linkstate::LinkState;
use crate::object::Object;
use crate::record::{Record, RecordType};
use crate::segment::{Segment, SegDef, SegName, Align, Combine};
use crate::symbols::Symbol;

#[derive(PartialEq, Clone, Copy)]
struct LibraryModule {
    lib: usize,
    modpage: usize,
}

struct LibraryModules {
    mods: Vec<LibraryModule>,
}

impl LibraryModules {
    pub fn new() -> Self {
        LibraryModules { mods: Vec::new() }
    } 

    pub fn has(&self, module: LibraryModule) -> bool {
        self.mods.iter().find(|m| *m == &module).is_some()
    }

    pub fn add(&mut self, module: LibraryModule) {
        if !self.has(module) {
            self.mods.push(module);
        }
    }
}

//
// Pass 1 logic
//

/// Execute pass 1. 
/// - Parse all objects from the command line.
/// 
pub fn pass1(state: &mut LinkState, objects: &mut Vec<Object>, libs: &[Library], args: &Args) -> Result<(), LinkerError> {
    //
    // Execute pass 1 on all command line object files
    //
    for objname in args.objects.iter() {
        let mut obj = Object::from_filename(objname)?;
        let data = obj.data.take().unwrap();
        pass1_object(state, &data, &mut obj, objname.as_os_str().to_str().unwrap())?;
        obj.data = Some(data);
        objects.push(obj);
    }

    let mut mods = LibraryModules::new();

    loop {
        //
        // Walk the library list looking for undefined symbols.
        //
        let undefined = state.symbols.undefined_symbols();

        if undefined.is_empty() {
            break;
        }

        let mut still_undefined = Vec::new();

        let old_mods = mods.mods.len();

        for sym in undefined {
            for (libidx, lib) in libs.iter().enumerate() {
                let modpage = lib.find_symbol_in_dictionary(sym)?;
                if let Some(modpage) = modpage {
                    mods.add(LibraryModule { lib: libidx, modpage });
                } else {
                    still_undefined.push(sym);
                }             
            }
        }

        //
        // If there are still undefined symbols after we have search the libraries, then 
        // those symbols are never going to be defined and we can give up.
        //
        if !still_undefined.is_empty() {
            for sym in still_undefined.iter() {
                println!("undefined external: {}", sym);
            }
            return Err(LinkerError::new(&format!("{} undefined externals.", still_undefined.len())));
        }

        for module in mods.mods.iter().skip(old_mods) {
            let lib = &libs[module.lib];
            println!("add {} modpage {}", lib.name, module.modpage);

            let objname = format!("{}@{:x}", lib.name, module.modpage);

            let mut obj = libs[module.lib].extract_module(module.modpage)?;
            let data = obj.data.take().unwrap();
            pass1_object(state, &data, &mut obj, &objname)?;
            obj.data = Some(data);
            objects.push(obj);
        }
    }




    Ok(())
}

/// Handle a THEADR record, which names the object file.
/// 
fn pass1_theadr(obj: &mut Object, rec: &mut Record) -> Result<(), LinkerError> {
    obj.name = rec.counted_string()?;
    
    Ok(())
}

// Handle an EXTDEF record, which maps an index in the object module to a symbol
// name to be resolved elsewhere.
//
fn pass1_extdef(obj: &mut Object, state: &mut LinkState, rec: &mut Record) -> Result<(), LinkerError> {
    while !rec.end() {
        let name = rec.counted_string()?;

        //
        // there is an unused type index after every name.
        //
        rec.index()?;

        //
        // Put the symbol in the symbol table, if it isn't already there,
        // as an undefined reference.
        //
        let symbol = Symbol::Undefined;
        state.symbols.update(&name, symbol)?;

        //
        // The name goes in the object's external definitions.
        //
        obj.extdefs.add(name.clone());
    }

    Ok(())
}

// Handle a PUBDEF record, which defines a symbol with an offset in a segment and/or group.
//
fn pass1_pubdef(obj: &mut Object, state: &mut LinkState, rec: &mut Record) -> Result<(), LinkerError> {
    let group = rec.index()?;
    let segment = rec.index()?;
    let frame = if segment == 0 { rec.word()? } else { 0 };

    if !obj.grpdefs.is_valid_index(group) {
        println!("grpdefs {:?}", obj.grpdefs);

        return Err(LinkerError::new(
            &format!("invalid group index {} in PUBDEF", group)
        ));
    }

    let group = obj.grpdefs.get(group);

    let segment = if segment == 0 {
        0
    } else {
        if segment != 0 && !obj.segdefs.is_valid_index(segment) {
            return Err(LinkerError::new(
                &format!("invalid segment index {} in PUBDEF", segment)
            ));
        }
        obj.segdefs[segment].segidx
    };

    while !rec.end() {
        let name = rec.counted_string()?;
        let offset = rec.word()?;

        //
        // There is an unused type index after each symbol.
        //
        rec.index()?;

        let symbol = Symbol::public(group, segment, frame, offset);
        state.symbols.update(&name, symbol)?;
    }

    Ok(())
}

/// Handle an LNAMES record, which lists names used by other records. All LNAMES are
/// stored in a global table, and each object contains a map from the object-based
/// index of the name to its index in the global table.
/// 
fn pass1_lnames(obj: &mut Object, state: &mut LinkState, rec: &mut Record) -> Result<(), LinkerError> {
    while !rec.end() {
        let lname = rec.counted_string()?;
        let index = state.lnames.find_or_add(&lname);
        obj.lnames.add(index);
    }
    
    Ok(())
}

/// Process a SEGDEF record. Complete segments are held at the linker level, and object modules
/// contain the bounds of the segment owned by the module.
/// 
fn pass1_segdef(obj: &mut Object, state: &mut LinkState, rec: &mut Record) -> Result<(), LinkerError> {
    let acbp = rec.byte()?;

    let align = Align::from_acbp(acbp)?;
    let combine = Combine::from_acbp(acbp)?;

    if align == Align::Absolute {
        //
        // We do not support it, but if align is Absolute, there are an absolute frame and offset.
        //
        let _frame = rec.word()?;
        let _offset = rec.byte()?;

        eprintln!("warning: segment has unsupported absolute aligment in module {}.", obj.name);
    }

    let length = rec.word()?;

    //
    // Name indices in the object file's lnames table.
    //
    let nameidx = rec.index()?;
    let classidx = rec.index()?;
    let ovlyidx = rec.index()?;

    if !(obj.lnames.is_valid_index(nameidx) && obj.lnames.is_valid_index(classidx) && obj.lnames.is_valid_index(ovlyidx)) {
        return Err(LinkerError::new(
            &format!("invalid name triplet {}.{}.{} for SEGDEF", nameidx, classidx, ovlyidx)
        ));
    }

    let nameidx = obj.lnames.get(nameidx);
    let classidx = obj.lnames.get(classidx);
    let ovlyidx = obj.lnames.get(ovlyidx);

    let segname = SegName::new(nameidx, classidx, ovlyidx);

    let bbit = (acbp & 0x01) != 0;

    let length = if bbit {
        if length != 0 {
            let segname = state.segname(&segname);
            return Err(LinkerError::new(&format!("{} has B bit set, but length is not set to zero.", segname)));
        } else {
            0x10000
        }
    } else {
        length as usize
    };

    //
    // Get or add the linker-level segment.
    //
    let index = if let Some(index) = state.get_segment_named(&segname) {
        index
    } else {
        let segment = Segment::new(segname, 0, align, combine);
        state.segments.add(segment)
    };

    let mut segdef = SegDef::new(index, length, align, combine);

    segdef.base = state.segments[index].add_segdef(&segdef)?;

    
    obj.segdefs.add(segdef);

    Ok(())
}

/// Process a GRPDEF record. Map the group in the object to the (possibly just created) group
/// in the linker, and make sure the linker state contains all of the referenced segments.
/// 
fn pass1_grpdef(obj: &mut Object, state: &mut LinkState, rec: &mut Record) -> Result<(), LinkerError> {
    let nameidx = rec.index()?;

    if !(obj.lnames.is_valid_index(nameidx)) {
        return Err(LinkerError::new(
            &format!("invalid name {} for GRPDEF", nameidx)
        ));
    }

    let nameidx = obj.lnames.get(nameidx);

    //
    // Get or add the linker-level group.
    //
    let index = if let Some(index) = state.get_group_named(nameidx) {
        index
    } else {
        let group = Group::new(nameidx);
        let index = state.groups.add(group);
        index
    };

    obj.grpdefs.add(index);
    let group = &mut state.groups[index];

    //
    // Walk the rest of the segments to add to the group.
    //
    while !rec.end() {
        //
        // There is an unused type field before every segment.
        //
        rec.byte()?;

        let segidx = rec.index()?;

        if !obj.segdefs.is_valid_index(segidx) {
            return Err(LinkerError::new(
                &format!("invalid segment index {} in GRPDEF", segidx)
            ));
        }
        
        //
        // Get the linker-level segment index.
        //
        let segidx = obj.segdefs[segidx].segidx;

        group.add(segidx);
    }

    Ok(())
}

/// Parse one object file in the context of pass 1. 
///
fn pass1_object(state: &mut LinkState, data: &[u8], obj: &mut Object, name: &str) -> Result<(), LinkerError> {
    let mut start = 0;

    while start < data.len() {
        let mut rec = Record::new(&data[start..])?;
        let reclen = rec.total_length();

        let result = match rec.rectype {
            RecordType::THEADR => pass1_theadr(obj, &mut rec),
            RecordType::EXTDEF => pass1_extdef(obj, state, &mut rec),
            RecordType::COMENT => Ok(()),
            RecordType::PUBDEF => pass1_pubdef(obj, state, &mut rec),
            RecordType::LNAMES => pass1_lnames(obj, state, &mut rec),
            RecordType::SEGDEF => pass1_segdef(obj, state, &mut rec),
            RecordType::GRPDEF => pass1_grpdef(obj, state, &mut rec),
            RecordType::MODEND => break,
            
            //
            // These records are for pass 2.
            //
            RecordType::LEDATA |
            RecordType::LIDATA |
            RecordType::FIXUPP => Ok(()),

            _ => Err(LinkerError::new(&format!("unhandled record {:?}", rec.rectype))),
        };

        match result { 
            Err(err) => {
                let modname = if obj.name.is_empty() { "".to_owned() } else { format!(" (module {})", obj.name) };

                return Err(LinkerError::new(                    
                    &format!("pass1: module {}{} at {:05X}H: {}", name, modname, start, err.to_string())
                ));
            },
            Ok(_) => {},
        };

        start += reclen;
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use crate::group::Group;
    use crate::symbols::Symbol;

    use super::*;

    #[test]
    fn test_theadr() -> Result<(), LinkerError> {
        let rec = [ 0x80, 0x05, 0x00, 0x03, 0x41, 0x42, 0x43, 0x00 ];
        let mut rec = Record::new(&rec)?;

        let mut obj = Object::new();
        assert_eq!(obj.name, "");
        pass1_theadr(&mut obj, &mut rec)?;
        assert_eq!(obj.name, "ABC");

        let rec = [ 0x80, 0x04, 0x00, 0x03, 0x41, 0x42, 0x43 ];
        let mut rec = Record::new(&rec)?;

        let mut obj = Object::new();
        assert_eq!(obj.name, "");
        assert!(pass1_theadr(&mut obj, &mut rec).is_err());

        Ok(())
    }
    
    #[test]
    fn test_lnames() -> Result<(), LinkerError> {
        let rec = [ 0x96, 0x09, 0x00, 0x03, 0x41, 0x42, 0x43, 0x03, 0x44, 0x45, 0x46, 0x00 ];
        let mut rec = Record::new(&rec)?;

        let mut obj = Object::new();
        let mut state: LinkState = LinkState::new();

        //
        // Force indices in the object to not be the same as the global state
        //
        state.lnames.add("XYZ");

        pass1_lnames(&mut obj, &mut state, &mut rec)?;

        assert_eq!(obj.lnames.len(), 2);
        assert_eq!(obj.lnames.get(1), 2);
        assert_eq!(obj.lnames.get(2), 3);

        assert_eq!(state.lnames.len(), 3);
        assert_eq!(state.lnames.get(1), "XYZ");
        assert_eq!(state.lnames.get(2), "ABC");
        assert_eq!(state.lnames.get(3), "DEF");

        Ok(())
    }

    #[test]
    fn test_segdef_new() -> Result<(), LinkerError> {
        //                                        PARA PUB     len=0x024f  name  class ovly
        let rec = [ 0x98, 0x07, 0x00, 0b011_010_00, 0x4f, 0x02, 0x01, 0x02, 0x00, 0x00 ]; 
        let mut rec = Record::new(&rec)?;

        let mut obj = Object::new();
        let mut state: LinkState = LinkState::new();

        state.lnames.add("");

        obj.lnames.add(state.lnames.add("_TEXT"));     // index 1 == _TEXT
        obj.lnames.add(state.lnames.add("CODE"));      // index 2 == CODE

        let seg = Segment::new(SegName::new(0, 0, 0), 0, Align::Byte, Combine::Private);

        // Make sure segment we add is at index 2 in the global list
        //
        state.segments.push(seg);

        pass1_segdef(&mut obj, &mut state, &mut rec)?;

        assert_eq!(state.segments.len(), 2);
        assert_eq!(obj.segdefs.len(), 1);

        let segdef = &obj.segdefs[1];

        assert_eq!(segdef.segidx, 2);
        assert_eq!(segdef.base, 0);
        assert_eq!(segdef.length, 0x24f);
        assert_eq!(segdef.align, Align::Para);
        assert_eq!(segdef.combine, Combine::Public);

        let segment = &state.segments[2];

        assert_eq!(segment.length, 0x24f);
        assert_eq!(state.segname(&segment.name), "_TEXT.CODE.");

        Ok(())
    }

    #[test]
    fn test_segdef_combine() -> Result<(), LinkerError> {
        //                                        PARA PUB     len=0x024f  name  class ovly
        let rec = [ 0x98, 0x07, 0x00, 0b011_010_00, 0x4f, 0x02, 0x01, 0x02, 0x00, 0x00 ]; 
        let mut rec = Record::new(&rec)?;

        let mut obj = Object::new();
        let mut state: LinkState = LinkState::new();

        obj.lnames.add(state.lnames.add("_TEXT"));     // index 1 == _TEXT
        obj.lnames.add(state.lnames.add("CODE"));      // index 2 == CODE

        let seg = Segment::new(SegName::new(0, 0, 0), 0, Align::Byte, Combine::Private);

        // Make sure segment we add is at index 2 in the global list
        //
        state.segments.push(seg);

        let seg = Segment::new(SegName::new(1, 2, 0), 0x100, Align::Byte, Combine::Public);

        // We should combine with this segment
        //
        state.segments.push(seg);

        pass1_segdef(&mut obj, &mut state, &mut rec)?;

        assert_eq!(state.segments.len(), 2);
        assert_eq!(obj.segdefs.len(), 1);

        let segdef = &obj.segdefs[1];

        assert_eq!(segdef.segidx, 2);
        assert_eq!(segdef.base, 0x100);
        assert_eq!(segdef.length, 0x24f);
        assert_eq!(segdef.align, Align::Para);
        assert_eq!(segdef.combine, Combine::Public);

        let segment = &state.segments[2];

        assert_eq!(segment.length, 0x34f);

        Ok(())
    }

    #[test]
    fn grpdef_new() -> Result<(), LinkerError> {
        //                                     NAME  ---   SEG0  ---   SEG1  
        let rec = [ 0x9a, 0x06, 0x00, 0x04, 0xFF, 0x01, 0xFF, 0x02, 0xff ];
        let mut rec = Record::new(&rec)?;

        let mut obj = Object::new();
        let mut state: LinkState = LinkState::new();

        state.lnames.add("");

        obj.lnames.add(state.lnames.add("_TEXT"));     // index 1 == _TEXT
        obj.lnames.add(state.lnames.add("CODE"));      // index 2 == CODE
        obj.lnames.add(state.lnames.add("DATA"));      // index 3 == DATA
        obj.lnames.add(state.lnames.add("DGROUP"));    // index 4 == DRGROUP

        let seg = Segment::new(SegName::new(0, 0, 0), 0, Align::Byte, Combine::Private);
        state.segments.push(seg);

        let seg = Segment::new(SegName::new(1, 2, 0), 0, Align::Byte, Combine::Public);
        let index = state.segments.add(seg);

        let segdef = SegDef::new(index, 100, Align::Byte, Combine::Public); 
        obj.segdefs.push(segdef);

        let seg = Segment::new(SegName::new(3, 3, 0), 0, Align::Byte, Combine::Public);
        let index = state.segments.add(seg);

        let segdef = SegDef::new(index, 200, Align::Byte, Combine::Public); 
        obj.segdefs.push(segdef);

        assert_eq!(obj.grpdefs.len(), 0);

        pass1_grpdef(&mut obj, &mut state, &mut rec)?;

        assert_eq!(obj.grpdefs.len(), 1);
        assert_eq!(obj.grpdefs.get(1), 1);

        assert_eq!(state.groups.len(), 1);

        assert!(state.groups[1].has(2));
        assert!(state.groups[1].has(3));

        Ok(())
    }

    #[test]
    fn grpdef_add() -> Result<(), LinkerError> {
        //                                     NAME  ---   SEG0  ---   SEG1  
        let rec = [ 0x9a, 0x06, 0x00, 0x04, 0xFF, 0x01, 0xFF, 0x02, 0xff ];
        let mut rec = Record::new(&rec)?;

        let mut obj = Object::new();
        let mut state: LinkState = LinkState::new();

        state.lnames.add("");

        obj.lnames.add(state.lnames.add("_TEXT"));     // index 1 == _TEXT
        obj.lnames.add(state.lnames.add("CODE"));      // index 2 == CODE
        obj.lnames.add(state.lnames.add("DATA"));      // index 3 == DATA
        obj.lnames.add(state.lnames.add("DGROUP"));    // index 4 == DRGROUP

        let seg = Segment::new(SegName::new(0, 0, 0), 0, Align::Byte, Combine::Private);
        state.segments.push(seg);

        let seg = Segment::new(SegName::new(1, 2, 0), 0, Align::Byte, Combine::Public);
        let index = state.segments.add(seg);

        let segdef = SegDef::new(index, 100, Align::Byte, Combine::Public); 
        obj.segdefs.push(segdef);

        let seg = Segment::new(SegName::new(3, 3, 0), 0, Align::Byte, Combine::Public);
        let index = state.segments.add(seg);

        let segdef = SegDef::new(index, 200, Align::Byte, Combine::Public); 
        obj.segdefs.push(segdef);

        let mut group = Group::new(5);
        group.add(1);

        state.groups.add(group);

        pass1_grpdef(&mut obj, &mut state, &mut rec)?;

        assert_eq!(state.groups.len(), 1);

        assert!(state.groups[1].has(1));
        assert!(state.groups[1].has(2));
        assert!(state.groups[1].has(3));

        Ok(())
    }

    #[test]
    fn extdef() -> Result<(), LinkerError> {
        let rec = [ 
            0x9a, 0x0b, 0x00, 
            0x03, 0x41, 0x42, 0x43, 0x02, 
            0x03, 0x44, 0x45, 0x46, 0x02, 
            0xff ];
        let mut rec = Record::new(&rec)?;

        let mut obj = Object::new();
        let mut state: LinkState = LinkState::new();

        pass1_extdef(&mut obj, &mut state, &mut rec)?;

        assert_eq!(obj.extdefs.len(), 2);
        assert_eq!(obj.extdefs[1], "ABC");
        assert_eq!(obj.extdefs[2], "DEF");

        //
        // EXTDEF's also get put into the symbol table as undefined's
        //
        let symbol = state.symbols.symbols.get("ABC");

        match symbol {
            Some(Symbol::Undefined) => {},
            None => panic!("Symbol was not added as undefined."),
            _ => panic!("Symbol was added as something else {:?}", symbol),
        }

        Ok(())
    }

    #[test]
    fn pubdefs() -> Result<(), LinkerError> {
        let rec = [ 
            0x9a, 0x11, 0x00,
            0x01,                           // base group index
            0x02,                           // base segment index
            0x03, 0x41, 0x42, 0x43, 0x34, 0x12, 0x02, 
            0x03, 0x44, 0x45, 0x46, 0x78, 0x56, 0x02, 
            0xff ];
        let mut rec = Record::new(&rec)?;

        let mut obj = Object::new();
        let mut state: LinkState = LinkState::new();

        state.lnames.add("");
        obj.lnames.add(state.lnames.add("_TEXT"));     // index 1 == _TEXT
        obj.lnames.add(state.lnames.add("CODE"));      // index 2 == CODE
        obj.lnames.add(state.lnames.add("DATA"));      // index 3 == DATA
        obj.lnames.add(state.lnames.add("DGROUP"));    // index 4 == DRGROUP
        obj.lnames.add(state.lnames.add("ZGROUP"));    // index 5 == ZRGROUP


        let group = Group::new(5);
        state.groups.add(group);

        let group = Group::new(4);
        obj.grpdefs.add(state.groups.add(group));

        let segment = Segment::new(SegName::new(1, 2, 0), 0, Align::Byte, Combine::Public);
        state.segments.add(segment);
        
        let segment = Segment::new(SegName::new(1, 2, 0), 0, Align::Byte, Combine::Public);
        let segidx = state.segments.add(segment);
        obj.segdefs.add(SegDef::new(segidx, 100, Align::Byte, Combine::Public));

        let segment = Segment::new(SegName::new(3, 3, 0), 0, Align::Byte, Combine::Public);
        let segidx = state.segments.add(segment);
        obj.segdefs.add(SegDef::new(segidx, 100, Align::Byte, Combine::Public));

        pass1_pubdef(&mut obj, &mut state, &mut rec)?;

        let symbol = state.symbols.symbols.get("ABC");

        match symbol {
            Some(Symbol::Public(public)) => {
                assert_eq!(public.segment, 3);
                assert_eq!(public.group, 2);
                assert_eq!(public.frame, 0);
                assert_eq!(public.offset, 0x1234);
            },
            None => panic!("Symbol ABC not added"),
            _ => panic!("Symbol ABC had an invalid value {:?}", symbol)
        }
        

        let symbol = state.symbols.symbols.get("DEF");

        match symbol {
            Some(Symbol::Public(public)) => {
                assert_eq!(public.segment, 3);
                assert_eq!(public.group, 2);
                assert_eq!(public.frame, 0);
                assert_eq!(public.offset, 0x5678);
            },
            None => panic!("Symbol DEF not added"),
            _ => panic!("Symbol DEF had an invalid value {:?}", symbol)
        }

        Ok(())
    }
}
