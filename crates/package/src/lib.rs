use filemeta::FileMeta;
use indexmap::IndexMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageMeta {
    pub file: FileMeta,
    pub description_md5: [u8; 16],
}

impl PackageMeta {
    fn serialize(&self, f: &mut dyn std::fmt::Write) -> std::fmt::Result {
        writeln!(f, "Filename: {}", self.file.path)?;
        writeln!(f, "Size: {}", self.file.size)?;
        writeln!(
            f,
            "Description-md5: {:x}",
            base16ct::HexDisplay(&self.description_md5)
        )?;
        let sums = &self.file.sums;
        writeln!(f, "MD5sum: {:x}", base16ct::HexDisplay(&sums.md5))?;
        writeln!(f, "SHA1: {:x}", base16ct::HexDisplay(&sums.sha1))?;
        write!(f, "SHA256: {:x}", base16ct::HexDisplay(&sums.sha256))?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub meta: PackageMeta,
    pub name: Box<str>,
    pub architecture: Box<str>,
    pub version: Box<str>,
    pub fields: IndexMap<Box<str>, Box<str>>,
}

impl Package {
    pub fn write_into_packages(&self, target: &mut String) -> std::fmt::Result {
        for field in self.fields.iter() {
            target.push_str(field.0);
            target.push_str(": ");
            target.push_str(field.1.trim());
            target.push('\n');
        }
        self.meta.serialize(target)
    }
}
