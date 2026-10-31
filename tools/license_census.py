#!/usr/bin/env python3
"""Sorts robots for OpenRumble by whether their jars have source and they have an acceptable license.

A robot can enter when the source of every class is in its jar and its license is permissive. The
license can come from the jar or from the robot's RoboWiki page. The wiki is taken as true unless the
jar says something that conflicts with it. docs/rules.md has the rules.

Jars are only read, never extracted or run, so this is safe to use on untrusted robots. Each robot
gets a verdict with the evidence for it, because a reviewer has to be able to check every one:

    eligible           source in the jar and an allowed license
    no-source          no source in the jar
    partial-source     source for only some of the robot's own classes
    no-license         source in the jar, but no license anywhere
    disallowed         a license that is not allowed, such as KPL, NPL or NRPL
    decide             a license with no ruling yet, such as GPL
    review-*           the evidence needs a person: a conflict, a weak or informal license,
                       embedded code under another license, or a library without source
    no-jar             the jar was not available

Overrides from a reviewer win over everything else. See load_overrides().

usage: license_census.py --list LIST.txt [--list ...] --jars DIR [--jars ...] [--wiki pages.json]
                         [--overrides overrides.tsv] --out robots.tsv
"""
import argparse
import collections
import csv
import json
import pathlib
import re
import sys
import zipfile

# Groups of licenses. An allowed license is permissive, or at least has no unreasonable conditions.
ALLOWED, DISALLOWED, DECIDE = "allowed", "disallowed", "decide"

# (name, group, pattern). Patterns look for wording that states a license, not for bare words, so
# that "zlib" in a comment about compression does not count.
I = re.IGNORECASE
LICENSES = [
    ("RWLPCL", ALLOWED, re.compile(r"rwlpcl|robo ?wiki limited public", I)),
    ("RWPCL", ALLOWED, re.compile(r"rwpcl|robo ?wiki public code|robocode wiki public code", I)),
    ("MIT", ALLOWED, re.compile(r"\bmit[- ]licen[cs]|licen[cs]ed under (the )?mit\b|mit licensed"
                                r"|permission is hereby granted, free of charge|opensource\.org/licenses/mit", I)),
    ("BSD", ALLOWED, re.compile(r"\bbsd[- ]*(2|3|two|three)?[- ]*(clause)?[- ]*licen[cs]|bsd licensed"
                                r"|redistribution and use in source and binary forms", I)),
    ("zlib", ALLOWED, re.compile(r"zlib[- /]licen[cs]e|terms of the zlib|wiki/zlib|licen[cs]e: *zlib|under (the )?zlib"
                                 r"|permission is granted to anyone to use this software for any purpose"
                                 r"|opensource\.org/licenses/zlib", I)),
    ("public-domain", ALLOWED, re.compile(r"public[- ]domain|\bcc0\b|creative ?commons ?zero", I)),
    ("WTFPL", ALLOWED, re.compile(r"\bwtfpl\b|do what the f", I)),
    ("Unlicense", ALLOWED, re.compile(r"\bthe unlicense\b|unlicense\.org", I)),
    ("Apache", ALLOWED, re.compile(r"apache licen[cs]e|apache-2\.0|apache 2\.0", I)),
    ("ISC", ALLOWED, re.compile(r"\bisc licen[cs]e", I)),
    ("CC-BY-NC", DECIDE, re.compile(r"cc[- ]by[- ]nc|attribution[- ]?non-?commercial|licenses/by-nc", I)),
    ("CC-BY-SA", DECIDE, re.compile(r"cc[- ]by[- ]sa|attribution[- ]?share ?alike|licenses/by-sa", I)),
    ("CC-BY-ND", DECIDE, re.compile(r"cc[- ]by[- ]nd|attribution[- ]?no-?deriv|licenses/by-nd", I)),
    ("CC-BY", ALLOWED, re.compile(r"cc[- ]by \d|creative commons attribution \d|licenses/by/\d", I)),
    ("LGPL", DECIDE, re.compile(r"\blgpl\b|gnu lesser general|gnu library general", I)),
    ("AGPL", DECIDE, re.compile(r"\bagpl\b|gnu affero", I)),
    ("GPL", DECIDE, re.compile(r"\bgpl\b|\bgplv\d|gnu general public licen|gnu public licen", I)),
    ("EPL", DECIDE, re.compile(r"eclipse public licen", I)),
    ("CPL", DECIDE, re.compile(r"common public licen", I)),
    ("MPL", DECIDE, re.compile(r"mozilla public licen", I)),
    # Case matters here, so that a variable called npl does not count.
    ("KPL", DISALLOWED, re.compile(r"\bKPL\b|[Kk]awigi ?[Pp]ublic ?[Ll]icen[cs]e")),
    ("NPL", DISALLOWED, re.compile(r"\bNPL\b|\bNPRL\b|\bNRPL\b|\bNat Public( Robot)? Licen[cs]e")),
    ("competition-ban", DISALLOWED, re.compile(
        r"competition[^\n]{0,80}(forbidden|prohibited|not allowed)|(forbidden|prohibited|not allowed)[^\n]{0,80}competition"
        r"|without[^\n]{0,30}permission[^\n]{0,80}(competition|rumble)", I)),
]
GROUP = {name: group for name, group, _ in LICENSES}
# These two only appear as the short license field of a wiki page. ACCSL calls itself closed source,
# but allows reuse, so it needs a ruling if a robot ever uses it.
GROUP.update({"Attribution": ALLOWED, "ACCSL": DECIDE})

