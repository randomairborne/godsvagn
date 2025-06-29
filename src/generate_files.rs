use filemeta::{FileMeta, FileSums};
use package::{Package, PackageMeta};
use rusqlite::Connection;

struct RawDbRow {
    name: Box<str>,
    version: Box<str>,
    architecture: Box<str>,
    control: Box<str>,
    filepath: Box<str>,
    size: usize,
    sums: FileSums,
    description_md5: [u8; 16],
}

fn get_packages(db: &Connection) -> Result<Vec<Package>, crate::Error> {
    const QUERY: &str = "SELECT name, version, architecture, control, filepath, size, sha1, sha256, md5, description_md5 FROM packages";
    db.prepare_cached(QUERY)?
        .query_map([], |v| {
            Ok(RawDbRow {
                name: v.get(0)?,
                version: v.get(1)?,
                architecture: v.get(2)?,
                control: v.get(3)?,
                filepath: v.get(4)?,
                size: v.get(5)?,
                sums: FileSums {
                    sha1: v.get(6)?,
                    sha256: v.get(7)?,
                    md5: v.get(8)?,
                },
                description_md5: v.get(9)?,
            })
        })?
        .map(|v| {
            v.map_err(crate::Error::from).and_then(|v| {
                let control = parsedeb::parse_control(&v.control)?;
                let file = FileMeta {
                    path: v.filepath,
                    size: v.size,
                    sums: v.sums,
                };
                let meta = PackageMeta {
                    file,
                    description_md5: v.description_md5,
                };
                Ok(Package {
                    meta,
                    name: v.name,
                    architecture: v.architecture,
                    version: v.version,
                    fields: control.into_iter().map(parsedeb::pack).collect(),
                })
            })
        })
        .collect()
}
