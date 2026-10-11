#!/usr/bin/env python3
"""Build small Procyon SKOS excerpts from pinned, attributed official RDF releases."""

import gzip
import hashlib
import json
from pathlib import Path
import sys
import urllib.request
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "frontend/src/features/settings/skos-starters"
RDF = "{http://www.w3.org/1999/02/22-rdf-syntax-ns#}"
SKOS = "{http://www.w3.org/2004/02/skos/core#}"
LANG = "{http://www.w3.org/XML/1998/namespace}lang"
BASE = "{http://www.w3.org/XML/1998/namespace}base"
CC_BY = "https://creativecommons.org/licenses/by/4.0/"
CC_BY_SA_IGO = "https://creativecommons.org/licenses/by-sa/3.0/igo/"
SOURCES = {
    "gemet": (
        "https://www.eionet.europa.eu/gemet/4.2.3/gemet.rdf.gz",
        "96002bb7cd1f89bccb05ee174fb834a04dd7342bdd1428f32105cd47fd6b73b6",
        "GEMET 4.2.3",
        CC_BY,
    ),
    "unesco": (
        "https://vocabularies.unesco.org/exports/thesaurus/latest/unesco-thesaurus.rdf",
        "977399deca61fd6e15a96990c916ddb860692d4558451df3e3bc8cbd18727f81",
        "UNESCO Thesaurus 2026-08-31 snapshot",
        CC_BY_SA_IGO,
    ),
}
STARTERS = (
    ("gemet-climate-energy", "Climate and renewable energy", "gemet",
     ("concept/1471", "concept/15271"), 2),
    ("unesco-natural-sciences", "Natural sciences", "unesco", ("mt2.75",), 0),
    ("unesco-information-technology", "Information technology", "unesco",
     ("mt5.40", "mt5.45"), 0),
    ("unesco-psychology", "Psychology", "unesco", ("mt4.10",), 0),
    ("unesco-defence", "Defence and military engineering", "unesco",
     ("concept2877", "concept613", "concept652"), 2),
)


def load_source(key):
    url, digest, _, _ = SOURCES[key]
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read()
    actual = hashlib.sha256(data).hexdigest()
    if actual != digest:
        raise ValueError(f"{url}: SHA-256 mismatch (expected {digest}, got {actual})")
    if key == "gemet":
        data = gzip.decompress(data)
    root = ET.fromstring(data)
    base = root.attrib.get(BASE, "http://vocabularies.unesco.org/thesaurus/")
    subjects = {}
    for item in root:
        uri = item.attrib.get(RDF + "about")
        if uri is not None:
            uri = uri if uri.startswith(("http://", "https://")) else base + uri
            fields = subjects.setdefault(uri, {})
            for field in item:
                if field.tag.startswith(SKOS):
                    fields.setdefault(field.tag[len(SKOS):], []).append(field)
    return base, subjects


def make_document(starter, source):
    identifier, title, key, roots, depth = starter
    base, subjects = source
    def resolve(uri):
        return uri if uri.startswith(("http://", "https://")) else base + uri

    selected = {resolve(member.attrib[RDF + "resource"])
                for uri in roots
                for member in subjects[resolve(uri)].get("member", ())}
    selected.update(resolve(uri) for uri in roots if not subjects[resolve(uri)].get("member"))
    frontier = set(selected)
    for _ in range(depth):
        frontier = {
            resolve(child.attrib[RDF + "resource"])
            for uri in frontier
            for child in subjects[uri].get("narrower", ())
        } - selected
        selected.update(frontier)

    concepts = []
    for uri in sorted(selected):
        fields = subjects[uri]

        def labels(field):
            return [item.text.strip() for item in fields.get(field, ())
                    if item.attrib.get(LANG) == "en" and item.text and item.text.strip()]

        preferred = labels("prefLabel")
        if len(preferred) != 1:
            raise ValueError(f"{key} {uri}: expected one English preferred label")

        def links(field):
            return sorted(resolve(item.attrib[RDF + "resource"])
                          for item in fields.get(field, ())
                          if resolve(item.attrib[RDF + "resource"]) in selected)

        concepts.append({
            "uri": uri,
            "prefLabels": {"en": preferred[0]},
            "altLabels": {"en": sorted(set(labels("altLabel")))} if labels("altLabel") else {},
            "definitions": {"en": labels("definition")[0]} if labels("definition") else {},
            "scopeNotes": {"en": labels("scopeNote")[0]} if labels("scopeNote") else {},
            "broader": links("broader"),
            "narrower": links("narrower"),
            "related": links("related"),
            "extensions": {},
        })

    by_uri = {concept["uri"]: concept for concept in concepts}
    for concept in concepts:
        for broader in concept["broader"]:
            if concept["uri"] not in by_uri[broader]["narrower"]:
                raise ValueError(f"{key}: asymmetric broader relation for {concept['uri']}")
        for narrower in concept["narrower"]:
            if concept["uri"] not in by_uri[narrower]["broader"]:
                raise ValueError(f"{key}: asymmetric narrower relation for {concept['uri']}")
    return {
        "format": "procyon-skos-1",
        "id": identifier,
        "name": f"{title} ({SOURCES[key][2]} excerpt)",
        "concepts": concepts,
        "extensions": {
            "https://procyon.app/ns/skos-starter/source": SOURCES[key][0],
            "https://procyon.app/ns/skos-starter/license": SOURCES[key][3],
            "https://procyon.app/ns/skos-starter/selection": list(roots),
        },
    }


def main():
    check = sys.argv[1:] == ["--check"]
    if sys.argv[1:] and not check:
        raise SystemExit("usage: generate-skos-starters.py [--check]")
    sources = {key: load_source(key) for key in SOURCES}
    for starter in STARTERS:
        document = make_document(starter, sources[starter[2]])
        target = OUTPUT / (starter[0] + ".json")
        content = json.dumps(document, indent=2, ensure_ascii=False) + "\n"
        if check:
            if not target.exists() or json.loads(target.read_text()) != document:
                raise ValueError(f"{target}: starter differs from pinned source")
        else:
            OUTPUT.mkdir(parents=True, exist_ok=True)
            target.write_text(content)
        print(f"{target.name}: {len(document['concepts'])} concepts")


if __name__ == "__main__":
    main()
