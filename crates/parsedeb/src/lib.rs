use std::{io::Read, str::FromStr};

use indexmap::IndexMap;

#[cfg(test)]
mod tests;

type PackageMap = IndexMap<Box<str>, Box<str>>;

pub fn deb_to_control(deb: impl std::io::Read) -> Result<(PackageMap, Box<str>), Error> {
    let raw_controlfile = parse_debfile(deb)?;
    Ok((
        get_control(&raw_controlfile)?
            .into_iter()
            .map(pack)
            .collect(),
        raw_controlfile,
    ))
}

pub fn pack((a, b): (&str, &str)) -> (Box<str>, Box<str>) {
    (a.into(), b.into())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct UnbracketedList<'a, T>(&'a Vec<T>);

impl<'a, T: std::fmt::Display> std::fmt::Display for UnbracketedList<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, item) in self.0.iter().enumerate() {
            write!(f, "{item}")?;
            if i + 1 != self.0.len() {
                write!(f, ", ")?;
            }
        }
        Ok(())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("no control.tar.gz file found")]
    NoControlBundle,
    #[error("no control file found")]
    NoControl,
    #[error("control file has a first field other than the package name")]
    DoesNotStartWithPackage,
    #[error("missing fields: {}", UnbracketedList(.0))]
    MissingFields(Vec<RequiredField>),
    #[error("includes forbidden fields {}", UnbracketedList(.0))]
    ForbiddenFields(Vec<ForbiddenField>),
    #[error("I/O error")]
    InvalidRead(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
}

pub fn get_control(control: &str) -> Result<IndexMap<&str, &str>, Error> {
    let parsed_map = parse_control(control)?;

    let keys = extract_keys(&parsed_map);
    if keys.first.is_none_or(|v| v != RequiredField::Package) {
        return Err(Error::DoesNotStartWithPackage);
    }

    let missing_fields: Vec<RequiredField> = RequiredField::ALL
        .into_iter()
        .filter(|req| !keys.required.contains(req))
        .collect();
    if !missing_fields.is_empty() {
        return Err(Error::MissingFields(missing_fields));
    }

    if !keys.forbidden.is_empty() {
        return Err(Error::ForbiddenFields(keys.forbidden));
    }
    Ok(parsed_map)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequiredField {
    Package,
    Version,
    Architecture,
    Maintainer,
    Description,
}

impl RequiredField {
    const ALL: [RequiredField; 5] = [
        Self::Package,
        Self::Version,
        Self::Architecture,
        Self::Maintainer,
        Self::Description,
    ];
}

impl std::fmt::Display for RequiredField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            Self::Package => "Package",
            Self::Version => "Version",
            Self::Architecture => "Architecture",
            Self::Maintainer => "Maintainer",
            Self::Description => "Description",
        };
        f.write_str(str)
    }
}

impl std::str::FromStr for RequiredField {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "package" => Ok(Self::Package),
            "version" => Ok(Self::Version),
            "architecture" => Ok(Self::Architecture),
            "maintainer" => Ok(Self::Maintainer),
            "description" => Ok(Self::Description),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForbiddenField {
    Filename,
    Size,
    Md5Sum,
    Sha1,
    Sha256,
    DescriptionMd5,
}

impl std::fmt::Display for ForbiddenField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            Self::Filename => "Filename",
            Self::Size => "Size",
            Self::Md5Sum => "MD5sum",
            Self::Sha1 => "SHA1",
            Self::Sha256 => "SHA256",
            Self::DescriptionMd5 => "Description-md5",
        };
        f.write_str(str)
    }
}

