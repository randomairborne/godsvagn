use std::{
    collections::{HashMap, hash_map::Entry},
    fmt::Write,
    io::Write as _,
};

use base16ct::HexDisplay;
use filemeta::FileMeta;
use flate2::{Compression, GzBuilder};
use package::Package;
use pgp::{
    composed::{ArmorOptions, CleartextSignedMessage},
    packet::SecretKey,
    ser::Serialize,
    types::Password,
};

const ARMOR_OPTS: ArmorOptions = ArmorOptions {
    headers: None,
    include_checksum: true,
};

pub fn generate_files(
    release_config: &ReleaseMetadata,
    key: &SecretKey,
    packages: &[Package],
) -> Result<Vec<FileToUpload>, GenerateError> {
    let indexes: Vec<PackageIndexFile> = generate_index_files(packages)?
        .into_iter()
        .flat_map(result_flat_mapper)
        .collect::<Result<_, _>>()?;

    let mut package_meta = Vec::new();
    let mut architectures = Vec::new();
    for PackageIndexFile { path, arch, data } in &indexes {
        let meta = match FileMeta::new(path.clone(), data) {
            Ok(v) => v,
            Err(e) => return Err(GenerateError::HashFile(path.clone(), e)),
        };
        package_meta.push(meta);
        architectures.push(&**arch);
    }

    let release = generate_release(release_config, &package_meta, &architectures)?;
    let sig = CleartextSignedMessage::sign(rand::thread_rng(), &release, key, &Password::empty())?;

    let indexes_base = [
        FileToUpload {
            destination_path: "InRelease".into(),
            data: sig.to_armored_bytes(ARMOR_OPTS)?.into(),
        },
        FileToUpload {
            destination_path: "Release".into(),
            data: release.as_bytes().into(),
        },
        FileToUpload {
            destination_path: "Release.gpg".into(),
            data: sig
                .signatures()
                .first()
                .ok_or(GenerateError::NoSignatures)?
                .to_armored_bytes(ARMOR_OPTS)?
                .into(),
        },
        FileToUpload {
            destination_path: "deriv-archive-keyring.pgp".into(),
            data: key.public_key().to_bytes()?.into(),
        },
    ];

    let to_upload = indexes
        .into_iter()
        .map(|v| FileToUpload {
            destination_path: v.path,
            data: v.data,
        })
        .chain(indexes_base)
        .collect();
    Ok(to_upload)
}

fn gzip(a: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut gz = Vec::new();
    let mut writer = GzBuilder::new().write(&mut gz, Compression::best());
    writer.write_all(a)?;
    writer.finish()?;
    Ok(gz)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PackageIndexFile {
    arch: Box<str>,
    path: Box<str>,
    data: Box<[u8]>,
}

fn result_flat_mapper(
    IndexFileWithArch { arch, contents }: IndexFileWithArch,
) -> Box<[Result<PackageIndexFile, GenerateError>]> {
    let base_path = format!("main/binary-{arch}/Packages");
    let gz = match gzip(contents.as_bytes()) {
        Ok(v) => v,
        Err(e) => return Box::new([Err(GenerateError::Compression("gz", base_path, e))]),
    };
    let xz = match liblzma::encode_all(contents.as_bytes(), 9) {
        Ok(v) => v,
        Err(e) => return Box::new([Err(GenerateError::Compression("gz", base_path, e))]),
    };
    Box::new([
        Ok(PackageIndexFile {
            path: format!("{base_path}.gz").into(),
            arch: arch.clone(),
            data: gz.into_boxed_slice(),
        }),
        Ok(PackageIndexFile {
            path: format!("{base_path}.xz").into(),
            arch: arch.clone(),
            data: xz.into_boxed_slice(),
        }),
        Ok(PackageIndexFile {
            path: base_path.into(),
            arch,
            data: contents.into_boxed_bytes(),
        }),
    ])
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct IndexFileWithArch {
    arch: Box<str>,
    contents: Box<str>,
}

fn generate_index_files(packages: &[Package]) -> Result<Vec<IndexFileWithArch>, GenerateError> {
    let mut aggregator = HashMap::with_capacity(8);

    for package in packages {
        match aggregator.entry(package.architecture.clone()) {
            Entry::Occupied(mut v) => {
                package.write_into_packages(v.get_mut())?;
                v.get_mut().push_str("\n\n");
            }
            Entry::Vacant(v) => {
                let mut s = String::with_capacity(1024);
                package.write_into_packages(&mut s)?;
                s.push_str("\n\n");
                v.insert(s);
            }
        }
    }

    Ok(aggregator
        .into_iter()
        .map(|(arch, d)| IndexFileWithArch {
            arch,
            contents: d.into_boxed_str(),
        })
        .collect())
}

fn generate_release(
    meta: &ReleaseMetadata,
    files: &[FileMeta],
    arches: &[&str],
) -> Result<String, std::fmt::Error> {
    let mut o = String::with_capacity(1024);
    writeln!(o, "Origin: {}", meta.origin)?;
    writeln!(o, "Label: {}", meta.label)?;
    writeln!(o, "Suite: {}", meta.suite)?;
    writeln!(o, "Version: {}", meta.version)?;
    writeln!(o, "Codename: {}", meta.codename)?;
    writeln!(o, "Date: {}", meta.date)?;
    writeln!(o, "Architectures: {}", arches.join(" "))?;
    writeln!(o, "Components: main")?;
    writeln!(o, "Acquire-By-Hash: no")?;
    writeln!(o, "Changelogs: no")?;
    writeln!(o, "Snapshots: no")?;

    writeln!(o, "MD5Sum:")?;
    for file in files {
        writeln!(
            o,
            " {:x} {} {}",
            HexDisplay(&file.sums.md5),
            file.size,
            file.path
        )?;
    }

    writeln!(o, "SHA1:")?;
    for file in files {
        writeln!(
            o,
            " {:x} {} {}",
            HexDisplay(&file.sums.sha1),
            file.size,
            file.path
        )?;
    }

    writeln!(o, "SHA256:")?;
    for file in files {
        writeln!(
            o,
            " {:x} {} {}",
            HexDisplay(&file.sums.sha256),
            file.size,
            file.path
        )?;
    }
    Ok(o)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileToUpload {
    pub destination_path: Box<str>,
    pub data: Box<[u8]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]

// these fields are pretty much all freeform, though see https://wiki.debian.org/DebianRepository/Format
pub struct ReleaseMetadata {
    pub origin: String,
    pub label: String,
    pub suite: String,
    pub codename: String,
    pub version: String,
    pub description: String,
    /// this one isn't freeform
    pub date: String,
}

#[derive(thiserror::Error, Debug)]
pub enum GenerateError {
    #[error("format error: {0}")]
    Format(#[from] std::fmt::Error),
    #[error("signing error")]
    Signing(#[from] pgp::errors::Error),
    #[error("could not complete {0} compression for file {1}: {2}")]
    Compression(&'static str, String, std::io::Error),
    #[error("could not complete hashing for file {0}: {1}")]
    HashFile(Box<str>, std::io::Error),
    #[error("no signatures created- this is a bug")]
    NoSignatures,
}
