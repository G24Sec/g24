# g24 — G24SEC from your terminal

Use it with arguments or as a shell (`g24` with no command drops
you into one — same commands, `help`, `exit` to quit).

## Install

```sh
cargo install --git https://github.com/G24Sec/g24sec-cli
```

## Auth (browser device flow — the CLI never sees your password)

```sh
g24 login
# → visit the printed URL, approve the XXXX-XXXX code, done.
# Token cached at ~/.config/g24/token (0600).
```

## Play

```sh
g24 list --category "Web Exploitation" --limit 20
g24 show 38
g24 get 38 --dest ./ctf/38      # artifacts with printed sha256
g24 submit 38 'G24{...}'
g24 hint 38
g24 leaderboard                  # live with --watch
g24 leaderboard --event specter-trials-2026 --watch --interval 10
g24 me
```

## Organize

```sh
g24 categories
g24 new --manifest g24.toml --event specter-trials-2026
g24 set 38 --set points=500 --set difficulty=hard
g24 files-add 38 ./artifacts/vuln.zip
g24 files-remove 38 912
g24 publish --manifest g24.toml   # create + verify every artifact
g24 verify 38 --manifest g24.toml
g24 delete 38 --confirm
```

Manifest (`g24.toml`):

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
# external = "https://kitchen.challenge.g24sec.com"
```