# Wording that holds rights back, unless the same file also grants a license. BSD's "without specific
# prior written permission" is about using names, so it does not count.
RESERVED = re.compile(
    r"all rights reserved|may not be (used|copied|distributed|reproduced|modified)"
    r"|without (the )?(express )?(prior )?written permission|\bproprietary\b|closed[- ]source"
    r"|do not (copy|steal|redistribute|decompile)", I)

# Plain permission without a named license.
INFORMAL = re.compile(
    r"feel free to (use|adopt|adapt|take|copy|borrow|modify|steal|reuse)|free to use"
    r"|use (the|this|my) code (as|how|for)|use it (as|how)ever|do (what|whatever|what ever) you (want|like|wish)"
    r"|you (can|may) (freely )?(use|copy|modify|reuse) (this|the|my|any of (this|the|my)) (source|code)", I)

SOURCE_FILE = re.compile(r"\.(java|kt|scala|groovy)$", I)
TEXT_FILE = re.compile(r"\.(java|kt|scala|groovy|txt|md|html?|properties)$|(^|/)(license|licence|copying|readme|notice)[^/]*$", I)
LICENSE_FILE = re.compile(r"(^|/)(un)?(license|licence|copying)[^/]*$", I)
DECLARED_TYPE = re.compile(r"\b(?:class|interface|enum|object|trait|record)\s+(\w+)")
MAX_TEXT_BYTES = 2_000_000
EVIDENCE_CHARS = 150


def evidence_line(text, start):
    """The line around a match, cleaned up for a table cell."""
    begin = text.rfind("\n", 0, start) + 1
    end = text.find("\n", start)
    line = text[begin:end if end >= 0 else len(text)]
    return re.sub(r"\s+", " ", re.sub(r"^[\s/*#!<>:;-]+", "", line)).strip()[:EVIDENCE_CHARS]


def find_licenses(text):
    """Every license a text states, each with the line that states it."""
    found = {}
    for name, _, pattern in LICENSES:
        match = pattern.search(text)
        if match:
            found[name] = evidence_line(text, match.start())
    # The LGPL text talks about the GPL, and this wording does not mean both apply.
    if "LGPL" in found:
        found.pop("GPL", None)
    return found


