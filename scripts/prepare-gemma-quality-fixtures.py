"""Build bounded, source-pinned retrieval fixtures without redistributing source assets."""

import csv
import gzip
import hashlib
import heapq
import json
import re
import sys
import urllib.request
from collections import defaultdict
from pathlib import Path

HASHES = {
    "swahili-corpus.jsonl.gz": "be45a839aa4445fb77ae8e43974a5310abc5665e1750bcedf7e7d5134e292fe1",
    "telugu-corpus.jsonl.gz": "6142ecfdfba4c77e60296bf3f62a48661db237057aa7c7facb4f92c213065726",
    "swahili-qrels.dev.txt": "37d52885b464c56b3724f250f11db4d9eb7e013f6f066e9a15b44af526612dfe",
    "telugu-qrels.dev.txt": "464fa7cc9c56ee2a200f332c2b70b5ab56f3058fc1339eec23604b3b5b14c534",
    "swahili-topics.dev.txt": "642ab84281195ef1257f211834c523b0025bd2eb16189bd4aed3eab814919f61",
    "telugu-topics.dev.txt": "6a994a6b043dd90d9504dbd5e8205102db90c1db8c4431cba58080a031c12d1f",
    "code-annotations.csv": "0340af32b551ceadb74fec147f97642b7fedf3ff039e38fb86baff49ee899846",
    "swap_obj.json": "073cdb8e253d053614e80710834d9773b09dbc1dd0a412f6f9492262caa1dcad",
}
PHOTOS = {
    "4": ("579635", "38980dbaeaf8b255b1d771ed80d66cc3ce0ad6540a10139000c791bd790f3862", "danramarch", "https://www.flickr.com/photos/danramarch/8933634910/", "CC BY 2.0"),
    "37": ("232348", "2c8120553032e12232a900a098b4d42b57c02818eba56b36485ef541bb750a72", "ex_magician", "https://www.flickr.com/photos/ex_magician/5926562644/", "CC BY 2.0"),
    "42": ("355610", "2ccae45b3bcbcc5f1fc797eacdf1151e50d86187e08c2fe3b9e01efd13529c58", "jepoirrier", "https://www.flickr.com/photos/jepoirrier/2090506037/", "CC BY-SA 2.0"),
    "43": ("179174", "db596b78008223a6d9d85f972d4ebd2afd74537eff9de803e93c4cf63de8de07", "33979492@N00", "https://www.flickr.com/photos/33979492@N00/7611201536/", "CC BY 2.0"),
    "44": ("7511", "105e70c573b87d7e3f034e1dc08cedcced39be4e52197139f52616c920dda3d7", "pajp", "https://www.flickr.com/photos/pajp/173980729/", "CC BY-SA 2.0"),
}
CODE_QUERIES = (
    "priority queue",
    "how to reverse a string",
    "buffered file reader read text",
    "parse binary file to custom class",
)
EXPANDED_CODE_QUERIES = (
    "aes encryption",
    "binomial distribution",
    "buffered file reader read text",
    "convert a date string into yyyymmdd",
    "convert json to csv",
    "convert string to number",
    "deducting the median from each column",
    "find int in string",
    "fuzzy match ranking",
    "get current ip address",
    "hash set for counting distinct elements",
    "how to reverse a string",
    "parse binary file to custom class",
    "priority queue",
)
# Checked against the GitHub license endpoint at each annotation's commit.
CODE_LICENSES = {
    "FreshXOpenSource/wallaby-base": "BSD-2-Clause",
    "NoneGG/aredis": "MIT",
    "RaRe-Technologies/smart_open": "MIT",
    "SmartTeleMax/iktomi": "MIT",
    "aragaer/channels": "MIT",
    "frnsys/broca": "MIT",
    "hyde/fswrap": "MIT",
    "iclab/centinel": "MIT",
    "it-geeks-club/pyspectator": "BSD-3-Clause",
    "jldantas/libmft": "BSD-3-Clause",
    "keon/algorithms": "MIT",
    "log2timeline/plaso": "Apache-2.0",
    "open-mmlab/mmcv": "Apache-2.0",
    "ray-project/ray": "Apache-2.0",
    "waqasbhatti/astrobase": "MIT",
    "williballenthin/python-evtx": "Apache-2.0",
}


