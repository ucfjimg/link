use crate::Args;
use crate::dosexe::DosExe;
use crate::linker_error::LinkerError;
use crate::linkstate::LinkState;
use crate::object::Object;
use crate::record::{Record, RecordType};

//
// Pass 2 logic
//

struct LastDataRegion {
    base: usize,
    length: usize,
}

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
    let mut lastdata = LastDataRegion{ base: 0, length: 0 };

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
            RecordType::LEDATA => pass2_ledata(&mut rec, state, obj, image, &mut lastdata),
            RecordType::LIDATA => pass2_lidata(&mut rec, state, obj, image, &mut lastdata),
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

/// Compute the linear base address of a location given by the index of an object file SEGDEF,
/// and an offset into that segment.
///
fn base_of_obj_seg_offset(obj: &Object, segidx: usize, offset: usize, state: &LinkState, datalen: usize, rectype: &str) -> Result<usize, LinkerError> {
    if segidx == 0 && !obj.segdefs.is_valid_index(segidx) {
        return Err(LinkerError::new(
            &format!("invalid segidx {} in {} record.", segidx, rectype)
        ));
    }

    let segdef = &obj.segdefs[segidx];

    if offset as usize >= segdef.length || offset as usize + datalen > segdef.length {
        return Err(LinkerError::new(
            &format!("invalid data range {:05X}H..{:05X}H in {} record.", offset, offset as usize + datalen, rectype)
        ));    
    }

    let segment = &state.segments[segdef.segidx];
    let base = segdef.base + segment.base + offset as usize;

    Ok(base)
}

/// Handle an LEDATA record, which contains literal data to be copied into the final executable.
///
fn pass2_ledata(rec: &mut Record, state: &LinkState, obj: &Object, image: &mut [u8], lastdata: &mut LastDataRegion) -> Result<(), LinkerError> {
    let segidx = rec.index()?;
    let offset = rec.word()?;
    let data = rec.rest();

    let base = base_of_obj_seg_offset(obj, segidx, offset as usize, state, data.len(), "LEDATA")?;

    image[base..base+data.len()].copy_from_slice(&data);

    *lastdata = LastDataRegion{ base, length: data.len()};
 
    Ok(())
}

/// Expand an LIDATA block (recursively) into the accumulator vector of bytes.
///
fn accum_lidata(rec: &mut Record, accum: &mut Vec<u8>) -> Result<(), LinkerError> {
    //
    // A block is: 2 bytes of repeat count, 2 bytes of block count, and content.
    // If block count is zero, then content is a counted byte array.
    // If block count is non-zero, then content is that many nested blocks.
    //
    let repeat_count = rec.word()? as usize;
    let block_count = rec.word()? as usize;

    let mut iterbytes = Vec::new();
    
    let bytes = if block_count == 0 {
        rec.counted_bytes()?
    } else {

        for _ in 0..block_count {
            accum_lidata(rec, &mut iterbytes)?;
        }

        &iterbytes
    };

    for _ in 0..repeat_count {
        accum.extend_from_slice(&bytes);
    }

    Ok(())
}

/// Handle expanding and installing iterated data.
///
fn pass2_lidata(rec: &mut Record, state: &LinkState, obj: &Object, image: &mut [u8], lastdata: &mut LastDataRegion) -> Result<(), LinkerError> {
    let segidx = rec.index()?;
    let offset = rec.word()? as usize;

    let mut data = Vec::new();

    while !rec.end() {
        accum_lidata(rec, &mut data)?;
    }

    let base = base_of_obj_seg_offset(obj, segidx, offset as usize, state, data.len(), "LIDATA")?;

    image[base..base+data.len()].copy_from_slice(&data);

    *lastdata = LastDataRegion{ base, length: data.len()};

    Ok(())
}