class Jar:
    """What one robot's jar says about its source and license."""

    def __init__(self, path):
        self.source = "no-jar"
        self.classes = 0
        self.missing = []
        self.main_class = ""
        self.author = ""
        self.strong = {}   # license -> evidence, stated for the robot as a whole
        self.weak = {}     # license -> evidence, stated in only some of the robot's own files
        self.embedded = {}  # license -> evidence, from files outside the robot's own package
        self.informal = ""
        self.reserved = ""  # wording in the robot's own files that holds rights back
        self.readable = False
        if path is not None:
            self.read(path)

    def read(self, path):
        try:
            archive = zipfile.ZipFile(path)
            names = archive.namelist()
        except (zipfile.BadZipFile, OSError):
            self.source = "unreadable"
            return
        self.readable = True
        texts = {}
        for info in archive.infolist():
            if not info.is_dir() and info.file_size <= MAX_TEXT_BYTES and TEXT_FILE.search(info.filename):
                try:
                    texts[info.filename] = archive.read(info).decode("latin-1")
                except (zipfile.BadZipFile, OSError, RuntimeError, NotImplementedError):
                    pass
        self.read_properties(texts)
        self.check_source(names, texts)
        self.check_licenses(texts)

    def read_properties(self, texts):
        for name, text in texts.items():
            if name.endswith(".properties"):
                found = re.search(r"^robot\.classname\s*[=:]\s*(\S+)", text, re.M)
                if found and not self.main_class:
                    self.main_class = found.group(1)
                    author = re.search(r"^robot\.author\.name[ \t]*[=:][ \t]*(.*)$", text, re.M)
                    self.author = author.group(1).strip() if author else ""

    def check_source(self, names, texts):
        """A class counts as having source if any source file in the jar declares it."""
        class_files = [n for n in names
                       if n.endswith(".class") and "$" not in n and not n.startswith("META-INF/")]
        declared = set()
        sources = 0
        for name, text in texts.items():
            if SOURCE_FILE.search(name):
                sources += 1
                declared.add(name.rsplit("/", 1)[-1].rsplit(".", 1)[0])
                declared.update(DECLARED_TYPE.findall(text))
        simple_name = lambda path: path.rsplit("/", 1)[-1][:-6]
        missing = [n for n in class_files if simple_name(n) not in declared]
        self.classes = len(class_files)
        self.missing = sorted({simple_name(n) for n in missing})
        root = self.main_class.split(".")[0] if "." in self.main_class else ""
        if sources == 0:
            self.source = "none"
        elif not missing:
            self.source = "full"
        elif root and not any(n.startswith(root + "/") for n in missing):
            self.source = "own"  # all its own code, but no source for a library it carries
        else:
            self.source = "partial"

    def check_licenses(self, texts):
        root = self.main_class.split(".")[0] if "." in self.main_class else ""
        main_file = self.main_class.replace(".", "/")
        own_sources = [n for n in texts if SOURCE_FILE.search(n) and (not root or n.startswith(root + "/"))]
        counts = collections.Counter()
        first = {}
        for name, text in texts.items():
            found = find_licenses(text)
            own = not root or name.startswith(root + "/")
            about_robot = (name.rsplit(".", 1)[0] == main_file
                           or (LICENSE_FILE.search(name) and not name.startswith("META-INF/")))
            for license_name, line in found.items():
                evidence = f"{name}: {line}"
                if about_robot:
                    self.strong.setdefault(license_name, evidence)
                elif own and SOURCE_FILE.search(name):
                    counts[license_name] += 1
                    first.setdefault(license_name, evidence)
                elif own or not name.startswith("META-INF/"):
                    # A file in the robot's folder that is not source, or code from another package.
                    (self.weak if own else self.embedded).setdefault(license_name, evidence)
                else:
                    self.embedded.setdefault(license_name, evidence)
            if own and not self.informal:
                match = INFORMAL.search(text)
                if match:
                    self.informal = f"{name}: {evidence_line(text, match.start())}"
            if own and not self.reserved and not found:
                match = RESERVED.search(text)
                if match:
                    self.reserved = f"{name}: {evidence_line(text, match.start())}"
        for license_name, count in counts.items():
            if own_sources and count * 2 >= len(own_sources):
                self.strong.setdefault(license_name, first[license_name])
            elif license_name not in self.strong:
                self.weak.setdefault(license_name, first[license_name])