def digest(path):
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def words(text):
    return set(re.findall(r"\w+", text.casefold()))


def multilingual(root, language):
    topics = {}
    for line in (root / f"{language}-topics.dev.txt").read_text().splitlines():
        key, text = line.split("\t", 1)
        topics[key] = text
    positives = defaultdict(set)
    for line in (root / f"{language}-qrels.dev.txt").read_text().splitlines():
        key, _, docid, grade = line.split()
        if int(grade) > 0:
            positives[key].add(docid)
    selected = list(topics)[:8]
    if any(not positives[key] for key in selected):
        raise ValueError("selected query lacks a positive judgement")
    lexical = {key: words(topics[key]) for key in selected}
    heaps = {key: [] for key in selected}
    positive_docs = {}
    with gzip.open(root / f"{language}-corpus.jsonl.gz", "rt") as stream:
        for line in stream:
            document = json.loads(line)
            docid = document["docid"]
            text = f'{document["title"]}\n{document["text"]}'
            if any(docid in positives[key] for key in selected):
                positive_docs[docid] = text
            tokens = words(text)
            for key in selected:
                if docid in positives[key]:
                    continue
                score = len(tokens & lexical[key])
                if score:
                    heapq.heappush(heaps[key], (score, docid, text))
                    if len(heaps[key]) > 3:
                        heapq.heappop(heaps[key])
    if any(docid not in positive_docs for key in selected for docid in positives[key]):
        raise ValueError("a judged-positive passage is missing from the pinned corpus")
    documents = dict(positive_docs)
    cases = []
    for key in selected:
        candidates = [docid for _, docid, _ in sorted(heaps[key], reverse=True)]
        for _, docid, text in heaps[key]:
            documents[docid] = text
        cases.append({
            "id": f"{language}-{key}",
            "query": topics[key],
            "relevantIds": sorted(positives[key]),
            "eligibleIds": sorted(positives[key]) + candidates,
        })
    return {
        "labelPolicy": "Mr. TyDi human positives; lexical distractors are unjudged, not verified negatives",
        "documents": [{"id": key, "text": value} for key, value in sorted(documents.items())],
        "cases": cases,
    }


