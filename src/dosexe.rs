use std::fs;
use std::io::Write;
use std::path::PathBuf;
use crate::linker_error::LinkerError;
use crate::linkstate::FarPtr;

/// A relocation table entry
///
pub struct Relocation {
    pub seg: u16,
    pub offset: u16,
}

/// A DOS executable.
///
pub struct DosExe<'a> {
    relocs: Vec<Relocation>,
    min_alloc: u16,
    max_alloc: u16,
    entry_point: FarPtr,
    init_stack: FarPtr,
    data: &'a [u8],
}

impl<'a> DosExe<'a> {
    pub fn new(data: &'a [u8]) -> DosExe<'a> {
        DosExe {
            relocs: Vec::new(),
            min_alloc: 0,
            max_alloc: 0xffff,
            entry_point: FarPtr::null(),
            init_stack: FarPtr::null(),
            data,
        }
    }

    /// Check if an object of `size` bytes pointed to by `farptr` is totally
    /// inside the executable image.
    ///
    fn far_ptr_in_range(&self, ptr: &FarPtr, size: usize) -> bool {
        ptr.to_linear() + size <= self.data.len()
    }

    /// Set the entry point of the executable. `seg` will be added to the executable's
    /// load address.
    ///
    pub fn set_entry_point(&mut self, entry: &FarPtr) -> Result<(), LinkerError> {
        if !self.far_ptr_in_range(entry, 1) {
            Err(LinkerError::new(&format!(
                "Entry point {:04x}:{:04x} is outside of the executable",
                entry.seg, entry.offset
            )))
        } else {
            self.entry_point = *entry;
            Ok(())
        }
    }

    pub fn set_stack(&mut self, seg: u16, offset: u16) {
        let stack = FarPtr::new(seg, offset);
        //
        // Don't bounds check the stack as it usually lives outside
        // the initialized data of the executable.
        //
        self.init_stack = stack;
    }

    /// Set the minimum allocation, in paragraphs, needed to load the executable.
    ///
    pub fn set_min_alloc(&mut self, min_alloc: u16) {
        self.min_alloc = min_alloc;
    }

    /// Set the maximum (desired) amount of memory, in paragraph,
    /// the program would like.
    ///
    pub fn set_max_alloc(&mut self, max_alloc: u16) {
        self.max_alloc = max_alloc;
    }

    /// Add an entry to the relocation table.
    ///
    pub fn add_relocation(&mut self, reloc: Relocation) {
        self.relocs.push(reloc);
    }

    pub fn write(&self, fname: &PathBuf) -> Result<(), LinkerError> {
        const OFF_MZ_SIG: usize = 0x00;
        const OFF_EXTRA_BYTES: usize = 0x02;
        const OFF_PAGES: usize = 0x04;
        const OFF_RELOCS: usize = 0x06;
        const OFF_HEADER_SIZE: usize = 0x08;
        const OFF_MIN_ALLOC: usize = 0x0a;
        const OFF_MAX_ALLOC: usize = 0x0c;
        const OFF_SS: usize = 0x0e;
        const OFF_SP: usize = 0x10;
        const _OFF_CHECKSUM: usize = 0x12;
        const OFF_IP: usize = 0x14;
        const OFF_CS: usize = 0x16;
        const OFF_RELOC_OFFSET: usize = 0x18;
        const OFF_OVERLAY: usize = 0x1a;
        const OFF_OVERLAY_DATA: usize = 0x1c;
        const _FIXED_HEADER_SIZE: usize = 0x1e;
        const PAGE_SIZE: usize = 512;
        const PARA_SIZE: usize = 16;

        //
        // NB this is where tlink starts relocations. We start them here as well,
        // just to make the binary diff with tlink output easier.
        //
        const RELOC_START: usize = 0x3e;

        if self.relocs.len() > 0xffff {
            return Err(LinkerError::new("Too many relocations (max 65535)"));
        }

        //
        // Figure out how many pages for the header
        //
        let header_size = RELOC_START + (self.relocs.len() * 4);
        let header_pages = (header_size + PAGE_SIZE - 1) / PAGE_SIZE;
        let image_pages = (self.data.len() + PAGE_SIZE - 1) / PAGE_SIZE;
        let total_pages = header_pages + image_pages;

        if image_pages > 0xffff {
            return Err(LinkerError::new("Executable image is too large."));
        }

        let mut header: Vec<u8> = Vec::new();
        header.resize(header_pages * PAGE_SIZE, 0);

        //
        // Build the header
        //
        header[OFF_MZ_SIG] = 'M' as u8;
        header[OFF_MZ_SIG+1] = 'Z' as u8;

        let extra_bytes = (self.data.len() % PAGE_SIZE) as u16;
        header[OFF_EXTRA_BYTES..OFF_EXTRA_BYTES+2].copy_from_slice(&extra_bytes.to_le_bytes());
        header[OFF_PAGES..OFF_PAGES+2].copy_from_slice(&(total_pages as u16).to_le_bytes());
        header[OFF_RELOCS..OFF_RELOCS+2].copy_from_slice(&(self.relocs.len() as u16).to_le_bytes());

        let header_para = (header_pages * PAGE_SIZE / PARA_SIZE) as u16;
        header[OFF_HEADER_SIZE..OFF_HEADER_SIZE+2].copy_from_slice(&header_para.to_le_bytes());

        header[OFF_MIN_ALLOC..OFF_MIN_ALLOC+2].copy_from_slice(&self.min_alloc.to_le_bytes());
        header[OFF_MAX_ALLOC..OFF_MAX_ALLOC+2].copy_from_slice(&self.max_alloc.to_le_bytes());

        header[OFF_SS..OFF_SS+2].copy_from_slice(&self.init_stack.seg.to_le_bytes());
        header[OFF_SP..OFF_SP+2].copy_from_slice(&self.init_stack.offset.to_le_bytes());

        //
        // TODO compute checksum
        //

        header[OFF_IP..OFF_IP+2].copy_from_slice(&self.entry_point.offset.to_le_bytes());
        header[OFF_CS..OFF_CS+2].copy_from_slice(&self.entry_point.seg.to_le_bytes());

        header[OFF_RELOC_OFFSET..OFF_RELOC_OFFSET+2].copy_from_slice(&(RELOC_START as u16).to_le_bytes());

        header[OFF_OVERLAY..OFF_OVERLAY+2].copy_from_slice(&0u16.to_le_bytes());
        header[OFF_OVERLAY_DATA..OFF_OVERLAY_DATA+2].copy_from_slice(&1u16.to_le_bytes());

        //
        // Relocations
        //
        for (i, reloc) in self.relocs.iter().enumerate() {
            let offset = i * 4 + RELOC_START;

            header[offset..offset+2].copy_from_slice(&reloc.offset.to_le_bytes());
            header[offset+2..offset+4].copy_from_slice(&reloc.seg.to_le_bytes());
        }

        //
        // Write the file
        //
        let mut exe = fs::File::create(fname)?;

        exe.write_all(&header)?;
        exe.write_all(&self.data)?;

        Ok(())
    }
}
