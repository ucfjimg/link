mod group;
mod index_map;
mod linker_error;
mod linkstate;
mod lnames;
mod omf_vec;
mod object;
mod pass1;
mod record;
mod segment;
mod symbols;

use clap::Parser;
use std::path::PathBuf;
use std::process::exit;
use linker_error::LinkerError;
use linkstate::LinkState;
use pass1::pass1;
use crate::symbols::Symbol;

#[derive(Parser, Debug)]
pub struct Args {
    #[arg(short)]
    pub output: Option<PathBuf>,
    #[arg(short)]
    pub libpath: Vec<PathBuf>,
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

fn main() -> Result<(), LinkerError> {
    let args = get_args();

    let mut linkstate = LinkState::new();
    let mut objects = Vec::new();

    pass1(&mut linkstate, &mut objects, &args)?;

    println!("OBJECTS");
    for obj in objects.iter() {
        println!("*** {}", obj.name);
        println!("EXTERNS");
        for ext in obj.extdefs.iter() {
            println!("  {}", ext);
        }

        println!("SEGDEFS");
        for segdef in obj.segdefs.iter() {
            println!("  #{:02} {:05X}H {:05X}H {:?} {:?}", segdef.segidx, segdef.base, segdef.length, segdef.align, segdef.combine);
        }
        println!();
    }

    println!("SEGMENTS");
    for (i,seg)  in linkstate.segments.iter().enumerate().map(|(i, seg) | (i+1, seg)) {
        let segname = linkstate.segname(&seg.name);
        println!("#{:02} {:30} {:05X}H {:?} {:?}", i, segname, seg.length, seg.align, seg.combine); 
    }
    println!();

    println!("SYMBOLS");

    let mut symnames = linkstate.symbols.symbols.keys().map(|name| &name[..]).collect::<Vec<&str>>();
    symnames.sort();


    for name in symnames {
        let sym = linkstate.symbols.symbols.get(name).unwrap();
        print!("  {:30} ", name);

        match sym {
            Symbol::Undefined => print!("UND"),
            Symbol::Public(p) => print!("PUB GROUP {:2} SEG {:2} FRAME {:04X}H {:05X}H", p.group, p.segment, p.frame, p.offset),
            Symbol::Common(_) => print!("COM"),
            _ => {},
        }

        println!();
    }

    Ok(())
}
