use crate::Args;
use crate::dosexe::{DosExe, Relocation};
use crate::linker_error::LinkerError;
use crate::linkstate::{FarPtr, LinkState};
use crate::object::Object;
use crate::record::{Record, RecordType};
use crate::segment::SegName;
use crate::symbols::{Symbol};

use std::cmp::max;

//
// Pass 2 logic
//

#[derive(Debug, PartialEq, Clone, Copy)]
enum TargetType {
    SEGDEF,
    GRPDEF,
    EXTDEF,
    Frame
}

impl TargetType {
    fn from_byte(b: u8) -> Result<TargetType, LinkerError> {
        Ok(match b {
            0 => TargetType::SEGDEF,
            1 => TargetType::GRPDEF,
            2 => TargetType::EXTDEF,
            3 => TargetType::Frame,
            _ => return Err(LinkerError::new(&format!("invalid target type: {:02X}", b)))
        })
    }
}

struct TargetThread {
    target_type: Option<TargetType>,
    target_index: usize
}

impl TargetThread {
    fn new() -> TargetThread {
        TargetThread {
            target_type: None,
            target_index: 0
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum FrameType {
    SEGDEF,
    GRPDEF,
    EXTDEF,
    ExplicitFrame,
    SegOfPrevData,
    FromTarget,
}

impl FrameType {
    fn from_byte(b: u8) -> Result<FrameType, LinkerError> {
        Ok(match b {
            0 => FrameType::SEGDEF,
            1 => FrameType::GRPDEF,
            2 => FrameType::EXTDEF,
            3 => FrameType::ExplicitFrame,
            4 => FrameType::SegOfPrevData,
            5 => FrameType::FromTarget,
            _ => return Err(LinkerError::new(&format!("invalid frame type: {:02X}", b)))
        })
    }
}

struct FrameThread {
    frame_type: Option<FrameType>,
    frame_index: usize,
}

impl FrameThread {
    fn new() -> FrameThread {
        FrameThread {
            frame_type: None,
            frame_index: 0
        }
    }
}

pub struct ThreadState {
    target_threads: Vec<TargetThread>,
    frame_threads: Vec<FrameThread>,
}

impl ThreadState {
    pub fn new() -> ThreadState {
        let mut target_threads = Vec::new();
        let mut frame_threads = Vec::new();

        for _ in 0..4 {
            target_threads.push(TargetThread::new());
            frame_threads.push(FrameThread::new());
        }

        ThreadState {
            target_threads,
            frame_threads
        }
    }
}

#[derive(Debug)]
enum Locat {
    LowOrderByte,
    Offset16,
    Segment16,
    FarPointer,
}

impl Locat {
    fn new(w: u16) -> Result<Locat, LinkerError> {
        Ok(match w {
            0 => Locat::LowOrderByte,
            1 | 5 => Locat::Offset16,
            2 => Locat::Segment16,
            3 => Locat::FarPointer,
            _ => return Err(LinkerError::new(&format!("invalid location type: {:04X}", w)))
        })
    }
}

struct LastDataRegion {
    frame: u16,
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
    let mut highwater = 0;

    image.resize(memsize, 0u8);

    let mut relocs = Vec::new();

    //
    // Execute pass 2 on all object files
    //
    for obj in objects.iter_mut() {
        let data = obj.data.take().unwrap();
        pass2_object(state, &data, obj, &mut image, &mut relocs, &mut highwater)?;
        obj.data = Some(data);
    }

    //
    // Trim the image of trailing, uninitialized data, and set the EXE header minalloc
    // to require that much extra memory.
    //
    let mut exe = DosExe::new(&image[..highwater]);

    const PARA_SIZE: usize = 16;
    let minalloc = (image.len() - highwater + PARA_SIZE - 1) / PARA_SIZE;

    exe.set_min_alloc(minalloc as u16);

    for reloc in relocs {
        exe.add_relocation(reloc);
    }

    //
    // Figure out the stack, if any
    //
    let nameidx = state.lnames.find_or_add("_STACK");
    let classidx = state.lnames.find_or_add("STACK");
    let ovlyidx = state.lnames.find_or_add("");

    let name = SegName{ nameidx, classidx, ovlyidx };

    if let Some(seg) = state.get_segment_named(&name) {
        let segment = &state.segments[seg];
        let frame = (segment.base >> 4) as u16;
        let mut offset = segment.length + (segment.base & 0x000f);
        if offset > 0xfffe {
            offset = 0xfffe;
        }

        exe.set_stack(frame, offset as u16);
    } else {
        eprintln!("warning: no stack.");
    }

    if let Some(entry) = &state.entry {
        exe.set_entry_point(&entry)?;
    } else {    
        eprintln!("warning: program has no entry point.");
    }

    exe.write(args.output.as_ref().unwrap())?;

    Ok(())
}

/// Handle one pass 2 object file.
/// 
fn pass2_object(state: &mut LinkState, data: &[u8], obj: &mut Object, image: &mut [u8], relocs: &mut Vec<Relocation>, highwater: &mut usize) -> Result<(), LinkerError> {
    let mut start = 0;
    let mut lastdata = LastDataRegion{ frame: 0, base: 0, length: 0 };
    let mut modend = false;

    while !modend && start < data.len() {
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
            RecordType::FIXUPP => pass2_fixupp(&mut rec, state, obj, image, &lastdata, relocs),
            RecordType::MODEND => { 
                modend = true; 
                pass2_modend(&mut rec, state, obj, &lastdata)
            },

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

        *highwater = max(*highwater, lastdata.base + lastdata.length);

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
    let frame = fixup_segdef_frame(state, obj, segidx)?;

    image[base..base+data.len()].copy_from_slice(&data);

    *lastdata = LastDataRegion{ frame, base, length: data.len() };
 
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
    let frame = fixup_segdef_frame(state, obj, segidx)?;

    image[base..base+data.len()].copy_from_slice(&data);

    *lastdata = LastDataRegion{ frame, base, length: data.len() };

    Ok(())
}

/// Given the index of a segdef, return the canonic frame of the containing segment.
/// 
fn fixup_segdef_frame(state: &LinkState, obj: &Object, segidx: usize) -> Result<u16, LinkerError> {
    let linear = fixup_segdef_base(state, obj, segidx)?;
    Ok((linear >> 4) as u16)
}

/// Given the index of a grpdef, return the canonic frame of the containing group.
/// 
fn fixup_grpdef_frame(state: &LinkState, obj: &Object, grpidx: usize) -> Result<u16, LinkerError> {
    let linear = fixup_grpdef_base(state, obj, grpidx)?;
    Ok((linear >> 4) as u16)
}

/// Given the index of an extdef, return the associated canonic frame.
/// 
fn fixup_extdef_frame(state: &LinkState, obj: &Object, extidx: usize) -> Result<u16, LinkerError> {
    if extidx == 0 || !obj.extdefs.is_valid_index(extidx) {
        Err(LinkerError::new(&format!("invalid external index {} in fixup", extidx)))
    } else {
        let symname = &obj.extdefs[extidx];
        
        let (grpidx, segidx, frame) = match state.symbols.symbols.get(symname) {
            Some(Symbol::Public(public)) => {
                (public.group, public.segment, public.frame)
            },
            Some(Symbol::Common(_)) => return Err(LinkerError::new(&format!("{}: COMDEF records are not yet implemented.", symname))),
            Some(Symbol::Undefined) => return Err(LinkerError::new(&format!("{}: symbol undefined in pass 2.", symname))),
            None => return Err(LinkerError::new(&format!("{}: symbol does not exist in pass 2.", symname))),
        };

        if segidx == 0 {
            Ok(frame as u16)
        } else if grpidx != 0 {
            fixup_grpdef_frame(state, obj, grpidx)
        } else {
            fixup_segdef_frame(state, obj, segidx)
        }
    }
}

/// Given the index of a segdef, return the linear base of the containing segment.
/// 
fn fixup_segdef_base(state: &LinkState, obj: &Object, segidx: usize) -> Result<usize, LinkerError> {
    if segidx == 0 || !obj.segdefs.is_valid_index(segidx) {
        Err(LinkerError::new(&format!("invalid segdef index {} in fixup", segidx)))
    } else {
        let segidx = obj.segdefs[segidx].segidx;
        let segment = &state.segments[segidx];

        Ok(segment.base)
    }
}

/// Given the index of a grpdef, return the linear base of the containing group.
/// 
fn fixup_grpdef_base(state: &LinkState, obj: &Object, grpidx: usize) -> Result<usize, LinkerError> {
    if grpidx == 0 || !obj.grpdefs.is_valid_index(grpidx) {
        Err(LinkerError::new(&format!("invalid group index {} in fixup", grpidx)))
    } else {
        let grpidx = obj.grpdefs.get(grpidx);
        let group = &state.groups[grpidx];

        Ok(group.base)
    }
}

/// Given the index of an extdef, return the associated linear address.
/// 
fn fixup_extdef_base(state: &LinkState, obj: &Object, extidx: usize) -> Result<usize, LinkerError> {
    if extidx == 0 || !obj.extdefs.is_valid_index(extidx) {
        Err(LinkerError::new(&format!("invalid external index {} in fixup", extidx)))
    } else {
        let symname = &obj.extdefs[extidx];
        
        let (segidx, offset) = match state.symbols.symbols.get(symname) {
            Some(Symbol::Public(public)) => {
                (public.segment, public.offset)
            },
            Some(Symbol::Common(_)) => return Err(LinkerError::new(&format!("{}: COMDEF records are not yet implemented.", symname))),
            Some(Symbol::Undefined) => return Err(LinkerError::new(&format!("{}: symbol undefined in pass 2.", symname))),
            None => return Err(LinkerError::new(&format!("{}: symbol does not exist in pass 2.", symname))),
        };

        Ok(if segidx == 0 {
            offset as usize 
        } else {
            let segment = &state.segments[segidx];
            segment.base + offset as usize
        })
    }
}

/// Process a thread subrecord of a FIXUPP.
/// 
fn pass2_fixupp_thread(rec: &mut Record, obj: &mut Object, b0: u8) -> Result<(), LinkerError> {
    let is_frame_thread = (b0 & 0x40) != 0;
    let thread = (b0 & 0x03) as usize;

    if is_frame_thread {
        //
        // Frame thread
        //
        let frame_type = FrameType::from_byte((b0 >> 2) & 0x07)?;

        let frame_index = match frame_type {
            FrameType::SEGDEF |
            FrameType::GRPDEF |
            FrameType::EXTDEF => rec.index()?,
            _ => 0,
        };

        obj.fixup_threads.frame_threads[thread] = FrameThread{ frame_type: Some(frame_type), frame_index };
    } else {
        //
        // Target thread
        //
        let target_type = TargetType::from_byte((b0 >> 2) & 0x03)?;

        let target_index = match target_type {
            TargetType::Frame => return Err(LinkerError::new("TargetType::Frame is not supported.")),
            _ => rec.index()?,
        };

        obj.fixup_threads.target_threads[thread] = TargetThread{ target_type: Some(target_type), target_index };
    }   

    Ok(())
}

fn pass2_fixup_data(rec: &mut Record, state: &LinkState, obj: &Object, lastdata: &LastDataRegion)  -> Result<(u16, usize), LinkerError> {
    let fixdat = rec.byte()?;
    let is_frame_thread = (fixdat & 0x80) != 0;
    let is_target_thread = (fixdat & 0x08) != 0;
    let has_target_disp = (fixdat & 0x04) == 0;

    //
    // Frame data
    //
    let (frame_type, frame_index) = if is_frame_thread {
        let threadidx = ((fixdat >> 4) & 0x03) as usize;
        let thread = &obj.fixup_threads.frame_threads[threadidx];

        match (thread.frame_type, thread.frame_index) {
            (Some(frame_type), frame_index) => (frame_type, frame_index),
            _ => return Err(LinkerError::new(&format!("use of undefined frame thread {}", threadidx))),
        }
    } else {
        let frame_type = FrameType::from_byte((fixdat >> 4) & 0x07)?;

        let frame_index = match frame_type {
            FrameType::SEGDEF |
            FrameType::GRPDEF |
            FrameType::EXTDEF => rec.index()?,
            FrameType::ExplicitFrame => rec.word()? as usize,
            _ => 0,
        };

        (frame_type, frame_index)
    };

    //
    // Target data 
    //
    let (target_type, target_index) = if is_target_thread {
        let threadidx = (fixdat & 0x03) as usize;
        let thread = &obj.fixup_threads.target_threads[threadidx];

        match (thread.target_type, thread.target_index) {
            (Some(target_type), target_index) => (target_type, target_index),
            _ => return Err(LinkerError::new(&format!("use of undefined target thread {}", threadidx))),
        }
    } else {
        let target_type = TargetType::from_byte(fixdat & 0x03)?;

        let target_index = match target_type {
            TargetType::Frame => return Err(LinkerError::new("TargetType::Frame is not supported.")),
            _ => rec.index()?,
        };

        (target_type, target_index)
    };

    let target_disp = if has_target_disp { rec.word()? } else { 0 };
    
    //
    // Compute frame.
    //
    let fbval = match frame_type {
        FrameType::SEGDEF => fixup_segdef_frame(state, obj, frame_index)?,
        FrameType::GRPDEF => fixup_grpdef_frame(state, obj, frame_index)?,
        FrameType::EXTDEF => fixup_extdef_frame(state, obj, frame_index)?,
        FrameType::ExplicitFrame => frame_index as u16,
        FrameType::SegOfPrevData => (lastdata.base >> 4) as u16,
        FrameType::FromTarget => match target_type {
            TargetType::SEGDEF => fixup_segdef_frame(state, obj, target_index)?,
            TargetType::GRPDEF => fixup_grpdef_frame(state, obj, target_index)?,
            TargetType::EXTDEF => fixup_extdef_frame(state, obj, target_index)?,
            _ => unreachable!(),
        } 
    };

    //
    // Compute target.
    //
    let target = match target_type {
        TargetType::SEGDEF => {
            let base = fixup_segdef_base(state, obj, target_index)?;
            let segdef = &obj.segdefs[target_index];
            base + segdef.base
        },
        TargetType::GRPDEF => fixup_grpdef_base(state, obj, target_index)?,
        TargetType::EXTDEF => fixup_extdef_base(state, obj, target_index)?,
        _ => unreachable!(),
    } + target_disp as usize;

    
    
    Ok((fbval, target))
}

/// Process a fixup subrecord of a FIXUPP.
/// 
fn pass2_fixupp_fixup(rec: &mut Record, state: &mut LinkState, obj: &Object, image: &mut[u8], b0: u8, lastdata: &LastDataRegion, relocs: &mut Vec<Relocation>) -> Result<(), LinkerError> {
    //
    // Fixup location.
    //
    let locat = ((b0 as u16) << 8) | (rec.byte()? as u16);
    let is_segment_rel = (locat & 0x4000) != 0;
    let loctype = Locat::new((locat >> 10) & 0x000f)?;

    let imageptr = lastdata.base + ((locat as usize) & 0x3ff);

    let (fbval, target) = pass2_fixup_data(rec, state, obj, lastdata)?;

    //
    // Compute the fixup
    //
    let frame_base = (fbval as i32) << 4;
    
    if is_segment_rel {
        let foval = (target as i32) - frame_base;

        if foval < 0 || foval > 0xffff {
            eprintln!("warning: fixup overflow.");
        }

        match loctype {
            Locat::Offset16 => {
                if imageptr + 2 > image.len() {
                    eprintln!("warning: fixup location {:08X}H outside of image {:08X}H.", imageptr, image.len());
                } else {
                    let slice = &mut image[imageptr..imageptr+2];
                    let curr = u16::from_le_bytes(slice.try_into().unwrap());
                    let next = u16::wrapping_add(curr, foval as u16);
                    
                    slice.copy_from_slice(&next.to_le_bytes());
                }
            },
            Locat::Segment16 => {
                if imageptr + 2 > image.len() {
                    eprintln!("warning: fixup location {:08X}H outside of image {:08X}H.", imageptr, image.len());
                } else {
                    let slice = &mut image[imageptr..imageptr+2];
                    let curr = u16::from_le_bytes(slice.try_into().unwrap());
                    let next = u16::wrapping_add(curr, fbval);
                    
                    slice.copy_from_slice(&next.to_le_bytes());

                    let reloc = Relocation {
                        seg: lastdata.frame,
                        offset: (imageptr - ((lastdata.frame as usize) << 4)) as u16,
                    };

                    relocs.push(reloc);
                }
            },
            Locat::FarPointer => {
                if imageptr + 4 > image.len() {
                    eprintln!("warning: fixup location {:08X}H outside of image {:08X}H.", imageptr, image.len());
                } else {
                    let slice = &mut image[imageptr..imageptr+2];
                    let curr = u16::from_le_bytes(slice.try_into().unwrap());
                    let next = u16::wrapping_add(curr, foval as u16);
                    
                    slice.copy_from_slice(&next.to_le_bytes());

                    let slice = &mut image[imageptr+2..imageptr+4];
                    let curr = u16::from_le_bytes(slice.try_into().unwrap());
                    let next = u16::wrapping_add(curr, fbval);
                    
                    slice.copy_from_slice(&next.to_le_bytes());

                    let reloc = Relocation {
                        seg: lastdata.frame,
                        offset: (imageptr + 2 - ((lastdata.frame as usize) << 4)) as u16,
                    };

                    relocs.push(reloc);
                }
            },
            Locat::LowOrderByte => {
                if imageptr >= image.len() {
                    eprintln!("warning: fixup location {:08X}H outside of image {:08X}H.", imageptr, image.len());
                } else {
                    let slice = &mut image[imageptr..imageptr+1];
                    let curr = u8::from_le_bytes(slice.try_into().unwrap());
                    let next = u8::wrapping_add(curr, fbval as u8);
                    
                    slice.copy_from_slice(&next.to_le_bytes());
                }
            },
        }
    } else {
        let loc_delta = (imageptr as i32) - frame_base;
        if loc_delta < 0 || loc_delta > 0xffff {
            eprintln!("warning: fixup location {:08X}H is outside frame {:04X}H", imageptr, frame_base >> 4);
        } else {
            let target_delta = (target as i32) - frame_base;

            if target_delta < 0 || target_delta > 0xffff {
                eprintln!("warning: fixup location {:08X}H is outside frame {:04X}H", imageptr, frame_base >> 4);
            }
        }

        match loctype {
            Locat::Offset16 => {
                let disp = (target as i32) - ((imageptr as i32) + 2);

                let slice = &mut image[imageptr..imageptr+2];
                let curr = u16::from_le_bytes(slice.try_into().unwrap());
                let next = u16::wrapping_add(curr, disp as u16);
                
                slice.copy_from_slice(&next.to_le_bytes());
            },
            _ => {
                return Err(LinkerError::new(
                    &format!("invalid self-relative fixup location type {:?}", loctype)
                ))
            }
        }    
    }

    Ok(())
}

/// Handle a FIXUPP record, which applies relocation changes to the final image.
/// 
fn pass2_fixupp(rec: &mut Record, state: &mut LinkState, obj: &mut Object, image: &mut[u8], lastdata: &LastDataRegion, relocs: &mut Vec<Relocation>) -> Result<(), LinkerError> {
    while !rec.end() {
        let b0 = rec.byte()?;

        if (b0 & 0x80) == 0x00 {
            pass2_fixupp_thread(rec, obj, b0)?;
        } else {
            pass2_fixupp_fixup(rec, state, obj, image, b0, lastdata, relocs)?;
        }
    }

    Ok(())
}

/// Handle a MODEND record, which potentially includes the program's start address
/// 
fn pass2_modend(rec: &mut Record, state: &mut LinkState, obj: &Object, lastdata: &LastDataRegion) -> Result<(), LinkerError> {
    const IS_MAIN: u8 = 0x80;
    const HAS_START: u8 = 0x40;

    if !rec.end() {
        let modtype = rec.byte()?;
        
        let is_main = (modtype & IS_MAIN) != 0;

        if is_main && (modtype & HAS_START) != 0 {
            if state.entry.is_some() {
                return Err(LinkerError::new("warning: program has multiple entry points."));
            } else {
                let (fbval, target) = pass2_fixup_data(rec, state, obj, lastdata)?;
                let frame_base = (fbval as i32) << 4;
                let foval = (target as i32) - frame_base;
                
                if foval < 0 || foval > 0xffff {
                    return Err(LinkerError::new("fixup overflow in MODEND start address."));
                }

                state.entry = Some(FarPtr{ seg: fbval, offset: foval as u16 });
            }
        }
    }

    Ok(())
}