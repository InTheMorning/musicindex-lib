# musicindex-lib

## What This Repository Is

This repository is for code that two or more apps of the suite use. These apps
are musicindex.org, musicindex-studio and HitIt. `build-info/` is also for
stophammer, musicindex-live-relay and musicindex-live-publisher. The repository
has one folder for each subject.

Code that only one app uses stays in that app. A player, an audio decoder and a
GPUI renderer are examples. Brand files, fonts, icons and tokens stay in
`../musicindex-assets`.

This repository states no rule. A rule about the code here lives in an ADR of the
repository that owns the subject. musicindex owns the shared interface rules.

## Where The Work Stands

`search/` holds the crate `musicindex-search`, the JavaScript module
`web/query-text.mjs` and the case file. musicindex ADR 0049 accepts `search/`. HitIt and
Studio pin the crate at commit `634e4cc`. The website keeps a copy of the module, and its
toolbar field uses that copy. The [search move plan](docs/plans/search-move-phase-plan.md)
gives the state of each phase. The crate and the module also give the field list rows and
the note words of musicindex ADR 0052. The three apps use them.

The crate and the module also read the field `host:` of musicindex ADR 0062. They give the
host match function of its rule R-QUERY-16. No app pins or copies that change at this time.

The module `name_order` of the crate is the client side of the name sort of Stophammer
ADR 0090. Stophammer keeps its own copy of the two functions. Its file
`tests/fixtures/order-key-vectors.json` is the contract, and
`search/cases/order-key-vectors.json` is a copy. The orchestrator session decided this
place on 2026-11-08. If Stophammer uses this crate, that change needs its own record.
No JavaScript port exists, because the website holds no list that merges rows by name.

`fit/` holds the JavaScript module `web/fit-check.mjs` of musicindex ADR 0065 R-FIT-5 and its
browser test. Its four helpers of R-FIT-3 find an edge cut, an overflow, an overlap of two
controls and a break inside a word. The fifth helper `unsizedIcons` of R-FIT-7 finds an `svg`
element with no author width or height. A text that must break inside a word has the attribute `data-fit-allow-break`. The website and Studio keep a copy
of the module from one commit. No app copies it at this time.

`value/` holds the crate `musicindex-value` of musicindex ADR 0075 R-VALUE-4. Its function
`percentages` gives the percentage of each recipient of a value block with the shares method
of musicindex ADR 0075 R-VALUE-5. A fee share is in the total, and the fee flag does not change the
arithmetic. Its function `parse_share` reads the text of a split as Studio reads it. No app
pins the crate at this time. Studio removes its own copy when it pins the crate. The
JavaScript copy in Studio stays open in ADR 0075, and `value/` has no JavaScript module.

The function `short_address` of the same crate gives the short form of a node address or a
Nostr public key (musicindex ADR 0076 R-ADDRESS-2). The file
`value/tests/short_address.json` holds its vectors. Each JavaScript copy of the function in an
app tests against a copy of that file.

`build-info/` holds the crate `musicindex-build-info` of musicindex ADR 0083 R-VERSION-8. It gives the version, the git revision and
the build time of a Rust app. The build script of the app calls `build::emit`, and the app
code calls the macro `build_info!`. `BuildInfo::version_line` gives the line for
`--version`, and the serde form of `BuildInfo` is the JSON object of the info route.

The request is [the build info request](docs/plans/research-build-info-request.md). The
operator agreed to the design on 2026-11-18. musicindex ADR 0083 has the status Proposed. No app
pins the crate before ADR 0083 is accepted. If the ADR changes a name, the crate follows the ADR.

`tags/` holds the crate `musicindex-tags` of musicindex ADR 0093 R-TAG-1. It writes and reads
the podcast frames of an audio file with the names of R-TAG-2. `write_song_tags` writes the
GUIDs of a song file (R-TAG-3). `write_episode_tags` writes the GUIDs, the value routes and
the chapters of an episode file (R-TAG-4). A remote chapter entry holds the routes of its
song when the app gives them (musicindex ADR 0093 R-TAG-6). Each of the two writes also
writes `TXXX:Podcast Image` when the app gives an image URL (musicindex ADR 0096 R-IMAGE-2).
Each write removes the frames with the "MusicIndex " names of R-TAG-2 item 5.

Each read of an ID3v2 tag uses the frame loop of `tags/src/id3v2.rs` (musicindex ADR 0093
R-TAG-7). The loop keeps each good frame and returns the bad frames in `TagProblems`. Each
MP3 write gives an ID3v2.4 tag with ID3v2.4 frames only, and removes the ID3v1 tag (R-TAG-8).
The table `OLD_FRAME_IDS` in `tags/src/audio.rs` converts or removes the frames of an older
version.

A write removes a bad frame, and writes a bad frame of the suite again from the data of the
app (R-TAG-9). `KnownBadFrame` is the list of known bad frames. Each entry needs a test file in
`tags/tests/id3v2.rs`.

A write keeps the `TLEN` of the file and ignores a `TLEN` edit (musicindex ADR 0093
R-TAG-11). When the file has no good `TLEN`, the MP3 write gives the length that lofty
measures, and else no `TLEN`.

