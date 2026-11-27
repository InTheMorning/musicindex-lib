# musicindex-lib

Shared code for the Music Index apps: musicindex.org, musicindex-studio, HitIt,
stophammer, musicindex-live-relay and musicindex-live-publisher.

Each folder holds one subject:

- `search`: the query crate and the query module for the website.
- `fit`: the browser fit check for the website and Studio.
- `value`: the split crate.
- `build-info`: the build values crate.
- `tags`: the tag crate for audio files.
- `transcript`: the parser for transcript and lyrics files.
- `fetch`: the remote item read and the enclosure download.
- `convert`: the WAV to FLAC converter and the MP3 converter.

`AGENTS.md` gives the decision record of each folder and the apps that use it. The shared
brand files, fonts, icons and tokens are in
[musicindex-assets](https://github.com/InTheMorning/musicindex-assets).
