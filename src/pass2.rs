use crate::Args;
use crate::dosexe::DosExe;
use crate::linker_error::LinkerError;
use crate::linkstate::LinkState;
use crate::object::Object;
use crate::record::{Record, RecordType};

//
// Pass 2 logic
//

/// Execute pass 2. 
/// - Process all LEDATA, LIDATA, and FIXUPP records
/// - Build final executable.
/// 
pub fn pass2(state: &mut LinkState, objects: &mut Vec<Object>, args: &Args) -> Result<(), LinkerError> {
    //
    // Allocate the memory image.
    //
    let lastseg = &state.segments[*state.segment_order.last().unwrap()];
    let memsize = lastseg.base + lastseg.length;
    let mut image = Vec::new();

    image.resize(memsize, 0u8);

    //
    // Execute pass 2 on all object files
    //
    for obj in objects.iter_mut() {
        let data = obj.data.take().unwrap();
        pass2_object(state, &data, obj, &mut image)?;
        obj.data = Some(data);
    }

    let exe = DosExe::new(&image);

    exe.write(args.output.as_ref().unwrap())?;

    Ok(())
}

/// Handle one pass 2 object file.
/// 
fn pass2_object(state: &mut LinkState, data: &[u8], obj: &mut Object, image: &mut [u8]) -> Result<(), LinkerError> {
    let mut start = 0;

    while start < data.len() {
        let mut rec = Record::new(&data[start..])?;
        let reclen = rec.total_length();

        let result = match rec.rectype {
            //
            // Records already processed in pass 1
            //
            RecordType::THEADR |
            RecordType::EXTDEF |
            RecordType::COMENT |
            RecordType::PUBDEF |
            RecordType::LNAMES |
            RecordType::SEGDEF |
            RecordType::GRPDEF => Ok(()),
            
            //
            // These records are for pass 2.
            //
            RecordType::LEDATA => pass2_ledata(&mut rec, state, obj, image),
            RecordType::LIDATA |
            RecordType::FIXUPP => Ok(()),
            RecordType::MODEND => break,

            _ => Err(LinkerError::new(&format!("unhandled record {:?}", rec.rectype))),
        };

        match result { 
            Err(err) => {
                return Err(LinkerError::new(                    
                    &format!("pass2: module {} at {:05X}H: {}", obj.name, start, err.to_string())
                ));
            },
            Ok(_) => {},
        };

        start += reclen;
    }
    
    Ok(())
}

/// Handle an LEDATA record, which contains literal data to be copied into the final executable.
///
fn pass2_ledata(rec: &mut Record, state: &LinkState, obj: &Object, image: &mut [u8]) -> Result<(), LinkerError> {
    let segidx = rec.index()?;

    if segidx == 0 && !obj.segdefs.is_valid_index(segidx) {
        return Err(LinkerError::new(
            &format!("invalid segidx {} in LEDATA record.", segidx)
        ));
    }

    let segdef = &obj.segdefs[segidx];

    let offset = rec.word()?;
    let data = rec.rest();

    if offset as usize >= segdef.length || offset as usize + data.len() > segdef.length {
        return Err(LinkerError::new(
            &format!("invalid data range {:05X}H..{:05X}H in LEDATA record.", offset, offset as usize + data.len())
        ));    
    }

    let segment = &state.segments[segdef.segidx];
    let ioffset = segdef.base + segment.base + offset as usize;

    image[ioffset..ioffset+data.len()].copy_from_slice(&data);

    Ok(())
}