//! Device and service descriptions (feature 022).

use crate::soap::{escape, CDS, CMS};

/// `description.xml`: a MediaServer:1 with ContentDirectory and ConnectionManager
pub fn device(name: &str, uuid: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<root xmlns=\"urn:schemas-upnp-org:device-1-0\" xmlns:dlna=\"urn:schemas-dlna-org:device-1-0\">\
<specVersion><major>1</major><minor>0</minor></specVersion>\
<device>\
<dlna:X_DLNADOC>DMS-1.50</dlna:X_DLNADOC>\
<deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>\
<friendlyName>{name}</friendlyName>\
<manufacturer>putio-sync</manufacturer>\
<modelName>putio-sync</modelName>\
<modelDescription>putio-sync media server</modelDescription>\
<modelNumber>{version}</modelNumber>\
<UDN>uuid:{uuid}</UDN>\
<serviceList>\
<service><serviceType>{CDS}</serviceType><serviceId>urn:upnp-org:serviceId:ContentDirectory</serviceId>\
<SCPDURL>/cds.xml</SCPDURL><controlURL>/ctl/cds</controlURL><eventSubURL>/evt/cds</eventSubURL></service>\
<service><serviceType>{CMS}</serviceType><serviceId>urn:upnp-org:serviceId:ConnectionManager</serviceId>\
<SCPDURL>/cms.xml</SCPDURL><controlURL>/ctl/cms</controlURL><eventSubURL>/evt/cms</eventSubURL></service>\
</serviceList>\
</device></root>",
        name = escape(name),
        uuid = escape(uuid),
        version = env!("CARGO_PKG_VERSION"),
    )
}

fn arg(name: &str, direction: &str, var: &str) -> String {
    format!(
        "<argument><name>{name}</name><direction>{direction}</direction>\
<relatedStateVariable>{var}</relatedStateVariable></argument>"
    )
}

fn action(name: &str, args: &[(&str, &str, &str)]) -> String {
    let list: String = args.iter().map(|(n, d, v)| arg(n, d, v)).collect();
    format!("<action><name>{name}</name><argumentList>{list}</argumentList></action>")
}

fn variable(name: &str, ty: &str, events: bool, allowed: &[&str]) -> String {
    let allowed = if allowed.is_empty() {
        String::new()
    } else {
        format!(
            "<allowedValueList>{}</allowedValueList>",
            allowed
                .iter()
                .map(|v| format!("<allowedValue>{v}</allowedValue>"))
                .collect::<String>()
        )
    };
    format!(
        "<stateVariable sendEvents=\"{}\"><name>{name}</name><dataType>{ty}</dataType>{allowed}</stateVariable>",
        if events { "yes" } else { "no" }
    )
}

fn scpd(actions: &[String], variables: &[String]) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<scpd xmlns=\"urn:schemas-upnp-org:service-1-0\"><specVersion><major>1</major><minor>0</minor></specVersion>\
<actionList>{}</actionList><serviceStateTable>{}</serviceStateTable></scpd>",
        actions.concat(),
        variables.concat()
    )
}

/// `cds.xml`: the ContentDirectory service description
pub fn content_directory() -> String {
    scpd(
        &[
            action(
                "Browse",
                &[
                    ("ObjectID", "in", "A_ARG_TYPE_ObjectID"),
                    ("BrowseFlag", "in", "A_ARG_TYPE_BrowseFlag"),
                    ("Filter", "in", "A_ARG_TYPE_Filter"),
                    ("StartingIndex", "in", "A_ARG_TYPE_Index"),
                    ("RequestedCount", "in", "A_ARG_TYPE_Count"),
                    ("SortCriteria", "in", "A_ARG_TYPE_SortCriteria"),
                    ("Result", "out", "A_ARG_TYPE_Result"),
                    ("NumberReturned", "out", "A_ARG_TYPE_Count"),
                    ("TotalMatches", "out", "A_ARG_TYPE_Count"),
                    ("UpdateID", "out", "A_ARG_TYPE_UpdateID"),
                ],
            ),
            action("GetSystemUpdateID", &[("Id", "out", "SystemUpdateID")]),
            action(
                "GetSearchCapabilities",
                &[("SearchCaps", "out", "SearchCapabilities")],
            ),
            action(
                "GetSortCapabilities",
                &[("SortCaps", "out", "SortCapabilities")],
            ),
        ],
        &[
            variable("A_ARG_TYPE_ObjectID", "string", false, &[]),
            variable(
                "A_ARG_TYPE_BrowseFlag",
                "string",
                false,
                &["BrowseMetadata", "BrowseDirectChildren"],
            ),
            variable("A_ARG_TYPE_Filter", "string", false, &[]),
            variable("A_ARG_TYPE_Index", "ui4", false, &[]),
            variable("A_ARG_TYPE_Count", "ui4", false, &[]),
            variable("A_ARG_TYPE_SortCriteria", "string", false, &[]),
            variable("A_ARG_TYPE_Result", "string", false, &[]),
            variable("A_ARG_TYPE_UpdateID", "ui4", false, &[]),
            variable("SystemUpdateID", "ui4", true, &[]),
            variable("SearchCapabilities", "string", false, &[]),
            variable("SortCapabilities", "string", false, &[]),
        ],
    )
}

