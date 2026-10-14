//! g24 — the G24SEC terminal client.
//!
//! Players: browse, download, submit, leaderboard. Organizers: author and
//! publish challenges. Auth is a browser device flow; the CLI never sees a
//! password.

use std::{fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::Deserialize;

const DEFAULT_API: &str = "https://api-3.g24sec.com";

#[derive(Parser)]
#[command(name = "g24", version, about = "G24SEC from your terminal")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
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
    },
    /// Show one challenge (description, points, artifacts)
    Show { id: u64 },
    /// Download artifacts with printed sha256
    Get {
        id: u64,
        #[arg(long, default_value = ".")] dest: PathBuf,
    },
    /// Submit a flag
    Submit { id: u64, flag: String },
    /// Next hint (may cost points)
    Hint { id: u64 },
    /// Leaderboard (global, or --event slug)
    Leaderboard {
        #[arg(long)] event: Option<String>,
        #[arg(long, default_value_t = 20)] limit: usize,
    },
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
        u["user_rating"].as_i64().unwrap_or(u["rating"].as_i64().unwrap_or(0)),
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

async fn cmd_list(api: &str, category: Option<String>, limit: usize) -> Result<()> {
    let mut q = vec![("standalone", "true".to_string()), ("limit", limit.to_string())];
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
            ch["points"].as_u64().unwrap_or(0),
            ch["difficulty"].as_str().unwrap_or("?"),
            ch["title"].as_str().unwrap_or("?"),
        );
    }
    if v["hasMore"].as_bool().unwrap_or(false) {
        println!("…more available, raise --limit");
    }
    Ok(())
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

async fn cmd_show(api: &str, id: u64) -> Result<()> {
    let ch = fetch_challenge(api, id).await?;
    println!("{}  [{}]", ch["title"].as_str().unwrap_or("?"), ch["id"].as_u64().unwrap_or(id));
    println!(
        "{} · {} · {}pts",
        ch["category"].as_str().unwrap_or("?"),
        ch["difficulty"].as_str().unwrap_or("?"),
        ch["points"].as_u64().unwrap_or(0),
    );
    println!();
    println!("{}", ch["description"].as_str().unwrap_or(""));
    if let Some(files) = ch["files"].as_array() {
        println!("\nartifacts:");
        for f in files {
            println!("  #{}  {} ({} bytes)", f["id"].as_u64().unwrap_or(0), f["name"].as_str().unwrap_or("?"), f["size_bytes"].as_u64().unwrap_or(0));
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
        println!("correct! +{} points", v["points"].as_u64().unwrap_or(0));
        if let Some(hp) = v["hintPenalty"].as_u64() {
            if hp > 0 {
                println!("(hint penalty -{}, base {})", hp, v["basePoints"].as_u64().unwrap_or(0));
            }
        }
    } else {
        println!("incorrect: {}", v["message"].as_str().unwrap_or("wrong flag"));
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

async fn cmd_leaderboard(api: &str, event: Option<String>, limit: usize) -> Result<()> {
    let path = match event {
        Some(slug) => format!("/api/ctf-leaderboard/{slug}"),
        None => "/api/ctf-leaderboard/global".to_string(),
    };
    let (c, base, token) = authed(api).await?;
    let v: serde_json::Value = auth(c
        .get(format!("{base}{path}"))
        .query(&[("limit", limit.to_string())]), &token)
        .send()
        .await?
        .json()
        .await?;
    let board = v["leaderboard"].as_array().cloned().unwrap_or_default();
    for (i, e) in board.iter().enumerate() {
        println!(
            "{:>3}  {:<24} {:>6} pts  rating {}",
            e["rank"].as_u64().unwrap_or(i as u64 + 1),
            e["username"].as_str().unwrap_or("?"),
            e["points"].as_u64().unwrap_or(e["score"].as_u64().unwrap_or(0)),
            e["rating"].as_i64().unwrap_or(e["elo"].as_i64().unwrap_or(0)),
        );
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let api = api_base(&cli.api);
    match cli.cmd {
        Cmd::Login => cmd_login(&api).await?,
        Cmd::Logout => {
            fs::remove_file(token_path()?).ok();
            println!("signed out");
        }
        Cmd::Me => cmd_me(&api).await?,
        Cmd::Categories => cmd_categories(&api).await?,
        Cmd::List { category, limit } => cmd_list(&api, category, limit).await?,
        Cmd::Show { id } => cmd_show(&api, id).await?,
        Cmd::Get { id, dest } => cmd_get(&api, id, dest).await?,
        Cmd::Submit { id, flag } => cmd_submit(&api, id, flag).await?,
        Cmd::Hint { id } => cmd_hint(&api, id).await?,
        Cmd::Leaderboard { event, limit } => cmd_leaderboard(&api, event, limit).await?,
    }
    Ok(())
}