def strip_markup(value):
    """Plain text from wiki markup."""
    value = re.sub(r"\[https?://\S+\s+([^\]]*)\]", r"\1", value)
    value = re.sub(r"\[\[(?:[^\]|]*\|)?([^\]]*)\]\]", r"\1", value)
    return re.sub(r"'{2,}|<[^>]+>", "", value).strip()


# Names as they appear in a short field, where a bare word does mean a license.
SHORT_NAMES = [
    ("RWLPCL", re.compile(r"rwlpcl", I)),
    ("RWPCL", re.compile(r"rwpcl|robo ?wiki public", I)),
    ("MIT", re.compile(r"\bmit\b", I)),
    ("BSD", re.compile(r"\b(free)?bsd\b", I)),
    ("zlib", re.compile(r"\bzlib\b", I)),
    # Some wiki pages give "Attribution" as the license: free to use, with credit given.
    ("Attribution", re.compile(r"^\W*attribution\W*$", I)),
    ("ACCSL", re.compile(r"\baccsl\b|aurora company closed source", I)),
    ("public-domain", re.compile(r"public[- ]domain|\bcc0\b", I)),
    ("WTFPL", re.compile(r"\bwtfpl\b", I)),
    ("Unlicense", re.compile(r"\bunlicense\b", I)),
    ("Apache", re.compile(r"\bapache\b", I)),
    ("CC-BY-NC", re.compile(r"cc[- ]by[- ]nc|non-?commercial", I)),
    ("CC-BY-SA", re.compile(r"cc[- ]by[- ]sa|share-?alike", I)),
    ("CC-BY-ND", re.compile(r"cc[- ]by[- ]nd", I)),
    ("LGPL", re.compile(r"\blgpl", I)),
    ("AGPL", re.compile(r"\bagpl", I)),
    ("GPL", re.compile(r"\bgpl|gnu general", I)),
    ("EPL", re.compile(r"\bepl\b|eclipse public", I)),
    ("MPL", re.compile(r"\bmpl\b|mozilla public", I)),
    ("KPL", re.compile(r"\bkpl\b|kawigi ?public", I)),
    ("NPL", re.compile(r"\bnpl\b|\bnprl\b|\bnrpl\b|nat public", I)),
]
SAYS_NO = re.compile(r"^\W*(no\b(?! strings?)|nope\b|not\b|sorry|none\b|closed\b|n/a\b)|closed[- ]source|not open[- ]source", I)


def short_licenses(value):
    return [name for name, pattern in SHORT_NAMES if pattern.search(value)]


