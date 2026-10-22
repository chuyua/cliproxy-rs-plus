#!/usr/bin/env python3
"""Check the frozen inventory against a read-only Go clone, without checking it out.

Usage: python3 docs/parity-audit/verify_reference.py /path/to/CLIProxyAPI

Derived from Go and compared both ways: test-suite files and their declarations,
router registrations (method and full path, resolving gin Group prefixes and string
constants and expanding method-list loops), YAML config leaves, CLI flags, plugin
RPC methods and added plugin API fields. Retained rows keep their text: suite rows
outside the fragments regenerated from Go, the upstream.* moves after rewriting
their approved prefix, and the other rewrites pinned to approved text. Not checked:
Rust behavior, fixture assertion coverage, whether each item's prose (handler,
type, default, meaning) is accurate, and routes without a statically resolved registration
(the WebUI OAuth forwarder, the AI Studio relay route and plugin routes).
"""

from collections import Counter
from functools import cache
import hashlib
import os
from pathlib import Path
import re
import subprocess
import sys
import traceback

import audit

OLD = "6fecc6e5567912661654a4eaf9b8f5436facd1c2"
NEW = "ae3ee3be303fb07fb8173279923413005bf32177"
BASE = "d1261f2"  # The Rust tree whose checklist holds the retained IDs.
TEST = re.compile(r"^func ((?:Test|Fuzz|Example|Benchmark)\w*)\(", re.M)
LINK = re.compile(r"https://github.com/router-for-me/CLIProxyAPI/blob/([a-f0-9]+)/([^#)]+)#L(\d+)(?:-L(\d+))?")
# Added Go test files with no declaration, excluded like the original support helpers.
SUPPORT_ONLY = {"internal/cache/issue6332_export_test.go"}
# Retained IDs whose contract text deliberately changed (reference-v8.0.35.md).
MOVED = {"M1-0027", "M1-0048", "M1-0049", "M1-0050", "M1-0051", "M1-0052", "M1-0053", "M1-0054", "M1-0055",
         "M1-0056", "M3-0137", "M3-0138", "M3-0139", "M3-0140", "M3-0141", "M3-0142", "M4-0133", "M4-0134"}
# The other rewrites, pinned to the approved text: the SHA-256 prefix of normalized(text).
PINNED = {"M1-0042": "b9a953a09aa5ebc0", "M1-0062": "830d521c48ab6c71", "M1-0063": "18647c89d4c9b36e",
          "M2-0014": "6b7ae606b4171005", "M3-0031": "f590c967b012cafd", "M4-0230": "637ee4b1a0c6f7e8",
          "M5-0042": "e26df97a522de247", "M5-0043": "4ee93575f834c205", "M5-0065": "34eac9c87a5e46b5",
          "M5-0066": "5557351c6d883bde"}
REWRITTEN = MOVED | PINNED.keys()
# Route items Go serves without a statically resolved path registration: the WebUI
# OAuth forwarder (a bare http.Server handler) and the AI Studio relay route.
DYNAMIC_ROUTES = {"M5-0020", "M5-0021"}
GIT_REPO_VARS = {"GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY",
                 "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_COMMON_DIR", "GIT_NAMESPACE"}
CONFIG_DIRS = ("internal/config/", "internal/registry/", "internal/pluginstore/", "sdk/pluginstore/", "sdk/config/")
ROUTE_SECTIONS = ("1. Public", "Temporary OAuth", "/v8/management", "/v0/management")
# Router call sites of the forms in the frozen tree (`receiver.METHOD(path, ...)`, with
# receivers that are the engine, a ServeMux or a Group variable). Every such call must
# resolve to methods and a full path, or be listed below. Other forms (Match, chained
# Group(...).GET) do not occur at v8.0.35 and are not recognized.
CALL = re.compile(r"\b([A-Za-z_][\w.]*)\.(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS|Any|HandleFunc|Handle)\((.*)")
GROUP = re.compile(r"^\s*(\w+)\s*:?=\s*([\w.]+)\.Group\(([^,)]*)")
CONST = re.compile(r'^\s*const\s+(\w+)\s*=\s*"([^"]*)"\s*$')
MUX = re.compile(r"^\s*(\w+)\s*:?=\s*http\.NewServeMux\(\)")
LOOP = re.compile(r"^\s*for\s+_,\s*(\w+)\s*:=\s*range\s+\[\]string\{([^}]*)\}")
GIN_ROOTS = ("s.engine",)
NOT_ROUTERS = {"windows"}  # windows.Handle(ptr) is a type conversion.
# AttachWebsocketRoute registers the wsrelay gateway path (M5-0021) passed in at runtime
# by sdk/cliproxy/service_lifecycle.go.
UNRESOLVED = {("internal/api/server_routes.go", "s.engine.GET(trimmed,")}
CITATION = re.compile(r"\b((?:cmd|internal|sdk)/[\w./\-]+\.go):(\d+)(?:-(\d+))?")
STRUCT = re.compile(r"^type (\w+) struct \{\n(.*?)^\}", re.M | re.S)


