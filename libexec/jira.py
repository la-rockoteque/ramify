#!/usr/bin/env python3
"""ramify jira: read and close Jira tickets through the REST API.

Usage:  jira.py status <site-url> <KEY>     prints  KEY<TAB>category<TAB>status name
        jira.py close  <site-url> <KEY>     moves KEY to a done status unless it is in one
        jira.py check  <site-url>           exits 0 when the credentials work

"Closed" is the status *category* done, not a status name: every workflow names its last
column differently, and the category is what Jira itself uses to mean finished.

Credentials: RAMIFY_JIRA_EMAIL and RAMIFY_JIRA_TOKEN, else the macOS Keychain item that
`ramify jira login` writes (service "ramify-jira:<host>", account = email, password = token).
RAMIFY_JIRA_DONE picks one done transition by name when a workflow has several.
"""
import base64
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request

TIMEOUT = 15


class Fail(Exception):
    pass


def validate_site(site):
    url = urllib.parse.urlparse(site)
    if url.scheme != "https" and not (url.scheme == "http" and url.hostname in ("localhost", "127.0.0.1")):
        raise Fail(f"RAMIFY_JIRA_URL must be an https:// URL, not {site}")
    if not url.hostname or url.username or url.password or url.query or url.fragment:
        raise Fail("RAMIFY_JIRA_URL must be a site URL without credentials, query, or fragment")


def transition(site, auth, key, stage, targets=None):
    path = f"/rest/api/3/issue/{urllib.parse.quote(key, safe='')}"
    issue = api(site, auth, "GET", path + "?fields=status")
    if issue is None:
        raise Fail(f"{key} not found in Jira")
    current = issue["fields"]["status"]
    if current["statusCategory"]["key"] == "done":
        return f"{key} already {current['name']}"
    setting = {"progress": "RAMIFY_JIRA_PROGRESS", "review": "RAMIFY_JIRA_REVIEW", "done": "RAMIFY_JIRA_DONE"}[stage]
    want = os.environ.get(setting, "In Progress" if stage == "progress" else "").strip()
    if stage != "done" and not want:
        raise Fail(f"set {setting} to the Jira status or transition name")
    if want and current["name"].casefold() == want.casefold():
        return f"{key} already {current['name']}"
    known = (targets or {}).get(stage, {})
    if known.get("mapping") == want and known.get("status") == current["name"]:
        return f"{key} already {current['name']}"
    result = api(site, auth, "GET", path + "/transitions")
    if result is None:
        raise Fail(f"cannot read transitions for {key}")
    choices = result["transitions"]
    if stage == "done":
        choices = [t for t in choices if t["to"]["statusCategory"]["key"] == "done"]
    if want:
        choices = [t for t in choices if want.casefold() in (t["name"].casefold(), t["to"]["name"].casefold()) or want == t["id"]]
    if len(choices) != 1:
        raise Fail(f"{key}: {setting}={want!r} matches {len(choices)} available transitions; configure one exact status, transition name, or ID")
    pick = choices[0]
    if current["name"] == pick["to"]["name"]:
        return f"{key} already {current['name']}"
    api(site, auth, "POST", path + "/transitions", {"transition": {"id": pick["id"]}})
    if targets is not None:
        targets[stage] = {"mapping": want, "status": pick["to"]["name"]}
    return f"{key}: {current['name']} → {pick['to']['name']}"


def start(site, auth, key, targets=None, progress=True):
    path = f"/rest/api/3/issue/{urllib.parse.quote(key, safe='')}"
    issue = api(site, auth, "GET", path + "?fields=status,assignee")
    if issue is None:
        raise Fail(f"{key} not found in Jira")
    if issue["fields"]["status"]["statusCategory"]["key"] == "done":
        return f"{key} already done"
    me = api(site, auth, "GET", "/rest/api/3/myself")
    if not me or not me.get("accountId"):
        raise Fail("Jira did not return the current user's accountId")
    if (issue["fields"].get("assignee") or {}).get("accountId") != me["accountId"]:
        api(site, auth, "PUT", path + "/assignee", {"accountId": me["accountId"]})
    return transition(site, auth, key, "progress", targets) if progress else f"{key} assigned to the current Jira user"


