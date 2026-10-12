#!/usr/bin/env python3
"""Build small Procyon SKOS excerpts from pinned, attributed official RDF releases."""

import gzip
import hashlib
import io
import json
from pathlib import Path
import sys
import urllib.request
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "frontend/src/features/settings/skos-starters"
UNESCO_ENRICHMENTS = ROOT / "scripts/unesco-enriched-definitions.json"
RDF = "{http://www.w3.org/1999/02/22-rdf-syntax-ns#}"
SKOS = "{http://www.w3.org/2004/02/skos/core#}"
LANG = "{http://www.w3.org/XML/1998/namespace}lang"
BASE = "{http://www.w3.org/XML/1998/namespace}base"
SKOS_XL = "{http://www.w3.org/2008/05/skos-xl#}"
CC_BY = "https://creativecommons.org/licenses/by/4.0/"
CC_BY_SA_IGO = "https://creativecommons.org/licenses/by-sa/3.0/igo/"
ENRICHMENT_KEY = "https://procyon.app/ns/skos-starter/enrichment"
UNESCO_ENRICHMENT_NOTICE = (
    "Adapted UNESCO Thesaurus excerpt with English definitions AI-enriched by Procyon "
    "for RAG where the source has none. Added definitions are not authored by UNESCO; "
    "no UNESCO endorsement is implied."
)
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
    "agrovoc": (
        "https://agrovoc.fao.org/latestAgrovoc/agrovoc_core.rdf.zip",
        "5212f7b1e173a61ad14d966ee9feea4a6f9a49aa5f906ca667cedb33703f1568",
        "AGROVOC 2026-10-07 snapshot",
        CC_BY,
    ),
}
AGROVOC_BASE = "http://aims.fao.org/aos/agrovoc/"
AGROVOC_SELECTION = (
    "c_33561", "c_92381", "c_98778f1a", "c_10967", "c_7d40c88b",
    "c_8a6031d8", "c_1374567058134", "c_33949", "c_33955", "c_37977",
    "c_331342", "c_36580", "c_fe89fcbc", "c_8315", "c_7165",
    "c_15911", "c_207", "c_1348040570280", "c_588e022e",
    "c_35332", "c_264f7edd", "c_bea5db85", "c_c9484b9b",
)
STARTERS = (
    ("gemet-climate-energy", "Climate and renewable energy", "gemet",
     ("concept/1471", "concept/15271"), 2),
    ("unesco-natural-sciences", "Natural sciences", "unesco", ("mt2.75",), 0),
    ("unesco-information-technology", "Information technology", "unesco",
     ("mt5.40", "mt5.45"), 0),
    ("unesco-psychology", "Psychology", "unesco", ("mt4.10",), 0),
    ("unesco-defence", "Defence and military engineering", "unesco",
     ("concept2877", "concept613", "concept652"), 2),
    ("agrovoc-sustainable-food", "Sustainable agriculture and food systems", "agrovoc",
     AGROVOC_SELECTION, 0),
)


class AgrovocXml:
    def __init__(self, source):
        self.source = source
        self.invalid_bytes = 0

    def read(self, size):
        chunk = self.source.read(size)
        # The pinned FAO RDF has one XML-invalid control byte in a Chinese definition.
        self.invalid_bytes += chunk.count(b"\x1b")
        return chunk.replace(b"\x1b", b"")