impl std::str::FromStr for ForbiddenField {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "filename" => Ok(Self::Filename),
            "size" => Ok(Self::Size),
            "md5sum" => Ok(Self::Md5Sum),
            "sha1" => Ok(Self::Sha1),
            "sha256" => Ok(Self::Sha256),
            "description-md5" => Ok(Self::DescriptionMd5),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ParseState<'a> {
    CreatingKey(usize),
    SkippingComment,
    SkippingNewlineComment,
    SkippingColon(&'a str),
    CreatingValue(&'a str, usize),
    ValueNewLine(&'a str, usize),
}

pub fn parse_control(input: &str) -> Result<IndexMap<&str, &str>, ParseError> {
    let mut output = IndexMap::new();

    let mut state = ParseState::CreatingKey(0);
    let mut idx = 0;
    for char in input.chars() {
        #[cfg(test)]
        eprintln!("{idx} {char:?} {state:?}");
        state = match state {
            ParseState::CreatingKey(s) => {
                if char == ':' {
                    ParseState::SkippingColon(&input[s..idx])
                } else if char == '#' {
                    ParseState::SkippingComment
                } else {
                    ParseState::CreatingKey(s)
                }
            }
            ParseState::SkippingColon(s) => {
                if char == '\n' {
                    return Err(ParseError::IncompleteKey(idx));
                } else {
                    ParseState::CreatingValue(s, idx)
                }
            }
            ParseState::CreatingValue(k, s) => {
                if char == '\n' {
                    ParseState::ValueNewLine(k, s)
                } else {
                    ParseState::CreatingValue(k, s)
                }
            }
            ParseState::ValueNewLine(k, s) => {
                if char == '\t' || char == ' ' {
                    ParseState::CreatingValue(k, s)
                } else {
                    if output.insert(k, &input[s..idx]).is_some() {
                        return Err(ParseError::DuplicateKey(k.to_owned()));
                    }
                    if char == '#' {
                        ParseState::SkippingComment
                    } else {
                        ParseState::CreatingKey(idx)
                    }
                }
            }
            ParseState::SkippingComment => {
                if char == '\n' {
                    ParseState::SkippingNewlineComment
                } else {
                    ParseState::SkippingComment
                }
            }
            ParseState::SkippingNewlineComment => {
                if char == '#' {
                    ParseState::SkippingComment
                } else {
                    ParseState::CreatingKey(idx)
                }
            }
        };
        idx += char.len_utf8();
    }

    #[cfg(test)]
    eprintln!("final: {idx} {state:?}");
    match state {
        ParseState::CreatingKey(s) => return Err(ParseError::IncompleteKey(s)),
        ParseState::SkippingColon(k) => return Err(ParseError::NoValueForKey(k.to_owned())),
        ParseState::CreatingValue(_, _) => return Err(ParseError::MustEndInNewline),
        ParseState::ValueNewLine(k, s) => {
            if output.insert(k, &input[s..idx]).is_some() {
                return Err(ParseError::DuplicateKey(k.to_owned()));
            }
        }
        ParseState::SkippingComment | ParseState::SkippingNewlineComment => {}
    };
    Ok(output)
}

fn parse_debfile(deb: impl std::io::Read) -> Result<Box<str>, Error> {
    let mut raw_ar = ar::Archive::new(deb);
    while let Some(entry) = raw_ar.next_entry().transpose()? {
        let tar_reader: Box<dyn Read> = match entry.header().identifier() {
            b"control.tar" => Box::new(entry),
            b"control.tar.gz" => Box::new(flate2::read::GzDecoder::new(entry)),
            b"control.tar.xz" => Box::new(liblzma::read::XzDecoder::new(entry)),
            b"control.tar.zst" => Box::new(zstd::Decoder::new(entry)?),
            _ => continue,
        };
        let mut untared = tar::Archive::new(tar_reader);
        let Some(control) = untared.entries()?.find(|r| {
            r.as_ref()
                .is_ok_and(|r| *r.path_bytes() == *b"control" || *r.path_bytes() == *b"./control")
        }) else {
            return Err(Error::NoControl);
        };
        let mut control = control?;
        let mut out_buf = String::with_capacity(control.size().try_into().unwrap_or(0));
        control.read_to_string(&mut out_buf)?;

        return Ok(out_buf.into_boxed_str());
    }
    Err(Error::NoControlBundle)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ExtractedKeys {
    required: Vec<RequiredField>,
    forbidden: Vec<ForbiddenField>,
    first: Option<RequiredField>,
}

fn extract_keys(vals: &IndexMap<&str, &str>) -> ExtractedKeys {
    ExtractedKeys {
        required: vals
            .iter()
            .filter_map(|(key, _)| RequiredField::from_str(key).ok())
            .collect(),
        forbidden: vals
            .iter()
            .filter_map(|(key, _)| ForbiddenField::from_str(key).ok())
            .collect(),
        first: vals.first().and_then(|a| RequiredField::from_str(a.0).ok()),
    }
}

#[derive(Debug, PartialEq, Eq, Hash, thiserror::Error)]
pub enum ParseError {
    #[error("duplicate key: `{0}`")]
    DuplicateKey(String),
    #[error("key without value: `{0}`")]
    NoValueForKey(String),
    #[error("key not complete at {0}")]
    IncompleteKey(usize),
    #[error("file must end in newline")]
    MustEndInNewline,
}
