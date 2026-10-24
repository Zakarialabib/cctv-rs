//! A small, total, namespace-prefix-agnostic XML scanner for device responses.
//!
//! Camera APIs (ONVIF SOAP, ISAPI, WS-Discovery) answer in XML whose namespace
//! prefixes differ per vendor and per firmware (`trt:`, `tt:`, `SOAP-ENV:`, or
//! none).  Every lookup here matches on the **local name** only, never panics,
//! and returns `None` rather than guessing.
//!
//! It is deliberately not a general XML parser: it finds elements, reads text
//! and attributes, and decodes the five standard entities plus numeric
//! character references.  That is all the drivers need, and it keeps the
//! parsers dependency-free and fully unit-testable.
//!
//! ```
//! use cctv_transport::xml;
//!
//! let doc = r#"<s:Body><tt:Profiles token="a&amp;b"><tt:Name>Main</tt:Name></tt:Profiles></s:Body>"#;
//! let profile = xml::elements(doc, "Profiles").remove(0);
//! assert_eq!(profile.attr("token").as_deref(), Some("a&b"));
//! assert_eq!(xml::element_text(profile.body, "Name").as_deref(), Some("Main"));
//! ```

/// Where an opening tag sits in a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementSpan {
    /// Byte offset of the `<`.
    pub start: usize,
    /// Byte offset just past the `>` of the opening tag.
    pub open_end: usize,
    /// Whether the tag is self-closing (`<x/>`).
    pub self_closing: bool,
}

/// One element found by [`elements`]: its opening tag and its inner content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Element<'a> {
    /// The opening tag, `<` to `>` inclusive.
    pub tag: &'a str,
    /// The content between the opening and the matching closing tag (empty for
    /// a self-closing element).
    pub body: &'a str,
}

impl Element<'_> {
    /// Read an attribute of this element by local name.
    pub fn attr(&self, name: &str) -> Option<String> {
        attribute(self.tag, name)
    }
}

/// Escape text for inclusion in an XML element or attribute value.
///
/// Device-supplied tokens are echoed back into requests, so they are untrusted
/// input: an unescaped `&` or `<` would make a malformed envelope.
pub fn escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Reverse the five standard entities and numeric character references.
/// Unknown entities are preserved verbatim rather than dropped.
pub fn unescape(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let Some(semi) = tail.find(';') else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => out.push(c),
            None => {
                out.push('&');
                out.push_str(entity);
                out.push(';');
            }
        }
        rest = &tail[semi + 1..];
    }
    out.push_str(rest);
    out
}

fn local(qname: &str) -> &str {
    qname.rsplit(':').next().unwrap_or(qname)
}

/// Find the next opening tag with this local name at or after byte `from`.
pub fn find_element(xml: &str, local_name: &str, from: usize) -> Option<ElementSpan> {
    let bytes = xml.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        let lt = xml.get(i..)?.find('<')? + i;
        let after = lt + 1;
        if after >= bytes.len() {
            return None;
        }
        if matches!(bytes[after], b'/' | b'!' | b'?') {
            i = after + 1;
            continue;
        }
        let name_end = xml[after..]
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .map(|p| p + after)
            .unwrap_or(bytes.len());
        let qname = &xml[after..name_end];
        let gt = xml.get(name_end..)?.find('>').map(|p| p + name_end)?;
        let self_closing = gt > 0 && bytes[gt - 1] == b'/';
        if local(qname) == local_name {
            return Some(ElementSpan {
                start: lt,
                open_end: gt + 1,
                self_closing,
            });
        }
        i = gt + 1;
    }
    None
}

/// Byte offset of the next closing tag with this local name at or after `from`.
pub fn find_close(xml: &str, local_name: &str, from: usize) -> Option<usize> {
    let mut i = from;
    while let Some(rel) = xml.get(i..)?.find("</") {
        let at = i + rel;
        let after = at + 2;
        let name_end = xml[after..]
            .find(|c: char| c.is_whitespace() || c == '>')
            .map(|p| p + after)?;
        if local(&xml[after..name_end]) == local_name {
            return Some(at);
        }
        i = name_end;
    }
    None
}

/// Every element with this local name, in document order.
///
/// The body runs to the first matching closing tag, so same-named elements are
/// assumed not to nest (true of every response the drivers read).
pub fn elements<'a>(xml: &'a str, local_name: &str) -> Vec<Element<'a>> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(span) = find_element(xml, local_name, cursor) {
        let tag = &xml[span.start..span.open_end];
        if span.self_closing {
            out.push(Element { tag, body: "" });
            cursor = span.open_end;
            continue;
        }
        let end = find_close(xml, local_name, span.open_end).unwrap_or(xml.len());
        out.push(Element {
            tag,
            body: &xml[span.open_end..end],
        });
        cursor = end.max(span.open_end);
    }
    out
}

/// The inner content of the first element with this local name.
pub fn element_body<'a>(xml: &'a str, local_name: &str) -> Option<&'a str> {
    let span = find_element(xml, local_name, 0)?;
    if span.self_closing {
        return Some("");
    }
    let end = find_close(xml, local_name, span.open_end).unwrap_or(xml.len());
    Some(&xml[span.open_end..end])
}

