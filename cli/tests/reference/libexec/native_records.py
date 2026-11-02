"""Read repository-owned record names without imposing Ramify metadata."""
import datetime
import os
import re
import yaml

SLUG = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*")


class Fail(Exception):
    pass


# Native records use YAML 1.2 booleans, so a date field named `on` stays a string.
class NativeRecordLoader(yaml.SafeLoader):
    pass


NativeRecordLoader.yaml_implicit_resolvers = {
    key: [(tag, pattern) for tag, pattern in rules if tag != "tag:yaml.org,2002:bool"]
    for key, rules in yaml.SafeLoader.yaml_implicit_resolvers.items()
}
NativeRecordLoader.add_implicit_resolver(
    "tag:yaml.org,2002:bool", re.compile(r"^(?:true|True|TRUE|false|False|FALSE)$"), list("tTfF"))





def check_native_field(key, value, path):
    allowed = {"filename", "workflow_field", "slug_field", "date_field", "note_field"}
    if key not in allowed:
        raise Fail(f"{path}: unknown record option {key}")
    if not isinstance(value, str):
        raise Fail(f"{path}: record.{key} must be a simple name")
    if not SLUG.fullmatch(value):
        raise Fail(f"{path}: record.{key} must be a simple name")


def check_native_record(defs, path):
    native = defs.get("record") or {}
    if not isinstance(native, dict):
        raise Fail(f"{path}: record must be a map")
    for key, value in native.items():
        check_native_field(key, value, path)


def check_step_files(d, path, step):
    files = (d or {}).get("files", [])
    if not isinstance(files, list):
        raise Fail(f"{path}: step {step} files must be a list of paths")
    if not all(valid_file_name(f) for f in files):
        raise Fail(f"{path}: step {step} files must be a list of paths")


def record_option(defs, key, fallback):
    return (defs.get("record") or {}).get(key, fallback)


def order_workflow(defs, meta):
    return meta[record_option(defs, "workflow_field", "workflow")]


def step_entry(defs, step, note=""):
    entry = {"step": step, record_option(defs, "date_field", "date"): datetime.date.today().isoformat()}
    if note:
        entry[record_option(defs, "note_field", "note")] = note
    return entry


def record_loader(defs):
    return NativeRecordLoader if (defs or {}).get("record") else yaml.SafeLoader


def check_order_keys(meta, defs, path):
    defs = defs or {}
    keys = [record_option(defs, "workflow_field", "workflow"), "lane"]
    if not defs.get("record"):
        keys.append("branch")
    for key in keys:
        if not isinstance(meta.get(key), str):
            raise Fail(f"{path}: no `{key}:`")


def check_native_start(defs):
    if defs.get("record"):
        raise Fail("the repository owns native record creation; use its pipeline")


def requested_files(defs, step, folder, opts):
    return opts.get("file") or [os.path.join(folder, f) for f in defs["steps"][step].get("files", [f"{step}.md"])]



def valid_file_name(value):
    return isinstance(value, str) and bool(value)
