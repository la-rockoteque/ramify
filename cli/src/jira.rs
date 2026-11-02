use crate::config::Config;
use base64_lite::encode;
use serde_json::{json, Value};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

mod base64_lite {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(input: &[u8]) -> String {
        let mut out = String::new();
        for chunk in input.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
            out.push(if chunk.len() > 1 {
                ALPHABET[((n >> 6) & 63) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[(n & 63) as usize] as char
            } else {
                '='
            });
        }
        out
    }
}

const TIMEOUT: u64 = 15;

pub fn jira_command(args: &[String]) -> i32 {
    let config = match Config::resolve() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ramify: {error}");
            return 1;
        }
    };
    match args.first().map(String::as_str) {
        Some("login") => login(&config),
        Some("check") => {
            if config.jira_url.is_empty() {
                eprintln!("ramify: no RAMIFY_JIRA_URL in {}", config.config_path.display());
                return 1;
            }
            match check(&config.jira_url) {
                Ok(message) => {
                    println!("{message}");
                    0
                }
                Err(error) => {
                    eprintln!("jira: {error}");
                    1
                }
            }
        }
        _ => {
            eprintln!("ramify: usage: ramify jira login | check");
            1
        }
    }
}

fn login(config: &Config) -> i32 {
    if config.jira_url.is_empty() {
        eprintln!(
            "ramify: set RAMIFY_JIRA_URL=https://<site>.atlassian.net in {} first",
            config.config_path.display()
        );
        return 1;
    }
    if !std::io::stdin().is_terminal() {
        eprintln!("ramify: 'ramify jira login' asks for a token: run it in a terminal");
        return 1;
    }
    let host = site_host(&config.jira_url);
    loop {
        let status = std::process::Command::new("security")
            .args(["delete-generic-password", "-s", &format!("ramify-jira:{host}")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if !matches!(status, Ok(status) if status.success()) {
            break;
        }
    }
    print!("Atlassian email for {host}: ");
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let mut email = String::new();
    let _ = std::io::stdin().read_line(&mut email);
    let email = email.trim().to_string();
    if email.is_empty() {
        eprintln!("ramify: no email given");
        return 1;
    }
    println!("API token, from https://id.atlassian.com/manage-profile/security/api-tokens");
    let status = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-s",
            &format!("ramify-jira:{host}"),
            "-a",
            &email,
            "-l",
            &format!("ramify Jira token ({host})"),
            "-w",
        ])
        .status();
    if !matches!(status, Ok(status) if status.success()) {
        eprintln!("ramify: the Keychain refused the token");
        return 1;
    }
    match check(&config.jira_url) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(error) => {
            eprintln!("jira: {error}");
            1
        }
    }
}

fn site_host(site: &str) -> String {
    url::Url::parse(site)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_default()
}

pub fn validate_site(site: &str) -> Result<(), String> {
    let url = url::Url::parse(site).map_err(|_| format!("RAMIFY_JIRA_URL must be an https:// URL, not {site}"))?;
    let local = url.host_str() == Some("localhost") || url.host_str() == Some("127.0.0.1");
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err(format!("RAMIFY_JIRA_URL must be an https:// URL, not {site}"));
    }
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("RAMIFY_JIRA_URL must be a site URL without credentials, query, or fragment".into());
    }
    Ok(())
}