def check(ok, message):
    # Explicit failures: `assert` disappears under `python -O`.
    if not ok:
        sys.exit(f"FAIL: {message}")


def path_expr(expr, consts):
    """A Go path expression built from string literals and known constants, or None."""
    out = ""
    for part in expr.split("+"):
        part = part.strip()
        if m := re.fullmatch(r'"([^"]*)"', part):
            out += m[1]
        elif part in consts:
            out += consts[part]
        else:
            return None
    return out


def method_names(expr, loops):
    expr = expr.strip()
    if m := re.fullmatch(r'"([A-Z]+)"', expr):
        return [m[1]]
    if m := re.fullmatch(r"http\.Method(\w+)", expr):
        return [m[1].upper()]
    return loops.get(expr)


def registrations_in(path, text):
    """[(line, method, full path)] for every router registration in one Go file.

    Resolves gin Group prefixes (including nested groups) and string constants, and
    expands a Handle(method, ...) inside `for _, method := range []string{...}` into
    one registration per method. net/http ServeMux registrations match any method.
    """
    consts, prefixes, loops, out = {}, dict.fromkeys(GIN_ROOTS, ""), {}, []
    muxes = set()
    for number, line in enumerate(text.splitlines(), 1):
        if m := CONST.match(line):
            consts[m[1]] = m[2]
        if m := MUX.match(line):
            muxes.add(m[1])
        if m := LOOP.match(line):
            names = [method_names(x, {}) for x in m[2].split(",") if x.strip()]
            if all(names):
                loops[m[1]] = [n for x in names for n in x]
        if (m := GROUP.match(line)) and m[2] in prefixes:
            prefix = path_expr(m[3], consts)
            check(prefix is not None, f"unresolved group prefix: {path}:{number}")
            prefixes[m[1]] = prefixes[m[2]] + prefix
        m = CALL.search(line)
        if not m or m[1] in NOT_ROUTERS:
            continue
        receiver, verb, args = m[1], m[2], [a.strip() for a in m[3].split(",")]
        methods, route = None, None
        if receiver in muxes and verb in ("Handle", "HandleFunc"):
            methods, route = ["ANY"], path_expr(args[0], consts)
        elif receiver in prefixes and verb == "Handle" and len(args) > 1:
            methods, route = method_names(args[0], loops), path_expr(args[1], consts)
        elif receiver in prefixes and verb not in ("Handle", "HandleFunc"):
            methods, route = ["ANY" if verb == "Any" else verb], path_expr(args[0], consts)
        if methods and route is not None:
            out.extend((number, method, prefixes.get(receiver, "") + route) for method in methods)
        else:
            check(any(path == p and line.strip().startswith(s) for p, s in UNRESOLVED),
                  f"unrecognized router registration: {path}:{number}: {line.strip()}")
    return out


def moved_back(text):
    """A move's text with its approved prefix rewritten to the previous v8 path."""
    return re.sub(r"^`([^`]+)` \(previous v8 alias `([^`]+)`, ", r"`\2` (", text, count=1)


