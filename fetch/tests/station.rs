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
    DownloadError, FeedDocument, FeedError, FeedReader, MAX_IMAGE_URL_CHARS, ReaderConfig,
    RemoteItemRef, ResolveError, download_enclosure, image_url, song_list, track_mp3_url,
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
    /// The `Content-Encoding` to send. `None` sends no header.
    encoding: Option<&'static str>,
    /// The `Location` to send. `None` sends no header.
    location: Option<String>,
}

fn ok(body: impl Into<Vec<u8>>) -> Answer {
    Answer {
        status: 200,
        body: body.into(),
        length: None,
        encoding: None,
        location: None,
    }
}

/// An answer with a body that the server compressed with `encoding`.
fn compressed(body: Vec<u8>, encoding: &'static str) -> Answer {
    Answer {
        encoding: Some(encoding),
        ..ok(body)
    }
}

fn brotli_bytes(text: &str) -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut output, 4096, 5, 22);
        writer.write_all(text.as_bytes()).expect("compress");
    }
    output
}

fn gzip_bytes(text: &str) -> Vec<u8> {
    let mut writer = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    writer.write_all(text.as_bytes()).expect("compress");
    writer.finish().expect("compress")
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
        encoding: None,
        location: None,
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
/// (musicindex ADR 0101 R-TAG-6 item 1).
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

/// Song feed C: the item has a Lightning block with no recipient, and the
/// channel has a block with a recipient.
fn song_feed_c() -> String {
    format!(
        r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
  <title>Album C</title>
  <podcast:guid>feed-c</podcast:guid>
  <podcast:medium>music</podcast:medium>
  <podcast:value type="lightning" method="keysend">
    <podcast:valueRecipient name="Label C" type="node" address="{NODE_B}" split="100"/>
  </podcast:value>
  <item>
    <title>Song C</title>
    <guid>song-c-1</guid>
    <podcast:value type="lightning" method="keysend"></podcast:value>
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
    start_with(|base| {
        HashMap::from([
            ("/feeds/a.xml", ok(song_feed_a(base))),
            ("/feeds/b.xml", ok(song_feed_b(base))),
            ("/feeds/c.xml", ok(song_feed_c())),
            ("/v1/feeds/feed-b", lookup_b(base)),
        ])
    })
}

/// Starts a server with the answers that `answers` makes from the base URL
/// of the server.
fn start_with(answers: impl FnOnce(&str) -> HashMap<&'static str, Answer>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the test server");
    let base = format!("http://{}", listener.local_addr().expect("server address"));
    let answers = answers(&base);
    start_on(listener, answers)
}

/// An HTTP 301 answer to `location`.
fn redirect(location: impl Into<String>) -> Answer {
    Answer {
        location: Some(location.into()),
        ..status(301)
    }
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
            let encoding = answer
                .encoding
                .map(|encoding| format!("Content-Encoding: {encoding}\r\n"))
                .unwrap_or_default();
            let location = answer
                .location
                .as_ref()
                .map(|location| format!("Location: {location}\r\n"))
                .unwrap_or_default();
            let head = format!(
                "HTTP/1.1 {} X\r\nContent-Length: {}\r\n{encoding}{location}Connection: close\r\n\r\n",
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
        "musicindex ADR 0101 R-TAG-4 item 4: the feed author never replaces the artist"
    );
    assert_eq!(b.enclosure_url, Some(server.url("/audio/b.mp3")));
    assert_eq!(
        b.value_routes,
        Some(json!([
            {"recipient_name": "Artist B", "route_type": "node", "split": 100.0, "fee": false,
             "address": NODE_B, "custom_key": null, "custom_value": null},
        ])),
        "musicindex ADR 0101 R-TAG-6 item 1: the channel block when the item has none"
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
            image_url: None,
        },
        "musicindex ADR 0092 R-STATION-2 item 1: the routes of the song (R-TAG-6 item 1)"
    );

    assert_eq!(
        splits[1].target,
        SplitTarget::Remote {
            feed_guid: "feed-b".into(),
            item_guid: None,
            value_routes: None,
            image_url: None,
        },
        "musicindex ADR 0101 R-TAG-6 item 2: no itemGuid gives no routes"
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

/// A song feed for the split image test. `channel` and `item` are the image
/// XML of the channel and of the item `song-guid`.
fn split_image_feed(feed_guid: &str, channel: &str, item: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel>
  <title>Album {feed_guid}</title>
  <podcast:guid>{feed_guid}</podcast:guid>
  <podcast:medium>music</podcast:medium>
  {channel}
  <item><title>Song {feed_guid}</title><guid>song-guid</guid>{item}</item>
</channel>
</rss>"#
    )
}

/// An episode with a remote split to each feed of
/// [`a_remote_split_gets_the_image_url_of_its_song`], one remote split to a
/// feed that the server does not have, and a local split.
fn split_image_episode(server: &Server) -> String {
    let split = |start: u32, feed: &str| {
        format!(
            r#"<podcast:valueTimeSplit startTime="{start}" duration="10">
        <podcast:remoteItem feedGuid="{feed}" feedUrl="{}" itemGuid="song-guid"/>
      </podcast:valueTimeSplit>"#,
            server.url(&format!("/feeds/{}.xml", feed.trim_start_matches("feed-")))
        )
    };
    format!(
        r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
  <title>Show</title>
  <podcast:guid>show-1</podcast:guid>
  <item>
    <title>Episode 159</title>
    <guid>episode-159</guid>
    <podcast:value type="lightning" method="keysend">
      <podcast:valueRecipient name="Host" type="node" address="{NODE_C}" split="100"/>
      {}
      {}
      {}
      {}
      <podcast:valueTimeSplit startTime="50" duration="10">
        <podcast:valueRecipient name="Guest" type="node" address="{NODE_A}" split="100"/>
      </podcast:valueTimeSplit>
    </podcast:value>
  </item>
</channel>
</rss>"#,
        split(0, "feed-image"),
        split(10, "feed-long"),
        split(20, "feed-none"),
        split(30, "feed-missing"),
    )
}

/// musicindex ADR 0099 R-IMAGE-4: a remote split gets the image URL of its
/// song from the song feed, with the sources and the URL limit of ADR 0096
/// R-IMAGE-1 items 2 and 3. A song with no image, a song feed that the reader
/// cannot read and a local split give `None`, and the other fields stay.
#[test]
fn a_remote_split_gets_the_image_url_of_its_song() {
    let long = format!("https://example.com/{}.jpg", "a".repeat(2049 - 24));
    assert_eq!(long.chars().count(), 2049);
    let server = start_with(|_| {
        HashMap::from([
            (
                "/feeds/image.xml",
                ok(split_image_feed(
                    "feed-image",
                    r#"<itunes:image href="https://example.com/channel.jpg"/>"#,
                    r#"<itunes:image href="https://example.com/song.jpg"/>"#,
                )),
            ),
            (
                "/feeds/long.xml",
                ok(split_image_feed(
                    "feed-long",
                    r#"<itunes:image href="https://example.com/long-channel.jpg"/>"#,
                    &format!(r#"<itunes:image href="{long}"/>"#),
                )),
            ),
            ("/feeds/none.xml", ok(split_image_feed("feed-none", "", ""))),
        ])
    });
    let episode = split_image_episode(&server);
    let episode =
        FeedDocument::parse("episode.xml", episode.into_bytes()).expect("parse the episode feed");
    let mut reader = server.reader();
    let result = reader
        .episode_splits(&episode.values.tracks[0])
        .expect("the splits are good");
    let images = result
        .splits
        .iter()
        .map(|split| match &split.target {
            SplitTarget::Remote { image_url, .. } => image_url.clone(),
            SplitTarget::Local { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        images,
        vec![
            Some("https://example.com/song.jpg".to_owned()),
            Some("https://example.com/long-channel.jpg".to_owned()),
            None,
            None,
            None,
        ],
        "musicindex ADR 0099 R-IMAGE-4 items 2 to 5"
    );
    assert!(matches!(result.splits[4].target, SplitTarget::Local { .. }));
    assert_eq!(
        result.splits[2].title.as_deref(),
        Some("Song feed-none"),
        "musicindex ADR 0099 R-IMAGE-4 item 5: a song with no image keeps its other fields"
    );
    assert!(
        matches!(
            &result.splits[3].target,
            SplitTarget::Remote { feed_guid, item_guid: Some(_), .. } if feed_guid == "feed-missing"
        ),
        "musicindex ADR 0099 R-IMAGE-4 item 5: an unreadable song feed keeps the split"
    );
    assert!(
        matches!(result.unresolved.as_slice(), [(3, _)]),
        "{:?}",
        result.unresolved
    );
}

/// musicindex ADR 0101 R-TAG-6 item 1: an item block with no recipient is a
/// block. The channel block does not replace it.
#[test]
fn an_item_block_with_no_recipient_is_a_block() {
    let server = song_server();
    let mut reader = server.reader();
    let song = reader
        .resolve(&RemoteItemRef {
            feed_guid: "feed-c".to_owned(),
            feed_url: Some(server.url("/feeds/c.xml")),
            item_guid: Some("song-c-1".to_owned()),
            title: None,
        })
        .expect("resolve song C");
    assert_eq!(
        song.value_routes,
        Some(json!([])),
        "musicindex ADR 0101 R-TAG-6 item 1: the item has a block, so the channel block does not apply"
    );
}

/// musicindex ADR 0092 R-STATION-2 item 1: a remote split with a `feedUrl`
/// reads that feed and makes no MusicIndex lookup.
#[test]
fn a_split_with_a_feed_url_reads_that_feed() {
    let server = song_server();
    let episode = format!(
        r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
  <title>Show</title>
  <podcast:guid>show-1</podcast:guid>
  <item>
    <title>Episode 158</title>
    <guid>episode-158</guid>
    <podcast:value type="lightning" method="keysend">
      <podcast:valueRecipient name="Host" type="node" address="{NODE_C}" split="100"/>
      <podcast:valueTimeSplit startTime="0" duration="200">
        <podcast:remoteItem feedGuid="feed-a" feedUrl="{}" itemGuid="song-a-1"/>
      </podcast:valueTimeSplit>
    </podcast:value>
  </item>
</channel>
</rss>"#,
        server.url("/feeds/a.xml")
    );
    let episode =
        FeedDocument::parse("episode.xml", episode.into_bytes()).expect("parse the episode feed");
    let mut reader = server.reader();
    let result = reader
        .episode_splits(&episode.values.tracks[0])
        .expect("the split is good");
    assert!(result.unresolved.is_empty(), "{:?}", result.unresolved);
    assert_eq!(result.splits[0].title.as_deref(), Some("Song A"));
    assert!(
        matches!(
            &result.splits[0].target,
            SplitTarget::Remote {
                value_routes: Some(_),
                ..
            }
        ),
        "musicindex ADR 0101 R-TAG-6 item 1: the split gets the routes of the song"
    );
    assert_eq!(server.count("/feeds/a.xml"), 1);
    assert_eq!(
        server.count("/v1/feeds/feed-a"),
        0,
        "musicindex ADR 0092 R-STATION-2 item 1: the feedUrl of the split comes before the lookup"
    );
}

/// R-STATION-1 item 2: the reader decodes a feed body that the server
/// compresses with Brotli or gzip, also when the request has no
/// `Accept-Encoding`.
#[test]
fn a_compressed_feed_gives_the_feed() {
    let feed = song_feed_c();
    let server = Server::start(HashMap::from([
        ("/feeds/br.xml", compressed(brotli_bytes(&feed), "br")),
        ("/feeds/gzip.xml", compressed(gzip_bytes(&feed), "gzip")),
    ]));
    let mut reader = server.reader();
    for path in ["/feeds/br.xml", "/feeds/gzip.xml"] {
        let document = reader
            .read(&server.url(path))
            .unwrap_or_else(|error| panic!("{path}: {error}"));
        assert_eq!(
            document.body,
            feed.as_bytes(),
            "{path}: the reader keeps the decoded body"
        );
        assert_eq!(
            document.values.tracks.len(),
            1,
            "{path}: the parser reads the feed"
        );
    }
}

/// R-STATION-1 item 4: a download keeps the bytes of the answer. The client
/// does not decode a body with a `Content-Encoding`.
#[test]
fn a_download_keeps_the_bytes_of_the_answer() {
    let body = gzip_bytes("ID3 test audio bytes");
    let server = Server::start(HashMap::from([(
        "/audio/gzip.mp3",
        compressed(body.clone(), "gzip"),
    )]));
    let folder = tempfile::TempDir::new().expect("temporary folder");
    let path = folder.path().join("gzip.mp3");
    let download =
        download_enclosure(&server.url("/audio/gzip.mp3"), &path, "test").expect("download");
    assert_eq!(std::fs::read(&path).expect("read the file"), body);
    assert_eq!(download.bytes, body.len() as u64);
}

/// R-STATION-1 item 4: a download follows a redirect with a raw space in
/// its `Location`. Real feeds have such values (HitIt musicindex ADR 0056).
#[test]
fn a_download_follows_a_location_with_a_space() {
    let audio = b"ID3 redirected audio".to_vec();
    let server = start_with(|base| {
        HashMap::from([
            (
                "/audio/old.mp3",
                redirect(format!("{base}/Music/song file.mp3")),
            ),
            ("/Music/song%20file.mp3", ok(audio.clone())),
        ])
    });
    let folder = tempfile::TempDir::new().expect("temporary folder");
    let path = folder.path().join("song.mp3");
    let download = download_enclosure(&server.url("/audio/old.mp3"), &path, "test")
        .expect("musicindex ADR 0092 R-STATION-1 item 4: follow the redirect");
    assert_eq!(std::fs::read(&path).expect("read the file"), audio);
    assert_eq!(download.bytes, audio.len() as u64);
}

/// R-STATION-3 item 1: the song list of a `musicL` feed does not contain the
/// remote item of the publisher.
#[test]
fn a_music_list_skips_the_publisher() {
    let feed = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
  <title>Station list</title>
  <podcast:guid>list-2</podcast:guid>
  <podcast:medium>musicL</podcast:medium>
  <podcast:publisher>
    <podcast:remoteItem feedGuid="publisher-1" feedUrl="https://example.com/publisher.xml"/>
  </podcast:publisher>
  <podcast:remoteItem medium="publisher" feedGuid="publisher-2"/>
  <podcast:remoteItem feedGuid="feed-a" itemGuid="song-a-1"/>
  <podcast:remoteItem feedGuid="feed-b" itemGuid="song-b-1"/>
</channel>
</rss>"#;
    let list = FeedDocument::parse("list.xml", feed.as_bytes().to_vec()).expect("parse the feed");
    let guids: Vec<_> = song_list(&list.values)
        .into_iter()
        .map(|song| song.feed_guid)
        .collect();
    assert_eq!(
        guids,
        ["feed-a", "feed-b"],
        "musicindex ADR 0092 R-STATION-3 item 1: a publisher remote item is not a song"
    );
}

/// A song feed for the MP3 selection. Each item has a different set of
/// enclosures.
fn mp3_feed(base: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0">
<channel>
  <title>Album M</title>
  <podcast:guid>feed-m</podcast:guid>
  <podcast:medium>music</podcast:medium>
  <item>
    <title>MP3 enclosure</title>
    <guid>m-mp3</guid>
    <enclosure url="{base}/audio/m1.mp3" type="audio/mpeg" length="5"/>
    <podcast:alternateEnclosure type="audio/mpeg" length="5">
      <podcast:source uri="{base}/audio/m1-alternate.mp3"/>
    </podcast:alternateEnclosure>
  </item>
  <item>
    <title>FLAC enclosure</title>
    <guid>m-flac</guid>
    <enclosure url="{base}/audio/m2.flac" type="audio/flac" length="5"/>
    <podcast:alternateEnclosure type="audio/flac" length="5">
      <podcast:source uri="{base}/audio/m2-alternate.flac"/>
    </podcast:alternateEnclosure>
    <podcast:alternateEnclosure type="audio/mpeg" length="5">
      <podcast:source uri="ipfs://only-ipfs-mp3"/>
    </podcast:alternateEnclosure>
    <podcast:alternateEnclosure type="Audio/MPEG; codecs=mp3" length="5" bitrate="128000">
      <podcast:source uri="ipfs://first-mp3"/>
      <podcast:source uri="{base}/audio/m2-first.mp3"/>
    </podcast:alternateEnclosure>
    <podcast:alternateEnclosure type="audio/mpeg" length="5" bitrate="320000">
      <podcast:source uri="{base}/audio/m2-second.mp3"/>
    </podcast:alternateEnclosure>
  </item>
  <item>
    <title>WAV enclosure</title>
    <guid>m-wav</guid>
    <enclosure url="{base}/audio/m3.wav" type="audio/wav" length="5"/>
  </item>
</channel>
</rss>"#
    )
}

/// musicindex ADR 0095 R-MP3-2 item 1: the MP3 selection gives the
/// enclosure when its type is `audio/mpeg`, else the first `audio/mpeg`
/// alternate enclosure in feed order, else none. A low bit rate does not
/// change the selection (R-MP3-2 item 2). An alternate enclosure with only
/// an `ipfs:` source has no HTTP URL, so it is not selected.
#[test]
fn the_mp3_file_of_an_item_follows_the_feed_order() {
    let base = "https://example.com";
    let feed = FeedDocument::parse("m.xml", mp3_feed(base).into_bytes()).expect("parse the feed");
    let urls: Vec<_> = feed.values.tracks.iter().map(track_mp3_url).collect();
    assert_eq!(
        urls,
        [
            Some("https://example.com/audio/m1.mp3"),
            Some("https://example.com/audio/m2-first.mp3"),
            None,
        ],
        "musicindex ADR 0095 R-MP3-2 item 1"
    );
}

/// musicindex ADR 0095 R-MP3-2 item 3: a resolved item keeps the alternate
/// enclosures, and its MP3 selection is the same as for the parser item.
#[test]
fn a_resolved_item_gives_its_alternate_enclosures() {
    let server = start_with(|base| HashMap::from([("/feeds/m.xml", ok(mp3_feed(base)))]));
    let mut reader = server.reader();
    let item = RemoteItemRef {
        feed_guid: "feed-m".into(),
        feed_url: Some(server.url("/feeds/m.xml")),
        item_guid: Some("m-flac".into()),
        title: None,
    };
    let resolved = reader.resolve(&item).expect("resolve the FLAC item");
    let types: Vec<_> = resolved
        .alternate_enclosures
        .iter()
        .map(|alternate| alternate.mime_type.as_deref())
        .collect();
    assert_eq!(
        types,
        [
            Some("audio/flac"),
            Some("Audio/MPEG; codecs=mp3"),
            Some("audio/mpeg"),
        ]
    );
    assert_eq!(
        resolved.alternate_enclosures[1].sources,
        [
            "ipfs://first-mp3".to_owned(),
            server.url("/audio/m2-first.mp3")
        ],
        "the URL of an alternate enclosure is in its podcast:source elements"
    );
    assert_eq!(
        resolved.mp3_url(),
        Some(server.url("/audio/m2-first.mp3").as_str()),
        "musicindex ADR 0095 R-MP3-2 item 1"
    );
}

/// A feed for the image selection. `channel` is the image XML of the
/// channel, and `items` is the XML of the items.
fn image_feed(channel: &str, items: &str) -> FeedDocument {
    let feed = format!(
        r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:podcast="https://podcastindex.org/namespace/1.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel>
  <title>Album I</title>
  <podcast:guid>feed-i</podcast:guid>
  <podcast:medium>music</podcast:medium>
  {channel}
  {items}
</channel>
</rss>"#
    );
    FeedDocument::parse("i.xml", feed.into_bytes()).expect("parse the feed")
}

fn image_item(guid: &str, images: &str) -> String {
    format!("<item><title>{guid}</title><guid>{guid}</guid>{images}</item>")
}

/// The image URL of each item of `feed`, in feed order.
fn image_urls(feed: &FeedDocument) -> Vec<Option<String>> {
    feed.values
        .tracks
        .iter()
        .map(|track| image_url(&feed.values, track))
        .collect()
}

/// musicindex ADR 0096 R-IMAGE-1 item 2: the order of the five sources.
#[test]
fn the_image_url_follows_the_order_of_the_sources() {
    let long = format!(
        "https://example.com/{}",
        "a".repeat(MAX_IMAGE_URL_CHARS + 1 - "https://example.com/".len())
    );
    assert_eq!(long.chars().count(), MAX_IMAGE_URL_CHARS + 1);
    let items = [
        image_item(
            "item-itunes",
            r#"<itunes:image href="https://example.com/item.jpg"/>
               <podcast:image href="https://example.com/podcast-image.jpg"/>"#,
        ),
        image_item("no-image", ""),
        image_item(
            "purpose",
            r#"<podcast:image href="https://example.com/social.jpg" purpose="social"/>
               <podcast:image href="https://example.com/no-purpose.jpg"/>"#,
        ),
        image_item(
            "artwork-word",
            r#"<podcast:image href="https://example.com/artwork.jpg" purpose="Social ARTWORK"/>"#,
        ),
        image_item(
            "bad-urls",
            &format!(
                r#"<itunes:image href="ftp://example.com/item.jpg"/>
                   <podcast:image href="{long}"/>
                   <podcast:images srcset="https://example.com/300.jpg 300w, https://example.com/1500.jpg 1500w, https://example.com/2x.jpg 2x"/>"#
            ),
        ),
        image_item(
            "bad-largest",
            r#"<podcast:images srcset="https://example.com/1000.jpg 1000w, ftp://example.com/3000.jpg 3000w"/>"#,
        ),
        image_item(
            "equal-widths",
            r#"<podcast:images srcset="https://example.com/first-500.jpg 500w, https://example.com/second-500.jpg 500w, https://example.com/9x.jpg 9x"/>"#,
        ),
    ]
    .concat();
    let feed = image_feed(
        r#"<itunes:image href="https://example.com/channel.jpg"/>
           <image><url>https://example.com/rss.jpg</url><title>Album I</title><link>https://example.com</link></image>"#,
        &items,
    );
    assert_eq!(
        image_urls(&feed),
        [
            Some("https://example.com/item.jpg".to_owned()),
            Some("https://example.com/channel.jpg".to_owned()),
            Some("https://example.com/no-purpose.jpg".to_owned()),
            Some("https://example.com/artwork.jpg".to_owned()),
            Some("https://example.com/1500.jpg".to_owned()),
            Some("https://example.com/channel.jpg".to_owned()),
            Some("https://example.com/first-500.jpg".to_owned()),
        ],
        "musicindex ADR 0096 R-IMAGE-1 items 2 and 3"
    );

    // A channel with only an RSS image, and a channel `itunes:image` with no
    // `href`, gives the RSS image.
    let feed = image_feed(
        r"<itunes:image/>
           <image><url>https://example.com/rss.jpg</url><title>Album I</title><link>https://example.com</link></image>",
        &image_item("rss-only", ""),
    );
    assert_eq!(
        image_urls(&feed),
        [Some("https://example.com/rss.jpg".to_owned())]
    );

    // A channel image that is not good gives the next source, and no good
    // source gives no image.
    let feed = image_feed(
        &format!(
            r#"<itunes:image href="{long}"/>
               <image><url>https://example.com/rss.jpg</url><title>Album I</title><link>https://example.com</link></image>"#
        ),
        &image_item("long-channel", ""),
    );
    assert_eq!(
        image_urls(&feed),
        [Some("https://example.com/rss.jpg".to_owned())]
    );
    let feed = image_feed(
        r#"<itunes:image href="javascript:alert(1)"/>"#,
        &image_item(
            "none",
            r#"<podcast:image href="https://example.com/social.jpg" purpose="social"/>"#,
        ),
    );
    assert_eq!(image_urls(&feed), [None], "no image is not an error");
}

/// musicindex ADR 0096 R-IMAGE-1 item 1: a resolved item has the image URL.
#[test]
fn a_resolved_item_gives_its_image_url() {
    let server = song_server();
    let mut reader = server.reader();
    let item = RemoteItemRef {
        feed_guid: "feed-a".into(),
        feed_url: Some(server.url("/feeds/a.xml")),
        item_guid: Some("song-a-1".into()),
        title: None,
    };
    assert_eq!(
        reader.resolve(&item).expect("resolve song A").image_url,
        None
    );
    let server = start_with(|base| {
        HashMap::from([(
            "/feeds/m.xml",
            ok(mp3_feed(base).replace(
                "<podcast:medium>music</podcast:medium>",
                r"<podcast:medium>music</podcast:medium><image><url>https://example.com/m.jpg</url><title>M</title><link>https://example.com</link></image>",
            )),
        )])
    });
    let mut reader = server.reader();
    let item = RemoteItemRef {
        feed_guid: "feed-m".into(),
        feed_url: Some(server.url("/feeds/m.xml")),
        item_guid: Some("m-wav".into()),
        title: None,
    };
    assert_eq!(
        reader
            .resolve(&item)
            .expect("resolve the WAV item")
            .image_url
            .as_deref(),
        Some("https://example.com/m.jpg"),
        "musicindex ADR 0096 R-IMAGE-1"
    );
}