def load_agrovoc(data):
    selected = {AGROVOC_BASE + uri for uri in AGROVOC_SELECTION}
    links = {}
    labels = {}
    definitions = {}
    modified = None
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        if archive.namelist() != ["agrovoc_core.rdf"]:
            raise ValueError("Unexpected AGROVOC archive contents")
        with archive.open("agrovoc_core.rdf") as source:
            cleaned = AgrovocXml(source)
            events = ET.iterparse(cleaned, events=("start", "end"))
            _, root = next(events)
            for event, item in events:
                if event != "end" or item.tag != RDF + "Description":
                    continue
                uri = item.attrib.get(RDF + "about", "")
                if uri in selected:
                    fields = links.setdefault(uri, {})
                    for field in item:
                        name = field.tag[len(SKOS):] if field.tag.startswith(SKOS) else (
                            field.tag[len(SKOS_XL):] if field.tag.startswith(SKOS_XL) else "")
                        if name not in ("prefLabel", "altLabel", "definition",
                                        "scopeNote", "broader", "related"):
                            continue
                        value = field.attrib.get(RDF + "resource") or field.text
                        if value and (field.attrib.get(RDF + "resource") or
                                      field.attrib.get(LANG) == "en"):
                            fields.setdefault(name, []).append(value.strip())
                elif uri.startswith(AGROVOC_BASE + "xl_"):
                    for field in item:
                        if field.tag == SKOS_XL + "literalForm" and field.attrib.get(LANG) == "en":
                            labels[uri] = field.text.strip() if field.text else ""
                elif uri.startswith(AGROVOC_BASE + "xDef_"):
                    for field in item:
                        if field.tag == RDF + "value" and field.attrib.get(LANG) == "en":
                            definitions[uri] = field.text.strip() if field.text else ""
                elif uri == AGROVOC_BASE.rstrip("/"):
                    for field in item:
                        if field.tag == "{http://purl.org/dc/terms/}modified":
                            modified = field.text
                root.remove(item)
            if cleaned.invalid_bytes != 1:
                raise ValueError("Unexpected AGROVOC XML control-byte count")
    if modified != "2026-10-07T10:12:36Z" or set(links) != selected:
        raise ValueError("AGROVOC snapshot or selected concepts changed")

    subjects = {}
    for uri, values in links.items():
        fields = {}
        for name, targets in values.items():
            elements = []
            for target in targets:
                if name in ("prefLabel", "altLabel"):
                    text = labels.get(target)
                elif name == "definition":
                    text = definitions.get(target)
                elif name == "scopeNote":
                    text = target
                else:
                    text = None
                if name in ("prefLabel", "altLabel", "definition", "scopeNote"):
                    if not text:
                        continue
                    element = ET.Element(SKOS + name, {LANG: "en"})
                    element.text = text
                else:
                    element = ET.Element(SKOS + name, {RDF + "resource": target})
                elements.append(element)
            fields[name] = elements
        subjects[uri] = fields
    for uri, fields in subjects.items():
        for parent in fields.get("broader", ()):
            ancestor = subjects.get(parent.attrib[RDF + "resource"])
            if ancestor is not None:
                ancestor.setdefault("narrower", []).append(
                    ET.Element(SKOS + "narrower", {RDF + "resource": uri}))
    return AGROVOC_BASE, subjects


def load_source(key):
    url, digest, _, _ = SOURCES[key]
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read()
    actual = hashlib.sha256(data).hexdigest()
    if actual != digest:
        raise ValueError(f"{url}: SHA-256 mismatch (expected {digest}, got {actual})")
    if key == "agrovoc":
        return load_agrovoc(data)
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


def make_document(starter, source, enrichments, used_enrichments):
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
        definitions = labels("definition")
        if key == "unesco" and not definitions:
            definition = enrichments.get(uri)
            if not isinstance(definition, str) or not definition.strip():
                raise ValueError(f"{key} {uri}: missing reviewed English definition")
            definitions = [definition]
            used_enrichments.add(uri)

        def links(field):
            return sorted(resolve(item.attrib[RDF + "resource"])
                          for item in fields.get(field, ())
                          if resolve(item.attrib[RDF + "resource"]) in selected)

        concepts.append({
            "uri": uri,
            "prefLabels": {"en": preferred[0]},
            "altLabels": {"en": sorted(set(labels("altLabel")))} if labels("altLabel") else {},
            "definitions": {"en": definitions[0]} if definitions else {},
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
    extensions = {
        "https://procyon.app/ns/skos-starter/source": SOURCES[key][0],
        "https://procyon.app/ns/skos-starter/license": SOURCES[key][3],
        "https://procyon.app/ns/skos-starter/selection": list(roots),
    }
    if key == "unesco":
        extensions[ENRICHMENT_KEY] = UNESCO_ENRICHMENT_NOTICE
    return {
        "format": "procyon-skos-1",
        "id": identifier,
        "name": f"{title} ({SOURCES[key][2]} excerpt)",
        "concepts": concepts,
        "extensions": extensions,
    }


def main():
    check = sys.argv[1:] == ["--check"]
    if sys.argv[1:] and not check:
        raise SystemExit("usage: generate-skos-starters.py [--check]")
    enrichments = json.loads(UNESCO_ENRICHMENTS.read_text())
    if not isinstance(enrichments, dict):
        raise ValueError("UNESCO enrichments must map concept URIs to English definitions")
    used_enrichments = set()
    sources = {key: load_source(key) for key in SOURCES}
    documents = [
        (starter, make_document(starter, sources[starter[2]], enrichments, used_enrichments))
        for starter in STARTERS
    ]
    if set(enrichments) != used_enrichments:
        raise ValueError(f"Unused UNESCO enrichments: {sorted(set(enrichments) - used_enrichments)}")
    for starter, document in documents:
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