class Wiki:
    """What a robot's RoboWiki page says about its license."""

    def __init__(self, full_name, author, pages):
        self.page = ""
        self.verified = False
        self.licenses = {}  # license -> evidence, from the infobox or "Can I use your code?"
        self.mentions = {}  # license -> evidence, from anywhere else on the page
        self.says_closed = ""
        self.informal = ""
        self.source_link = ""
        text = self.find_page(full_name, pages)
        if text is not None:
            self.read(full_name, author, text)

    def find_page(self, full_name, pages):
        name = full_name.split(".")[-1]
        # MediaWiki capitalizes the first letter of a title.
        name = name[0].upper() + name[1:]
        for title in (name, f"{name} (robot)", f"{name} (bot)"):
            for _ in range(3):
                text = pages.get(title)
                if text is None:
                    break
                redirect = re.match(r"\s*#REDIRECT\s*\[\[([^\]|#]+)", text, I)
                if not redirect:
                    self.page = title
                    return text
                title = redirect.group(1).strip().replace("_", " ")
        return None

    def read(self, full_name, author, text):
        field = lambda key: re.search(rf"^\|\s*{key}\s*=[ \t]*(.*)$", text, re.M | I)
        # The page is about this robot if it names the package, or names the jar's author.
        wiki_author = field("author")
        wiki_author = strip_markup(wiki_author.group(1)).lower() if wiki_author else ""
        package = full_name.split(".")[0].lower()
        names_author = bool(author) and re.search(rf"\b{re.escape(author)}\b", text, I) is not None
        # The infobox's author, including its link, often contains the package: Sheldor2 for sheldor.
        raw_author = re.sub(r"[^a-z0-9.]", "", field("author").group(1).lower()) if field("author") else ""
        package_in_author = package == wiki_author.replace(" ", "") or (len(package) >= 3 and package in raw_author)
        self.verified = full_name.lower() in text.lower() or names_author or package_in_author

        link = field("source_link")
        self.source_link = strip_markup(link.group(1)) if link else ""

        box = field("license")
        if box:
            value = strip_markup(box.group(1))
            for name in short_licenses(box.group(1)):
                self.licenses.setdefault(name, f"infobox license: {value[:EVIDENCE_CHARS]}")
            if not self.licenses and value and SAYS_NO.search(value):
                self.says_closed = f"infobox license: {value[:EVIDENCE_CHARS]}"
            elif not self.licenses and value:
                self.informal = f"infobox license: {value[:EVIDENCE_CHARS]}"
        is_open = field("isOpenSource")
        if is_open and is_open.group(1).strip().lower() in ("no", "false") and not self.licenses:
            self.says_closed = self.says_closed or "infobox isOpenSource: no"

        answer = re.search(r"^[;=]+\s*(?:Can|May) I use (?:your|the|this) (?:code|source)\??\s*=*\s*\n"
                           r"((?:(?!^[;=]).*\n?){1,12})", text, re.M | I)
        if answer:
            body = strip_markup(answer.group(1))
            line = re.sub(r"\s+", " ", body.lstrip(": ")).strip()[:EVIDENCE_CHARS]
            for name in short_licenses(answer.group(1)):
                self.licenses.setdefault(name, f"can I use your code: {line}")
            # The whole answer counts, because "Sure, but..." can end in a ban on competitions.
            for name, evidence in find_licenses(body).items():
                self.licenses.setdefault(name, f"can I use your code: {evidence}")
            if not self.licenses and line:
                if SAYS_NO.search(line):
                    self.says_closed = self.says_closed or f"can I use your code: {line}"
                elif re.search(r"\b(yes|sure|free|go ahead|of course|feel free|ok)\b", line, I):
                    self.informal = self.informal or f"can I use your code: {line}"

        for name, evidence in find_licenses(strip_markup(text)).items():
            if name not in self.licenses:
                self.mentions[name] = evidence


def license_groups(licenses):
    return {GROUP[name] for name in licenses}


def names_of(licenses, group):
    return [name for name in licenses if GROUP[name] == group]


def decide_verdict(jar, wiki):
    """Returns (verdict, license, reason) from the rules in docs/rules.md."""
    if jar.source in ("no-jar", "unreadable"):
        return "no-jar", "", jar.source
    if jar.source == "none":
        return "no-source", "", "no source files in the jar"
    if jar.source == "partial":
        return "partial-source", "", f"no source for {len(jar.missing)} of {jar.classes} classes"

    verdict, license_name, reason = judge_license(jar, wiki)
    if verdict == "eligible" and wiki.verified:
        mentioned = names_of(wiki.mentions, DISALLOWED)
        if mentioned:
            return "review-conflict", license_name, "the wiki page also mentions " + "+".join(mentioned)
    if jar.source == "own" and verdict == "eligible":
        library = " ".join(jar.missing[:3])
        return "review-library-source", license_name, f"{reason}; no source for the library class {library}"
    return verdict, license_name, reason


