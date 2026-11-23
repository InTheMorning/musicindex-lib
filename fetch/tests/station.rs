//! The checks of musicindex ADR 0092 §Checks for musicindex-lib. The tests
//! use a local HTTP server.

// Each time in a test feed is an exact binary number, so an exact compare is
// correct.
#![allow(clippy::float_cmp)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use musicindex_fetch::{
    DownloadError, FeedDocument, FeedError, FeedReader, ReaderConfig, RemoteItemRef, ResolveError,
    download_enclosure, song_list,
};
use musicindex_tags::SplitTarget;
use serde_json::json;

/// One answer of the test server.
#[derive(Clone)]
struct Answer {
    status: u16,
    body: Vec<u8>,
    /// The `Content-Length` to send. `None` sends the length of `body`.
    length: Option<usize>,
}

fn ok(body: impl Into<Vec<u8>>) -> Answer {
    Answer {
        status: 200,
        body: body.into(),
        length: None,
    }
}

/// An answer that stops before the end of the body that its
/// `Content-Length` gives.
fn cut(body: impl Into<Vec<u8>>, length: usize) -> Answer {
    Answer {
        length: Some(length),
        ..ok(body)
    }
}

fn status(status: u16) -> Answer {
    Answer {
        status,
        body: Vec::new(),
        length: None,
    }
}

/// A local HTTP server. It gives the answer of each path and records each
/// request path.
struct Server {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start(answers: HashMap<&'static str, Answer>) -> Self {
        start_on(
            TcpListener::bind("127.0.0.1:0").expect("bind the test server"),
            answers,
        )
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn count(&self, path: &str) -> usize {
        self.requests
            .lock()
            .expect("lock")
            .iter()
            .filter(|request| *request == path)
            .count()
    }

    fn reader(&self) -> FeedReader {
        FeedReader::new(ReaderConfig {
            min_interval: Duration::ZERO,
            musicindex_base_url: self.base.clone(),
            ..ReaderConfig::default()
        })
        .expect("make the reader")
    }
}

const NODE_A: &str = "02aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NODE_B: &str = "03bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NODE_C: &str = "02cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

/// Song feed A: the item has its own value block.
fn song_feed_a(base: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel>
  <title>Album A</title>
  <podcast:guid>feed-a</podcast:guid>
  <podcast:medium>music</podcast:medium>
  <itunes:author>Artist A</itunes:author>
  <item>
    <title>Song A</title>
    <itunes:author>Artist A</itunes:author>
    <guid>song-a-1</guid>
    <enclosure url="{base}/audio/a.mp3" type="audio/mpeg" length="5"/>
    <podcast:value type="lightning" method="keysend">
      <podcast:valueRecipient name="Artist A" type="node" address="{NODE_A}" split="95"/>
      <podcast:valueRecipient name="App" type="node" address="{NODE_C}" split="5" fee="true"/>
    </podcast:value>
  </item>
  <item>
    <title>Song A2</title>
    <guid>song-a-2</guid>
    <enclosure url="{base}/audio/a2.mp3" type="audio/mpeg" length="5"/>
  </item>
</channel>
</rss>"#
    )
}

/// Song feed B: the item has no value block, so the channel block applies
/// (musicindex ADR 0093 R-TAG-6 item 1).
fn song_feed_b(base: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel>
  <title>Album B</title>
  <podcast:guid>feed-b</podcast:guid>
  <podcast:medium>music</podcast:medium>
  <itunes:author>Artist B</itunes:author>
  <podcast:value type="lightning" method="keysend">
    <podcast:valueRecipient name="Artist B" type="node" address="{NODE_B}" split="100"/>
  </podcast:value>
  <item>
    <title>Song B</title>
    <guid>song-b-1</guid>
    <enclosure url="{base}/audio/b.mp3" type="audio/mpeg" length="5"/>
  </item>
</channel>
</rss>"#
    )
}

/// A `musicL` feed with two remote items: one with a feed URL, and one with
/// only a feed GUID.
fn music_list(base: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
  <title>Station list</title>
  <podcast:guid>list-1</podcast:guid>
  <podcast:medium>musicL</podcast:medium>
  <podcast:remoteItem feedGuid="feed-a" feedUrl="{base}/feeds/a.xml" itemGuid="song-a-1"/>
  <podcast:remoteItem feedGuid="feed-b" itemGuid="song-b-1"/>
</channel>
</rss>"#
    )
}

