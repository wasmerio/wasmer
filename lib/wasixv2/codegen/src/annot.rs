//! Doc-comment annotations (`@name` / `@name(args)` lines).

use anyhow::{Result, bail};

/// Every annotation the generator understands, used to tell "unknown" from "misplaced".
const KNOWN: &[&str] = &[
    "abi-error",
    "align",
    "c",
    "c-enum",
    "c-name",
    "c-one-slot",
    "c-param",
    "c-skip",
    "import",
    "kernel-only",
    "merged-i64",
    "module",
    "name",
    "newtype",
    "noreturn",
    "nr",
    "nullable",
    "pointer-sized",
    "ptr",
    "repr",
    "returns",
    "size",
    "slice",
    "sys-name",
    "union",
    "unsupported",
    "value",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Annotation {
    pub name: String,
    pub args: Option<String>,
}

/// Docs split into free text and annotations, consumed by placement-specific accessors.
#[derive(Debug)]
pub struct Annotations {
    location: String,
    items: Vec<Annotation>,
}

/// Splits a doc comment into free-text lines and annotations.
pub fn parse_docs(docs: Option<&str>, location: &str) -> Result<(Vec<String>, Annotations)> {
    let mut text = Vec::new();
    let mut items = Vec::new();
    for line in docs.unwrap_or_default().lines() {
        // wit-parser keeps the ` * ` prefix of `/** */` block comment lines.
        let line = match line.trim_start().strip_prefix('*') {
            Some(rest) => rest.strip_prefix(' ').unwrap_or(rest),
            None => line,
        };
        // Section dividers (`// ==== Sockets ====`) attach to the next item; they are not docs.
        if line.trim().starts_with("====") {
            continue;
        }
        match line.trim().strip_prefix('@') {
            Some(rest) => items.push(parse_annotation(rest, location)?),
            None => text.push(line.trim_end().to_string()),
        }
    }
    while text.first().is_some_and(|l| l.is_empty()) {
        text.remove(0);
    }
    while text.last().is_some_and(|l| l.is_empty()) {
        text.pop();
    }
    let annotations = Annotations {
        location: location.to_string(),
        items,
    };
    Ok((text, annotations))
}

fn parse_annotation(rest: &str, location: &str) -> Result<Annotation> {
    let name_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(rest.len());
    let (name, tail) = rest.split_at(name_end);
    if name.is_empty() {
        bail!("{location}: malformed annotation `@{rest}`");
    }
    let args = match tail {
        "" => None,
        _ => {
            let inner = tail
                .strip_prefix('(')
                .and_then(|t| t.strip_suffix(')'))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "{location}: malformed annotation `@{rest}` (expected `@{name}` or `@{name}(...)`)"
                    )
                })?;
            Some(inner.trim().to_string())
        }
    };
    Ok(Annotation {
        name: name.to_string(),
        args,
    })
}

impl Annotations {
    pub fn location(&self) -> &str {
        &self.location
    }

    fn take_all(&mut self, name: &str) -> Vec<Annotation> {
        let (taken, kept) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|a| a.name == name);
        self.items = kept;
        taken
    }

    fn take_one(&mut self, name: &str) -> Result<Option<Annotation>> {
        let mut all = self.take_all(name);
        if all.len() > 1 {
            bail!("{}: duplicate annotation `@{name}`", self.location);
        }
        Ok(all.pop())
    }

    /// An argument-less marker such as `@noreturn`.
    pub fn flag(&mut self, name: &str) -> Result<bool> {
        match self.take_one(name)? {
            None => Ok(false),
            Some(Annotation { args: None, .. }) => Ok(true),
            Some(_) => bail!("{}: `@{name}` takes no arguments", self.location),
        }
    }

    /// A single `@name(args)`.
    pub fn value(&mut self, name: &str) -> Result<Option<String>> {
        self.take_one(name)?
            .map(|a| self.require_args(a))
            .transpose()
    }

    /// A repeatable `@name(args)`.
    pub fn values(&mut self, name: &str) -> Result<Vec<String>> {
        self.take_all(name)
            .into_iter()
            .map(|a| self.require_args(a))
            .collect()
    }

    pub fn required_value(&mut self, name: &str) -> Result<String> {
        self.value(name)?
            .ok_or_else(|| anyhow::anyhow!("{}: missing required `@{name}(...)`", self.location))
    }

    fn require_args(&self, a: Annotation) -> Result<String> {
        match a.args {
            Some(args) if !args.is_empty() => Ok(args),
            _ => bail!("{}: `@{}` requires an argument", self.location, a.name),
        }
    }

    /// Errors on any annotation not consumed by the caller.
    pub fn finish(self, placement: &str) -> Result<()> {
        let Some(a) = self.items.first() else {
            return Ok(());
        };
        if KNOWN.contains(&a.name.as_str()) {
            bail!(
                "{}: annotation `@{}` is not allowed on {placement}",
                self.location,
                a.name
            );
        }
        bail!("{}: unknown annotation `@{}`", self.location, a.name)
    }
}

