# g24 — G24SEC from your terminal

Play, submit, and climb from the command line. Organizer commands for
authoring land next.

## Install

```sh
cargo install --git https://github.com/G24Sec/g24
```

## Auth (browser device flow — the CLI never sees your password)

```sh
g24 login
# → visit the printed URL, approve the XXXX-XXXX code, done.
# Token cached at ~/.config/g24/token (0600).
```

## Play

```sh
g24 categories
g24 list --category "Web Exploitation" --limit 20
g24 show 38
g24 get 38 --dest ./ctf/38      # artifacts with printed sha256
g24 submit 38 'G24{...}'
g24 hint 38
g24 leaderboard                  # global
g24 leaderboard --event specter-trials-2026
g24 me
g24 logout
```

`--api` overrides the backend (default `https://api-3.g24sec.com`),
or set `G24_API`.
