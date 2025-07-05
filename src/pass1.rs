use crate::Args;
use crate::group::Group;
use crate::linker_error::LinkerError;
use crate::linkstate::LinkState;
use crate::object::Object;
use crate::record::{Record, RecordType};
use crate::segment::{Segment, SegDef, SegName, Align, Combine};

//
// Pass 1 logic
//

/// Execute pass 1. 
/// - Parse all objects from the command line.
/// 
pub fn pass1(state: &mut LinkState, objects: &mut Vec<Object>, args: &Args) -> Result<(), LinkerError> {
    for objname in args.objects.iter() {
        let mut obj = Object::from_filename(objname)?;
        let data = obj.data.take().unwrap();
        pass1_object(state, &data, &mut obj, objname.as_os_str().to_str().unwrap())?;
        obj.data = Some(data);
        objects.push(obj);
    }

    Ok(())
}

/// Handle a THEADR record, which names the object file.
/// 
fn pass1_theadr(obj: &mut Object, rec: &mut Record) -> Result<(), LinkerError> {
    obj.name = rec.counted_string()?;
    
    Ok(())
}

/// Handle an LNAMES record, which lists names used by other records. All LNAMES are
/// stored in a global table, and each object contains a map from the object-based
/// index of the name to its index in the global table.
/// 
fn pass1_lnames(obj: &mut Object, state: &mut LinkState, rec: &mut Record) -> Result<(), LinkerError> {
    while !rec.end() {
        let lname = rec.counted_string()?;
        let index = state.lnames.add(&lname);
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
        state.groups.add(group)
    };

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

        match rec.rectype {
            RecordType::THEADR => pass1_theadr(obj, &mut rec)?,
            RecordType::COMENT => {},
            RecordType::LNAMES => pass1_lnames(obj, state, &mut rec)?,
            RecordType::SEGDEF => pass1_segdef(obj, state, &mut rec)?,
            RecordType::GRPDEF => pass1_grpdef(obj, state, &mut rec)?,
            RecordType::MODEND => break,
            
            //
            // These records are for pass 2.
            //
            RecordType::LEDATA |
            RecordType::LIDATA |
            RecordType::FIXUPP => {},

            _ => eprintln!("pass1: {}: unhandled record {:?} at offset {:05X}H", name, rec.rectype, start),
        }


        start += reclen;
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use crate::group::Group;

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

        pass1_grpdef(&mut obj, &mut state, &mut rec)?;

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
}