`involved_people_value` gives the edit value of a `TIPL` or `TMCL` frame from (role, name)
pairs. The crate owns the separators of that value (musicindex ADR 0093 R-TAG-1 item 4).

`read_podcast_tags` and `track::read_tags` read only the new names.
`write_id3v24_edits` writes the field edits of HitIt, and the trait `ReferenceSource` gives it
the pictures and the transcripts, because the app owns the network.

The test `tests/words.rs` is the check of musicindex ADR 0087 R-WORD-14. No app pins the
crate at this time. HitIt and musicindex-live-publisher remove their own copies when they
pin it.

`fetch/` holds the crate `musicindex-fetch` of musicindex ADR 0092 R-STATION-1 and
R-STATION-2. HitIt and musicindex-live-publisher use it. `FeedReader` reads a feed with
`stophammer-parser` and keeps the host limits. `FeedReader::resolve` gives the data of one
`podcast:remoteItem`, and gets a missing feed URL from MusicIndex by the feed GUID.
`ResolvedItem::mp3_url` and `track_mp3_url` select the MP3 file that the feed gives for an
item: the `audio/mpeg` enclosure, else the first `audio/mpeg` alternate enclosure in feed
order (musicindex ADR 0095 R-MP3-2 item 1).
`ResolvedItem.image_url` and `image_url` give the image URL of an item from five sources in
a fixed order, with only an `http` or `https` URL of a maximum of 2,048 characters
(musicindex ADR 0096 R-IMAGE-1).
`FeedReader::episode_splits` gives the `Split` values of an episode for `write_episode_tags`.
`download_enclosure` writes one enclosure through a temporary file and gives the SHA-256
hash.

`musicindex-tags` has no dependency on `stophammer-parser` (musicindex ADR 0092 R-STATION-2
item 2).

`convert/` holds the crate `musicindex-convert` of musicindex ADR 0094. It is the one WAV to
FLAC converter for HitIt and musicindex-live-publisher (R-CONVERT-1). It starts `flac` first
and then `ffmpeg`, and it probes each program with a time limit and an output limit. When no
program passes its probe, it gives `ConvertError::NoConverter`, and the app decides. The
crate also converts audio to MP3 with `ffmpeg` only, at 256 kbps CBR when the app sets no
other bit rate (musicindex ADR 0095 R-MP3-3). With no `ffmpeg`, it gives
`ConvertError::NoMp3Encoder`. The crate operates only on Unix.

No ADR accepts `moodbar/`, and it has no folder.

| Subject | Content | Research |
| --- | --- | --- |
| `search/` | The query crate for HitIt and Studio, the JavaScript module for the website, and the case file that both must pass | `research_notes/search/shared-query-text.md` in `../musicindex-research` |
| `value/` | The split crate for Studio and HitIt | `research_notes/podcast-apps/value-split-fee-2026-11-15.md` in `../musicindex-research` |
| `build-info/` | The build values crate of musicindex ADR 0083 R-VERSION-8 (Proposed) for stophammer, Studio, HitIt, the relay and the publisher | `docs/plans/research-app-version-request.md` in `../musicindex` |
| `tags/` | The tag crate of musicindex ADR 0093 for HitIt and musicindex-live-publisher | No research document |
| `fetch/` | The remote item read and the enclosure download of musicindex ADR 0092 for HitIt and musicindex-live-publisher | `research_notes/music-feeds/station-import-request.md` in `../musicindex-research` |
| `convert/` | The WAV to FLAC converter of musicindex ADR 0094 and the MP3 converter of musicindex ADR 0095 for HitIt and musicindex-live-publisher | No research document |
| `fit/` | The browser fit check for the website and Studio, and its test in headless Chromium | No research document |
| `moodbar/` | The analysis crate for Studio and HitIt, the browser drawing code, and the test data | No research document |

## How An App Uses This Code

musicindex ADR 0049 R-QUERY-4 owns these rules for `search/`:

- A Rust app uses a crate as a Git dependency, pinned to a commit.
- The website keeps a copy of each JavaScript file, because it has no build step. A
  check finds a copy with other bytes.

musicindex ADR 0065 R-FIT-5 item 3 gives the same procedure to `fit/` for the website and Studio.
musicindex ADR 0075 R-VALUE-4 item 5 gives the procedure for a Rust app to `value/`.

The orchestrator session decided this item on 2026-11-08, and no ADR states it:

- An app that merges its rows with node rows by name calls `name_order::check_vectors`
  in one of its own tests. Its toolchain and its lock file then run the vectors.

## Working Rules

These rules are advisory. The `project-baseline` skill owns them.

- Add a code folder only when an ADR accepts its subject and the code for it exists.
- Run the gate of `project-baseline` in each crate that you change.
- Run `node --test fit/tests/fit_check.test.mjs` when you change `fit/`. It needs Node.js 22 and Chromium.
- A crate must build with the lowest Rust version of an app that uses it.
- Write prose in ASD-STE100 Simplified Technical English.
