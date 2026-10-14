# g24 — G24SEC from your terminal

## What is G24SEC?

G24SEC is an open cybersecurity competition platform. Organizers host
Capture The Flag (CTF) events — timed hacking competitions — and players
solve security challenges (web exploitation, cryptography, forensics,
reverse engineering, and more) to earn points, climb a global Elo-rated
leaderboard, and collect badges.

If you are new: create a free account on the website, then come back
here. Everything below assumes you have one.

## What is this tool?

`g24` is the official command-line client. Use it two ways:

- **One-shot commands** — `g24 list`, `g24 submit 38 'G24{...}'`
- **Interactive shell** — run bare `g24`, get a `g24>` prompt with the
  same commands, plus command history. `help` lists them, `exit` quits.

## Install

Requires a Rust toolchain, or grab a ready binary from
[Releases](https://github.com/G24Sec/g24sec-cli/releases):

```sh
# from source
cargo install --git https://github.com/G24Sec/g24sec-cli

# or download g24-x86_64-linux from Releases, then:
chmod +x g24-x86_64-linux && sudo mv g24-x86_64-linux /usr/local/bin/g24
```

Check it works: `g24 categories` prints the challenge taxonomy —
no account needed for that one.

## Sign in (browser device flow)

The CLI never sees your password:

```sh
g24 login
# 1. It prints a URL and a XXXX-XXXX code.
# 2. Open the URL (already signed in? one click), check the code matches.
# 3. Approve. The terminal signs itself in.
#
# Token cached at ~/.config/g24/token with 0600 permissions.
# Never approve a code you did not request.
```

```sh
g24 logout   # forget the cached token
g24 me       # your handle, rating, id
```

## Playing

```sh
g24 list --category "Web Exploitation" --limit 20
g24 show 38                       # description, points, artifacts
g24 get 38 --dest ./ctf/38        # downloads with printed sha256
g24 submit 38 'G24{...}'          # correct answers show +points
g24 hint 38                       # may cost points, like the website
```

Challenges already solved are marked `*` in `list`. Wrong flags can
cost rating points, exactly like the website — same rules, same math.

## Leaderboards

```sh
g24 leaderboard                   # global top 20
g24 leaderboard --event specter-trials-2026
g24 leaderboard --watch           # live refresh (default 15s)
g24 leaderboard --watch --interval 10
```

`--watch` clears and re-renders until Ctrl-C.

## Organizing (organizer accounts)

Author challenges from a manifest file (`g24.toml`):

```toml
title = "Cold Kitchen"
category = "Boot to Root"
difficulty = "hard"
points = 500
description = """
A kitchen unit boots to a first process that has never been told what it is cooking.
"""
flags = ["G24{...}"]
hints = ["Unpack it and read the first thing it runs."]
files = ["challenge/initramfs.cpio.gz"]
# external = "https://kitchen.example.com"   # hosted challenges
```

```sh
g24 new --manifest g24.toml --event specter-trials-2026
g24 set 38 --set points=500 --set difficulty=hard
g24 files-add 38 ./artifacts/vuln.zip
g24 files-remove 38 912
g24 publish --manifest g24.toml   # create, then download every artifact
                                  # and compare bytes (3 retries). Fails loud.
g24 verify 38 --manifest g24.toml # health + sha256 vs local files
g24 verify 38                     # reachability only
g24 delete 38 --confirm
```

`publish` refuses to call it done unless every attached file downloads
byte-identical — the same guarantee the platform team uses when
authoring its own events.

## Files and flags

- Attachments cap at 10 per request and 50MB per file.
- HTML markup in artifacts is rejected at upload (browsers and the CDN
  refuse to serve it later — better to hear it now with a byte offset).
- Filenames and bytes are preserved exactly; downloads print sha256 so
  you can prove which bytes you solved.

## Troubleshooting

| Symptom | Cause |
|---|---|
| `not signed in` | Run `g24 login` first |
| `session expired` | Token revoked or rotated — `g24 login` again |
| `Server busy` on submit | Platform pool saturated — wait a moment, retry |
| `Rate limit exceeded` | 5 flag tries per minute per challenge |
| publish reports MISMATCH | Re-upload; the remote bytes differ from local |

## Contributing

Issues and pull requests welcome. Player-facing output strings are the
contract — changing them is a breaking change, say so in the PR.