fn credentials(site: &str) -> Result<(String, String), String> {
    if let (Ok(email), Ok(token)) = (
        std::env::var("RAMIFY_JIRA_EMAIL"),
        std::env::var("RAMIFY_JIRA_TOKEN"),
    ) {
        if !email.is_empty() && !token.is_empty() {
            return Ok((email, token));
        }
    }
    let service = format!("ramify-jira:{}", site_host(site));
    let attrs = std::process::Command::new("security")
        .args(["find-generic-password", "-s", &service])
        .output();
    let token = std::process::Command::new("security")
        .args(["find-generic-password", "-s", &service, "-w"])
        .output();
    let (Ok(attrs), Ok(token)) = (attrs, token) else {
        return Err("no Jira credentials: run 'ramify jira login' (or set RAMIFY_JIRA_EMAIL and RAMIFY_JIRA_TOKEN)".into());
    };
    if !attrs.status.success() || !token.status.success() {
        return Err("no Jira credentials: run 'ramify jira login' (or set RAMIFY_JIRA_EMAIL and RAMIFY_JIRA_TOKEN)".into());
    }
    let attrs = String::from_utf8_lossy(&attrs.stdout).into_owned();
    let token = String::from_utf8_lossy(&token.stdout).trim().to_string();
    let regex = regex::Regex::new(r#""acct"<blob>="([^"]*)""#).map_err(|e| e.to_string())?;
    let email = regex
        .captures(&attrs)
        .and_then(|captures| captures.get(1))
        .map(|found| found.as_str().to_string())
        .unwrap_or_default();
    if email.is_empty() || token.is_empty() {
        return Err(format!("the Keychain item {service} is incomplete: run 'ramify jira login' again"));
    }
    Ok((email, token))
}

fn auth_header(email: &str, token: &str) -> String {
    format!("Basic {}", encode(format!("{email}:{token}").as_bytes()))
}

fn api(
    site: &str,
    auth: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Result<Option<Value>, String> {
    let url = format!("{}{}", site.trim_end_matches('/'), path);
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(TIMEOUT)))
            .build(),
    );
    let headers = |request: ureq::RequestBuilder<ureq::typestate::WithoutBody>| {
        request
            .header("Authorization", auth)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
    };
    let response = match method {
        "GET" => headers(agent.get(&url)).call(),
        "POST" => agent
            .post(&url)
            .header("Authorization", auth)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .send(body.unwrap_or_else(|| json!({})).to_string()),
        "PUT" => agent
            .put(&url)
            .header("Authorization", auth)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .send(body.unwrap_or_else(|| json!({})).to_string()),
        _ => return Err(format!("unsupported method {method}")),
    };
    let mut response = response.map_err(|error| format!("cannot reach {site}: {error}"))?;
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().unwrap_or_default();
    if status == 404 && method == "GET" {
        return Ok(None);
    }
    if status == 401 || status == 403 {
        return Err(format!("Jira refused the credentials ({status}): run 'ramify jira login'"));
    }
    if status >= 400 {
        return Err(format!(
            "Jira answered {status} to {method} {path}: {}",
            text.chars().take(300).collect::<String>()
        ));
    }
    if text.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&text).map(Some).map_err(|error| format!("Jira returned invalid JSON: {error}"))
}