fn lookup_b(base: &str) -> Answer {
    ok(
        json!({ "data": { "feed_guid": "feed-b", "feed_url": format!("{base}/feeds/b.xml") } })
            .to_string(),
    )
}

/// Starts a server with the two song feeds and the MusicIndex lookup of feed
/// B. The feeds hold URLs of the server, so the test finds a free port
/// before the start.
fn song_server() -> Server {
    let probe = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = probe.local_addr().expect("address").port();
    drop(probe);
    let base = format!("http://127.0.0.1:{port}");
    let answers = HashMap::from([
        ("/feeds/a.xml", ok(song_feed_a(&base))),
        ("/feeds/b.xml", ok(song_feed_b(&base))),
        ("/v1/feeds/feed-b", lookup_b(&base)),
    ]);
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind the song server");
    start_on(listener, answers)
}

/// Answers each request on `listener` from `answers`, and records the path.
fn start_on(listener: TcpListener, answers: HashMap<&'static str, Answer>) -> Server {
    let base = format!("http://{}", listener.local_addr().expect("server address"));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().expect("clone the stream"));
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                    break;
                }
            }
            recorded.lock().expect("lock").push(path.clone());
            let answer = answers.get(path.as_str()).cloned().unwrap_or(status(404));
            let head = format!(
                "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                answer.status,
                answer.length.unwrap_or(answer.body.len())
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&answer.body);
        }
    });
    Server { base, requests }
}

/// R-STATION-1 items 2 and 3: a `musicL` feed file with two remote items
/// gives the item GUID, the enclosure URL and the value block of each song.
#[test]
fn a_music_list_gives_each_song() {
    let server = song_server();
    let list = FeedDocument::parse("list.xml", music_list(&server.base).into_bytes())
        .expect("parse the musicL feed");
    let songs = song_list(&list.values);
    assert_eq!(songs.len(), 2, "musicindex ADR 0092 R-STATION-3 item 1");
    assert_eq!(songs[1].feed_url, None);

    let mut reader = server.reader();
    let a = reader.resolve(&songs[0]).expect("resolve song A");
    assert_eq!(a.item_guid, "song-a-1");
    assert_eq!(a.title.as_deref(), Some("Song A"));
    assert_eq!(a.artist.as_deref(), Some("Artist A"));
    assert_eq!(a.enclosure_url, Some(server.url("/audio/a.mp3")));
    assert_eq!(
        a.value_routes,
        Some(json!([
            {"recipient_name": "Artist A", "route_type": "node", "split": 95.0, "fee": false,
             "address": NODE_A, "custom_key": null, "custom_value": null},
            {"recipient_name": "App", "route_type": "node", "split": 5.0, "fee": true,
             "address": NODE_C, "custom_key": null, "custom_value": null},
        ])),
        "musicindex ADR 0092 R-STATION-1 item 3: the value block of the item"
    );

    let b = reader
        .resolve(&songs[1])
        .expect("resolve song B by its feed GUID");
    assert_eq!(
        server.count("/v1/feeds/feed-b"),
        1,
        "musicindex ADR 0092 R-STATION-1 item 3: the feed URL comes from MusicIndex"
    );
    assert_eq!(b.feed_url, server.url("/feeds/b.xml"));
    assert_eq!(
        (b.artist.as_deref(), b.author.as_deref()),
        (None, Some("Artist B")),
        "musicindex ADR 0093 R-TAG-4 item 4: the feed author never replaces the artist"
    );
    assert_eq!(b.enclosure_url, Some(server.url("/audio/b.mp3")));
    assert_eq!(
        b.value_routes,
        Some(json!([
            {"recipient_name": "Artist B", "route_type": "node", "split": 100.0, "fee": false,
             "address": NODE_B, "custom_key": null, "custom_value": null},
        ])),
        "musicindex ADR 0093 R-TAG-6 item 1: the channel block when the item has none"
    );
}