/// `cms.xml`: the ConnectionManager service description
pub fn connection_manager() -> String {
    scpd(
        &[
            action(
                "GetProtocolInfo",
                &[
                    ("Source", "out", "SourceProtocolInfo"),
                    ("Sink", "out", "SinkProtocolInfo"),
                ],
            ),
            action(
                "GetCurrentConnectionIDs",
                &[("ConnectionIDs", "out", "CurrentConnectionIDs")],
            ),
            action(
                "GetCurrentConnectionInfo",
                &[
                    ("ConnectionID", "in", "A_ARG_TYPE_ConnectionID"),
                    ("RcsID", "out", "A_ARG_TYPE_RcsID"),
                    ("AVTransportID", "out", "A_ARG_TYPE_AVTransportID"),
                    ("ProtocolInfo", "out", "A_ARG_TYPE_ProtocolInfo"),
                    (
                        "PeerConnectionManager",
                        "out",
                        "A_ARG_TYPE_ConnectionManager",
                    ),
                    ("PeerConnectionID", "out", "A_ARG_TYPE_ConnectionID"),
                    ("Direction", "out", "A_ARG_TYPE_Direction"),
                    ("Status", "out", "A_ARG_TYPE_ConnectionStatus"),
                ],
            ),
        ],
        &[
            variable("SourceProtocolInfo", "string", true, &[]),
            variable("SinkProtocolInfo", "string", true, &[]),
            variable("CurrentConnectionIDs", "string", true, &[]),
            variable(
                "A_ARG_TYPE_ConnectionStatus",
                "string",
                false,
                &["OK", "Unknown"],
            ),
            variable("A_ARG_TYPE_ConnectionManager", "string", false, &[]),
            variable(
                "A_ARG_TYPE_Direction",
                "string",
                false,
                &["Input", "Output"],
            ),
            variable("A_ARG_TYPE_ProtocolInfo", "string", false, &[]),
            variable("A_ARG_TYPE_ConnectionID", "i4", false, &[]),
            variable("A_ARG_TYPE_AVTransportID", "i4", false, &[]),
            variable("A_ARG_TYPE_RcsID", "i4", false, &[]),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn well_formed(xml: &str) {
        let mut reader = quick_xml::Reader::from_str(xml);
        loop {
            if reader.read_event().expect("well-formed xml") == quick_xml::events::Event::Eof {
                break;
            }
        }
    }

    #[test]
    fn device_escapes_the_name_and_lists_both_services() {
        let x = device("Tom & <Jerry>", "abc");
        well_formed(&x);
        assert!(x.contains("<friendlyName>Tom &amp; &lt;Jerry&gt;</friendlyName>"));
        assert!(x.contains("<UDN>uuid:abc</UDN>"));
        assert!(x.contains("urn:schemas-upnp-org:device:MediaServer:1"));
        assert!(x.contains("<controlURL>/ctl/cds</controlURL>"));
        assert!(x.contains("<eventSubURL>/evt/cms</eventSubURL>"));
    }

    #[test]
    fn scpds_are_well_formed_and_list_their_actions() {
        let cds = content_directory();
        well_formed(&cds);
        for a in [
            "Browse",
            "GetSystemUpdateID",
            "GetSearchCapabilities",
            "GetSortCapabilities",
        ] {
            assert!(cds.contains(&format!("<name>{a}</name>")), "{a}");
        }
        let cms = connection_manager();
        well_formed(&cms);
        for a in [
            "GetProtocolInfo",
            "GetCurrentConnectionIDs",
            "GetCurrentConnectionInfo",
        ] {
            assert!(cms.contains(&format!("<name>{a}</name>")), "{a}");
        }
    }
}