def code(root, queries=CODE_QUERIES, licenses=CODE_LICENSES, per_grade_cap=None):
    judgements = defaultdict(list)
    with (root / "code-annotations.csv").open(newline="") as stream:
        for row in csv.DictReader(stream):
            if row["Language"] == "Python" and row["Query"] in queries:
                judgements[(row["Query"], row["GitHubUrl"])].append(int(row["Relevance"]))
    grouped = defaultdict(list)
    for (query, url), grades in judgements.items():
        if all(grade >= 2 for grade in grades):
            grouped[query].append((url, True))
        elif all(grade == 0 for grade in grades):
            grouped[query].append((url, False))
    files = {}
    documents = {}
    cases = []
    for query in queries:
        relevant, eligible = [], []
        entries = grouped[query]
        if per_grade_cap is not None:
            permitted = []
            for url, positive in entries:
                match = re.fullmatch(
                    r"https://github.com/([^/]+/[^/]+)/blob/([0-9a-f]{40})/(.+)#L(\d+)-L(\d+)",
                    url,
                )
                if match and f"{match[1]}@{match[2]}" in licenses:
                    permitted.append((url, positive))
            entries = (
                sorted((url, grade) for url, grade in permitted if grade)[:per_grade_cap]
                + sorted((url, grade) for url, grade in permitted if not grade)[:per_grade_cap]
            )
        for url, positive in entries:
            match = re.fullmatch(
                r"https://github.com/([^/]+/[^/]+)/blob/([0-9a-f]{40})/(.+)#L(\d+)-L(\d+)",
                url,
            )
            if not match:
                continue
            repo, commit, filename, first, last = match.groups()
            license_name = licenses.get(f"{repo}@{commit}" if per_grade_cap is not None else repo)
            if license_name is None:
                continue
            first, last = int(first), int(last)
            if first < 1 or last < first or last - first > 130:
                continue
            source = f"https://raw.githubusercontent.com/{repo}/{commit}/{filename}"
            if source not in files:
                with urllib.request.urlopen(source, timeout=30) as response:
                    content = response.read(2_000_001)
                if len(content) > 2_000_000:
                    raise ValueError(f"source exceeds the bounded download: {source}")
                files[source] = (hashlib.sha256(content).hexdigest(), content.decode("utf-8").splitlines())
            if last > len(files[source][1]):
                raise ValueError(f"annotation line outside source: {url}")
            identifier = hashlib.sha256(url.encode()).hexdigest()[:16]
            documents[identifier] = {
                "id": identifier,
                "text": "\n".join(files[source][1][first - 1:last]),
                "source": url,
                "license": license_name,
            }
            eligible.append(identifier)
            if positive:
                relevant.append(identifier)
        if not relevant or len(eligible) - len(relevant) < 2:
            raise ValueError(f"{query} lacks unanimously judged positives and hard negatives")
        cases.append({
            "id": query,
            "query": query,
            "relevantIds": relevant,
            "eligibleIds": eligible,
        })
    return {
        "labelPolicy": "CodeSearchNet unanimous grades >=2 relevant, all-zero irrelevant; grades 1/conflicts and cross-query snippets excluded",
        "documents": list(documents.values()),
        "cases": cases,
        "sourceFileSha256": {source: checksum for source, (checksum, _) in files.items()},
    }


def main():
    if len(sys.argv) == 3 and sys.argv[2] == "--expanded-code":
        root = Path(sys.argv[1])
        if digest(root / "code-annotations.csv") != HASHES["code-annotations.csv"]:
            raise ValueError("pinned code annotations differ")
        manifest = Path(__file__).parent / "fixtures" / "embedding-code-licenses-v2.json"
        licenses = json.loads(manifest.read_text())
        fixture = root / "code-expanded-fixture.json"
        fixture.write_text(json.dumps(
            code(root, EXPANDED_CODE_QUERIES, licenses, per_grade_cap=2), indent=2
        ) + "\n")
        print(fixture.name, digest(fixture))
        return
    if len(sys.argv) not in (1, 2):
        raise ValueError("usage: prepare-gemma-quality-fixtures.py [DATA_DIR [--expanded-code]]")
    root = Path(sys.argv[1]) if len(sys.argv) == 2 else Path("target/gemma-quality-data")
    for name, expected in HASHES.items():
        if digest(root / name) != expected:
            raise ValueError(f"pinned benchmark source differs: {name}")
    for language in ("swahili", "telugu"):
        (root / f"{language}-fixture.json").write_text(
            json.dumps(multilingual(root, language), ensure_ascii=False, indent=2) + "\n"
        )
    (root / "code-fixture.json").write_text(json.dumps(code(root), indent=2) + "\n")
    pairs = json.loads((root / "swap_obj.json").read_text())
    photos = []
    for key, (identifier, expected, author, page, license_name) in PHOTOS.items():
        image = root / "photos" / f"{identifier}.jpg"
        if digest(image) != expected or pairs[key]["filename"] != f"{int(identifier):012d}.jpg":
            raise ValueError(f"pinned photograph differs: {identifier}")
        photos.append({
            "id": identifier, "path": str(image.resolve()), "sha256": expected,
            "positiveCaption": pairs[key]["caption"],
            "negativeCaption": pairs[key]["negative_caption"],
            "flickrAuthor": author, "flickrPage": page, "license": license_name,
        })
    (root / "photos-fixture.json").write_text(json.dumps(photos, indent=2) + "\n")
    for name in ("swahili-fixture.json", "telugu-fixture.json", "code-fixture.json", "photos-fixture.json"):
        print(name, digest(root / name))


if __name__ == "__main__":
    main()