/// R-STATION-1 item 2: two items of one feed give one feed request.
#[test]
fn two_items_of_one_feed_give_one_request() {
    let server = song_server();
    let list = FeedDocument::parse("list.xml", music_list(&server.base).into_bytes())
        .expect("parse the musicL feed");
    let first = song_list(&list.values).remove(0);
    let second = RemoteItemRef {
        item_guid: Some("song-a-2".to_owned()),
        title: None,
        ..first.clone()
    };
    let mut reader = server.reader();
    let one = reader.resolve(&first).expect("first item");
    let two = reader.resolve(&second).expect("second item");
    assert_eq!(one.item_guid, "song-a-1");
    assert_eq!(two.item_guid, "song-a-2");
    assert_eq!(two.enclosure_url, Some(server.url("/audio/a2.mp3")));
    assert_eq!(
        server.count("/feeds/a.xml"),
        1,
        "musicindex ADR 0092 R-STATION-1 item 2: the reader reads each feed URL one time"
    );
}

/// R-STATION-1 item 2: HTTP 429 stops the next request to that host.
#[test]
fn http_429_stops_the_next_request_to_the_host() {
    let server = Server::start(HashMap::from([
        ("/feeds/one.xml", status(429)),
        ("/feeds/two.xml", ok("<rss/>")),
    ]));
    let mut reader = server.reader();
    assert!(matches!(
        reader.read(&server.url("/feeds/one.xml")),
        Err(FeedError::Throttled(_))
    ));
    let second = reader.read(&server.url("/feeds/two.xml"));
    assert!(
        matches!(second, Err(FeedError::HostStopped { .. })),
        "musicindex ADR 0092 R-STATION-1 item 2: no request after HTTP 429: {second:?}"
    );
    assert_eq!(
        server.count("/feeds/two.xml"),
        0,
        "musicindex ADR 0092 R-STATION-1 item 2: the server got a request after HTTP 429"
    );
    assert_eq!(reader.stopped_hosts().collect::<Vec<_>>(), ["127.0.0.1"]);
}

/// R-STATION-1 item 4: a failed download leaves no file at the path.
#[test]
fn a_failed_download_leaves_no_file() {
    let server = Server::start(HashMap::new());
    let folder = tempfile::TempDir::new().expect("temporary folder");
    let path = folder.path().join("songs/missing.mp3");
    let result = download_enclosure(&server.url("/audio/missing.mp3"), &path, "test");
    assert!(matches!(
        result,
        Err(DownloadError::Status { status: 404, .. })
    ));
    assert!(
        !path.exists(),
        "musicindex ADR 0092 R-STATION-1 item 4: a failed download left a file"
    );
    let left: Vec<_> = std::fs::read_dir(folder.path().join("songs"))
        .expect("the folder exists")
        .collect();
    assert!(
        left.is_empty(),
        "musicindex ADR 0092 R-STATION-1 item 4: the temporary file stayed: {left:?}"
    );
}

/// R-STATION-1 item 4: a body that stops after the write starts leaves no
/// file at the path, no temporary file, and the file that was at the path.
#[test]
fn a_cut_download_leaves_no_new_file() {
    let server = Server::start(HashMap::from([(
        "/audio/cut.mp3",
        cut(b"ID3 part".to_vec(), 4096),
    )]));
    let folder = tempfile::TempDir::new().expect("temporary folder");
    let path = folder.path().join("cut.mp3");
    std::fs::write(&path, b"old file").expect("write the old file");
    let result = download_enclosure(&server.url("/audio/cut.mp3"), &path, "test");
    assert!(result.is_err(), "the cut body gave {result:?}");
    assert_eq!(
        std::fs::read(&path).expect("read the old file"),
        b"old file",
        "musicindex ADR 0092 R-STATION-1 item 4: a failed download changed the file"
    );
    let names: Vec<_> = std::fs::read_dir(folder.path())
        .expect("read the folder")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(
        names,
        ["cut.mp3"],
        "musicindex ADR 0092 R-STATION-1 item 4: the temporary file stayed"
    );
}

