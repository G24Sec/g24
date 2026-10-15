//! g24 — the G24SEC terminal client.
//!
//! Players: browse, download, submit, leaderboard. Organizers: author and
//! publish challenges. Auth is a browser device flow; the CLI never sees a
//! password.

use std::{fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::Deserialize;

mod out {
    use owo_colors::OwoColorize;

    pub fn ok(msg: &str) {
        println!("{} {}", "✓".green().bold(), msg);
    }
    // Reserved for verbose/diagnostic output; kept so the vocabulary stays
    // in one place instead of ad-hoc println! calls.
    #[allow(dead_code)]
    pub fn info(msg: &str) {
        println!("{} {}", "»".cyan().bold(), msg);
    }
    pub fn warn(msg: &str) {
        println!("{} {}", "!".yellow().bold(), msg);
    }
    pub fn err(msg: &str) {
        eprintln!("{} {}", "✗".red().bold(), msg);
    }
    #[allow(dead_code)]
    pub fn header(msg: &str) {
        println!("\n{}", msg.cyan().bold());
    }
    // NOTE: mirrors toxi-cli output vocabulary on purpose. If toxi-cli ever
    // exports its output module as a library, swap this mod for
    // `use toxi_cli::output as out;` and delete it.
}

const DEFAULT_API: &str = "https://api-3.g24sec.com";

#[derive(Parser)]
#[command(name = "g24", version, about = "G24SEC from your terminal")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    /// API base URL (default https://api-3.g24sec.com)
    #[arg(long, global = true)]
    api: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Sign in via browser approval
    Login,
    /// Sign out (forget the cached token)
    Logout,
    /// Who am I (solves, points, rating)
    Me,
    /// Challenge taxonomy with live counts
    Categories,
    /// List challenges, optionally by category
    List {
        #[arg(long)] category: Option<String>,
        #[arg(long, default_value_t = 20)] limit: usize,
        /// Only challenges from this event slug
        #[arg(long)] event: Option<String>,
    },
    /// Show one challenge (description, points, artifacts)
    Show { id: Option<String> },
    /// Download artifacts with printed sha256
    Get {
        id: Option<String>,
        #[arg(long, default_value = ".")] dest: PathBuf,
    },
    /// Submit a flag
    Submit { id: Option<String>, flag: String },
    /// Next hint (may cost points)
    Hint { id: Option<String> },
    /// Leaderboard (global, or --event slug)
    Leaderboard {
        #[arg(long)] event: Option<String>,
        #[arg(long, default_value_t = 20)] limit: usize,
        /// Live refresh every N seconds
        #[arg(long)] watch: bool,
        #[arg(long, default_value_t = 15)] interval: u64,
    },
    /// Create a challenge from a manifest (organizer)
    New {
        #[arg(long, default_value = "g24.toml")] manifest: PathBuf,
        #[arg(long)] event: Option<String>,
    },
    /// Patch challenge fields key=value (organizer)
    Set {
        id: u64,
        #[arg(long = "set", value_parser = parse_kv)]
        values: Vec<(String, String)>,
    },
    /// Upload attachments (organizer)
    FilesAdd {
        id: u64,
        paths: Vec<PathBuf>,
        #[arg(long)] manifest_dir: Option<PathBuf>,
    },
    /// Remove an attachment by file id (organizer)
    FilesRemove {
        id: u64,
        file_id: u64,
    },
    /// Create then verify every artifact downloads byte-identical (organizer)
    Publish {
        #[arg(long, default_value = "g24.toml")] manifest: PathBuf,
        #[arg(long)] event: Option<String>,
    },
    /// Verify artifacts: health + sha256 vs local files
    Verify {
        id: String,
        #[arg(long)] manifest: Option<PathBuf>,
    },
    /// Delete a challenge (organizer, needs --confirm)
    Delete {
        id: u64,
        #[arg(long)] confirm: bool,
    },
    /// Show an event (state, dates, your registration)
    Event {
        slug: String,
    },
    /// Register for an event
    Join {
        slug: String,
        #[arg(long)] team: Option<String>,
    },
}

fn parse_kv(s: &str) -> Result<(String, String), String> {
    match s.split_once('=') {
        Some((k, v)) if !k.is_empty() => Ok((k.to_string(), v.to_string())),
        _ => Err("expected key=value".into()),
    }
}

fn config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".config/g24"))
}

fn token_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("token"))
}

fn read_token() -> Result<String> {
    let token = fs::read_to_string(token_path()?).context("not signed in (run `g24 login`)")?;
    Ok(token.trim().to_string())
}

fn save_token(token: &str) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("token");
    fs::write(&path, token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("http client")
}

fn api_base(cli_api: &Option<String>) -> String {
    cli_api.clone().unwrap_or_else(|| {
        std::env::var("G24_API").unwrap_or_else(|_| DEFAULT_API.to_string())
    })
    .trim_end_matches('/')
    .to_string()
}

