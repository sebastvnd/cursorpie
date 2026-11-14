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
        /// The commands this extension knows (`media.next`...). Empty for an extension from before
        /// commands existed, which only knows tabs.
        #[serde(default)]
        commands: Vec<String>,
    },
    /// The whole list of open `http(s)` tabs (it replaces the previous one).
    Tabs { tabs: Vec<WireTab> },
    /// How much is playing and how many chats wait: numbers only, never a title or a name. It
    /// replaces the previous one.
    State {
        /// Tabs that are making sound.
        #[serde(default)]
        audible: u32,
        /// Tabs on a player site (YouTube, Spotify...).
        #[serde(default, rename = "mediaTabs")]
        media_tabs: u32,
        /// The chat sites that are open, with their unread number when the page says it.
        #[serde(default)]
        chats: Vec<WireChat>,
    },
    /// What happened to a command the app sent (see [`ToBrowser::Command`]).
    #[serde(rename = "result")]
    Outcome {
        /// The `id` of the command.
        id: u64,
        #[serde(default)]
        ok: bool,
        /// What to tell the person: what went wrong, or the answer to a question.
        #[serde(default)]
        message: String,
        /// The extension has decided the person should be shown `message` (it follows its option
        /// "Notifications").
        #[serde(default)]
        notify: bool,
    },
    /// Show a panel (see [`crate::panel`]) and answer with [`ToBrowser::PanelResult`] carrying the
    /// same `req`. `spec` is the description of the panel; the app checks it and refuses what is
    /// not valid, and refuses all of them unless the person allowed panels from the extension.
    Panel { req: u64, spec: serde_json::Value },
    /// Change the open panel `panel` (progress, new options, a new title).
    #[serde(rename = "panel_update")]
    PanelUpdate {
        panel: String,
        patch: crate::panel::Patch,
    },
    /// Close the open panel `panel` as cancelled.
    #[serde(rename = "panel_close")]
    PanelClose { panel: String },
}

/// A chat site that is open (`whatsapp`, `telegram`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WireChat {
    pub site: String,
    /// The number the page shows in its title, when it shows one.
    #[serde(default)]
    pub unread: Option<u32>,
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
    /// Do something in the browser: `cmd` is one of [`commands::CATALOG`](super::commands::CATALOG)
    /// (`media.next`, `tab.close`...), `arg` its argument (empty when it has none). The extension
    /// answers with a [`FromBrowser::Outcome`] that has the same `id`.
    Command { id: u64, cmd: String, arg: String },
    /// The answer to a [`FromBrowser::Panel`]: `result` is a [`PanelResult`](crate::panel::PanelResult)
    /// as JSON (`outcome`, `button`, `values`, `reason`).
    #[serde(rename = "panel_result")]
    PanelResult { req: u64, result: serde_json::Value },
    /// The app has written the files of version `version` into the folder this extension was loaded
    /// from (it does that when it starts after an update): load them. The extension reloads itself
    /// once per version asked; one from before this message existed ignores it.
    Reload { version: String },
    /// The app has just accepted this connection (it started after the browser, or restarted):
    /// say hello again and send the open tabs now, without waiting for a tab event. An extension
    /// from before this message ignores it and keeps the old behavior (first tab event).
    Sync,
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
    fn sync_is_a_bare_type() {
        let out = serde_json::to_value(ToBrowser::Sync).unwrap();
        assert_eq!(out, serde_json::json!({ "type": "sync" }));
    }

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
    fn the_reload_request_names_the_version_to_load() {
        let out = serde_json::to_value(ToBrowser::Reload {
            version: "0.9.0".into(),
        })
        .unwrap();
        assert_eq!(out["type"], "reload");
        assert_eq!(out["version"], "0.9.0");
    }

    #[test]
    fn panel_messages_are_read_and_written() {
        let m: FromBrowser = serde_json::from_str(
            r#"{"type":"panel","req":7,"spec":{"title":"Hi","widgets":[{"type":"input","id":"a"}]}}"#,
        )
        .unwrap();
        let FromBrowser::Panel { req, spec } = m else {
            panic!("panel")
        };
        assert_eq!(req, 7);
        assert_eq!(spec["title"], "Hi");
        let m: FromBrowser = serde_json::from_str(
            r#"{"type":"panel_update","panel":"p","patch":{"widget":"bar","value":0.5}}"#,
        )
        .unwrap();
        let FromBrowser::PanelUpdate { panel, patch } = m else {
            panic!("update")
        };
        assert_eq!((panel.as_str(), patch.widget.as_str()), ("p", "bar"));
        assert_eq!(
            serde_json::from_str::<FromBrowser>(r#"{"type":"panel_close","panel":"p"}"#).unwrap(),
            FromBrowser::PanelClose { panel: "p".into() }
        );
        let out = serde_json::to_value(ToBrowser::PanelResult {
            req: 7,
            result: serde_json::json!({"outcome":"cancelled"}),
        })
        .unwrap();
        assert_eq!(out["type"], "panel_result");
        assert_eq!(out["req"], 7);
        assert_eq!(out["result"]["outcome"], "cancelled");
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
                version: "0.8.4".into(),
                commands: Vec::new(),
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
    fn hello_state_and_results_of_the_new_extension_are_read() {
        let hello: FromBrowser = serde_json::from_str(
            r#"{"type":"hello","browser":"firefox","version":"0.9.0","commands":["media.next","tab.close"]}"#,
        )
        .unwrap();
        let FromBrowser::Hello { commands, .. } = hello else {
            panic!("hello")
        };
        assert_eq!(commands, ["media.next", "tab.close"]);

        let state: FromBrowser = serde_json::from_str(
            r#"{"type":"state","audible":1,"mediaTabs":3,
                "chats":[{"site":"whatsapp","unread":4},{"site":"telegram","unread":null}]}"#,
        )
        .unwrap();
        assert_eq!(
            state,
            FromBrowser::State {
                audible: 1,
                media_tabs: 3,
                chats: vec![
                    WireChat {
                        site: "whatsapp".into(),
                        unread: Some(4)
                    },
                    WireChat {
                        site: "telegram".into(),
                        unread: None
                    }
                ],
            }
        );
        // every field of a state may be missing: an older or smaller extension still parses
        assert_eq!(
            serde_json::from_str::<FromBrowser>(r#"{"type":"state"}"#).unwrap(),
            FromBrowser::State {
                audible: 0,
                media_tabs: 0,
                chats: Vec::new()
            }
        );

        let result: FromBrowser = serde_json::from_str(
            r#"{"type":"result","id":12,"ok":false,"message":"No player found","notify":true}"#,
        )
        .unwrap();
        assert_eq!(
            result,
            FromBrowser::Outcome {
                id: 12,
                ok: false,
                message: "No player found".into(),
                notify: true
            }
        );
        // a result needs the id of its command
        assert!(serde_json::from_str::<FromBrowser>(r#"{"type":"result","ok":true}"#).is_err());
    }

    #[test]
    fn a_command_goes_to_the_extension_with_the_names_of_the_script() {
        let c = serde_json::to_string(&ToBrowser::Command {
            id: 3,
            cmd: "chat.send_to".into(),
            arg: "Anna :: \"hi\"\nbye".into(),
        })
        .unwrap();
        assert_eq!(
            c,
            r#"{"type":"command","id":3,"cmd":"chat.send_to","arg":"Anna :: \"hi\"\nbye"}"#
        );
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