/// R-STATION-1 item 4: a good download gives the SHA-256 hash of the file.
#[test]
fn a_good_download_gives_the_sha256_hash() {
    let audio = b"ID3 test audio bytes".to_vec();
    let server = Server::start(HashMap::from([("/audio/a.mp3", ok(audio.clone()))]));
    let folder = tempfile::TempDir::new().expect("temporary folder");
    let path = folder.path().join("a.mp3");
    let download =
        download_enclosure(&server.url("/audio/a.mp3"), &path, "test").expect("download");
    // The SHA-256 hash of the bytes, from `sha256sum`.
    let expected = "9c78bdd8559354187b2450347cc6f39f5b43ca6cf284c29449ed7ad2f4b9f32d";
    assert_eq!(
        download.sha256, expected,
        "musicindex ADR 0092 R-STATION-1 item 4: the hash of the file"
    );
    assert_eq!(download.bytes, audio.len() as u64);
    assert_eq!(std::fs::read(&path).expect("read the file"), audio);
}

/// An episode feed with a remote split, a remote split with no `itemGuid`
/// and a local split.
fn episode_feed() -> String {
    format!(
        r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
  <title>Show</title>
  <podcast:guid>show-1</podcast:guid>
  <item>
    <title>Episode 157</title>
    <guid>episode-157</guid>
    <podcast:value type="lightning" method="keysend">
      <podcast:valueRecipient name="Host" type="node" address="{NODE_C}" split="100"/>
      <podcast:valueTimeSplit startTime="60" duration="180" remotePercentage="90">
        <podcast:remoteItem feedGuid="feed-b" itemGuid="song-b-1"/>
      </podcast:valueTimeSplit>
      <podcast:valueTimeSplit startTime="300" duration="120">
        <podcast:remoteItem feedGuid="feed-b"/>
      </podcast:valueTimeSplit>
      <podcast:valueTimeSplit startTime="500.5" duration="60">
        <podcast:valueRecipient name="Guest" type="node" address="{NODE_A}" split="100"/>
      </podcast:valueTimeSplit>
    </podcast:value>
  </item>
</channel>
</rss>"#
    )
}

/// R-STATION-2 item 1: the `valueTimeSplit` elements of an episode give
/// `Split` values with the routes of R-TAG-6.
#[test]
fn the_time_splits_of_an_episode_give_split_values() {
    let server = song_server();
    let episode = FeedDocument::parse("episode.xml", episode_feed().into_bytes())
        .expect("parse the episode feed");
    let mut reader = server.reader();
    let result = reader
        .episode_splits(&episode.values.tracks[0])
        .expect("the splits are good");
    let splits = result.splits;
    assert_eq!(splits.len(), 3, "musicindex ADR 0092 R-STATION-2 item 1");

    assert_eq!(splits[0].start_time, 60.0);
    assert_eq!(splits[0].duration, 180.0);
    assert_eq!(splits[0].remote_percentage, Some(90));
    assert_eq!(splits[0].title.as_deref(), Some("Song B"));
    assert_eq!(splits[0].author.as_deref(), Some("Artist B"));
    assert_eq!(
        splits[0].target,
        SplitTarget::Remote {
            feed_guid: "feed-b".into(),
            item_guid: Some("song-b-1".into()),
            value_routes: Some(json!([
                {"recipient_name": "Artist B", "route_type": "node", "split": 100.0, "fee": false,
                 "address": NODE_B, "custom_key": null, "custom_value": null},
            ])),
        },
        "musicindex ADR 0092 R-STATION-2 item 1: the routes of the song (R-TAG-6 item 1)"
    );

    assert_eq!(
        splits[1].target,
        SplitTarget::Remote {
            feed_guid: "feed-b".into(),
            item_guid: None,
            value_routes: None,
        },
        "musicindex ADR 0093 R-TAG-6 item 2: no itemGuid gives no routes"
    );
    assert!(matches!(
        result.unresolved.as_slice(),
        [(1, ResolveError::NoItemGuid { .. })]
    ));

    assert_eq!(splits[2].start_time, 500.5);
    assert_eq!(splits[2].remote_percentage, None);
    assert_eq!(
        splits[2].target,
        SplitTarget::Local {
            value_routes: json!([
                {"recipient_name": "Guest", "route_type": "node", "split": 100.0, "fee": false,
                 "address": NODE_A, "custom_key": null, "custom_value": null},
            ]),
        },
        "musicindex ADR 0092 R-STATION-2 item 1: a local split keeps its recipients"
    );
}
