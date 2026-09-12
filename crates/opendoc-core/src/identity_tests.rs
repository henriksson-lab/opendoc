use crate::*;

#[test]
fn hash_ref_requires_algorithm() {
    let hash = HashRef::parse("sha256:abc").unwrap();
    assert_eq!(hash.algorithm(), "sha256");
    assert_eq!(hash.digest(), "abc");
    assert_eq!(hash.to_string(), "sha256:abc");
    assert!(HashRef::parse("abc").is_err());
}

#[test]
fn hash_ref_rejects_path_hostile_components() {
    for value in [
        "sha256:",
        ":abc",
        " sha256:abc",
        "sha256:abc ",
        "sha/256:abc",
        "sha256:../abc",
        "sha256:abc/def",
        "sha256:abc\\def",
        "sha256:abc:def",
        ".:abc",
        "..:abc",
        "sha256:.",
        "sha256:..",
        "sha256:abc\ndef",
    ] {
        assert!(HashRef::parse(value).is_err(), "{value:?} parsed");
    }

    let hash = HashRef::parse("blake3-v1:abc_DEF-123.xyz").unwrap();
    assert_eq!(hash.to_string(), "blake3-v1:abc_DEF-123.xyz");
}

#[test]
fn stable_id_parse_rejects_surrounding_whitespace() {
    assert_eq!(StableId::parse("block-1").unwrap().as_str(), "block-1");
    assert!(matches!(
        StableId::parse(" block-1 "),
        Err(ModelError::InvalidId(
            "stable id has surrounding whitespace"
        ))
    ));
}

#[test]
fn document_uuid_parse_rejects_surrounding_whitespace() {
    assert_eq!(DocumentUuid::parse("doc-1").unwrap().as_str(), "doc-1");
    assert!(matches!(
        DocumentUuid::parse(" doc-1 "),
        Err(ModelError::InvalidId(
            "document uuid has surrounding whitespace"
        ))
    ));
}

#[test]
fn digest_bytes_is_algorithm_explicit() {
    let hash = digest_bytes("sha256", b"hello").unwrap();
    assert_eq!(hash.algorithm(), "sha256");
    assert_eq!(
        hash.digest(),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    assert!(digest_bytes("unknown", b"hello").is_err());
}
