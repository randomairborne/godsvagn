use std::{
    fs::OpenOptions,
    io::{BufReader, Error as IoError, ErrorKind as IoErrorKind, Seek},
    path::{Path, PathBuf},
};

use filemeta::{FileMeta, FileSums};
use indexgen::ReleaseMetadata;
use md5::{Digest, Md5};
use package::{Package, PackageMeta};
use parsedeb::RequiredFields;
use pgp::composed::{Deserializable, SignedSecretKey};

#[derive(serde::Deserialize, Debug)]
pub struct Config {
    pub release: ConfigReleaseMetadata,
}

#[derive(serde::Deserialize, Debug)]
pub struct ConfigReleaseMetadata {
    pub origin: String,
    pub label: String,
    pub suite: String,
    pub codename: String,
    pub version: String,
    pub description: String,
}

#[derive(argh::FromArgs)]
#[argh(description = "Generate a valid debian repository from a directory full of .deb files")]
struct Args {
    #[argh(option, short = 'c')]
    /// config file for godsvagn
    config: PathBuf,
    #[argh(option, short = 'o')]
    /// where to generate a valid debian repo
    output_dir: PathBuf,
    #[argh(option, short = 'i')]
    /// where to get the debfiles to generate the repo from
    input_dir: PathBuf,
    #[argh(option, short = 'k')]
    /// key to sign the repository with
    keyfile: PathBuf,
    #[argh(switch)]
    /// whether to overwrite an existing directory or to error out
    overwrite: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Args = argh::from_env();
    let config = std::fs::read_to_string(&args.config)?;
    let config: Config = toml::from_str(&config)?;

    let key = SignedSecretKey::from_armor_file(&args.keyfile)?.0;

    if args.overwrite {
        if let Err(e) = std::fs::remove_dir_all(&args.output_dir) {
            match e.kind() {
                std::io::ErrorKind::NotFound => {}
                _ => {
                    Err(format!(
                        "Could not remove folder {}",
                        args.output_dir.display()
                    ))?;
                }
            }
        }
    } else if std::fs::exists(&args.output_dir)? {
        return Err("output dir exists, specify --overwrite to delete it".into());
    }

    let packages: Vec<Package> = {
        let mut packages = Vec::new();
        get_packages(&args.input_dir, &mut packages)?;
        for (start_path, package) in &packages {
            let end_path = args.output_dir.join(&*package.meta.file.path);
            std::fs::create_dir_all(
                end_path
                    .parent()
                    .ok_or("Tried to take parent of root directory")?,
            )?;
            std::fs::copy(start_path, &end_path).map_err(|_| {
                format!(
                    "Could not copy {} to {}",
                    start_path.display(),
                    end_path.display()
                )
            })?;
        }
        packages.into_iter().map(|v| v.1).collect()
    };

    let rc = config.release;
    let release_meta = ReleaseMetadata {
        origin: rc.origin,
        label: rc.label,
        suite: rc.suite,
        codename: rc.codename,
        version: rc.version,
        description: rc.description,
        date: jiff::fmt::rfc2822::to_string(&jiff::Timestamp::now().in_tz("UTC")?)?,
    };

    let to_update = indexgen::generate_files(&release_meta, &key, &packages)?;

    for item in to_update {
        let create_file_at = PathBuf::from(&*item.destination_path);
        let parent_dir = create_file_at
            .parent()
            .ok_or("tried to create root directory")?;
        let parent_dir_to_create = &args.output_dir.join(parent_dir);
        std::fs::create_dir_all(parent_dir_to_create).map_err(|_| {
            format!(
                "Could not create directory {}",
                parent_dir_to_create.display()
            )
        })?;
        let file_to_write = &args.output_dir.join(create_file_at);
        std::fs::write(file_to_write, item.data)
            .map_err(|_| format!("Unable to create file {}", file_to_write.display()))?;
    }

    Ok(())
}

fn get_packages(
    dir: &Path,
    write_into: &mut Vec<(PathBuf, Package)>,
) -> Result<(), PackageReadError> {
    let dir = match std::fs::read_dir(dir) {
        Err(e) if e.kind() == IoErrorKind::NotFound => {
            return Err(PackageReadError::Io(IoError::new(
                e.kind(),
                format!("Could not list directory {}: not found", dir.display()),
            )));
        }
        Err(e) if e.kind() == IoErrorKind::NotADirectory => {
            return Err(PackageReadError::Io(IoError::new(
                e.kind(),
                format!(
                    "Could not list directory {}: not a directory",
                    dir.display()
                ),
            )));
        }
        Err(e) => return Err(e.into()),
        Ok(v) => v,
    };
    for entry in dir {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            get_packages(&path, write_into)?;
        } else if file_type.is_file() {
            let package = read_package(&path)?;
            write_into.push((path, package));
        } else {
            return Err(PackageReadError::UnsupportedFileKind);
        }
    }
    Ok(())
}

#[derive(thiserror::Error, Debug)]
enum PackageReadError {
    #[error("unsupported file type")]
    UnsupportedFileKind,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    PackageRead(#[from] parsedeb::Error),
    #[error("non-utf-8 path encountered")]
    InvalidPath,
    #[error("Files more than 4 gb are only supported on 64 bit platforms")]
    FileTooBig,
    #[error("Could not deserialize controlfile")]
    InvalidControl,
}

fn read_package(p: &Path) -> Result<Package, PackageReadError> {
    let mut raw_file = OpenOptions::new().read(true).open(p)?;
    let mut reader = BufReader::new(&mut raw_file);
    let (fields, _controlfile) = parsedeb::deb_to_control(&mut reader)?;

    reader.rewind()?;
    let sums = FileSums::new(&mut reader)?;

    let size = raw_file
        .metadata()?
        .len()
        .try_into()
        .map_err(|_| PackageReadError::FileTooBig)?;

    let file_meta = FileMeta {
        path: p.to_str().ok_or(PackageReadError::InvalidPath)?.into(),
        size,
        sums,
    };

    let description_md5 = fields
        .iter()
        .find(|(k, _v)| k.eq_ignore_ascii_case("description"))
        .and_then(
            |(_k, v)| /* accounts for the "starting at the second character" rule */ v.get(1..),
        )
        .map(|v| Md5::new().chain_update(v).finalize())
        .unwrap_or_else(|| Md5::new().finalize())
        .into();

    let meta = PackageMeta {
        file: file_meta,
        description_md5,
    };

    let RequiredFields {
        package: name,
        architecture,
        version,
        ..
    } = RequiredFields::from_map(&fields).ok_or(PackageReadError::InvalidControl)?;

    let path = format!("pool/main/{name}_{version}_{architecture}.deb",).into_boxed_str();

    let package = Package {
        meta: PackageMeta {
            file: FileMeta { path, ..meta.file },
            ..meta
        },
        name,
        architecture,
        version,
        fields,
    };
    Ok(package)
}
