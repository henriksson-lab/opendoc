//! A reader for the generated TypeScript DTO files.
//!
//! Only as much TypeScript as the contract generator emits: `export type X = {
//! … };` object literals and `export type X = "a" | "b";` string unions. Every
//! declaration the generator can produce has to land in one of those buckets or
//! in [`TsDecl::Other`], and [`super::dto_contract_tests`] fails on anything it
//! cannot account for, so a new emitted shape cannot slip past unnoticed.

use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TsField {
    /// The `?` in `field?: T` — the key may be absent.
    pub(crate) optional: bool,
    /// `null` appears as a top-level member of the field's type union.
    pub(crate) nullable: bool,
    pub(crate) ty: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TsDecl {
    Object(BTreeMap<String, TsField>),
    /// A union of string literals, e.g. `"text" | "base64"`.
    StringUnion(Vec<String>),
    /// Anything else — a reference, a union of named types, a generic.
    Other(String),
}

#[derive(Clone, Debug)]
pub(crate) struct TsTypes {
    pub(crate) decls: BTreeMap<String, TsDecl>,
}

impl TsTypes {
    pub(crate) fn parse(source: &str) -> Self {
        let stripped = strip_comments(source);
        let mut decls = BTreeMap::new();
        let mut rest = stripped.as_str();
        while let Some(at) = rest.find("export type ") {
            rest = &rest[at + "export type ".len()..];
            let Some(equals) = rest.find('=') else { break };
            let name = rest[..equals].trim().to_string();
            let body_start = equals + 1;
            let Some(end) = terminator(&rest[body_start..]) else {
                break;
            };
            let body = rest[body_start..body_start + end].trim().to_string();
            rest = &rest[body_start + end..];
            // `CommandArgs<K extends …>` and friends: generic aliases, not DTO
            // shapes. Keyed under the bare name so the caller can still see one
            // was declared.
            if name.contains('<') {
                decls.insert(
                    name.split('<').next().unwrap_or(&name).trim().to_string(),
                    TsDecl::Other(body),
                );
                continue;
            }
            decls.insert(name, classify(&body));
        }
        Self { decls }
    }
}

fn classify(body: &str) -> TsDecl {
    if let Some(inner) = body.strip_prefix('{').and_then(|b| b.strip_suffix('}')) {
        let mut fields = BTreeMap::new();
        for member in split_top_level(inner, ';') {
            let member = member.trim();
            if member.is_empty() {
                continue;
            }
            let Some(colon) = find_top_level(member, ':') else {
                continue;
            };
            let (name, ty) = member.split_at(colon);
            let ty = ty[1..].trim().to_string();
            let name = name.trim();
            let (name, optional) = match name.strip_suffix('?') {
                Some(bare) => (bare.trim(), true),
                None => (name, false),
            };
            let nullable = split_top_level(&ty, '|')
                .iter()
                .any(|part| part.trim() == "null");
            fields.insert(
                name.to_string(),
                TsField {
                    optional,
                    nullable,
                    ty,
                },
            );
        }
        return TsDecl::Object(fields);
    }
    // A union may be written on one line or with a leading `|` on each
    // member, which leaves an empty first part; drop the blanks either way.
    let parts: Vec<String> = split_top_level(body, '|')
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect();
    if parts.len() > 1
        && parts.iter().all(|part| {
            let part = part.trim();
            part.len() >= 2 && part.starts_with('"') && part.ends_with('"')
        })
    {
        return TsDecl::StringUnion(
            parts
                .iter()
                .map(|part| part.trim().trim_matches('"').to_string())
                .collect(),
        );
    }
    TsDecl::Other(body.to_string())
}

/// The offset of the `;` that ends a top-level declaration.
fn terminator(text: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (index, ch) in text.char_indices() {
        match ch {
            '{' | '[' | '(' | '<' => depth += 1,
            '}' | ']' | ')' | '>' => depth -= 1,
            ';' if depth <= 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn split_top_level(text: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for ch in text.chars() {
        match ch {
            '{' | '[' | '(' | '<' => {
                depth += 1;
                current.push(ch);
            }
            '}' | ']' | ')' | '>' => {
                depth -= 1;
                current.push(ch);
            }
            _ if ch == separator && depth == 0 => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

fn find_top_level(text: &str, needle: char) -> Option<usize> {
    let mut depth = 0i32;
    for (index, ch) in text.char_indices() {
        match ch {
            '{' | '[' | '(' | '<' => depth += 1,
            '}' | ']' | ')' | '>' => depth -= 1,
            _ if ch == needle && depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn strip_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'/' && index + 1 < bytes.len() {
            if bytes[index + 1] == b'*' {
                match source[index + 2..].find("*/") {
                    Some(end) => {
                        index = index + 2 + end + 2;
                        out.push(' ');
                        continue;
                    }
                    None => break,
                }
            }
            if bytes[index + 1] == b'/' {
                match source[index..].find('\n') {
                    Some(end) => {
                        index += end;
                        continue;
                    }
                    None => break,
                }
            }
        }
        let ch = source[index..].chars().next().expect("char boundary");
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fields_optionality_and_nullability() {
        let types = TsTypes::parse(
            r#"
// a line comment
export type Thing = {
  /** doc */
  plain: string;
  maybe?: number;
  nullable: string | null;
  both?: AppTable | null;
  mapped: Record<string, string>;
  nested: AppBlock[][][];
};
"#,
        );
        let TsDecl::Object(fields) = &types.decls["Thing"] else {
            panic!("expected an object literal");
        };
        assert_eq!(fields.len(), 6);
        assert_eq!(
            fields["plain"],
            TsField {
                optional: false,
                nullable: false,
                ty: "string".to_string()
            }
        );
        assert!(fields["maybe"].optional && !fields["maybe"].nullable);
        assert!(!fields["nullable"].optional && fields["nullable"].nullable);
        assert!(fields["both"].optional && fields["both"].nullable);
        assert_eq!(fields["mapped"].ty, "Record<string, string>");
        assert_eq!(fields["nested"].ty, "AppBlock[][][]");
    }

    #[test]
    fn parses_string_unions_and_ignores_generics() {
        let types = TsTypes::parse(
            r#"
export type Mode = "a" | "b" | "c";
export type Leading =
  | "a"
  | "b"
  | "c";
export type Alias<K extends string> = K;
"#,
        );
        assert_eq!(types.decls["Leading"], types.decls["Mode"]);
        assert_eq!(
            types.decls["Mode"],
            TsDecl::StringUnion(vec!["a".to_string(), "b".to_string(), "c".to_string()])
        );
        assert!(matches!(types.decls["Alias"], TsDecl::Other(_)));
    }

    /// The parser must not be fooled by a `;` inside a nested literal — the
    /// one construct that would silently truncate a declaration.
    #[test]
    fn nested_braces_do_not_end_a_declaration() {
        let types = TsTypes::parse("export type T = { a: { b: string; c: string }; d: number };");
        let TsDecl::Object(fields) = &types.decls["T"] else {
            panic!("expected an object literal");
        };
        assert_eq!(
            fields.keys().cloned().collect::<Vec<_>>(),
            vec!["a".to_string(), "d".to_string()]
        );
    }
}