/// Trimmed, unescaped text of the first element with this local name.
///
/// `None` for a self-closing element, an element whose content is only
/// whitespace, or one whose content starts with a child element.
pub fn element_text(xml: &str, local_name: &str) -> Option<String> {
    let span = find_element(xml, local_name, 0)?;
    if span.self_closing {
        return None;
    }
    let rest = &xml[span.open_end..];
    let content_end = rest.find('<')?;
    let raw = rest[..content_end].trim();
    if raw.is_empty() {
        None
    } else {
        Some(unescape(raw))
    }
}

/// Read an attribute by local name from an opening-tag slice.
///
/// An attribute whose value is empty reads as `None`.
pub fn attribute(tag: &str, want: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let eq = tag.get(i..)?.find('=')? + i;
        let mut name_end = eq;
        while name_end > i && bytes[name_end - 1].is_ascii_whitespace() {
            name_end -= 1;
        }
        let mut name_start = name_end;
        while name_start > i
            && !bytes[name_start - 1].is_ascii_whitespace()
            && bytes[name_start - 1] != b'<'
        {
            name_start -= 1;
        }
        let qname = &tag[name_start..name_end];
        let after_eq = &tag[eq + 1..];
        let vstart_rel = after_eq.find(['"', '\''])?;
        let quote = char::from(after_eq.as_bytes()[vstart_rel]);
        let vstart = eq + 1 + vstart_rel + 1;
        let vend_rel = tag.get(vstart..)?.find(quote)?;
        let value = &tag[vstart..vstart + vend_rel];
        if local(qname) == want {
            let v = unescape(value);
            return if v.is_empty() { None } else { Some(v) };
        }
        i = vstart + vend_rel + 1;
    }
    None
}

/// The reason of a SOAP fault, if the document is one.
///
/// Matches `Fault` under any prefix (`soap:`, `SOAP-ENV:`, `s:`, none) and
/// prefers the human-readable reason over the code.  A fault with no reason
/// still yields `Some`, so a caller never mistakes a fault for success.
pub fn soap_fault(xml: &str) -> Option<String> {
    let body = element_body(xml, "Fault")?;
    for tag in ["Text", "Reason", "faultstring", "Subcode", "Value", "Code"] {
        if let Some(v) = element_text(body, tag) {
            return Some(v);
        }
    }
    Some("unspecified SOAP fault".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fault_is_detected_under_any_namespace_prefix() {
        for p in ["soap", "SOAP-ENV", "s", "env"] {
            let xml = format!(
                r#"<{p}:Envelope><{p}:Body><{p}:Fault>
                <{p}:Code><{p}:Value>{p}:Sender</{p}:Value></{p}:Code>
                <{p}:Reason><{p}:Text>Action Not Supported</{p}:Text></{p}:Reason>
                </{p}:Fault></{p}:Body></{p}:Envelope>"#
            );
            assert_eq!(
                soap_fault(&xml).as_deref(),
                Some("Action Not Supported"),
                "{p}"
            );
        }
        let bare =
            "<Envelope><Body><Fault><faultstring>Bad auth</faultstring></Fault></Body></Envelope>";
        assert_eq!(soap_fault(bare).as_deref(), Some("Bad auth"));
        assert!(soap_fault("<Envelope><Body><Fault></Fault></Body></Envelope>").is_some());
        assert!(soap_fault("<MediaUri><Uri>rtsp://x/y</Uri></MediaUri>").is_none());
    }

    #[test]
    fn escape_and_unescape_round_trip() {
        assert_eq!(escape("&<>\"'"), "&amp;&lt;&gt;&quot;&apos;");
        assert_eq!(unescape("&amp;&lt;&gt;&quot;&apos;"), "&<>\"'");
        assert_eq!(unescape("&#65;&#x42;&bogus;"), "AB&bogus;");
        assert_eq!(unescape("dangling & amp"), "dangling & amp");
    }

    #[test]
    fn elements_are_scoped_to_their_own_body() {
        let xml = "<L><P token=\"a\"><Name>One</Name></P><P token=\"b\"></P><P token=\"c\"/></L>";
        let found = elements(xml, "P");
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].attr("token").as_deref(), Some("a"));
        assert_eq!(element_text(found[0].body, "Name").as_deref(), Some("One"));
        assert_eq!(element_text(found[1].body, "Name"), None);
        assert_eq!(found[2].body, "");
    }

    #[test]
    fn text_with_child_elements_or_whitespace_is_none() {
        assert_eq!(element_text("<a><b>x</b></a>", "a"), None);
        assert_eq!(element_text("<a>   </a>", "a"), None);
        assert_eq!(element_text("<a/>", "a"), None);
        assert_eq!(
            element_text("<x:a> v &amp; w </x:a>", "a").as_deref(),
            Some("v & w")
        );
    }

    #[test]
    fn attributes_match_by_local_name_and_either_quote() {
        let tag = r#"<tt:PanTilt x='0.5' tt:y="-1" space="">"#;
        assert_eq!(attribute(tag, "x").as_deref(), Some("0.5"));
        assert_eq!(attribute(tag, "y").as_deref(), Some("-1"));
        assert_eq!(attribute(tag, "space"), None);
        assert_eq!(attribute(tag, "missing"), None);
    }

    #[test]
    fn truncated_input_never_panics() {
        for doc in ["<", "<a", "<a x=\"1", "</", "<a></", "<a>text"] {
            let _ = elements(doc, "a");
            let _ = element_text(doc, "a");
            let _ = soap_fault(doc);
        }
    }
}