# History fragments, matched entry by entry so prose around or inside them stays compared.
DECL = r"`(?:Test|Fuzz|Benchmark|Example)\w*`"
REMOVED_NOTE = re.compile(rf" Removed upstream declarations: {DECL}(?:, {DECL})* \(historical, not active requirements\)\.")
MOVED_ENTRY = rf"{DECL} to [\w./\-]+_test\.go:\d+"
MOVED_NOTE = re.compile(rf" Moved upstream declarations: {MOVED_ENTRY}(?:; {MOVED_ENTRY})*\.")
DECLARED = rf"(?P<declared>`(?P<name>(?:Test|Fuzz|Benchmark|Example)\w*)` \(L(?P<line>\d+)\)(?:; )?)"
# One pattern for every span regenerated from Go, so a single pass over the original
# text records and removes each one; removing a span can never form another.
REGENERATED = re.compile(rf"(?P<removed>{REMOVED_NOTE.pattern})|(?P<moved>{MOVED_NOTE.pattern})|{DECLARED}")
# Markers no skeleton may keep: a surviving one means a span escaped the parse.
MARKER = re.compile(rf"{DECL} \(L|upstream declarations:")


SENTINEL = "\x00"  # Never in checklist text; stands in for each removed span.


def suite_parts(text):
    """(skeleton, [(declared name, line)], retired names, [(moved name, target, line)]).

    Links are stripped first, then one pass over that text replaces every span
    regenerated from Go with a sentinel and records it for checking against Go.
    A sentinel keeps a removal from joining its neighbours into a new match. Line
    numbers and LOC counts are normalized last: moved entries need their target
    lines, and that step only drops digits after a file extension or before " LOC".
    The skeleton compares without the sentinels.
    """
    check(SENTINEL not in text, "checklist text contains the verifier's sentinel")
    declared, retired, moved = [], [], []

    def take(match):
        if match["declared"]:
            declared.append((match["name"], int(match["line"])))
        elif match["removed"]:
            retired.extend(re.findall(r"`(\w+)`", match[0]))
        else:
            moved.extend((n, t, int(line)) for n, t, line in
                         re.findall(r"`(\w+)` to ([\w./\-]+_test\.go):(\d+)", match[0]))
        return SENTINEL

    text = REGENERATED.sub(take, audit.LINK.sub(r"\1", text))
    return normalized_lines(text).replace(SENTINEL, ""), declared, retired, moved


def suite_skeleton(text):
    """Suite text without the fragments regenerated from Go: line ranges, declarations, history."""
    return suite_parts(text)[0]


def normalized(text):
    """Contract text without links, Go line numbers or LOC counts, which follow the tree."""
    return normalized_lines(audit.LINK.sub(r"\1", text))


def normalized_lines(text):
    text = re.sub(r"(\.(?:go|md|json|yaml)):\d+(?:-\d+)?", r"\1", text)
    return re.sub(r"\d+ LOC", "N LOC", text)