/// Parses `@value`-style integers: decimal or `0x` hex, optionally negative.
pub fn parse_int(s: &str, location: &str) -> Result<i128> {
    let (neg, digits) = match s.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, s),
    };
    let invalid = || anyhow::anyhow!("{location}: invalid integer `{s}`");
    let parsed = match digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        Some(hex) => i128::from_str_radix(&hex.replace('_', ""), 16),
        None => digits.replace('_', "").parse::<i128>(),
    };
    // std parsers accept a sign of their own (`--5`, `-0x-5`), we don't.
    let body = digits.trim_start_matches("0x").trim_start_matches("0X");
    if body.starts_with(['+', '-']) {
        return Err(invalid());
    }
    let v = parsed.map_err(|_| invalid())?;
    Ok(if neg { -v } else { v })
}

pub fn parse_u32(s: &str, location: &str) -> Result<u32> {
    let v = parse_int(s, location)?;
    u32::try_from(v).map_err(|_| anyhow::anyhow!("{location}: `{s}` out of range for u32"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<(Vec<String>, Annotations)> {
        parse_docs(Some(s), "loc")
    }

    #[test]
    fn splits_text_and_annotations() {
        let (text, mut a) = parse("Free text.\n  @c(struct statx)\n@union\nmore").unwrap();
        assert_eq!(text, vec!["Free text.", "more"]);
        assert_eq!(a.value("c").unwrap().as_deref(), Some("struct statx"));
        assert!(a.flag("union").unwrap());
        a.finish("a record").unwrap();
    }

    #[test]
    fn block_comment_star_prefix() {
        // What wit-parser yields for `/**\n * Text.\n * @union\n */` and `/** @ptr(const u8)\n * @nullable */`.
        let (text, mut a) = parse("\n * Text.\n * @union\n ").unwrap();
        assert_eq!(text, vec!["Text."]);
        assert!(a.flag("union").unwrap());
        a.finish("a record").unwrap();
        let (_, mut a) = parse(" @ptr(const u8)\n * @nullable ").unwrap();
        assert_eq!(a.value("ptr").unwrap().as_deref(), Some("const u8"));
        assert!(a.flag("nullable").unwrap());
        a.finish("a type alias").unwrap();
    }

    #[test]
    fn section_dividers_are_dropped() {
        let (text, _) = parse(" ==== Sockets ====\nReal docs.").unwrap();
        assert_eq!(text, vec!["Real docs."]);
    }

    #[test]
    fn unknown_annotation_is_error() {
        let (_, a) = parse("@bogus(1)").unwrap();
        let err = a.finish("a record").unwrap_err().to_string();
        assert_eq!(err, "loc: unknown annotation `@bogus`");
    }

    #[test]
    fn misplaced_annotation_is_error() {
        let (_, a) = parse("@noreturn").unwrap();
        let err = a.finish("a record").unwrap_err().to_string();
        assert_eq!(
            err,
            "loc: annotation `@noreturn` is not allowed on a record"
        );
    }

    #[test]
    fn malformed_annotations() {
        assert!(parse("@c(struct x").is_err());
        assert!(parse("@c foo").is_err());
        assert!(parse("@").is_err());
    }

    #[test]
    fn argument_rules() {
        let (_, mut a) = parse("@noreturn(1)").unwrap();
        assert!(a.flag("noreturn").is_err());
        let (_, mut a) = parse("@nr").unwrap();
        assert!(a.value("nr").is_err());
        let (_, mut a) = parse("@nr(1)\n@nr(2)").unwrap();
        assert!(a.value("nr").is_err());
        let (_, mut a) = parse("@c-param(a: int)\n@c-param(b: long)").unwrap();
        assert_eq!(a.values("c-param").unwrap(), vec!["a: int", "b: long"]);
    }

    #[test]
    fn integers() {
        assert_eq!(parse_int("42", "l").unwrap(), 42);
        assert_eq!(parse_int("-100", "l").unwrap(), -100);
        assert_eq!(parse_int("0x80000000", "l").unwrap(), 0x8000_0000);
        assert_eq!(parse_int("-0x10", "l").unwrap(), -16);
        assert!(parse_int("0100x", "l").is_err());
        for bad in ["--5", "-0x-5", "+5", "0x+5", "-+5", "", "-", "0x"] {
            assert!(parse_int(bad, "l").is_err(), "{bad}");
        }
        assert!(parse_u32("-1", "l").is_err());
    }
}
