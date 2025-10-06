use chardetng::EncodingDetector;

use crate::pristine::{Inode, InodeMetadata};
use crate::text_encoding::Encoding;

#[cfg(feature = "ondisk-repos")]
pub mod filesystem;
#[cfg(feature = "ondisk-repos")]
pub use filesystem::FileSystem;

pub mod memory;
pub use memory::Memory;

pub trait WorkingCopyRead {
    type Error: std::error::Error + Send;
    fn file_metadata(&self, file: &str) -> Result<InodeMetadata, Self::Error>;
    fn read_file(&self, file: &str, buffer: &mut Vec<u8>) -> Result<(), Self::Error>;
    fn modified_time(&self, file: &str) -> Result<std::time::SystemTime, Self::Error>;
    /// Read the file into the buffer
    ///
    /// Returns the file's text encoding or None if it was a binary file
    fn decode_file(
        &self,
        file: &str,
        buffer: &mut Vec<u8>,
    ) -> Result<Option<Encoding>, Self::Error> {
        let init = buffer.len();
        self.read_file(&file, buffer)?;
        let mut detector = EncodingDetector::new();
        detector.feed(&buffer[init..], true);
        if let Some(e) = crate::get_valid_encoding(&detector, None, true, &buffer[init..]) {
            Ok(Some(Encoding(e)))
        } else {
            Ok(None)
        }
    }
}

pub trait WorkingCopy: WorkingCopyRead {
    fn is_writable(&self, _path: &str) -> Result<bool, Self::Error> {
        Ok(true)
    }
    fn create_dir_all(&self, path: &str) -> Result<(), Self::Error>;
    fn remove_path(&self, name: &str, rec: bool) -> Result<(), Self::Error>;
    fn rename(&self, former: &str, new: &str) -> Result<(), Self::Error>;
    fn set_permissions(&self, name: &str, permissions: u16) -> Result<(), Self::Error>;

    type Writer: std::io::Write;
    fn write_file(&self, file: &str, inode: Inode) -> Result<Self::Writer, Self::Error>;
}

#[derive(Clone)]
pub struct Sink {}

pub fn sink() -> Sink {
    Sink {}
}

impl WorkingCopyRead for Sink {
    type Error = std::io::Error;
    fn file_metadata(&self, _file: &str) -> Result<InodeMetadata, Self::Error> {
        panic!("file_metadata not implemented: {:?}", _file)
    }
    fn read_file(&self, _file: &str, _buffer: &mut Vec<u8>) -> Result<(), Self::Error> {
        panic!("read_file not implemented: {:?}", _file)
    }
    fn modified_time(&self, _file: &str) -> Result<std::time::SystemTime, Self::Error> {
        panic!("modified_time not implemented: {:?}", _file)
    }
}

impl WorkingCopy for Sink {
    fn is_writable(&self, _path: &str) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn create_dir_all(&self, _path: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn remove_path(&self, _name: &str, _rec: bool) -> Result<(), Self::Error> {
        Ok(())
    }
    fn rename(&self, _former: &str, _new: &str) -> Result<(), Self::Error> {
        Ok(())
    }
    fn set_permissions(&self, _name: &str, _permissions: u16) -> Result<(), Self::Error> {
        Ok(())
    }

    type Writer = std::io::Sink;
    fn write_file(&self, _file: &str, _inode: Inode) -> Result<Self::Writer, Self::Error> {
        Ok(std::io::sink())
    }
}
