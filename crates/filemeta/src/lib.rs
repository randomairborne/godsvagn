use std::io::BufRead;

use digest::Digest;
use md5::Md5;
use sha1::Sha1;
use sha2::Sha256;

pub struct FileMeta {
    pub path: Box<str>,
    pub size: usize,
    pub sums: FileSums,
}

impl FileMeta {
    pub fn new(path: Box<str>, file: &[u8]) -> Result<Self, std::io::Error> {
        Ok(Self {
            size: file.len(),
            path,
            sums: FileSums::new(file)?,
        })
    }
}

pub struct FileSums {
    pub sha1: [u8; 20],
    pub sha256: [u8; 32],
    pub md5: [u8; 16],
}

impl FileSums {
    pub fn new(mut r: impl BufRead) -> Result<Self, std::io::Error> {
        let mut sha1 = Sha1::new();
        let mut md5 = Md5::new();
        let mut sha256 = Sha256::new();
        let mut buf = [0; 1024 * 64];
        loop {
            let valid_buf_len = r.read(&mut buf)?;
            if valid_buf_len == 0 {
                break;
            }
            let valid_buf = &buf[..valid_buf_len];

            sha1.update(valid_buf);
            md5.update(valid_buf);
            sha256.update(valid_buf);
        }

        Ok(Self {
            sha1: sha1.finalize().into(),
            sha256: sha256.finalize().into(),
            md5: md5.finalize().into(),
        })
    }
}