fn status(site: &str, auth: &str, key: &str) -> Result<Option<(String, String)>, String> {
    let issue = api(
        site,
        auth,
        "GET",
        &format!("/rest/api/3/issue/{}?fields=status", url::form_urlencoded::byte_serialize(key.as_bytes()).collect::<String>()),
        None,
    )?;
    let Some(issue) = issue else {
        return Ok(None);
    };
    let category = issue["fields"]["status"]["statusCategory"]["key"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let name = issue["fields"]["status"]["name"].as_str().unwrap_or("").to_string();
    Ok(Some((category, name)))
}

pub fn ticket_status(site: &str, key: &str) -> Result<Option<(String, String)>, String> {
    validate_site(site)?;
    let (email, token) = credentials(site)?;
    status(site, &auth_header(&email, &token), key)
}

pub fn ticket_close(site: &str, key: &str) -> Result<String, String> {
    validate_site(site)?;
    let (email, token) = credentials(site)?;
    let auth = auth_header(&email, &token);
    let Some((category, name)) = status(site, &auth, key)? else {
        return Ok(format!("{key} not found in Jira, nothing closed"));
    };
    if category == "done" {
        return Ok(format!("{key} already {name}"));
    }
    let path = format!(
        "/rest/api/3/issue/{}/transitions",
        url::form_urlencoded::byte_serialize(key.as_bytes()).collect::<String>()
    );
    let transitions = api(site, &auth, "GET", &path, None)?.unwrap_or_else(|| json!({}));
    let done: Vec<Value> = transitions["transitions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|transition| transition["to"]["statusCategory"]["key"].as_str() == Some("done"))
        .collect();
    if done.is_empty() {
        return Err(format!("{key} is {name} and its workflow has no transition to a done status from there"));
    }
    let want = std::env::var("RAMIFY_JIRA_DONE").unwrap_or_default().to_lowercase();
    let pick = done
        .iter()
        .find(|transition| {
            !want.is_empty()
                && (transition["name"].as_str().unwrap_or("").to_lowercase().contains(&want)
                    || transition["to"]["name"].as_str().unwrap_or("").to_lowercase().contains(&want))
        })
        .unwrap_or(&done[0]);
    let id = pick["id"].as_str().unwrap_or("");
    api(site, &auth, "POST", &path, Some(json!({"transition": {"id": id}})))?;
    Ok(format!(
        "{key} closed ({name} → {})",
        pick["to"]["name"].as_str().unwrap_or("")
    ))
}

fn check(site: &str) -> Result<String, String> {
    validate_site(site)?;
    let (email, token) = credentials(site)?;
    let auth = auth_header(&email, &token);
    let me = api(site, &auth, "GET", "/rest/api/3/myself", None)?;
    match me {
        Some(me) => Ok(format!(
            "signed in to {site} as {}",
            me["displayName"].as_str().unwrap_or(&email)
        )),
        None => Err(format!("{site} has no Jira REST API")),
    }
}

pub fn ticket_command(args: &[String]) -> i32 {
    let config = match Config::resolve() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ramify: {error}");
            return 1;
        }
    };
    let action = args.first().map(String::as_str).unwrap_or("sync");
    match action {
        "start" | "sync" | "watch" => {}
        _ => {
            eprintln!("ramify: usage: ramify ticket start | sync | watch");
            return 1;
        }
    }
    if config.jira_url.is_empty() || config.value("RAMIFY_JIRA_SYNC").as_deref() == Some("0") {
        return 3;
    }
    let branch = crate::config::git(&["symbolic-ref", "--quiet", "--short", "HEAD"], &config.repo_root)
        .unwrap_or_default()
        .trim()
        .to_string();
    let key = match ticket_key(&branch, &config) {
        Some(key) => key,
        None => return 0,
    };
    let state = crate::commands::cksum(config.repo_root.to_string_lossy().as_bytes());
    let path = config.state_dir.join("tickets").join(format!("{state}.json"));
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new("/tmp")));
    if action == "watch" {
        return watch(&config, &path);
    }
    match sync(&config, &branch, &key, &path, action == "start") {
        Ok(messages) => {
            for message in messages {
                println!("{message}");
            }
            0
        }
        Err(error) => {
            eprintln!("ticket: {error}; retry with ramify ticket sync (watcher retries automatically)");
            1
        }
    }
}

fn ticket_key(branch: &str, config: &Config) -> Option<String> {
    let pattern = config
        .value("RAMIFY_TICKET_PATTERN")
        .unwrap_or_else(|| "[A-Za-z][A-Za-z0-9]*-[0-9]+".into());
    let regex = regex::Regex::new(&format!("-({pattern})-")).ok()?;
    let text = format!("-{}-", branch.replace('/', "-"));
    regex
        .captures(&text)
        .and_then(|captures| captures.get(1))
        .map(|found| found.as_str().to_uppercase())
}

fn lock(path: &Path, kind: &str) -> Option<std::fs::File> {
    use std::os::unix::io::AsRawFd;
    let lock_path = PathBuf::from(format!("{}.{kind}.lock", path.display()));
    let _ = std::fs::create_dir_all(lock_path.parent().unwrap_or(Path::new("/tmp")));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .ok()?;
    let status = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    (status == 0).then_some(file)
}

fn read_state(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({}))
}

fn save_state(path: &Path, state: &Value) {
    let temporary = PathBuf::from(format!("{}.tmp", path.display()));
    let _ = std::fs::write(&temporary, format!("{state}\n"));
    let _ = std::fs::rename(temporary, path);
}

