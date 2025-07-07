use crate::linker_error::LinkerError;

//
// OMF record parsing.
//

///
/// Known record types.
///
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum RecordType {
    THEADR = 0x80,
    COMENT = 0x88,
    MODEND = 0x8a,
    EXTDEF = 0x8c,
    PUBDEF = 0x90,
    LNAMES = 0x96,
    SEGDEF = 0x98,
    GRPDEF = 0x9a,
    FIXUPP = 0x9c,
    LEDATA = 0xa0,
    LIDATA = 0xa2,
    COMDEF = 0xb0,
    LEXTDEF = 0xb4,
    LPUBDEF = 0xb6,
    LCOMDEF = 0xb8,
    LIBHDR = 0xf0,
    LIBEND = 0xf1,
    EXTDCT = 0xf2,
}

impl RecordType {
    fn from_data(ty: u8) -> Result<Self, LinkerError> {
        Ok(match ty {
            0x80 => RecordType::THEADR,
            0x88 => RecordType::COMENT,
            0x8a => RecordType::MODEND,
            0x8c => RecordType::EXTDEF,
            0x90 => RecordType::PUBDEF,
            0x96 => RecordType::LNAMES,
            0x98 => RecordType::SEGDEF,
            0x9a => RecordType::GRPDEF,
            0x9c => RecordType::FIXUPP,
            0xa0 => RecordType::LEDATA,
            0xa2 => RecordType::LIDATA,
            0xb0 => RecordType::COMDEF,
            0xb4 => RecordType::LEXTDEF,
            0xb6 => RecordType::LPUBDEF,
            0xb8 => RecordType::LCOMDEF,
            0xf0 => RecordType::LIBHDR,
            0xf1 => RecordType::LIBEND,
            0xf2 => RecordType::EXTDCT,
            _ => return Err(LinkerError::new(&format!("unknown record type {:02x}.", ty)))
        })
    }
}

pub struct Record<'a> {
    /// Parse the framing of an OMF record. The general format is
    ///
    /// +00 u8  record type
    /// +01 u16 record length, not including the type and length fields
    /// +03     start of record proper, length-1 bytes
    /// +XX     checksum byte
    ///
    data: &'a [u8],
    pub rectype: RecordType,
    next: usize,    
}