#[derive(Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
struct DeviceStart {
    success: bool,
    #[serde(default)]
    device_code: String,
    #[serde(default, alias = "user_code")]
    user_code: String,
    #[serde(default)]
    verification_url: String,
    message: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
#[serde(rename_all = "camelCase")]
struct DevicePoll {
    success: bool,
    #[serde(default)]
    status: String,
    #[serde(default)]
    token: Option<String>,
    message: Option<String>,
}

async fn cmd_login(api: &str) -> Result<()> {
    let res: DeviceStart = client()
        .post(format!("{api}/api/auth/device/request"))
        .send()
        .await?
        .json()
        .await?;
    if !res.success {
        bail!("login start failed");
    }
    println!("Visit this URL and approve the code:\n\n  {}\n\nCode: {}\n", res.verification_url, res.user_code);
    let deadline = std::time::Instant::now() + Duration::from_secs(600);
    loop {
        if std::time::Instant::now() > deadline {
            bail!("code expired, run `g24 login` again");
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
        let poll: DevicePoll = client()
            .get(format!("{api}/api/auth/device/poll"))
            .query(&[("device_code", res.device_code.as_str())])
            .send()
            .await?
            .json()
            .await?;
        match poll.status.as_str() {
            "approved" => {
                let token = poll.token.context("approved without token")?;
                save_token(&token)?;
                println!("Signed in. Token cached (0600).");
                return Ok(());
            }
            "expired" => bail!("{}", poll.message.unwrap_or_else(|| "code expired".into())),
            _ => print!("."),
        }
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
}

async fn authed(api: &str) -> Result<(reqwest::Client, String, Option<String>)> {
    Ok((client(), api.to_string(), read_token().ok()))
}

fn auth(b: reqwest::RequestBuilder, token: &Option<String>) -> reqwest::RequestBuilder {
    match token {
        Some(t) => b.bearer_auth(t),
        None => b,
    }
}

async fn get_json(api: &str, path: &str) -> Result<serde_json::Value> {
    let (c, base, token) = authed(api).await?;
    let res = auth(c.get(format!("{base}{path}")), &token).send().await?;
    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("session expired, run `g24 login`");
    }
    Ok(res.json().await?)
}

async fn cmd_me(api: &str) -> Result<()> {
    read_token()?;
    let v = get_json(api, "/api/auth/me").await?;
    let u = &v["user"];
    println!(
        "@{}  rating {}  id {}",
        u["username"].as_str().unwrap_or("?"),
u["userRating"]
            .as_i64()
            .or_else(|| u["user_rating"].as_i64())
            .or_else(|| u["rating"].as_i64())
            .unwrap_or(0),
        u["id"].as_str().unwrap_or("?"),
    );
    Ok(())
}

async fn cmd_categories(api: &str) -> Result<()> {
    let (c, base, _) = (client(), api.to_string(), String::new());
    let v: serde_json::Value = c
        .get(format!("{base}/api/categories"))
        .send()
        .await?
        .json()
        .await?;
    for cat in v["categories"].as_array().cloned().unwrap_or_default() {
        println!(
            "{:28} {:>4} challenges",
            cat["name"].as_str().unwrap_or("?"),
            cat["challenges"].as_u64().unwrap_or(0),
        );
    }
    Ok(())
}

async fn cmd_list(api: &str, category: Option<String>, limit: usize, event: Option<String>) -> Result<()> {
    let mut q = vec![("limit", limit.to_string())];
    if let Some(slug) = &event {
        // resolve slug -> numeric id for the event filter
        let (c0, base0, t0) = authed(api).await?;
        let ev: serde_json::Value = auth(c0.get(format!("{base0}/api/ctf-events/{slug}")), &t0)
            .send()
            .await?
            .json()
            .await?;
        let eid = ev["event"]["id"]
            .as_u64()
            .or_else(|| ev["event"]["event_id"].as_u64())
            .with_context(|| format!("event '{slug}' not found"))?;
        q.push(("eventId", eid.to_string()));
    } else {
        q.push(("standalone", "true".to_string()));
    }
    if let Some(cat) = category {
        q.push(("category", cat));
    }
    let (c, base, token) = authed(api).await?;
    let v: serde_json::Value = auth(c
        .get(format!("{base}/api/ctf-challenges"))
        .query(&q), &token)
        .send()
        .await?
        .json()
        .await?;
    println!("total: {}", v["total"].as_u64().unwrap_or(0));
    for ch in v["challenges"].as_array().cloned().unwrap_or_default() {
        let solved = ch.get("isSolved").and_then(|s| s.as_bool()).unwrap_or(false);
        println!(
            "{}{:>6}  {:>4}pts  {:<9} {}",
            if solved { "*" } else { " " },
            ch["id"].as_u64().unwrap_or(0),
            live_points(&ch),
            ch["difficulty"].as_str().unwrap_or("?"),
            ch["title"].as_str().unwrap_or("?"),
        );
        let fmt = ch["challengeFormat"].as_str().unwrap_or("flag");
        let ctype = ch["challengeType"].as_str().unwrap_or("");
        if fmt != "flag" || !ctype.is_empty() {
            println!("      type:{ctype} format:{fmt}");
        }
    }
    if v["hasMore"].as_bool().unwrap_or(false) {
        println!("…more available, raise --limit");
    }
    Ok(())
}

async fn pick_challenge(api: &str, prompt: &str) -> Result<u64> {
    let (c, base, token) = authed(api).await?;
    let v: serde_json::Value = auth(
        c.get(format!("{base}/api/ctf-challenges"))
            .query(&[("standalone", "true"), ("limit", "50")]),
        &token,
    )
    .send()
    .await?
    .json()
    .await?;
    let items = v["challenges"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        anyhow::bail!("no challenges found");
    }
    let labels: Vec<String> = items
        .iter()
        .map(|ch| {
            format!(
                "{} — {} ({}pts{})",
                ch["id"].as_u64().unwrap_or(0),
                ch["title"].as_str().unwrap_or("?"),
                ch["points"].as_u64().unwrap_or(0),
                if ch.get("isSolved").and_then(|s| s.as_bool()).unwrap_or(false) {
                    ", solved"
                } else {
                    ""
                },
            )
        })
        .collect();
    let sel = dialoguer::Select::new()
        .with_prompt(prompt)
        .items(&labels)
        .default(0)
        .interact()?;
    Ok(items[sel]["id"].as_u64().unwrap_or(0))
}

async fn resolve_name(api: &str, id: Option<String>, prompt: &str) -> Result<u64> {
    let raw = match id {
        Some(v) => v,
        None => return pick_challenge(api, prompt).await,
    };
    if let Ok(n) = raw.trim().parse::<u64>() {
        return Ok(n);
    }
    let needle = raw.to_lowercase();
    let (c, base, token) = authed(api).await?;
    let mut items: Vec<serde_json::Value> = Vec::new();
    let mut page = 1u32;
    loop {
        let v: serde_json::Value = auth(
            c.get(format!("{base}/api/ctf-challenges"))
                .query(&[("standalone", "true"), ("limit", "200"), ("page", &page.to_string())]),
            &token,
        )
        .send()
        .await?
        .json()
        .await?;
        let batch = v["challenges"].as_array().cloned().unwrap_or_default();
        let total = v["total"].as_u64().unwrap_or(0) as usize;
        items.extend(batch.iter().cloned());
        if items.len() >= total || v["hasMore"].as_bool() != Some(true) || page >= 25 {
            break;
        }
        page += 1;
    }
    let hits: Vec<&serde_json::Value> = items
        .iter()
        .filter(|ch| {
            ch["title"]
                .as_str()
                .map(|t| t.to_lowercase().contains(&needle))
                .unwrap_or(false)
        })
        .collect();
    if let Some(exact) = hits.iter().find(|ch| {
        ch["title"]
            .as_str()
            .map(|t| t.to_lowercase() == needle)
            .unwrap_or(false)
    }) {
        return Ok(exact["id"].as_u64().unwrap_or(0));
    }
    match hits.len() {
        0 => anyhow::bail!("no challenge matching '{raw}'"),
        1 => Ok(hits[0]["id"].as_u64().unwrap_or(0)),
        _ => {
            let labels: Vec<String> = hits
                .iter()
                .map(|ch| {
                    format!(
                        "{} — {}",
                        ch["id"].as_u64().unwrap_or(0),
                        ch["title"].as_str().unwrap_or("?")
                    )
                })
                .collect();
            let sel = dialoguer::Select::new()
                .with_prompt("multiple matches")
                .items(&labels)
                .default(0)
                .interact()?;
            Ok(hits[sel]["id"].as_u64().unwrap_or(0))
        }
    }
}

async fn fetch_challenge(api: &str, id: u64) -> Result<serde_json::Value> {
    let (c, base, token) = authed(api).await?;
    let v: serde_json::Value = auth(c
        .get(format!("{base}/api/ctf-challenges/{id}")), &token)
        .send()
        .await?
        .json()
        .await?;
    if v["success"].as_bool().unwrap_or(false) {
        Ok(v["challenge"].clone())
    } else {
        bail!("{}", v["message"].as_str().unwrap_or("challenge not found"));
    }
}

/// Live award if the server computed decay, else the base points.
fn live_points(ch: &serde_json::Value) -> u64 {
    ch.get("value")
        .and_then(|v| v.as_u64())
        .or_else(|| ch["points"].as_u64())
        .unwrap_or(0)
}

fn u64_either(ch: &serde_json::Value, camel: &str, snake: &str) -> Option<u64> {
    ch.get(camel)
        .and_then(|v| v.as_u64())
        .or_else(|| ch.get(snake).and_then(|v| v.as_u64()))
}

async fn cmd_show(api: &str, id: u64) -> Result<()> {
    let ch = fetch_challenge(api, id).await?;
    println!("{}  [{}]", ch["title"].as_str().unwrap_or("?"), ch["id"].as_u64().unwrap_or(id));
    println!(
        "{} · {} · {}pts",
        ch["category"].as_str().unwrap_or("?"),
        ch["difficulty"].as_str().unwrap_or("?"),
        live_points(&ch),
    );
    if let Some(dp) = u64_either(&ch, "initialPoints", "initial_points") {
        println!(
            "scoring: dynamic {} -> {} (decay limit {})",
            dp,
            u64_either(&ch, "minPoints", "min_points").unwrap_or(10),
            u64_either(&ch, "decayLimit", "decay_limit").unwrap_or(50),
        );
    }
    if let Some(reqs) = ch.get("unlockRequirements").and_then(|v| v.as_array())
        && !reqs.is_empty() {
            println!("unlocks after: {}", reqs.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", "));
        }
    println!();
    println!("{}", ch["description"].as_str().unwrap_or(""));
    if let Some(files) = ch["files"].as_array() {
        println!("\nartifacts:");
        for f in files {
            let size = f["sizeBytes"].as_u64().or_else(|| f["size_bytes"].as_u64()).unwrap_or(0);
            println!("  #{}  {} ({} bytes)", f["id"].as_u64().unwrap_or(0), f["name"].as_str().unwrap_or("?"), size);
        }
    }
    Ok(())
}

async fn cmd_get(api: &str, id: u64, dest: PathBuf) -> Result<()> {
    let ch = fetch_challenge(api, id).await?;
    let files = ch["files"].as_array().cloned().unwrap_or_default();
    if files.is_empty() {
        println!("no artifacts on this challenge");
        return Ok(());
    }
    fs::create_dir_all(&dest)?;
    let (c, base, token) = authed(api).await?;
    for f in files {
        let fid = f["id"].as_u64().unwrap_or(0);
        let name = f["name"].as_str().unwrap_or("artifact").replace(['/', '\\'], "_");
        let bytes = auth(c
            .get(format!("{base}/api/ctf-challenges/files/{fid}/download")), &token)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        use sha2::{Digest, Sha256};
        let digest = hex::encode(Sha256::digest(&bytes));
        let path = dest.join(&name);
        fs::write(&path, &bytes)?;
        println!("{}  sha256:{}  {} bytes", path.display(), digest, bytes.len());
    }
    Ok(())
}

async fn cmd_submit(api: &str, id: u64, flag: String) -> Result<()> {
    let token = read_token()?;
    let (c, base, _) = authed(api).await?;
    let v: serde_json::Value = c
        .post(format!("{base}/api/ctf-challenges/{id}/submit"))
        .bearer_auth(token)
        .json(&serde_json::json!({ "flag": flag }))
        .send()
        .await?
        .json()
        .await?;
    if v["success"].as_bool().unwrap_or(false) {
        out::ok(&format!("correct! +{} points", v["points"].as_u64().unwrap_or(0)));
        if let Some(hp) = v["hintPenalty"].as_u64()
            && hp > 0 {
                out::warn(&format!("hint penalty -{}, base {}", hp, v["basePoints"].as_u64().unwrap_or(0)));
            }
    } else {
        out::err(&format!("incorrect: {}", v["message"].as_str().unwrap_or("wrong flag")));
        if let Some(r) = v.get("rating") {
            println!("rating {} → {}", r["oldRating"], r["newRating"]);
        }
        std::process::exit(1);
    }
    Ok(())
}

async fn cmd_hint(api: &str, id: u64) -> Result<()> {
    let token = read_token()?;
    let (c, base, _) = authed(api).await?;
    let v: serde_json::Value = c
        .post(format!("{base}/api/ctf/hints/{id}"))
        .bearer_auth(token)
        .send()
        .await?
        .json()
        .await?;
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(())
}

async fn cmd_leaderboard(api: &str, event: &Option<String>, limit: usize, watch: bool, interval: u64) -> Result<()> {
    if !watch {
        return render_board(api, event, limit).await;
    }
    loop {
        print!("\x1b[2J\x1b[H");
        use std::io::Write;
        std::io::stdout().flush().ok();
        println!("== leaderboard (refresh {}s, Ctrl-C to quit) ==\n", interval);
        if let Err(e) = render_board(api, event, limit).await {
            println!("error: {:#}", e);
        }
        tokio::time::sleep(Duration::from_secs(interval.max(5))).await;
    }
}

#[derive(Debug, Deserialize, Default)]
struct Manifest {
    #[serde(default)]
    title: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    difficulty: String,
    #[serde(default)]
    points: u64,
    #[serde(default)]
    description: String,
    #[serde(default)]
    flags: Vec<String>,
    #[serde(default)]
    hints: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    external: Option<String>,
    #[serde(default)]
    event_id: Option<u64>,
}

fn load_manifest(path: &PathBuf) -> Result<(Manifest, PathBuf)> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let m: Manifest = toml::from_str(&text).context("parse manifest TOML")?;
    validate_manifest(&m)?;
    let dir = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    Ok((m, dir))
}