def judge_license(jar, wiki):
    """The verdict for a robot whose own source is all in its jar."""
    trusted_wiki = wiki.licenses if wiki.verified else {}
    everything = {**jar.embedded, **jar.weak, **jar.strong}

    # A license that is not allowed, anywhere in the jar, rules the robot out: its conditions
    # apply to any robot using that code.
    banned = names_of(everything, DISALLOWED)
    if banned:
        return "disallowed", "+".join(banned), "stated in the jar"
    wiki_banned = names_of(trusted_wiki, DISALLOWED)
    jar_allowed = names_of(jar.strong, ALLOWED)
    wiki_allowed = names_of(trusted_wiki, ALLOWED)
    jar_decide = names_of(jar.strong, DECIDE)
    wiki_decide = names_of(trusted_wiki, DECIDE)

    if wiki_banned:
        if jar_allowed:
            return "review-conflict", "+".join(wiki_banned + jar_allowed), "the wiki and the jar disagree"
        return "disallowed", "+".join(wiki_banned), "stated on the wiki"

    other_code = names_of(jar.embedded, DECIDE) + names_of(jar.weak, DECIDE)
    if wiki_allowed:
        if jar_decide:
            return "review-conflict", "+".join(wiki_allowed + jar_decide), "the wiki and the jar disagree"
        if jar.reserved and not jar_allowed:
            return "review-conflict", "+".join(wiki_allowed), "the jar holds rights back: " + jar.reserved
        if other_code:
            return "review-embedded", "+".join(wiki_allowed), "contains code under " + "+".join(other_code)
        return "eligible", "+".join(wiki_allowed), "wiki"
    if jar_allowed:
        if wiki.verified and wiki.says_closed:
            return "review-conflict", "+".join(jar_allowed), "the wiki says it cannot be used"
        if jar_decide:
            return "decide", "+".join(jar_decide), "the jar also states " + "+".join(jar_allowed)
        if wiki_decide:
            return "review-conflict", "+".join(jar_allowed + wiki_decide), "the wiki and the jar disagree"
        if other_code:
            return "review-embedded", "+".join(jar_allowed), "contains code under " + "+".join(other_code)
        return "eligible", "+".join(jar_allowed), "jar"
    if jar_decide or wiki_decide:
        return "decide", "+".join(dict.fromkeys(jar_decide + wiki_decide)), "license with no ruling yet"
    if wiki.licenses and not wiki.verified:
        return "review-wiki-page", "+".join(wiki.licenses), "the wiki page may be about another robot"
    if wiki.verified and wiki.informal:
        if other_code:
            return "review-embedded", "", "permission in plain words, and contains code under " + "+".join(other_code)
        return "review-informal", "", "permission in plain words, no named license"
    weak_allowed = names_of(jar.weak, ALLOWED)
    if weak_allowed:
        return "review-weak", "+".join(weak_allowed), "only stated in some of its files"
    if wiki.verified and names_of(wiki.mentions, ALLOWED):
        return "review-weak", "+".join(names_of(wiki.mentions, ALLOWED)), "only mentioned in the text of its wiki page"
    if wiki.verified and wiki.says_closed:
        return "no-license", "", "the wiki says it cannot be used"
    if jar.informal:
        if other_code:
            return "review-embedded", "", "permission in plain words, and contains code under " + "+".join(other_code)
        return "review-informal", "", "permission in plain words, no named license"
    return "no-license", "", "no license in the jar or on the wiki"


def read_lists(paths):
    """Participants as {name: {version-qualified entry details}}, keeping which lists they are on."""
    robots = {}
    for path in paths:
        label = pathlib.Path(path).stem
        for line in pathlib.Path(path).read_text(errors="replace").splitlines():
            match = re.match(r"\s*([\w.]+) ([\w.\-]+)\s*,\s*(\S+)", line)
            if match:
                name = f"{match.group(1)} {match.group(2)}"
                robot = robots.setdefault(name, {"url": match.group(3), "lists": set()})
                robot["lists"].add(label)
    return robots


