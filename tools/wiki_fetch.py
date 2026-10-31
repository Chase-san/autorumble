#!/usr/bin/env python3
"""Fetches RoboWiki pages politely through Special:Export, many pages per request.

Pages are cached in a JSON file, so a stopped run can resume. One request every SPACING seconds, and
a long wait after any refusal, because this host's protection has blocked this address before.

usage: wiki_fetch.py <cache.json> <titles file, one per line>
"""
import json
import pathlib
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET

CACHE, TITLES = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
BATCH = 100
SPACING = 120
REFUSED_WAIT = 600
MAX_REFUSALS = 6
AGENT = "Mozilla/5.0 (RoboRumble license survey; Chase-san)"
NS = "{http://www.mediawiki.org/xml/export-0.11/}"


def load():
    return json.loads(CACHE.read_text(encoding="utf-8")) if CACHE.exists() else {"pages": {}, "asked": []}


def save(cache):
    temporary = CACHE.with_suffix(".tmp")
    temporary.write_text(json.dumps(cache, ensure_ascii=False), encoding="utf-8")
    temporary.replace(CACHE)


def export(titles):
    form = urllib.parse.urlencode({"pages": "\n".join(titles), "curonly": "1", "action": "submit"}).encode()
    request = urllib.request.Request("https://robowiki.net/wiki/Special:Export", data=form, headers={"User-Agent": AGENT})
    with urllib.request.urlopen(request, timeout=60) as response:
        root = ET.fromstring(response.read())
    pages = {}
    for page in root.findall(NS + "page"):
        title = page.findtext(NS + "title")
        text = page.find(f"{NS}revision/{NS}text")
        pages[title] = text.text if text is not None and text.text else ""
    return pages


def main():
    cache = load()
    asked = set(cache["asked"])
    titles = [t.strip() for t in TITLES.read_text(encoding="utf-8").splitlines() if t.strip()]
    wanted = [t for t in dict.fromkeys(titles) if t not in asked]
    print(f"{len(titles)} titles, {len(wanted)} still to ask for", flush=True)
    refusals = 0
    position = 0
    while position < len(wanted):
        batch = wanted[position:position + BATCH]
        try:
            pages = export(batch)
        except (urllib.error.URLError, TimeoutError, ET.ParseError) as error:
            refusals += 1
            print(f"refused or failed ({error}); {refusals} of {MAX_REFUSALS}, waiting {REFUSED_WAIT}s", flush=True)
            if refusals >= MAX_REFUSALS:
                sys.exit("giving up for now; run again later to resume")
            time.sleep(REFUSED_WAIT)
            continue
        refusals = 0
        cache["pages"].update(pages)
        cache["asked"].extend(batch)
        save(cache)
        position += len(batch)
        print(f"{position}/{len(wanted)} asked, {len(cache['pages'])} pages found", flush=True)
        if position < len(wanted):
            time.sleep(SPACING)
    print("done", flush=True)


main()