const DIFFICULTIES: &[&str] = &["beginner", "easy", "medium", "hard", "insane", "expert"];

/// Fail fast with a local message instead of a server 400 roundtrip.
/// Mirrors the creation guards in backend-next/lib/challenges.ts, where
/// descriptions carry no minimum (file-only challenges need barely any).
fn validate_manifest(m: &Manifest) -> Result<()> {
    if m.title.trim().len() < 3 {
        bail!("manifest: title must be at least 3 characters");
    }
    if m.category.trim().len() < 2 {
        bail!("manifest: category is required");
    }
    if !DIFFICULTIES.contains(&m.difficulty.to_lowercase().as_str()) {
        bail!("manifest: difficulty must be one of: {}", DIFFICULTIES.join(", "));
    }
    if m.points < 10 || m.points > 1000 {
        bail!("manifest: points must be between 10 and 1000");
    }
    Ok(())
}

fn manifest_form(
    m: &Manifest,
    dir: &std::path::Path,
    event: Option<String>,
) -> Result<reqwest::multipart::Form> {
    let mut form = reqwest::multipart::Form::new()
        .text("title", m.title.clone())
        .text("category", m.category.clone())
        .text("difficulty", m.difficulty.clone())
        .text("points", m.points.to_string())
        .text("description", m.description.clone())
        .text(
            "flags",
            serde_json::to_string(
                &m.flags
                    .iter()
                    .map(|f| serde_json::json!({ "flag": f }))
                    .collect::<Vec<_>>(),
            )?,
        )
        .text("hints", serde_json::to_string(&m.hints)?);
    if let Some(e) = event.or_else(|| m.event_id.map(|id| id.to_string())) {
        form = form.text("eventId", e);
    }
    if let Some(url) = &m.external {
        form = form.text("externalUrl", url.clone());
    }
    for f in &m.files {
        let p = dir.join(f);
        let bytes = fs::read(&p).with_context(|| format!("read {}", p.display()))?;
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        form = form.part(
            "file",
            reqwest::multipart::Part::bytes(bytes).file_name(name),
        );
    }
    Ok(form)
}

