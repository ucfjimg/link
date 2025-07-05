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

fn pass1_theadr(obj: &mut Object, rec: &mut Record) -> Result<(), LinkerError> {
    obj.name = rec.counted_string()?;
    
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
    
}