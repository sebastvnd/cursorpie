//! The messages of the browser bridge, and the framing of the native-messaging pipe.
//!
//! Two links carry them (see [`bridge`](super)):
//!
//! * browser <-> host process: **native messaging**, `stdin`/`stdout` frames of a 4-byte length in
//!   the machine's byte order followed by that many bytes of JSON ([`read_frame`], [`write_frame`]);
//! * host process <-> the app: the same JSON, one message per line, over a loopback TCP socket that
//!   starts with an [`Auth`] line.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

/// The name of the native messaging host: the `name` of its manifest, the file name of that
/// manifest (`<name>.json`), and what `assets/browser-extension/background.js` connects to.
pub const HOST_NAME: &str = "io.github.cursorpie.bridge";

/// The biggest message a browser accepts from a host (Chromium: 1 MiB).
pub const MAX_TO_BROWSER: usize = 1024 * 1024;

/// The biggest message taken from the browser. Chromium allows 64 MiB; a list of tabs is far
/// smaller, so a bigger one is a mistake and ends the link.
pub const MAX_FROM_BROWSER: usize = 8 * 1024 * 1024;

/// Reads one native-messaging frame. `Ok(None)` when the other side closed the pipe.
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let n = u32::from_ne_bytes(len) as usize;
    if n > MAX_FROM_BROWSER {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native message too large",
        ));
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

/// Writes one native-messaging frame and flushes it.
pub fn write_frame(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_TO_BROWSER {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "native message too large",
        ));
    }
    w.write_all(&(payload.len() as u32).to_ne_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

/// What the extension tells the app.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FromBrowser {
    /// First message of a connection: which browser this is.
    Hello {
        browser: String,
        #[serde(default)]
        version: String,
    },
    /// The whole list of open `http(s)` tabs (it replaces the previous one).
    Tabs { tabs: Vec<WireTab> },
}

/// One open tab, as the extension reports it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WireTab {
    pub id: i64,
    #[serde(rename = "windowId")]
    pub window_id: i64,
    pub url: String,
    #[serde(default)]
    pub active: bool,
}

/// What the app asks the extension to do.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToBrowser {
    /// Go to this tab and focus its window; when the tab is gone, open `url`.
    Activate {
        #[serde(rename = "tabId")]
        tab_id: i64,
        #[serde(rename = "windowId")]
        window_id: i64,
        url: String,
    },
    /// Open `url` in a new window (`new_window`) or a new tab.
    Open {
        url: String,
        #[serde(rename = "newWindow")]
        new_window: bool,
    },
}

/// The first line a host process sends to the app.
#[derive(Debug, Serialize, Deserialize)]
pub struct Auth {
    pub auth: String,
}

/// `bridge.json` in the app's support folder: where the running app listens, and the secret a host
/// must send first.
#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeFile {
    pub port: u16,
    pub token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_goes_through_the_pipe_and_back() {
        let mut pipe = Vec::new();
        write_frame(&mut pipe, br#"{"type":"hello","browser":"firefox"}"#).unwrap();
        write_frame(&mut pipe, b"{}").unwrap();
        let mut r = pipe.as_slice();
        assert_eq!(
            read_frame(&mut r).unwrap().unwrap(),
            br#"{"type":"hello","browser":"firefox"}"#
        );
        assert_eq!(read_frame(&mut r).unwrap().unwrap(), b"{}");
        assert_eq!(read_frame(&mut r).unwrap(), None, "closed pipe");
    }

    #[test]
    fn a_frame_that_is_too_big_is_refused_both_ways() {
        let mut huge = ((MAX_FROM_BROWSER + 1) as u32).to_ne_bytes().to_vec();
        huge.extend_from_slice(b"x");
        assert!(read_frame(&mut huge.as_slice()).is_err());
        let big = vec![b'x'; MAX_TO_BROWSER + 1];
        assert!(write_frame(&mut Vec::new(), &big).is_err());
    }

    #[test]
    fn messages_from_the_extension_are_read() {
        let hello: FromBrowser =
            serde_json::from_str(r#"{"type":"hello","browser":"brave","version":"0.8.4"}"#)
                .unwrap();
        assert_eq!(
            hello,
            FromBrowser::Hello {
                browser: "brave".into(),
                version: "0.8.4".into()
            }
        );
        let tabs: FromBrowser = serde_json::from_str(
            r#"{"type":"tabs","tabs":[{"id":7,"windowId":2,"url":"https://a.b/","active":true},
                                      {"id":8,"windowId":2,"url":"https://c.d/"}]}"#,
        )
        .unwrap();
        let FromBrowser::Tabs { tabs } = tabs else {
            panic!("tabs")
        };
        assert_eq!(tabs.len(), 2);
        assert_eq!(
            (tabs[0].id, tabs[0].window_id, tabs[0].active),
            (7, 2, true)
        );
        assert!(!tabs[1].active, "missing means not active");
        assert!(serde_json::from_str::<FromBrowser>(r#"{"type":"nope"}"#).is_err());
    }

    #[test]
    fn messages_to_the_extension_use_the_names_of_the_script() {
        let a = serde_json::to_string(&ToBrowser::Activate {
            tab_id: 7,
            window_id: 2,
            url: "https://a.b/".into(),
        })
        .unwrap();
        assert_eq!(
            a,
            r#"{"type":"activate","tabId":7,"windowId":2,"url":"https://a.b/"}"#
        );
        let o = serde_json::to_string(&ToBrowser::Open {
            url: "https://a.b/".into(),
            new_window: true,
        })
        .unwrap();
        assert_eq!(
            o,
            r#"{"type":"open","url":"https://a.b/","newWindow":true}"#
        );
    }
}