async fn cmd_new(api: &str, manifest: PathBuf, event: Option<String>) -> Result<()> {
    let token = read_token()?;
    let (m, dir) = load_manifest(&manifest)?;
    let (c, base, _) = authed(api).await?;
    let v: serde_json::Value = c
        .post(format!("{base}/api/ctf-challenges"))
        .bearer_auth(token)
        .multipart(manifest_form(&m, &dir, event)?)
        .send()
        .await?
        .json()
        .await?;
    if !v["success"].as_bool().unwrap_or(false) {
        bail!("{}", v["message"].as_str().unwrap_or("create failed"));
    }
    let ch = &v["challenge"];
    println!(
        "created #{} {}",
        ch["id"].as_u64().unwrap_or(0),
        ch["title"].as_str().unwrap_or("?")
    );
    for f in v["files"].as_array().cloned().unwrap_or_default() {
        println!(
            "  file #{} {} ({} bytes)",
            f["id"], f["name"], f["sizeBytes"]
        );
    }
    Ok(())
}

async fn cmd_set(api: &str, id: u64, values: Vec<(String, String)>) -> Result<()> {
    let token = read_token()?;
    let mut obj = serde_json::Map::new();
    for (k, v) in values {
        let val: serde_json::Value = match k.as_str() {
            "points" | "initialPoints" | "minPoints" | "decayLimit" | "sortOrder" => v
                .parse::<i64>()
                .map(serde_json::Value::from)
                .unwrap_or(serde_json::Value::String(v)),
            "unlockRequirements" => {
                serde_json::from_str(&v).unwrap_or(serde_json::Value::String(v))
            }
            _ => serde_json::Value::String(v),
        };
        obj.insert(k, val);
    }
    let (c, base, _) = authed(api).await?;
    let v: serde_json::Value = c
        .put(format!("{base}/api/ctf-challenges/{id}"))
        .bearer_auth(token)
        .json(&obj)
        .send()
        .await?
        .json()
        .await?;
    if !v["success"].as_bool().unwrap_or(false) {
        bail!("{}", v["message"].as_str().unwrap_or("update failed"));
    }
    println!("updated #{id}");
    Ok(())
}