def load_overrides(path):
    """A reviewer's rulings: a TSV of robot name, verdict, license and reason."""
    overrides = {}
    if path and pathlib.Path(path).exists():
        for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines():
            if line.strip() and not line.startswith("#"):
                name, verdict, license_name, reason = line.split("\t")
                overrides[name] = (verdict, license_name, reason)
    return overrides


def find_jar(name, directories):
    file_name = name.replace(" ", "_") + ".jar"
    for directory in directories:
        path = pathlib.Path(directory) / file_name
        if path.exists():
            return path
    return None


def write_participants(path, rows):
    """A participant list with the evidence for each robot in a comment above it."""
    lines = ["# Written by tools/license_census.py. See docs/rules.md and participants/review/."]
    for row in rows:
        lines.append(f"# license={row['license']} source=jar from={row['reason'].split(';')[0]}")
        lines.append(f"{row['robot']},{row['url']}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--list", action="append", required=True)
    parser.add_argument("--jars", action="append", required=True)
    parser.add_argument("--wiki")
    parser.add_argument("--overrides")
    parser.add_argument("--write-list", action="append", default=[], metavar="LABEL=PATH",
                        help="write the eligible robots of a list as a participant list")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    robots = read_lists(args.list)
    labels = sorted({label for robot in robots.values() for label in robot["lists"]})
    pages = json.loads(pathlib.Path(args.wiki).read_text(encoding="utf-8"))["pages"] if args.wiki else {}
    overrides = load_overrides(args.overrides)

    rows = []
    for name in sorted(robots, key=str.lower):
        robot = robots[name]
        jar = Jar(find_jar(name, args.jars))
        wiki = Wiki(name.split(" ")[0], jar.author, pages)
        verdict, license_name, reason = decide_verdict(jar, wiki)
        if name in overrides:
            verdict, license_name, reason = overrides[name]
            reason = "reviewer: " + reason
        row = {"robot": name, "verdict": verdict, "license": license_name, "reason": reason}
        row.update({label: "yes" if label in robot["lists"] else "" for label in labels})
        row.update({
            "author": jar.author,
            "source": jar.source,
            "classes": jar.classes,
            "no_source_for": " ".join(jar.missing[:8]),
            "jar_license": "; ".join(f"{k} [{v}]" for k, v in jar.strong.items()),
            "jar_weak": "; ".join(f"{k} [{v}]" for k, v in jar.weak.items()),
            "jar_embedded": "; ".join(f"{k} [{v}]" for k, v in jar.embedded.items()),
            "jar_informal": jar.informal,
            "jar_reserved": jar.reserved,
            "wiki_page": (wiki.page + ("" if wiki.verified else " (unverified)")) if wiki.page else "",
            "wiki_license": "; ".join(f"{k} [{v}]" for k, v in wiki.licenses.items()),
            "wiki_mentions": "; ".join(f"{k} [{v}]" for k, v in wiki.mentions.items()),
            "wiki_other": wiki.says_closed or wiki.informal,
            "wiki_source_link": wiki.source_link,
            "url": robot["url"],
        })
        rows.append(row)

    with open(args.out, "w", newline="", encoding="utf-8") as out:
        writer = csv.DictWriter(out, fieldnames=list(rows[0]), delimiter="\t", quoting=csv.QUOTE_MINIMAL)
        writer.writeheader()
        writer.writerows(rows)

    for request in args.write_list:
        label, path = request.split("=", 1)
        write_participants(pathlib.Path(path), [row for row in rows if row[label] and row["verdict"] == "eligible"])

    print(f"{len(rows)} robots")
    for label in labels:
        counts = collections.Counter(row["verdict"] for row in rows if row[label])
        print(f"\n{label}: {sum(counts.values())}")
        for verdict, count in counts.most_common():
            print(f"  {count:5} {verdict}")


if __name__ == "__main__":
    sys.exit(main())