fn sync(
    config: &Config,
    branch: &str,
    key: &str,
    path: &Path,
    start: bool,
) -> Result<Vec<String>, String> {
    let Some(_lock) = lock(path, "sync") else {
        return Ok(Vec::new());
    };
    let site = config.jira_url.clone();
    validate_site(&site)?;
    let mut state = read_state(path);
    let identity = json!([site, branch, key]);
    if state.get("identity") != Some(&identity) {
        state = json!({"identity": identity});
    }
    if start && state.get("started").and_then(Value::as_bool) != Some(true) {
        state["pending_start"] = Value::Bool(true);
        save_state(path, &state);
    }
    let remote = request_state(&config.repo_root, branch);
    let (email, token) = credentials(&site)?;
    let auth = auth_header(&email, &token);
    let mut messages = Vec::new();
    if state.get("pending_start").and_then(Value::as_bool) == Some(true) {
        let remote_ok = matches!(&remote, Ok(Some(state)) if state == "open" || state == "merged");
        messages.push(jira_start(&site, &auth, key, !remote_ok)?);
        state["started"] = Value::Bool(true);
        state["pending_start"] = Value::Bool(false);
        save_state(path, &state);
    }
    let remote = remote?;
    if let Some(remote) = remote {
        if remote == "open" || remote == "merged" {
            messages.push(jira_transition(
                &site,
                &auth,
                key,
                if remote == "open" { "review" } else { "done" },
            )?);
            save_state(path, &state);
        }
    }
    Ok(messages)
}

fn watch(config: &Config, path: &Path) -> i32 {
    let Some(_lock) = lock(path, "watch") else {
        return 0;
    };
    let initial = crate::config::git(&["symbolic-ref", "--quiet", "--short", "HEAD"], &config.repo_root)
        .unwrap_or_default()
        .trim()
        .to_string();
    if initial.is_empty() {
        return 0;
    }
    let interval: u64 = config
        .value("RAMIFY_TICKET_INTERVAL")
        .and_then(|value| value.parse().ok())
        .unwrap_or(60)
        .max(5);
    while config.repo_root.is_dir() {
        let branch = crate::config::git(&["symbolic-ref", "--quiet", "--short", "HEAD"], &config.repo_root)
            .unwrap_or_default()
            .trim()
            .to_string();
        if branch != initial {
            break;
        }
        let Some(key) = ticket_key(&branch, config) else {
            break;
        };
        let _ = sync(config, &branch, &key, path, false);
        std::thread::sleep(std::time::Duration::from_secs(interval));
    }
    0
}

fn jira_start(site: &str, auth: &str, key: &str, progress: bool) -> Result<String, String> {
    let path = issue_path(key);
    let issue = api(site, auth, "GET", &format!("{path}?fields=status,assignee"), None)?;
    let Some(issue) = issue else {
        return Err(format!("{key} not found in Jira"));
    };
    if issue["fields"]["status"]["statusCategory"]["key"].as_str() == Some("done") {
        return Ok(format!("{key} already done"));
    }
    let me = api(site, auth, "GET", "/rest/api/3/myself", None)?;
    let account = me
        .as_ref()
        .and_then(|me| me["accountId"].as_str())
        .map(str::to_string);
    let Some(account) = account else {
        return Err("Jira did not return the current user's accountId".into());
    };
    if issue["fields"]["assignee"]["accountId"].as_str() != Some(&account) {
        api(
            site,
            auth,
            "PUT",
            &format!("{path}/assignee"),
            Some(json!({"accountId": account})),
        )?;
    }
    if progress {
        jira_transition(site, auth, key, "progress")
    } else {
        Ok(format!("{key} assigned to the current Jira user"))
    }
}