def main():
    check(len(sys.argv) == 2, "usage: verify_reference.py /path/to/CLIProxyAPI")
    clone = sys.argv[1]

    # A hook's GIT_DIR and friends would override -C and read another repository.
    env = {k: v for k, v in os.environ.items() if k not in GIT_REPO_VARS}

    def git(*args, repo=clone):
        try:
            return subprocess.check_output(["git", "-C", repo, *args], text=True, stderr=subprocess.PIPE, env=env)
        except (OSError, subprocess.CalledProcessError) as e:
            sys.exit(f"FAIL: git {' '.join(args)} in {repo}: {getattr(e, 'stderr', '') or e}")

    @cache
    def source(ref, path):
        return git("show", f"{ref}:{path}")

    @cache
    def tree(ref):
        return git("ls-tree", "-r", "--name-only", ref).splitlines()

    def go_files(ref, prefixes):
        return [p for p in tree(ref) if p.startswith(prefixes) and p.endswith(".go") and not p.endswith("_test.go")]

    def test_files(ref):
        return {p for p in tree(ref) if p.endswith("_test.go")}

    check(git("rev-parse", "v8.0.35^{commit}").strip() == NEW, "v8.0.35 does not match the frozen commit")
    rows = audit.items()
    by_id = {r["id"]: r for r in rows}
    check(len(by_id) == len(rows), "duplicate checklist IDs")

    # Stable IDs: retained IDs keep their milestone, section and text. Suite rows compare
    # their text outside the fragments regenerated from Go, the 18 moves compare after
    # rewriting the approved prefix, and the other rewrites are pinned to approved text.
    original = git("show", f"{BASE}:docs/parity-audit/checklist.md", repo=audit.ROOT)
    old, section = {}, None
    for line in original.splitlines():
        if m := re.match(r"^(#{2,3}) (.*)", line):
            section = m[2].strip()
        if m := audit.ITEM.match(line):
            old[m[2]] = {"ms": m[1], "section": section, "text": audit.LINK.sub(r"\1", m[3].strip())}
    check(old.keys() <= by_id.keys(), f"existing IDs removed: {sorted(old.keys() - by_id.keys())}")
    changed = set()
    for i, before in old.items():
        now = by_id[i]
        check((now["ms"], now["section"]) == (before["ms"], before["section"]), f"{i} moved milestone or section")
        suite = audit.test_file(before["text"])
        if suite:
            # Declaration lists and history follow Go and are checked against it below.
            check(audit.test_file(now["text"]) == suite, f"suite ID changed its file: {i}")
            check(suite_skeleton(now["text"]) == suite_skeleton(before["text"]), f"suite text changed: {i}")
            check(not MARKER.search(suite_skeleton(now["text"])), f"unparsed declaration or history text: {i}")
        elif normalized(now["text"]) != normalized(before["text"]):
            changed.add(i)
    check(changed == REWRITTEN, f"retained contracts rewritten without a listed reason: {sorted(changed ^ REWRITTEN)}")
    aliases = set(re.findall(r'\{"(oauth\.providers\.[^"]+)", "(upstream\.[^"]+)"\}',
                             source(NEW, "internal/config/config_v8.go")))
    for i in sorted(MOVED):
        canonical = re.match(r"`([^`]+)` \(previous v8 alias `([^`]+)`", by_id[i]["text"])
        check(canonical and (canonical[2], canonical[1]) in aliases, f"{i} is not a Go upstream.* move")
        check(normalized(moved_back(by_id[i]["text"])) == normalized(old[i]["text"]),
              f"{i} changed more than its approved prefix")
    for i, digest in sorted(PINNED.items()):
        check(hashlib.sha256(normalized(by_id[i]["text"]).encode()).hexdigest()[:16] == digest,
              f"{i} differs from its approved rewrite")
    new_ids = by_id.keys() - old.keys()
    check(len(new_ids) == 264, f"expected 264 additions, got {len(new_ids)}")
    # The freeze recorded every addition as partial or missing; porting work may since
    # have covered some, so only a judgment with evidence is required now.
    manual = audit.load_manual()
    check(all(manual.get(i, {}).get("status") in ("covered", "partial", "missing")
              and manual[i]["evidence"] for i in new_ids),
          "every addition needs a judgment and evidence")

    # Test suites and their declarations.
    suites = {audit.test_file(r["text"]): r for r in rows if audit.test_file(r["text"])}
    check(len(suites) == 989, f"expected 989 suite files, got {len(suites)}")
    check(sum(bool(audit.test_file(r["text"])) for r in rows) == len(suites), "duplicate suite files")
    added = test_files(NEW) - test_files(OLD)
    check(len(added) == 228, f"expected 228 added Go test files, got {len(added)}")
    check(added - SUPPORT_ONLY <= suites.keys(), f"added suites missing: {sorted(added - SUPPORT_ONLY - suites.keys())}")
    check(not SUPPORT_ONLY & suites.keys(), "support-only files are not suites")
    check(not any(TEST.search(source(NEW, p)) for p in SUPPORT_ONLY), "a support-only file declares tests")
    check(not test_files(OLD) - test_files(NEW), "removed Go test files need explicit retirement")
    retired = moved = 0
    for path, row in suites.items():
        text = source(NEW, path)
        declarations = [(m[1], text.count("\n", 0, m.start()) + 1) for m in TEST.finditer(text)]
        skeleton, listed, names, moves = suite_parts(row["text"])
        check(not MARKER.search(skeleton), f"unparsed declaration or history text: {row['id']} {path}")
        check(listed == declarations, f"stale declarations or line numbers: {row['id']} {path}")
        span = re.match(rf"\*\*{re.escape(path)}:1-(\d+)\*\*", row["text"])
        check(span and int(span[1]) == len(text.splitlines()), f"suite range is not the whole file: {row['id']}")
        current = {name for name, _ in declarations}
        before = set(TEST.findall(source(OLD, path))) if path in test_files(OLD) else set()
        retired += len(names)
        check(set(names) <= before - current, f"retired declarations not removed from this file since {OLD[:7]}: {path}")
        for name, target, line in moves:
            moved += 1
            lines = source(NEW, target).splitlines() if target in test_files(NEW) else []
            check(name in before - current and 0 < line <= len(lines)
                  and lines[line - 1].startswith(f"func {name}("),
                  f"moved declaration not from this file or not at its target: {row['id']} {name}")
    check(retired == 43, f"expected 43 retired declarations, got {retired}")
    check(moved == 5, f"expected 5 moved declarations, got {moved}")

    page = Path(audit.HERE, "checklist.md").read_text()
    citations = LINK.findall(page)
    for ref, path, first, last in citations:
        check(ref == NEW, f"stale reference: {path}")
        lines = source(ref, path).splitlines()
        check(1 <= int(first) <= int(last or first) <= len(lines), f"out-of-bounds citation: {path}:{first}-{last}")
    cited = {}  # Go path -> [(first, last, id)]
    for row in rows:
        for path, first, last in CITATION.findall(row["text"]):
            cited.setdefault(path, []).append((int(first), int(last or first), row["id"]))

    def citing(path, line):
        return {i for first, last, i in cited.get(path, ()) if first <= line <= last}

    # Routes: every registration (method and full path) is an item citing it, and every
    # route item cites a registration with its method and full path.
    registrations = [(path, *r) for path in go_files(NEW, ("internal/", "sdk/"))
                     for r in registrations_in(path, source(NEW, path))]
    route_items = {}
    for row in rows:
        if row["section"].startswith(ROUTE_SECTIONS) and (m := re.search(r"`([A-Z]+) (/[^`\s]*)`", row["text"])):
            route_items[row["id"]] = (m[1], m[2])

    def matches(item, registration):
        return route_items[item] == registration[2:]

    for registration in registrations:
        check(any(matches(i, registration) for i in citing(*registration[:2]) if i in route_items),
              f"route registration missing from the checklist: {registration}")
    for item in route_items.keys() - DYNAMIC_ROUTES:
        check(any(item in citing(*r[:2]) and matches(item, r) for r in registrations),
              f"{item} does not cite a matching Go registration")

    # Config leaves: YAML paths derived from the Config struct equal the checklist's legacy paths.
    def leaves(ref):
        structs, alias = {}, {}
        for path in go_files(ref, CONFIG_DIRS):
            text = source(ref, path)
            structs.update((m[1], m[2]) for m in STRUCT.finditer(text))
            alias.update((m[1], m[2]) for m in re.finditer(r"^type (\w+) = (?:\w+\.)?(\w+)\s*$", text, re.M)
                         if m[1] != m[2])
        check("Config" in structs, f"no Config struct at {ref}")
        out = set()

        def walk(name, prefix, seen):
            for field, kind, tag in re.findall(r'^[ \t]*(\w+)\s+([^`\n]*?)\s*`[^`]*yaml:"([^"]*)"', structs[name], re.M):
                key, *options = tag.split(",")
                kind = kind or field  # Embedded struct.
                if key == "-" or (not key and "inline" not in options):
                    continue
                suffix = ""
                while True:
                    kind = kind.lstrip("*")
                    if kind.startswith("[]"):
                        suffix, kind = suffix + "[]", kind[2:]
                    elif m := re.match(r"map\[[\w.]+\](.*)", kind):
                        suffix, kind = suffix + ".{}", m[1]
                    else:
                        break
                base = kind.split(".")[-1]
                base = alias.get(base, base)
                path = prefix + key
                if base in structs and base not in seen:
                    walk(base, path + suffix + "." if key else prefix, seen | {base})
                else:
                    out.add(path)

        walk("Config", "", {"Config"})
        return out

    config_rows = [r for r in rows if r["section"].startswith("5. Config")]
    listed = Counter()
    for row in config_rows:
        if m := re.search(r"legacy `([^`]+)`\)", row["text"]):
            listed[re.sub(r"\{[^}]*\}", "{}", m[1])] += 1
    go_leaves, old_leaves = leaves(NEW), leaves(OLD)
    check(listed and max(listed.values()) == 1, "missing or duplicate config leaves")
    check(set(listed) == go_leaves, f"config leaves differ from Go: {sorted(set(listed) ^ go_leaves)}")
    additions = [r for r in config_rows if r["section"] == "5. Config: v8.0.35 additions"]
    legacy = [re.search(r"legacy `([^`]+)`\)", r["text"]) for r in additions]
    check(all(legacy), "a config addition names no legacy path")
    added_leaves = {re.sub(r"\{[^}]*\}", "{}", m[1]) for m in legacy}
    check(added_leaves == go_leaves - old_leaves and old_leaves <= go_leaves, "config additions differ from Go")

    # CLI flags, including the discover subcommand's flag set.
    main_go = source(NEW, "cmd/server/main.go")
    flags = {f"--{n}" for n in re.findall(r'^\s*flag\.\w+\((?:&\w+, )?"([^"]+)"', main_go, re.M)}
    flags |= {f"discover --{n}" for n in re.findall(r'\bdiscoverFlags\.\w+\((?:&\w+, )?"([^"]+)"', main_go, re.M)}
    listed_flags = [m[1] for r in rows if r["section"] == "CLI flags" and (m := re.match(r"`((?:discover )?--[\w-]+)`", r["text"]))]
    check(len(listed_flags) == len(set(listed_flags)) and set(listed_flags) == flags, "CLI flag inventory differs from Go")

    # Plugin ABI: RPC methods, and every exported API field added since the old reference is cited.
    methods = set(re.findall(r'^\s*Method\w+\s*=\s*"([^"]+)"', source(NEW, "sdk/pluginabi/types.go"), re.M))
    listed_methods = [m[1] for r in rows if (m := re.match(r"RPC `([^`]+)`", r["text"]))]
    check(len(listed_methods) == len(set(listed_methods)) and set(listed_methods) == methods, "RPC method inventory differs from Go")

    def api_fields(ref):
        out = {}
        for path in go_files(ref, ("sdk/pluginapi/", "sdk/pluginabi/")):
            text = source(ref, path)
            for m in STRUCT.finditer(text):
                for f in re.finditer(r"^\t([A-Z]\w*)\s+\S", m[2], re.M):
                    out[f"{m[1]}.{f[1]}"] = (path, text.count("\n", 0, m.start(2) + f.start()) + 1)
        return out

    fields, old_fields = api_fields(NEW), api_fields(OLD)
    check(old_fields.keys() <= fields.keys(), "plugin API fields removed")
    added_fields = fields.keys() - old_fields.keys()
    for name in added_fields:
        path, line = fields[name]
        whole = len(source(NEW, path).splitlines())
        # A whole-file citation (a suite's source list) does not inventory a field.
        check(any(first <= line <= last and (first, last) != (1, whole) for first, last, _ in cited.get(path, ())),
              f"added plugin API field not cited: {name}")
    for row in rows:
        if row["text"].startswith("Plugin data contract"):
            for name in re.findall(r"\b([A-Z]\w*\.[A-Z]\w*)\b", row["text"].split(". ")[0]):
                check(name in added_fields and row["id"] in citing(*fields[name]), f"{row['id']} names {name} wrongly")

    totals = Counter(r["ms"] for r in rows)
    check(totals == {"M1": 151, "M2": 189, "M3": 424, "M4": 589, "M5": 331, "M6": 267}, totals)
    print(f"OK: {len(rows)} items, {len(old)} retained IDs (text compared; suite rows outside regenerated "
          f"fragments, {len(MOVED)} moves after their approved prefix, {len(PINNED)} rewrites pinned); "
          f"{len(added) - len(SUPPORT_ONLY)} added / {len(suites)} total suites; {retired} retired and {moved} moved "
          f"declarations; {len(registrations)} route registrations (method and full path); {len(go_leaves)} config leaves "
          f"({len(go_leaves - old_leaves)} added); {len(flags)} CLI flags; {len(methods)} RPC methods; "
          f"{len(added_fields)} added plugin API fields; {len(citations)} current, in-bounds citations")
    print("Not checked: Rust behavior, item prose (handlers, types, defaults, meanings), plugin capabilities "
          "and pre-existing data contracts, and routes without a statically resolved path registration "
          f"({', '.join(sorted(DYNAMIC_ROUTES))} and plugin routes)")


if __name__ == "__main__":
    try:
        main()
    except Exception:  # Report anything unforeseen as a failure, not a crash.
        traceback.print_exc()
        sys.exit("FAIL: unexpected error (traceback above)")
