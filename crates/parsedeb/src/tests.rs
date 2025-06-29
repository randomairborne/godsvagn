use super::*;

#[test]
fn boring() {
    let out = parse_control(include_str!("testfiles/boring.control")).unwrap();
    let expected = IndexMap::from([
        ("Package", " testpackage\n"),
        ("Description", " a package for testing\n"),
    ]);
    assert_eq!(expected, out)
}

#[test]
fn boring_nonewline() {
    assert_eq!(
        parse_control(include_str!("testfiles/noextranewline.control")).unwrap_err(),
        ParseError::MustEndInNewline
    );
}

#[test]
fn comments() {
    let out = parse_control(include_str!("testfiles/comments.control")).unwrap();
    let expected = IndexMap::from([
        ("Package", " testpackage\n"),
        ("Description", " a package for testing\n"),
    ]);
    assert_eq!(expected, out)
}

#[test]
fn multiline() {
    let out = parse_control(include_str!("testfiles/multiline.control")).unwrap();
    let expected = IndexMap::from([
        ("Package", " testpackage\n"),
        (
            "Description",
            " a package for testing\n  and its description has multiple lines\n",
        ),
    ]);
    assert_eq!(expected, out)
}

#[test]
fn novalue() {
    let err = parse_control(include_str!("testfiles/novalue.control")).unwrap_err();
    let invalid = matches!(err, ParseError::NoValueForKey(_));
    assert!(invalid);
}

#[test]
fn nocolon() {
    let err =
        parse_control(include_str!("testfiles/novalue.control").trim_end_matches(':')).unwrap_err();
    let invalid = matches!(err, ParseError::IncompleteKey(_));
    assert!(invalid);
}

#[test]
fn duplicate() {
    let repeated = include_str!("testfiles/boring.control").repeat(2);
    let err = parse_control(&repeated).unwrap_err();
    let invalid = matches!(err, ParseError::DuplicateKey(_));
    assert!(invalid);
}
