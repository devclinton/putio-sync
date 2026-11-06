//! SOAP for UPnP control: reading an action, writing a reply or a fault (feature 022).

use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;

/// ContentDirectory service type
pub const CDS: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";
/// ConnectionManager service type
pub const CMS: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";

/// Escape text for XML content or an attribute value
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters other than tab and newlines can't be in XML 1.0
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// A parsed SOAP action
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// The service type from the action element's namespace, when it has one
    pub service: Option<String>,
    /// The action name, `Browse`
    pub name: String,
    /// Arguments in order, by local name
    pub args: Vec<(String, String)>,
}

impl Action {
    /// The value of an argument
    pub fn arg(&self, name: &str) -> Option<&str> {
        self.args
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Why a body isn't an action
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("not a SOAP action: {0}")]
pub struct ParseError(pub String);

fn local(name: &[u8]) -> String {
    let s = String::from_utf8_lossy(name);
    s.rsplit(':').next().unwrap_or("").to_string()
}

fn prefix(name: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(name);
    s.split_once(':').map(|(p, _)| p.to_string())
}

/// Read the action from a SOAP envelope. Namespace prefixes don't matter.
pub fn parse_action(body: &str) -> Result<Action, ParseError> {
    walk(body)
}

/// The single pass over the envelope
fn walk(body: &str) -> Result<Action, ParseError> {
    let mut reader = Reader::from_str(body);
    let mut ns: HashMap<String, String> = HashMap::new();
    let mut depth = 0usize;
    let mut saw_body = false;
    let mut action: Option<Action> = None;
    let mut arg: Option<(String, String)> = None;
    loop {
        match reader.read_event().map_err(|e| ParseError(e.to_string()))? {
            Event::Start(e) => {
                depth += 1;
                open(&e, depth, &mut ns, &mut saw_body, &mut action, &mut arg);
            }
            Event::Empty(e) => {
                depth += 1;
                open(&e, depth, &mut ns, &mut saw_body, &mut action, &mut arg);
                if depth == 4 {
                    if let (Some(a), Some(v)) = (action.as_mut(), arg.take()) {
                        a.args.push(v);
                    }
                }
                depth -= 1;
            }
            Event::Text(t) => {
                if depth == 4 {
                    if let Some((_, v)) = arg.as_mut() {
                        v.push_str(&t.unescape().map_err(|e| ParseError(e.to_string()))?);
                    }
                }
            }
            Event::CData(t) => {
                if depth == 4 {
                    if let Some((_, v)) = arg.as_mut() {
                        v.push_str(&String::from_utf8_lossy(&t.into_inner()));
                    }
                }
            }
            Event::End(_) => {
                if depth == 4 {
                    if let (Some(a), Some(v)) = (action.as_mut(), arg.take()) {
                        a.args.push(v);
                    }
                }
                depth = depth.saturating_sub(1);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    action.ok_or_else(|| ParseError("no action in the body".into()))
}

fn open(
    e: &quick_xml::events::BytesStart<'_>,
    depth: usize,
    ns: &mut HashMap<String, String>,
    saw_body: &mut bool,
    action: &mut Option<Action>,
    arg: &mut Option<(String, String)>,
) {
    for a in e.attributes().flatten() {
        let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
        let val = a
            .unescape_value()
            .map(|v| v.to_string())
            .unwrap_or_default();
        if key == "xmlns" {
            ns.insert(String::new(), val);
        } else if let Some(p) = key.strip_prefix("xmlns:") {
            ns.insert(p.to_string(), val);
        }
    }
    let name = local(e.name().as_ref());
    match depth {
        2 if name == "Body" => *saw_body = true,
        3 if *saw_body && action.is_none() => {
            let service = ns
                .get(&prefix(e.name().as_ref()).unwrap_or_default())
                .cloned();
            *action = Some(Action {
                service,
                name,
                args: Vec::new(),
            });
        }
        4 if action.is_some() => *arg = Some((name, String::new())),
        _ => {}
    }
}

/// A reply envelope: `<u:{action}Response>` with escaped argument values
pub fn envelope(action: &str, service: &str, args: &[(&str, String)]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>",
    );
    out.push_str(&format!(
        "<u:{action}Response xmlns:u=\"{}\">",
        escape(service)
    ));
    for (name, value) in args {
        out.push_str(&format!("<{name}>{}</{name}>", escape(value)));
    }
    out.push_str(&format!("</u:{action}Response></s:Body></s:Envelope>"));
    out
}

/// A UPnP fault envelope (sent with HTTP 500)
pub fn fault(code: u32, description: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><s:Fault>\
<faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail>\
<UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\"><errorCode>{code}</errorCode>\
<errorDescription>{}</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>",
        escape(description)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const BROWSE: &str = r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:Browse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1"><ObjectID>downloads/A%20%26%20B</ObjectID><BrowseFlag>BrowseDirectChildren</BrowseFlag><Filter>*</Filter><StartingIndex>0</StartingIndex><RequestedCount>10</RequestedCount><SortCriteria></SortCriteria></u:Browse></s:Body></s:Envelope>"#;

    #[test]
    fn reads_a_browse_with_the_s_prefix() {
        let a = parse_action(BROWSE).unwrap();
        assert_eq!(a.name, "Browse");
        assert_eq!(a.service.as_deref(), Some(CDS));
        assert_eq!(a.arg("ObjectID"), Some("downloads/A%20%26%20B"));
        assert_eq!(a.arg("RequestedCount"), Some("10"));
        assert_eq!(a.arg("SortCriteria"), Some(""));
        assert_eq!(a.arg("Nope"), None);
    }

    #[test]
    fn prefix_names_do_not_matter() {
        let body = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://schemas.xmlsoap.org/soap/envelope/"><SOAP-ENV:Body><ns0:GetSystemUpdateID xmlns:ns0="urn:schemas-upnp-org:service:ContentDirectory:1"/></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;
        let a = parse_action(body).unwrap();
        assert_eq!(a.name, "GetSystemUpdateID");
        assert_eq!(a.service.as_deref(), Some(CDS));
        assert!(a.args.is_empty());
    }

    #[test]
    fn default_namespace_and_escaped_text() {
        let body = r#"<Envelope xmlns="http://schemas.xmlsoap.org/soap/envelope/"><Body><Browse xmlns="urn:schemas-upnp-org:service:ContentDirectory:1"><ObjectID>a&amp;b&lt;c</ObjectID><Filter/></Browse></Body></Envelope>"#;
        let a = parse_action(body).unwrap();
        assert_eq!(a.name, "Browse");
        assert_eq!(a.service.as_deref(), Some(CDS));
        assert_eq!(a.arg("ObjectID"), Some("a&b<c"));
        assert_eq!(a.arg("Filter"), Some(""));
    }

    #[test]
    fn rubbish_is_an_error() {
        assert!(parse_action("hello").is_err());
        assert!(parse_action("<a><b></a>").is_err());
        assert!(parse_action("<Envelope><Body></Body></Envelope>").is_err());
    }

    #[test]
    fn envelope_escapes_values() {
        let x = envelope(
            "Browse",
            CDS,
            &[
                ("Result", "<DIDL a=\"1\">&</DIDL>".into()),
                ("NumberReturned", "1".into()),
            ],
        );
        assert!(x.contains(
            "<u:BrowseResponse xmlns:u=\"urn:schemas-upnp-org:service:ContentDirectory:1\">"
        ));
        assert!(x.contains("<Result>&lt;DIDL a=&quot;1&quot;&gt;&amp;&lt;/DIDL&gt;</Result>"));
        // And it reads back
        let a = parse_action(&x).unwrap();
        assert_eq!(a.name, "BrowseResponse");
        assert_eq!(a.arg("Result"), Some("<DIDL a=\"1\">&</DIDL>"));
    }

    #[test]
    fn fault_carries_code_and_description() {
        let x = fault(701, "No such object <x>");
        assert!(x.contains("<errorCode>701</errorCode>"));
        assert!(x.contains("No such object &lt;x&gt;"));
        assert!(x.contains("<faultstring>UPnPError</faultstring>"));
    }

    #[test]
    fn escape_drops_control_characters() {
        assert_eq!(escape("a\u{1}b&"), "ab&amp;");
    }
}
