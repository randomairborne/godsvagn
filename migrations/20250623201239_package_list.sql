-- Add migration script here
CREATE TABLE packages (
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    architecture TEXT NOT NULL,
    control TEXT NOT NULL,
    size INTEGER NOT NULL,
    filepath TEXT NOT NULL,
    md5 BLOB NOT NULL,
    description_md5 BLOB NOT NULL,
    sha1 BLOB NOT NULL,
    sha256 BLOB NOT NULL
) STRICT;

CREATE UNIQUE INDEX avoid_dupes ON packages (version, name, architecture);
CREATE INDEX by_arch ON packages (architecture);