async fn cmd_files_add(api: &str, id: u64, paths: Vec<PathBuf>) -> Result<()> {
    let token = read_token()?;
    let mut form = reqwest::multipart::Form::new();
    for p in &paths {
        let bytes = fs::read(p).with_context(|| format!("read {}", p.display()))?;
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        form = form.part(
            "file",
            reqwest::multipart::Part::bytes(bytes).file_name(name),
        );
    }
    let (c, base, _) = authed(api).await?;
    let v: serde_json::Value = c
        .put(format!("{base}/api/ctf-challenges/{id}"))
        .bearer_auth(token)
        .multipart(form)
        .send()
        .await?
        .json()
        .await?;
    if !v["success"].as_bool().unwrap_or(false) {
        bail!("{}", v["message"].as_str().unwrap_or("upload failed"));
    }
    println!("uploaded {} file(s) to #{id}", paths.len());
    Ok(())
}

async fn cmd_files_remove(api: &str, id: u64, file_id: u64) -> Result<()> {
    let token = read_token()?;
    let (c, base, _) = authed(api).await?;
    let v: serde_json::Value = c
        .delete(format!("{base}/api/ctf-challenges/files/{file_id}"))
        .bearer_auth(token)
        .send()
        .await?
        .json()
        .await?;
    if !v["success"].as_bool().unwrap_or(false) {
        bail!("{}", v["message"].as_str().unwrap_or("remove failed"));
    }
    println!("removed file #{file_id} from challenge #{id}");
    Ok(())
}

