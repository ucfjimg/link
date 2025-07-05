use crate::Args;
use crate::linker_error::LinkerError;
use crate::linkstate::LinkState;
use crate::object::Object;
use crate::record::{Record, RecordType};

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
        let index = state.lnames.add(lname);
        obj.lnames.push(index);
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
        state.lnames.push("XYZ".to_owned());

        pass1_lnames(&mut obj, &mut state, &mut rec)?;

        assert_eq!(obj.lnames.len(), 2);
        assert_eq!(obj.lnames[1], 2);
        assert_eq!(obj.lnames[2], 3);

        assert_eq!(state.lnames.len(), 3);
        assert_eq!(state.lnames[1], "XYZ");
        assert_eq!(state.lnames[2], "ABC");
        assert_eq!(state.lnames[3], "DEF");

        Ok(())
    }
}