fn jira_transition(site: &str, auth: &str, key: &str, stage: &str) -> Result<String, String> {
    let path = issue_path(key);
    let issue = api(site, auth, "GET", &format!("{path}?fields=status"), None)?;
    let Some(issue) = issue else {
        return Err(format!("{key} not found in Jira"));
    };
    let current = &issue["fields"]["status"];
    if current["statusCategory"]["key"].as_str() == Some("done") {
        return Ok(format!("{key} already {}", current["name"].as_str().unwrap_or("")));
    }
    let setting = match stage {
        "progress" => "RAMIFY_JIRA_PROGRESS",
        "review" => "RAMIFY_JIRA_REVIEW",
        _ => "RAMIFY_JIRA_DONE",
    };
    let want = std::env::var(setting)
        .unwrap_or_else(|_| if stage == "progress" { "In Progress".into() } else { String::new() })
        .trim()
        .to_string();
    if stage != "done" && want.is_empty() {
        return Err(format!("set {setting} to the Jira status or transition name"));
    }
    if !want.is_empty()
        && current["name"].as_str().unwrap_or("").to_lowercase() == want.to_lowercase()
    {
        return Ok(format!("{key} already {}", current["name"].as_str().unwrap_or("")));
    }
    let result = api(site, auth, "GET", &format!("{path}/transitions"), None)?;
    let Some(result) = result else {
        return Err(format!("cannot read transitions for {key}"));
    };
    let mut choices: Vec<Value> = result["transitions"].as_array().cloned().unwrap_or_default();
    if stage == "done" {
        choices.retain(|choice| choice["to"]["statusCategory"]["key"].as_str() == Some("done"));
    }
    if !want.is_empty() {
        choices.retain(|choice| {
            let name = choice["name"].as_str().unwrap_or("").to_lowercase();
            let to = choice["to"]["name"].as_str().unwrap_or("").to_lowercase();
            name == want.to_lowercase() || to == want.to_lowercase() || choice["id"].as_str() == Some(&want)
        });
    }
    if choices.len() != 1 {
        return Err(format!(
            "{key}: {setting}={want:?} matches {} available transitions; configure one exact status, transition name, or ID",
            choices.len()
        ));
    }
    let pick = &choices[0];
    let to = pick["to"]["name"].as_str().unwrap_or("");
    if current["name"].as_str() == Some(to) {
        return Ok(format!("{key} already {}", current["name"].as_str().unwrap_or("")));
    }
    api(
        site,
        auth,
        "POST",
        &format!("{path}/transitions"),
        Some(json!({"transition": {"id": pick["id"]}})),
    )?;
    Ok(format!(
        "{key}: {} → {to}",
        current["name"].as_str().unwrap_or("")
    ))
}

fn issue_path(key: &str) -> String {
    format!(
        "/rest/api/3/issue/{}",
        url::form_urlencoded::byte_serialize(key.as_bytes()).collect::<String>()
    )
}