def credentials(site):
    email, token = os.environ.get("RAMIFY_JIRA_EMAIL"), os.environ.get("RAMIFY_JIRA_TOKEN")
    if email and token:
        return email, token
    service = f"ramify-jira:{urllib.parse.urlparse(site).hostname}"
    try:
        attrs = subprocess.run(["security", "find-generic-password", "-s", service],
                               capture_output=True, text=True, check=True).stdout
        token = subprocess.run(["security", "find-generic-password", "-s", service, "-w"],
                               capture_output=True, text=True, check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        raise Fail("no Jira credentials: run 'ramify jira login' (or set RAMIFY_JIRA_EMAIL and RAMIFY_JIRA_TOKEN)")
    m = re.search(r'"acct"<blob>="([^"]*)"', attrs)
    if not m or not token:
        raise Fail(f"the Keychain item {service} is incomplete: run 'ramify jira login' again")
    return m.group(1), token


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def api(site, auth, method, path, body=None):
    req = urllib.request.Request(
        site.rstrip("/") + path, method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"Authorization": f"Basic {auth}", "Accept": "application/json",
                 "Content-Type": "application/json"})
    try:
        with urllib.request.build_opener(NoRedirect).open(req, timeout=TIMEOUT) as res:
            raw = res.read()
            return json.loads(raw) if raw else None
    except urllib.error.HTTPError as e:
        if e.code == 404 and method == "GET":
            return None
        if e.code in (401, 403):
            raise Fail(f"Jira refused the credentials ({e.code}): run 'ramify jira login'")
        detail = e.read().decode(errors="replace")[:300]
        raise Fail(f"Jira answered {e.code} to {method} {path}: {detail}")
    except (urllib.error.URLError, TimeoutError) as e:
        raise Fail(f"cannot reach {site}: {getattr(e, 'reason', e)}")


def status(site, auth, key):
    issue = api(site, auth, "GET", f"/rest/api/3/issue/{urllib.parse.quote(key)}?fields=status")
    if issue is None:
        return None
    st = issue["fields"]["status"]
    return st["statusCategory"]["key"], st["name"]


def close(site, auth, key):
    now = status(site, auth, key)
    if now is None:
        return f"{key} not found in Jira, nothing closed"
    category, name = now
    if category == "done":
        return f"{key} already {name}"
    path = f"/rest/api/3/issue/{urllib.parse.quote(key)}/transitions"
    done = [t for t in api(site, auth, "GET", path)["transitions"] if t["to"]["statusCategory"]["key"] == "done"]
    if not done:
        raise Fail(f"{key} is {name} and its workflow has no transition to a done status from there")
    want = os.environ.get("RAMIFY_JIRA_DONE", "").lower()
    pick = next((t for t in done if want and want in (t["name"].lower(), t["to"]["name"].lower())), done[0])
    api(site, auth, "POST", path, {"transition": {"id": pick["id"]}})
    return f"{key} closed ({name} → {pick['to']['name']})"


def main(argv):
    if len(argv) < 2 or argv[0] not in ("status", "close", "check"):
        print(__doc__.split("\n\n")[1], file=sys.stderr)
        return 2
    cmd, site, keys = argv[0], argv[1], argv[2:]
    validate_site(site)
    email, token = credentials(site)
    auth = base64.b64encode(f"{email}:{token}".encode()).decode()
    if cmd == "check":
        me = api(site, auth, "GET", "/rest/api/3/myself")
        if me is None:
            raise Fail(f"{site} has no Jira REST API")
        print(f"signed in to {site} as {me.get('displayName', email)}")
        return 0
    for key in keys:
        if cmd == "status":
            now = status(site, auth, key)
            print(f"{key}\t{now[0] if now else 'missing'}\t{now[1] if now else ''}")
        else:
            print(close(site, auth, key))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Fail as e:
        print(f"jira: {e}", file=sys.stderr)
        sys.exit(1)