async fn verify_artifacts(api: &str, id: u64, local: &[(String, Vec<u8>)]) -> Result<bool> {
    let (c, base, token) = authed(api).await?;
    let health: serde_json::Value = auth(
        c.get(format!(
            "{base}/api/ctf-challenges/{id}/files/health"
        )),
        &token,
    )
    .send()
    .await?
    .json()
    .await?;
    let mut ok = true;
    for f in health["files"].as_array().cloned().unwrap_or_default() {
        let name = f["name"].as_str().unwrap_or("?");
        if !f["reachable"].as_bool().unwrap_or(false) {
            println!("  UNREACHABLE {name}");
            ok = false;
            continue;
        }
        if let Some((_, bytes)) = local.iter().find(|(n, _)| n == name) {
            use sha2::{Digest, Sha256};
            let want = hex::encode(Sha256::digest(bytes));
            let got = f["sha256"].as_str().unwrap_or("");
            if want == got {
                println!("  ok {name} sha256:{want}");
            } else {
                println!("  MISMATCH {name} local:{want} remote:{got}");
                ok = false;
            }
            continue;
        }
        println!("  ok {name} (reachable)");
    }
    Ok(ok)
}

async fn cmd_publish(api: &str, manifest: PathBuf, event: Option<String>) -> Result<()> {
    let (m, dir) = load_manifest(&manifest)?;
    let local: Vec<(String, Vec<u8>)> = m
        .files
        .iter()
        .map(|f| {
            let p = dir.join(f);
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();
            fs::read(&p)
                .map(|b| (name, b))
                .with_context(|| format!("read {}", p.display()))
        })
        .collect::<Result<_>>()?;
    let token = read_token()?;
    let (c, base, _) = authed(api).await?;
    let v: serde_json::Value = c
        .post(format!("{base}/api/ctf-challenges"))
        .bearer_auth(&token)
        .multipart(manifest_form(&m, &dir, event)?)
        .send()
        .await?
        .json()
        .await?;
    if !v["success"].as_bool().unwrap_or(false) {
        bail!("{}", v["message"].as_str().unwrap_or("create failed"));
    }
    let id = v["challenge"]["id"].as_u64().context("no id returned")?;
    println!("created #{id}, verifying artifacts...");
    let mut ok = false;
    for _ in 0..3 {
        ok = verify_artifacts(api, id, &local).await?;
        if ok {
            break;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    if !ok {
        bail!("publish failed: artifacts do not verify");
    }
    println!("published #{id}, all artifacts verified");
    Ok(())
}

async fn cmd_verify(api: &str, id: u64, manifest: Option<PathBuf>) -> Result<()> {
    let local = if let Some(p) = manifest {
        let (m, dir) = load_manifest(&p)?;
        m.files
            .iter()
            .map(|f| {
                let path = dir.join(f);
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("file")
                    .to_string();
                fs::read(&path)
                    .map(|b| (name, b))
                    .with_context(|| format!("read {}", path.display()))
            })
            .collect::<Result<_>>()?
    } else {
        vec![]
    };
    if !verify_artifacts(api, id, &local).await? {
        std::process::exit(1);
    }
    Ok(())
}

async fn cmd_delete(api: &str, id: u64, confirm: bool) -> Result<()> {
    if !confirm {
        bail!("refusing without --confirm");
    }
    let token = read_token()?;
    let (c, base, _) = authed(api).await?;
    let v: serde_json::Value = c
        .delete(format!("{base}/api/ctf-challenges/{id}"))
        .bearer_auth(token)
        .send()
        .await?
        .json()
        .await?;
    if !v["success"].as_bool().unwrap_or(false) {
        bail!("{}", v["message"].as_str().unwrap_or("delete failed"));
    }
    println!("deleted #{id}");
    Ok(())
}

async fn cmd_event(api: &str, slug: String) -> Result<()> {
    let (c, base, token) = authed(api).await?;
    let v: serde_json::Value = auth(c.get(format!("{base}/api/ctf-events/{slug}")), &token)
        .send()
        .await?
        .json()
        .await?;
    let e = &v["event"];
    if e.is_null() {
        bail!("{}", v["message"].as_str().unwrap_or("event not found"));
    }
    println!("{}  [{}]", e["title"].as_str().unwrap_or("?"), e["state"].as_str().unwrap_or("?"));
    println!(
        "{} — {}",
        e["eventStart"].as_str().unwrap_or(e["event_start"].as_str().unwrap_or("?")),
        e["eventEnd"].as_str().unwrap_or(e["event_end"].as_str().unwrap_or("?")),
    );
    if let Some(reg) = e.get("registration").filter(|r| !r.is_null()) {
        println!("registered: {} ({})", reg["status"].as_str().unwrap_or("?"), reg["team"].as_str().unwrap_or("solo"));
    }
    Ok(())
}

async fn cmd_join(api: &str, slug: String, team: Option<String>) -> Result<()> {
    let token = read_token()?;
    let (c, base, _) = authed(api).await?;
    // Registration takes no team field; teams are created separately below.
    let v: serde_json::Value = c
        .post(format!("{base}/api/ctf-registrations/{slug}"))
        .bearer_auth(&token)
        .json(&serde_json::json!({}))
        .send()
        .await?
        .json()
        .await?;
    if !v["success"].as_bool().unwrap_or(false) {
        bail!("{}", v["message"].as_str().unwrap_or("registration failed"));
    }
    println!("{}", v["message"].as_str().unwrap_or("registered"));
    if let Some(name) = team {
        let t: serde_json::Value = c
            .post(format!("{base}/api/ctf-teams/{slug}"))
            .bearer_auth(&token)
            .json(&serde_json::json!({ "name": name }))
            .send()
            .await?
            .json()
            .await?;
        if !t["success"].as_bool().unwrap_or(false) {
            bail!(
                "registered, but team creation failed: {}",
                t["message"].as_str().unwrap_or("unknown error")
            );
        }
        println!("team '{}' created", t["team"]["name"].as_str().unwrap_or(&name));
        if let Some(code) = t["team"]["invite_code"].as_str().or_else(|| t["team"]["inviteCode"].as_str()) {
            println!("invite code: {code} (expires in 24h)");
        }
    }
    Ok(())
}

async fn render_board(api: &str, event: &Option<String>, limit: usize) -> Result<()> {
    let path = match event {
        Some(slug) => format!("/api/ctf-leaderboard/{slug}"),
        None => "/api/ctf-leaderboard/global".to_string(),
    };
    let (c, base, token) = authed(api).await?;
    let v: serde_json::Value = auth(
        c.get(format!("{base}{path}"))
            .query(&[("limit", limit.to_string())]),
        &token,
    )
    .send()
    .await?
    .json()
    .await?;
    use owo_colors::OwoColorize;
    // The global board always returns its top 100; honor --limit client-side.
    let board = v["leaderboard"].as_array().cloned().unwrap_or_default();
    let board: Vec<serde_json::Value> = board.into_iter().take(limit.max(1)).collect();
    for (i, e) in board.iter().enumerate() {
        let rank = e["rank"].as_u64().unwrap_or(i as u64 + 1);
        let name = e["username"]
            .as_str()
            .or_else(|| e["name"].as_str())
            .unwrap_or("?");
        let line = format!(
            "{:>3}  {:<24} {:>6} pts  rating {}",
            rank,
            name,
            e["points"]
                .as_u64()
                .unwrap_or(e["score"].as_u64().unwrap_or(0)),
            e["rating"].as_i64().unwrap_or(e["elo"].as_i64().unwrap_or(0)),
        );
        match rank {
            1 => println!("{}", line.yellow().bold()),
            2 => println!("{}", line.white().bold()),
            3 => println!("{}", line.bright_yellow()),
            _ => println!("{line}"),
        }
    }
    Ok(())
}


fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in line.chars() {
        match (quote, ch) {
            (None, '"' ) | (None, '\'') => quote = Some(ch),
            (Some(q), c) if c == q => quote = None,
            (None, ' ') | (None, '\t') => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            (_, c) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

async fn dispatch(cmd: Cmd, api: &str) -> Result<()> {
    match cmd {
        Cmd::Login => cmd_login(api).await?,
        Cmd::Logout => {
            fs::remove_file(token_path()?).ok();
            println!("signed out");
        }
        Cmd::Me => cmd_me(api).await?,
        Cmd::Categories => cmd_categories(api).await?,
        Cmd::List { category, limit, event } => cmd_list(api, category, limit, event).await?,
        Cmd::Event { slug } => cmd_event(api, slug).await?,
        Cmd::Join { slug, team } => cmd_join(api, slug, team).await?,
        Cmd::Show { id } => { let id = resolve_name(api, id, "challenge").await?; cmd_show(api, id).await? }
        Cmd::Get { id, dest } => { let id = resolve_name(api, id, "challenge").await?; cmd_get(api, id, dest).await? }
        Cmd::Submit { id, flag } => { let id = resolve_name(api, id, "challenge to submit to").await?; cmd_submit(api, id, flag).await? }
        Cmd::Hint { id } => { let id = resolve_name(api, id, "challenge").await?; cmd_hint(api, id).await? }
        Cmd::Leaderboard { event, limit, watch, interval } => {
            cmd_leaderboard(api, &event, limit, watch, interval).await?
        }
        Cmd::New { manifest, event } => cmd_new(api, manifest, event).await?,
        Cmd::Set { id, values } => cmd_set(api, id, values).await?,
        Cmd::FilesAdd { id, paths, manifest_dir: _ } => cmd_files_add(api, id, paths).await?,
        Cmd::FilesRemove { id, file_id } => cmd_files_remove(api, id, file_id).await?,
        Cmd::Publish { manifest, event } => cmd_publish(api, manifest, event).await?,
        Cmd::Verify { id, manifest } => { let rid = if id.trim().parse::<u64>().is_ok() { id.trim().parse().unwrap_or(0) } else { resolve_name(api, Some(id), "challenge").await? }; cmd_verify(api, rid, manifest).await? }
        Cmd::Delete { id, confirm } => cmd_delete(api, id, confirm).await?,
    }
    Ok(())
}

async fn shell(api: &str) -> Result<()> {
    println!("g24 shell — type `help` for commands, `exit` to quit.");
    let mut rl = rustyline::DefaultEditor::new()?;
    let hist = config_dir().map(|d| d.join("history")).unwrap_or_else(|_| PathBuf::from(".g24-history"));
    let _ = rl.load_history(&hist);
    while let Ok(input) = rl.readline("g24> ") {
        let line = input.trim();
        if line.is_empty() {
            continue;
        }
        if line == "exit" || line == "quit" {
            break;
        }
        if line == "help" {
            println!("login logout me categories list show get submit hint leaderboard event join new set files-add files-remove publish verify delete");
            println!("examples: list --limit 10 | submit 38 'G24{{...}}' | leaderboard --watch");
            continue;
        }
        let argv = std::iter::once("g24".to_string()).chain(split_args(line));
        let cmd = match Cli::try_parse_from(argv) {
            Ok(c) => c.cmd,
            Err(e) => {
                e.print()?;
                continue;
            }
        };
        let Some(cmd) = cmd else {
            println!("empty command (this cannot happen)");
            continue;
        };
        if let Err(e) = dispatch(cmd, api).await {
            println!("error: {:#}", e);
        }
    }
    let _ = rl.save_history(&hist);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let api = api_base(&cli.api);
    match cli.cmd {
        Some(cmd) => dispatch(cmd, &api).await,
        None => shell(&api).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn split_args_respects_quotes() {
        assert_eq!(split_args("submit 38 'G24{abc}'"), ["submit", "38", "G24{abc}"]);
        assert_eq!(split_args("show \"xor vault\""), ["show", "xor vault"]);
        assert_eq!(split_args("  list   --limit 10 "), ["list", "--limit", "10"]);
        assert_eq!(split_args(""), Vec::<String>::new());
    }

    #[test]
    fn parse_kv_accepts_and_rejects() {
        assert_eq!(parse_kv("points=500").unwrap(), ("points".to_string(), "500".to_string()));
        assert!(parse_kv("points").is_err());
        assert!(parse_kv("=500").is_err());
    }

    fn good_manifest() -> Manifest {
        Manifest {
            title: "Cold Kitchen".into(),
            category: "Boot to Root".into(),
            difficulty: "hard".into(),
            points: 500,
            description: "A sufficiently long description here.".into(),
            flags: vec!["G24{test}".into()],
            hints: vec![],
            files: vec![],
            external: None,
            event_id: None,
        }
    }

    #[test]
    fn manifest_validation_mirrors_server_guards() {
        assert!(validate_manifest(&good_manifest()).is_ok());
        let mut m = good_manifest();
        m.title = "ab".into();
        assert!(validate_manifest(&m).is_err());
        // Short and empty descriptions are legal (file-only challenges).
        let mut m = good_manifest();
        m.description = "go".into();
        assert!(validate_manifest(&m).is_ok());
        let mut m = good_manifest();
        m.description = "".into();
        assert!(validate_manifest(&m).is_ok());
        let mut m = good_manifest();
        m.difficulty = "nightmare".into();
        assert!(validate_manifest(&m).is_err());
        let mut m = good_manifest();
        m.difficulty = "expert".into();
        assert!(validate_manifest(&m).is_ok());
        let mut m = good_manifest();
        m.points = 5;
        assert!(validate_manifest(&m).is_err());
        let mut m = good_manifest();
        m.points = 1001;
        assert!(validate_manifest(&m).is_err());
    }

    #[test]
    fn live_points_prefers_decayed_value() {
        assert_eq!(live_points(&json!({"points": 500, "value": 450})), 450);
        assert_eq!(live_points(&json!({"points": 500})), 500);
        assert_eq!(live_points(&json!({})), 0);
    }

    #[test]
    fn u64_either_reads_both_casings() {
        assert_eq!(u64_either(&json!({"initialPoints": 500}), "initialPoints", "initial_points"), Some(500));
        assert_eq!(u64_either(&json!({"initial_points": 400}), "initialPoints", "initial_points"), Some(400));
        assert_eq!(u64_either(&json!({}), "initialPoints", "initial_points"), None);
    }
}