fn request_state(root: &Path, branch: &str) -> Result<Option<String>, String> {
    let (provider, hostname, repo) = remote_info(root)?;
    let token = host_token(&provider, &hostname);
    let mut headers: Vec<(String, String)> = vec![("Accept".into(), "application/json".into())];
    if let Some(token) = &token {
        headers.push(("Authorization".into(), format!("Bearer {token}")));
    }
    let mut base = std::env::var("RAMIFY_GIT_API_URL").unwrap_or_default().trim_end_matches('/').to_string();
    if base.is_empty() {
        base = match provider.as_str() {
            "github" => {
                if hostname == "github.com" {
                    "https://api.github.com".into()
                } else {
                    format!("https://{hostname}/api/v3")
                }
            }
            "gitlab" => format!("https://{hostname}/api/v4"),
            _ => "https://api.bitbucket.org/2.0".into(),
        };
    }
    validate_site(&base)?;
    let main = std::env::var("RAMIFY_MAIN").unwrap_or_else(|_| "main".into());
    let owner = repo.split('/').next().unwrap_or("");
    let url = match provider.as_str() {
        "github" => format!(
            "{base}/repos/{repo}/pulls?state=all&head={}&base={main}&sort=created&direction=desc&per_page=100",
            urlencoding(&format!("{owner}:{branch}"))
        ),
        "gitlab" => {
            if let Some(token) = &token {
                headers.push(("PRIVATE-TOKEN".into(), token.clone()));
            }
            format!(
                "{base}/projects/{}/merge_requests?scope=all&state=all&source_branch={}&target_branch={main}&order_by=created_at&sort=desc&per_page=100",
                urlencoding(&repo),
                urlencoding(branch)
            )
        }
        _ => {
            if let Ok(email) = std::env::var("RAMIFY_BITBUCKET_EMAIL") {
                if let Some(token) = &token {
                    headers.retain(|(key, _)| key != "Authorization");
                    headers.push((
                        "Authorization".into(),
                        format!("Basic {}", encode(format!("{email}:{token}").as_bytes())),
                    ));
                }
            }
            let query = format!(
                "state=OPEN&state=MERGED&state=DECLINED&state=SUPERSEDED&q={}",
                urlencoding(&format!(
                    "source.branch.name = {} AND destination.branch.name = {}",
                    json!(branch),
                    json!(main)
                ))
            );
            format!("{base}/repositories/{repo}/pullrequests?{query}&sort=-created_on&pagelen=100")
        }
    };
    let rows = if provider == "gitlab" && token.is_none() {
        if std::env::var("RAMIFY_GIT_API_URL").is_ok() {
            return Err("set RAMIFY_GIT_TOKEN when RAMIFY_GIT_API_URL overrides the GitLab API".into());
        }
        gitlab_rows(root, &hostname, url.split("/api/v4/").nth(1).unwrap_or(""))
    } else {
        pages(&url, &headers, &provider)?
    };
    for row in rows {
        match provider.as_str() {
            "github" => {
                let head = &row["head"];
                if head["ref"].as_str() != Some(branch) {
                    continue;
                }
                if head["repo"]["full_name"].as_str().unwrap_or("").to_lowercase() != repo.to_lowercase() {
                    continue;
                }
                if row["base"]["ref"].as_str().unwrap_or(&main) != main {
                    continue;
                }
                return Ok(Some(
                    if row["merged_at"].is_string() {
                        "merged"
                    } else if row["state"].as_str() == Some("open") {
                        "open"
                    } else {
                        "closed"
                    }
                    .into(),
                ));
            }
            "gitlab" => {
                if row["source_branch"].as_str() != Some(branch) {
                    continue;
                }
                if row["source_project_id"] != row["target_project_id"] {
                    continue;
                }
                if row["target_branch"].as_str().unwrap_or(&main) != main {
                    continue;
                }
                return Ok(Some(
                    match row["state"].as_str() {
                        Some("opened") => "open",
                        Some("merged") => "merged",
                        _ => "closed",
                    }
                    .into(),
                ));
            }
            _ => {
                if row["source"]["branch"]["name"].as_str() != Some(branch) {
                    continue;
                }
                if row["source"]["repository"]["full_name"].as_str().unwrap_or("").to_lowercase()
                    != repo.to_lowercase()
                {
                    continue;
                }
                if row["destination"]["branch"]["name"].as_str().unwrap_or(&main) != main {
                    continue;
                }
                return Ok(Some(
                    match row["state"].as_str() {
                        Some("OPEN") => "open",
                        Some("MERGED") => "merged",
                        _ => "closed",
                    }
                    .into(),
                ));
            }
        }
    }
    Ok(None)
}

fn remote_info(root: &Path) -> Result<(String, String, String), String> {
    let remote = crate::config::git(&["remote", "get-url", "origin"], root)
        .map_err(|_| "cannot identify Git host from the configured remote".to_string())?;
    let remote = remote.trim();
    let normalized = if !remote.contains("://") {
        let regex = regex::Regex::new(r"^(?:[^@/:]+@)?([^/:]+):(.+)$").map_err(|e| e.to_string())?;
        let Some(captures) = regex.captures(remote) else {
            return Err("cannot identify Git host from the configured remote".into());
        };
        format!("ssh://{}/{}", &captures[1], &captures[2])
    } else {
        remote.to_string()
    };
    let parsed = url::Url::parse(&normalized).map_err(|_| "cannot identify Git host from the configured remote".to_string())?;
    let host = parsed.host_str().unwrap_or("").to_string();
    let provider = std::env::var("RAMIFY_GIT_HOST").ok().or_else(|| {
        match host.as_str() {
            "github.com" => Some("github".into()),
            "gitlab.com" => Some("gitlab".into()),
            "bitbucket.org" => Some("bitbucket".into()),
            _ => None,
        }
    }).ok_or_else(|| "set RAMIFY_GIT_HOST=github|gitlab|bitbucket for this remote".to_string())?;
    if !matches!(provider.as_str(), "github" | "gitlab" | "bitbucket") {
        return Err("set RAMIFY_GIT_HOST=github|gitlab|bitbucket for this remote".into());
    }
    let repo = parsed
        .path()
        .trim_matches('/')
        .trim_end_matches(".git")
        .to_string();
    if !repo.contains('/') {
        return Err("the Git remote needs an owner and repository".into());
    }
    Ok((provider, host, repo))
}