impl<'a> Record<'a> {
    /// Construct a record from a slice.
    ///
    pub fn new(data: &'a[u8]) -> Result<Record<'a>, LinkerError> {
        if data.len() < 4 {
            Err(LinkerError::new("Truncated OMF record."))
        } else {
            let rectype = RecordType::from_data(data[0])?;

            let len = u16::from_le_bytes([data[1], data[2]]) as usize;

            //
            // 3 bytes header + data
            //
            if 3 + len > data.len() {
                Err(LinkerError::new("Truncated OMF record."))
            } else if len == 0 {
                Err(LinkerError::new("Invalid OMF record length."))
            } else {
                //
                // len-1 because we don't want to include the checksum byte
                //    
                Ok(Record{
                    data: &data[3..3+len-1],
                    rectype,
                    next: 0
                })
            }
        }
    }

    /// Get a number of bytes from the record; if there are not that many bytes
    /// left in the record, return a truncation error.
    fn get(&mut self, expected: usize) -> Result<&[u8], LinkerError> {
        if self.next + expected > self.data.len() {
            Err(LinkerError::new("record is truncated."))
        } else {
            let offset = self.next;
            self.next += expected;
            Ok(&self.data[offset..self.next])
        }
    }

    /// Extract the next byte from the record.
    ///
    pub fn byte(&mut self) -> Result<u8, LinkerError> {
        if self.next == self.data.len() {
            Err(LinkerError::new(&format!("OMF record truncated; expected byte at offset {:X}", self.next)))
        } else {
            self.next += 1;
            Ok(self.data[self.next - 1])
        }
    }

    /// Extract the next LE word from the record.
    ///
    pub fn word(&mut self) -> Result<u16, LinkerError> {
        let bytes = self.get(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Extract the next LE dword from the record.
    ///
    pub fn dword(&mut self) -> Result<u32, LinkerError> {
        let bytes = self.get(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Extract the next packed index from the record.
    /// 
    pub fn index(&mut self) -> Result<usize, LinkerError> {
        let byte0 = self.byte()? as usize;

        if byte0 < 0x80 {
            Ok(byte0)
        } else {
            let byte1 = self.byte()? as usize;
            Ok(((byte0 & 0x7f) << 8) | byte1)
        }
    }

    /// Extract the next COMDEF-format length field from the record.
    /// 
    pub fn comdef_length(&mut self) -> Result<usize, LinkerError> {
        let b0 = self.byte()?;

        if b0 <= 0x80 {
            Ok(b0 as usize)
        } else {
            match b0 {
                0x81 => {
                    let data = self.get(2)?;
                    Ok(u16::from_le_bytes([data[0], data[1]]) as usize)
                },
                0x84 => {
                    let data = self.get(3)?;
                    Ok(u32::from_le_bytes([data[0], data[1], data[2], 0]) as usize)
                },
                0x88 => {
                    let data = self.get(4)?;
                    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize)
                },
                _ => Err(LinkerError::new(&format!("invalid comdef length lead byte {:02X}H", b0)))
            }
        }
    }

    /// Extract a counted string, which has one byte of length and then
    /// that any bytes of ASCII text.
    ///
    pub fn counted_string(&mut self) -> Result<String, LinkerError> {
        let count = self.byte()? as usize;

        let text = self.get(count)?;

        match std::str::from_utf8(text) {
            Ok(text) => Ok(text.to_string()),
            Err(err) => Err(LinkerError::new(&format!("invalid counted string: {}", err))),
        }
    }

    /// Check if all data in the record has been parsed.
    ///
    pub fn end(&self) -> bool {
        self.next == self.data.len()
    }

    /// Return the total length of the record, including the header and the
    /// checksum byte.
    ///
    pub fn total_length(&self) -> usize {
        self.data.len() + 4
    }
} 

#[cfg(test)]
mod test
{
    use super::{Record, RecordType, LinkerError};

    #[test]
    fn construct() {
        //
        // Too short for header.
        //
        let rec = [0x88, 0x04];
        assert!(Record::new(&rec).is_err());
        
        //
        // Invalid length
        //
        let rec = [0x88, 0x04, 0x00];
        assert!(Record::new(&rec).is_err());

        //
        // Too short for checksum
        //
        let rec = [0x88, 0x00, 0x00];
        assert!(Record::new(&rec).is_err());

        //
        // Empty but ok
        //
        let rec = [0x88, 0x01, 0x00, 0x00];
        assert!(Record::new(&rec).is_ok());

        //
        // Empty, should fail to get byte.
        //
        let mut rec = Record::new(&rec).unwrap(); 
        assert!(rec.byte().is_err());    

        //
        // Exactly one byte has one byte.
        //
        let rec = [0x88, 0x02, 0x00, 0xff, 0x00];
        let mut rec = Record::new(&rec).unwrap(); 

        assert_eq!(rec.rectype, RecordType::COMENT);
        
        assert_eq!(rec.byte().expect("failed to get byte"), 0xff);

        assert!(rec.end());
        assert!(rec.byte().is_err());    
        assert_eq!(rec.total_length(), 5);
    }

    #[test]
    fn get()  -> Result<(), LinkerError> {
        let rec = [0x88, 0x05, 0x00, 0x03, 0x41, 0x42, 0x43, 0x00];

        let mut rec = Record::new(&rec)?;

        assert_eq!(rec.get(4)?, [0x03, 0x41, 0x42, 0x43]);

        Ok(())
    }

    #[test]
    fn get_truncated()  -> Result<(), LinkerError> {
        let rec = [0x88, 0x05, 0x00, 0x03, 0x41, 0x42, 0x43, 0x00];

        let mut rec = Record::new(&rec)?;

        assert!(rec.get(5).is_err());

        Ok(())
    }

    #[test]
    fn counted_string_ok() -> Result<(), LinkerError> {
        let rec = [0x88, 0x05, 0x00, 0x03, 0x41, 0x42, 0x43, 0x00];

        let mut rec = Record::new(&rec)?;

        assert_eq!(rec.counted_string()?, "ABC");

        Ok(())
    }

    #[test]
    fn counted_string_truncated() -> Result<(), LinkerError> {
        //
        // Count is too short
        //
        let rec = [0x88, 0x05, 0x00, 0x04, 0x41, 0x42, 0x43, 0x00];
        let mut rec = Record::new(&rec)?;
        assert!(rec.counted_string().is_err());

        //
        // Not even room for count
        //
        let rec = [0x88, 0x01, 0x00, 0x00];
        let mut rec = Record::new(&rec)?;
        assert!(rec.counted_string().is_err());
        Ok(())
    }

    #[test]
    fn word_ok() -> Result<(), LinkerError> {
        //
        // Count is too short
        //
        let rec = [0x88, 0x03, 0x00, 0x34, 0x12, 0x00];
        let mut rec = Record::new(&rec)?;
        assert_eq!(rec.word()?, 0x1234);

        Ok(())
    }

    #[test]
    fn word_truncated() -> Result<(), LinkerError> {
        //
        // Count is too short
        //
        let rec = [0x88, 0x02, 0x00, 0x34, 0x12];
        let mut rec = Record::new(&rec)?;
        assert!(rec.word().is_err());

        Ok(())
    }

    #[test]
    fn dword_ok() -> Result<(), LinkerError> {
        //
        // Count is too short
        //
        let rec = [0x88, 0x05, 0x00, 0x78, 0x56, 0x34, 0x12, 0x00];
        let mut rec = Record::new(&rec)?;
        assert_eq!(rec.dword()?, 0x12345678);

        Ok(())
    }

    #[test]
    fn dword_truncated() -> Result<(), LinkerError> {
        //
        // Count is too short
        //
        let rec = [0x88, 0x04, 0x00, 0x78, 0x56, 0x34, 0x12];
        let mut rec = Record::new(&rec)?;
        assert!(rec.dword().is_err());

        Ok(())
    }

    #[test]
    fn index_short_ok() -> Result<(), LinkerError> {
        let rec = [0x88, 0x02, 0x00, 0x7f, 0x00];
        let mut rec = Record::new(&rec)?;
        assert_eq!(rec.index()?, 0x7f);

        Ok(())
    }

    #[test]
    fn index_long_ok() -> Result<(), LinkerError> {
        let rec = [0x88, 0x03, 0x00, 0xc0, 0x7a, 0x00];
        let mut rec = Record::new(&rec)?;
        assert_eq!(rec.index()?, 0x407a);

        Ok(())
    }

    #[test]
    fn index_truncated() -> Result<(), LinkerError> {
        let rec = [0x88, 0x02, 0x00, 0xc0, 0x00];
        let mut rec = Record::new(&rec)?;
        assert!(rec.index().is_err());

        Ok(())
    }

    #[test]
    fn comdef_one_byte() -> Result<(), LinkerError> {
        let rec = [0x88, 0x02, 0x00, 0x42, 0x00];
        let mut rec = Record::new(&rec)?;

        assert_eq!(rec.comdef_length()?, 0x42);        

        Ok(())
    }

    #[test]
    fn comdef_one_byte_truncated() -> Result<(), LinkerError> {
        let rec = [0x88, 0x01, 0x00, 0x00];
        let mut rec = Record::new(&rec)?;

        assert!(rec.comdef_length().is_err());        

        Ok(())
    }

    #[test]
    fn comdef_two_bytes() -> Result<(), LinkerError> {
        let rec = [0x88, 0x04, 0x00, 0x81, 0x42, 0x12, 0x00];
        let mut rec = Record::new(&rec)?;

        assert_eq!(rec.comdef_length()?, 0x1242);        

        Ok(())
    }

    #[test]
    fn comdef_two_bytes_truncated() -> Result<(), LinkerError> {
        let rec = [0x88, 0x03, 0x00, 0x81, 0x42, 0x00];
        let mut rec = Record::new(&rec)?;

        assert!(rec.comdef_length().is_err());        

        Ok(())
    }

    #[test]
    fn comdef_three_bytes() -> Result<(), LinkerError> {
        let rec = [0x88, 0x05, 0x00, 0x84, 0xff, 0x42, 0x12, 0x00];
        let mut rec = Record::new(&rec)?;

        assert_eq!(rec.comdef_length()?, 0x1242ff);        

        Ok(())
    }

    #[test]
    fn comdef_three_bytes_truncated() -> Result<(), LinkerError> {
        let rec = [0x88, 0x03, 0x00, 0x81, 0x42, 0x00];
        let mut rec = Record::new(&rec)?;

        assert!(rec.comdef_length().is_err());        

        Ok(())
    }

    #[test]
    fn comdef_four_bytes() -> Result<(), LinkerError> {
        let rec = [0x88, 0x06, 0x00, 0x88, 0xee, 0xff, 0x42, 0x12, 0x00];
        let mut rec = Record::new(&rec)?;

        assert_eq!(rec.comdef_length()?, 0x1242ffee);        

        Ok(())
    }

    #[test]
    fn comdef_four_bytes_truncated() -> Result<(), LinkerError> {
        let rec = [0x88, 0x05, 0x00, 0x88, 0xee, 0xff, 0x42, 0x12];
        let mut rec = Record::new(&rec)?;

        assert!(rec.comdef_length().is_err());        

        Ok(())
    }


    #[test]
    fn comdef_invalid_lead_byte() -> Result<(), LinkerError> {
        let rec = [0x88, 0x06, 0x00, 0x89, 0xee, 0xff, 0x42, 0x12, 0x00];
        let mut rec = Record::new(&rec)?;

        assert!(rec.comdef_length().is_err());        

        Ok(())
    }
}
