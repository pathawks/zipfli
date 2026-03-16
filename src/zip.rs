use std::io::Write;

#[repr(u16)]
pub enum CompressionMethod {
    Store = 0u16,
    Deflate = 8,
}

#[derive(Debug, Clone)]
pub struct LocalFileHeader {
    pub version_to_extract: u16,
    pub general_purpose_flag: u16,
    pub compression_method: u16,
    pub last_mod_time: u16,
    pub last_mod_date: u16,
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub file_name: String,
}

impl Default for LocalFileHeader {
    fn default() -> Self {
        Self {
            version_to_extract: 20,
            general_purpose_flag: 0,
            compression_method: CompressionMethod::Deflate as u16,
            last_mod_time: 0,
            last_mod_date: 0,
            crc32: 0,
            compressed_size: 0,
            uncompressed_size: 0,
            file_name: String::new(),
        }
    }
}

impl LocalFileHeader {
    pub fn new(
        filename: &str,
        compressed_size: u32,
        uncompressed_size: u32,
        crc32: u32,
        dt: crate::DosDateTime,
        store_only: bool,
    ) -> Self {
        let compression_method = if store_only {
            CompressionMethod::Store
        } else {
            CompressionMethod::Deflate
        } as u16;

        Self {
            file_name: filename.to_string(),
            uncompressed_size,
            compressed_size,
            compression_method,
            last_mod_time: dt.time,
            last_mod_date: dt.date,
            crc32,
            ..Default::default()
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![];
        buf.write_all(&[0x50, 0x4b, 0x03, 0x04]).unwrap(); // local file header signature
        buf.write_all(&self.version_to_extract.to_le_bytes())
            .unwrap();
        buf.write_all(&self.general_purpose_flag.to_le_bytes())
            .unwrap();
        buf.write_all(&self.compression_method.to_le_bytes())
            .unwrap();
        buf.write_all(&self.last_mod_time.to_le_bytes()).unwrap();
        buf.write_all(&self.last_mod_date.to_le_bytes()).unwrap();
        buf.write_all(&self.crc32.to_le_bytes()).unwrap();
        buf.write_all(&self.compressed_size.to_le_bytes()).unwrap();
        buf.write_all(&self.uncompressed_size.to_le_bytes())
            .unwrap();
        buf.write_all(&(self.file_name.len() as u16).to_le_bytes())
            .unwrap();
        buf.write_all(&0u16.to_le_bytes()).unwrap(); // extra field length
        buf.write_all(self.file_name.as_bytes()).unwrap();
        buf
    }
}

#[derive(Debug, Clone)]
pub struct CentralDirectoryHeader {
    pub version_made_by: u16,
    pub version_to_extract: u16,
    pub general_purpose_flag: u16,
    pub compression_method: u16,
    pub last_mod_time: u16,
    pub last_mod_date: u16,
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub file_name: String,
    pub local_header_offset: u32,
}

impl CentralDirectoryHeader {
    pub fn new(
        filename: &str,
        compressed_size: u32,
        uncompressed_size: u32,
        crc32: u32,
        dt: crate::DosDateTime,
        store_only: bool,
        local_header_offset: u32,
    ) -> Self {
        let compression_method = if store_only {
            CompressionMethod::Store
        } else {
            CompressionMethod::Deflate
        } as u16;

        Self {
            file_name: filename.to_string(),
            uncompressed_size,
            compressed_size,
            compression_method,
            last_mod_time: dt.time,
            last_mod_date: dt.date,
            version_made_by: 20,
            version_to_extract: 20,
            general_purpose_flag: 0,
            crc32,
            local_header_offset,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![];
        buf.write_all(&[0x50, 0x4b, 0x01, 0x02]).unwrap(); // central directory file header signature
        buf.write_all(&self.version_made_by.to_le_bytes()).unwrap();
        buf.write_all(&self.version_to_extract.to_le_bytes())
            .unwrap();
        buf.write_all(&self.general_purpose_flag.to_le_bytes())
            .unwrap();
        buf.write_all(&self.compression_method.to_le_bytes())
            .unwrap();
        buf.write_all(&self.last_mod_time.to_le_bytes()).unwrap();
        buf.write_all(&self.last_mod_date.to_le_bytes()).unwrap();
        buf.write_all(&self.crc32.to_le_bytes()).unwrap();
        buf.write_all(&self.compressed_size.to_le_bytes()).unwrap();
        buf.write_all(&self.uncompressed_size.to_le_bytes())
            .unwrap();
        buf.write_all(&(self.file_name.len() as u16).to_le_bytes())
            .unwrap();
        buf.write_all(&0u16.to_le_bytes()).unwrap(); // extra field length
        buf.write_all(&0u16.to_le_bytes()).unwrap(); // file comment length
        buf.write_all(&0u16.to_le_bytes()).unwrap(); // disk number start
        buf.write_all(&0u16.to_le_bytes()).unwrap(); // internal file attributes
        buf.write_all(&0u32.to_le_bytes()).unwrap(); // external file attributes
        buf.write_all(&self.local_header_offset.to_le_bytes())
            .unwrap();
        buf.write_all(self.file_name.as_bytes()).unwrap();
        buf
    }
}

#[derive(Debug, Clone)]
pub struct EndOfCentralDirectory {
    pub num_entries: u16,
    pub central_dir_offset: u32,
    pub central_dir_size: u32,
}

impl EndOfCentralDirectory {
    pub fn new(num_entries: u16, central_dir_offset: u32, central_dir_size: u32) -> Self {
        Self {
            num_entries,
            central_dir_offset,
            central_dir_size,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![];
        buf.write_all(&[0x50, 0x4b, 0x05, 0x06]).unwrap(); // EOCD signature
        buf.write_all(&0u16.to_le_bytes()).unwrap(); // disk number
        buf.write_all(&0u16.to_le_bytes()).unwrap(); // disk with start of CD
        buf.write_all(&self.num_entries.to_le_bytes()).unwrap(); // total CD entries on this disk
        buf.write_all(&self.num_entries.to_le_bytes()).unwrap(); // total CD entries overall
        buf.write_all(&self.central_dir_size.to_le_bytes()).unwrap(); // CD size
        buf.write_all(&self.central_dir_offset.to_le_bytes())
            .unwrap(); // CD offset
        buf.write_all(&0u16.to_le_bytes()).unwrap(); // comment length
        buf
    }
}