fn host_token(provider: &str, hostname: &str) -> Option<String> {
    if let Ok(token) = std::env::var("RAMIFY_GIT_TOKEN") {
        if !token.is_empty() {
            return Some(token);
        }
    }
    let name = match provider {
        "github" => "GH_TOKEN",
        "gitlab" => "GITLAB_TOKEN",
        _ => "RAMIFY_BITBUCKET_TOKEN",
    };
    if let Ok(token) = std::env::var(name) {
        if !token.is_empty() {
            return Some(token);
        }
    }
    match provider {
        "gitlab" => None,
        "github" => std::process::Command::new("gh")
            .args(["auth", "token", "--hostname", hostname])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|token| !token.is_empty()),
        _ => None,
    }
}

fn pages(url: &str, headers: &[(String, String)], provider: &str) -> Result<Vec<Value>, String> {
    let origin = url::Url::parse(url).map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    let mut next = Some(url.to_string());
    let next_regex = regex::Regex::new(r#"<([^>]+)>;\s*rel="next""#).ok();
    let mut rows = Vec::new();
    while let Some(current) = next {
        if !seen.insert(current.clone()) {
            return Err("Git host returned an unsafe or repeated pagination URL".into());
        }
        let parsed = url::Url::parse(&current).map_err(|_| "Git host returned an unsafe or repeated pagination URL".to_string())?;
        if parsed.scheme() != origin.scheme() || parsed.host_str() != origin.host_str() {
            return Err("Git host returned an unsafe or repeated pagination URL".into());
        }
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_global(Some(std::time::Duration::from_secs(30)))
                .build(),
        );
        let mut request = agent.get(&current);
        for (key, value) in headers {
            request = request.header(key, value);
        }
        let mut response = request.call().map_err(|error| {
            format!("cannot read Git host request state ({error})")
        })?;
        let status = response.status().as_u16();
        let link = response
            .headers()
            .get("Link")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let next_page = response
            .headers()
            .get("X-Next-Page")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let text = response.body_mut().read_to_string().unwrap_or_default();
        if status >= 400 {
            return Err(format!("Git host returned HTTP {status}; check host credentials and repository access"));
        }
        let data: Value = serde_json::from_str(&text)
            .map_err(|error| format!("cannot read Git host request state ({error})"))?;
        if provider == "bitbucket" {
            if let Some(values) = data["values"].as_array() {
                rows.extend(values.iter().cloned());
            }
            next = data["next"].as_str().map(str::to_string);
        } else if provider == "github" {
            if let Some(values) = data.as_array() {
                rows.extend(values.iter().cloned());
            }
            next = link.and_then(|link| {
                next_regex
                    .as_ref()
                    .and_then(|regex| regex.captures(&link))
                    .and_then(|captures| captures.get(1))
                    .map(|found| found.as_str().to_string())
            });
        } else {
            if let Some(values) = data.as_array() {
                rows.extend(values.iter().cloned());
            }
            next = next_page.map(|page| {
                let mut next = parsed.clone();
                next.query_pairs_mut().append_pair("page", &page);
                next.to_string()
            });
        }
    }
    Ok(rows)
}

fn gitlab_rows(root: &Path, hostname: &str, endpoint: &str) -> Vec<Value> {
    let output = std::process::Command::new("glab")
        .args(["api", endpoint, "--hostname", hostname, "--paginate", "--output", "ndjson"])
        .current_dir(root)
        .output();
    match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect(),
        _ => Vec::new(),
    }
}

fn urlencoding(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
