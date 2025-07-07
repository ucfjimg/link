mod group;
mod index_map;
mod library;
mod linker_error;
mod linkstate;
mod lnames;
mod omf_vec;
mod object;
mod pass1;
mod record;
mod segment;
mod symbols;

#[cfg(test)]
mod testlib;

use clap::Parser;
use std::path::PathBuf;
use std::process::exit;
use library::Library;
use linker_error::LinkerError;
use linkstate::LinkState;
use pass1::pass1;
use symbols::Symbol;

#[derive(Parser, Debug)]
pub struct Args {
    #[arg(short)]
    pub output: Option<PathBuf>,
    #[arg(short)]
    pub libpath: Vec<PathBuf>,
    #[arg(short = 'L')]
    pub libs: Vec<PathBuf>,
    pub objects: Vec<PathBuf>,
}

/// Parse the command line arguments, and construct missing but needed output
/// filenames.
///
fn get_args() -> Args {
    let mut args = Args::parse();

    if args.objects.is_empty() {
        eprintln!("No objects specified");
        exit(1);
    }

    if args.output.is_none() {
        let mut output = args.objects[0].clone();
        output.set_extension("exe");
        args.output = Some(output);
    }

    args
}

/// Locate and preload libraries on the command line
/// 
fn get_libs(args: &Args) -> Result<Vec<Library>, LinkerError> {
    let mut libs = Vec::new();

    for lib in args.libs.iter() {
        let libpath = if lib.exists() {
            Some(lib.clone())
        } else {
            args.libpath.iter().map(|path| path.join(lib)).find(|path| path.exists())
        };

        let library = match libpath {
            Some(path) => Library::new(lib.as_os_str().to_str().unwrap(), path)?,
            None => return Err(LinkerError::new(&format!("library {:?} not found in current directory or library path.", lib))),
        };

        libs.push(library);
    }

    Ok(libs)
}

fn main() -> Result<(), LinkerError> {
    let args = get_args();

    let mut linkstate = LinkState::new();
    let mut objects = Vec::new();
    let libs = get_libs(&args)?;

    pass1(&mut linkstate, &mut objects, &libs, &args)?;

    println!("\n Start  Stop   Length Name               Class\n");
    for segidx in linkstate.segment_order.iter().map(|x| *x) {
        let seg = &linkstate.segments[segidx];

        println!(" {:05X}H {:05X}H {:05X}H {:18} {}", 
            seg.base,
            if seg.length == 0 { seg.base } else { seg.base + seg.length - 1},
            seg.length, 
            linkstate.lnames.get(seg.name.nameidx), 
            linkstate.lnames.get(seg.name.classidx));
    }
    
    println!("\n\nDetailed map of segments\n");

    for segidx in linkstate.segment_order.iter() {
        let seg = &linkstate.segments[*segidx];
        let grp = if seg.group != 0 { linkstate.lnames.get(linkstate.groups[seg.group].name)  } else { "(none)" };

        let base = if seg.group == 0 { seg.base } else { linkstate.groups[seg.group].base };
        
        for obj in objects.iter() {
            for segdef in obj.segdefs.iter() {
                if segdef.segidx == *segidx {
                    let linear = seg.base + segdef.base;

                    let frame = base >> 4;
                    let offset = (base & 0x000f) + (linear - base);


                    println!(" {:04X}:{:04X} {:04X} C={:6} S={:14} G={:7} M={:10} ACBP={:02X}", 
                        frame, offset, 
                        segdef.length,   
                        linkstate.lnames.get(seg.name.classidx),
                        linkstate.lnames.get(seg.name.nameidx),
                        grp,
                        obj.name,
                        segdef.acbp
                    );
                }
            }
        }
    }

    struct SortSym {
        name: String,
        frame: usize,
        offset: usize,
        linear: usize
    };

    println!("\n  Address         Publics by Name\n");

    let mut symbols = linkstate.symbols.symbols.keys().map(|name| name.to_owned()).collect::<Vec<String>>();
    symbols.sort();

    let mut byvalue = Vec::new();

    for name in symbols.iter() {
        let sym = linkstate.symbols.symbols.get(name).unwrap();

        let (frame,offset) = match &sym {
            &Symbol::Public(p) => {
                if p.segment != 0 {
                    let linear = linkstate.segments[p.segment].base + p.offset as usize;

                    let base = if p.group != 0 {
                        linkstate.groups[p.group].base
                    } else {
                        linkstate.segments[p.segment].base
                    };

                    let frame = base >> 4;
                    let offset = (base & 0x000f) + (linear - base);

                    (frame, offset)
                } else {
                    (p.frame as usize, p.offset as usize)
                }
            },
            &Symbol::Common(_c) => { (0,0) },
            _ => continue,
        };

        byvalue.push(SortSym{
            name: name.to_owned(),
            frame,
            offset,
            linear: (frame << 4) + offset
        });

        println!(" {:04X}:{:04X}       {}", frame, offset, name);
    }

    byvalue.sort_by_key(|sym| sym.linear);

    println!("\n  Address         Publics by Value\n");

    for sym in byvalue.iter() {
        println!(" {:04X}:{:04X}       {}", sym.frame, sym.offset, sym.name);
    }


    Ok(())
}
