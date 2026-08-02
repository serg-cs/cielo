#!/usr/bin/env python3

import argparse
import csv
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ElementTree
import zipfile
from collections import defaultdict
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Optional


PROJECT_ROOT = Path(__file__).resolve().parents[1]
CATALOG_DIRECTORY = PROJECT_ROOT / "src" / "generation" / "location_names"
DEFAULT_OUTPUT = CATALOG_DIRECTORY / "municipalities.csv"
OVERRIDES_PATH = CATALOG_DIRECTORY / "overrides.csv"

INE_WORKBOOK_URL = "https://www.ine.es/daco/daco42/codmun/diccionario26.xlsx"
WIKIDATA_QUERY_URL = "https://query.wikidata.org/sparql"
USER_AGENT = "cielo-location-catalog/1.0 (https://github.com/serg-cs/cielo)"
RETRYABLE_HTTP_STATUSES = {429, 500, 502, 503, 504}
WIKIDATA_BATCH_SIZE = 500
WIKIDATA_REQUEST_INTERVAL_SECONDS = 0.5
XML_NAMESPACE = {
    "x": "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
}


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Regenerate the reviewed Spanish municipality-name catalog."
    )
    parser.add_argument(
        "--ine-workbook",
        type=Path,
        help="Use a local INE XLSX workbook instead of downloading it.",
    )
    parser.add_argument(
        "--wikidata-response",
        type=Path,
        help="Use a local Wikidata SPARQL JSON response instead of downloading it.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT,
        help="Catalog path to write.",
    )
    return parser.parse_args()


def retry_delay(error: urllib.error.HTTPError, attempt: int) -> float:
    retry_after = error.headers.get("Retry-After")
    if retry_after is not None:
        try:
            return max(0.0, float(retry_after))
        except ValueError:
            try:
                retry_at = parsedate_to_datetime(retry_after)
                if retry_at.tzinfo is None:
                    retry_at = retry_at.replace(tzinfo=timezone.utc)
                return max(
                    0.0,
                    (retry_at - datetime.now(timezone.utc)).total_seconds(),
                )
            except (TypeError, ValueError):
                pass

    return float(2**attempt)


def fetch(url: str, data: Optional[bytes] = None) -> bytes:
    headers = {
        "Accept": "application/sparql-results+json",
        "User-Agent": USER_AGENT,
    }
    request = urllib.request.Request(url, data=data, headers=headers)

    for attempt in range(4):
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                return response.read()
        except urllib.error.HTTPError as error:
            details = " ".join(
                error.read(500).decode("utf-8", errors="replace").split()
            )
            message = f"HTTP {error.code}"
            if details:
                message = f"{message}: {details}"
            if (
                attempt == 3
                or error.code not in RETRYABLE_HTTP_STATUSES
            ):
                raise RuntimeError(f"failed to download {url}: {message}") from error
            time.sleep(retry_delay(error, attempt))
        except OSError as error:
            if attempt == 3:
                raise RuntimeError(f"failed to download {url}: {error}") from error
            time.sleep(2**attempt)

    raise RuntimeError(f"failed to download {url}")


def download(
    url: str,
    destination: Path,
    data: Optional[bytes] = None,
) -> None:
    destination.write_bytes(fetch(url, data))


def read_cell(cell: ElementTree.Element, shared_strings: list[str]) -> str:
    value = cell.find("x:v", XML_NAMESPACE)
    if value is None or value.text is None:
        return ""
    if cell.get("t") == "s":
        return shared_strings[int(value.text)]
    return value.text


def read_ine_municipalities(workbook_path: Path) -> dict[str, str]:
    with zipfile.ZipFile(workbook_path) as workbook:
        shared_root = ElementTree.fromstring(workbook.read("xl/sharedStrings.xml"))
        shared_strings = [
            "".join(
                text.text or ""
                for text in item.findall(".//x:t", XML_NAMESPACE)
            )
            for item in shared_root.findall("x:si", XML_NAMESPACE)
        ]
        sheet_root = ElementTree.fromstring(
            workbook.read("xl/worksheets/sheet1.xml")
        )

    municipalities = {}
    rows = sheet_root.findall(".//x:sheetData/x:row", XML_NAMESPACE)
    for row in rows[2:]:
        cells = {}
        for cell in row.findall("x:c", XML_NAMESPACE):
            reference = cell.get("r", "")
            match = re.match(r"[A-Z]+", reference)
            if match is not None:
                cells[match.group()] = read_cell(cell, shared_strings)

        municipality_id = cells["B"].zfill(2) + cells["C"].zfill(3)
        official_name = cells["E"]
        if municipality_id in municipalities:
            raise ValueError(f"duplicate INE municipality code: {municipality_id}")
        municipalities[municipality_id] = official_name

    return municipalities


def collect_wikidata_labels(
    response: dict,
    labels: dict[str, set[str]],
) -> None:
    for binding in response["results"]["bindings"]:
        municipality_id = binding["code"]["value"]
        label = binding.get("label", {}).get("value", "")
        if label:
            labels[municipality_id].add(label)


def read_wikidata_labels(response_path: Path) -> dict[str, set[str]]:
    response = json.loads(response_path.read_text(encoding="utf-8"))
    labels = defaultdict(set)
    collect_wikidata_labels(response, labels)

    return labels


def wikidata_query(municipality_ids: list[str]) -> str:
    values = " ".join(json.dumps(code) for code in municipality_ids)

    return f"""
SELECT ?code ?label WHERE {{
  VALUES ?code {{ {values} }}
  ?item wdt:P772 ?code.
  OPTIONAL {{
    ?item rdfs:label ?label.
    FILTER(LANG(?label) = "es")
  }}
}}
""".strip()


def download_wikidata_labels(
    municipality_ids: list[str],
) -> dict[str, set[str]]:
    labels = defaultdict(set)
    batch_count = (
        len(municipality_ids) + WIKIDATA_BATCH_SIZE - 1
    ) // WIKIDATA_BATCH_SIZE
    previous_request_started = None

    # Keep every query well below WDQS's hard deadline and respect its request rate.
    for batch_index in range(batch_count):
        batch_start = batch_index * WIKIDATA_BATCH_SIZE
        batch = municipality_ids[
            batch_start:batch_start + WIKIDATA_BATCH_SIZE
        ]
        if previous_request_started is not None:
            elapsed = time.monotonic() - previous_request_started
            time.sleep(max(0.0, WIKIDATA_REQUEST_INTERVAL_SECONDS - elapsed))

        print(
            f"fetching Wikidata labels: batch {batch_index + 1}/{batch_count}",
            file=sys.stderr,
        )
        query = urllib.parse.urlencode(
            {"query": wikidata_query(batch)}
        ).encode()
        previous_request_started = time.monotonic()
        response = json.loads(fetch(WIKIDATA_QUERY_URL, query))
        collect_wikidata_labels(response, labels)

    return labels


def read_overrides() -> dict[str, str]:
    overrides = {}

    with OVERRIDES_PATH.open(encoding="utf-8", newline="") as source:
        rows = csv.reader(source)
        if next(rows, None) != ["code", "name"]:
            raise ValueError("invalid override CSV header")

        for line_number, row in enumerate(rows, start=2):
            if len(row) != 2:
                raise ValueError(
                    f"invalid override on line {line_number}: expected 2 columns"
                )
            municipality_id, name = row
            if municipality_id in overrides:
                raise ValueError(f"duplicate override code: {municipality_id}")
            overrides[municipality_id] = name

    return overrides


def validate_display_name(municipality_id: str, name: str) -> None:
    if not name or name != name.strip():
        raise ValueError(f"invalid Spanish name for {municipality_id}")
    if "/" in name or any(
        character.isspace() and character != " " for character in name
    ):
        raise ValueError(f"non-display Spanish name for {municipality_id}: {name}")


def resolve_names(
    municipalities: dict[str, str],
    labels: dict[str, set[str]],
    overrides: dict[str, str],
) -> dict[str, str]:
    names = {}
    errors = []

    for municipality_id in sorted(municipalities):
        candidates = labels.get(municipality_id, set())
        if municipality_id in overrides:
            name = overrides[municipality_id]
        elif len(candidates) == 1:
            name = next(iter(candidates))
        elif not candidates:
            errors.append(f"{municipality_id}: missing Spanish label")
            continue
        else:
            values = ", ".join(sorted(candidates))
            errors.append(
                f"{municipality_id}: ambiguous Spanish labels: {values}"
            )
            continue

        validate_display_name(municipality_id, name)
        names[municipality_id] = name

    unused_overrides = sorted(set(overrides) - set(municipalities))
    errors.extend(
        f"{code}: override is not an active municipality"
        for code in unused_overrides
    )
    if errors:
        raise ValueError("\n".join(errors))

    return names


def write_catalog(names: dict[str, str], output_path: Path) -> None:
    with output_path.open("w", encoding="utf-8", newline="") as destination:
        writer = csv.writer(destination, lineterminator="\n")
        writer.writerow(["code", "name"])
        writer.writerows(names.items())


def main() -> int:
    arguments = parse_arguments()

    # Keep downloaded source artifacts outside the repository.
    with TemporaryDirectory(prefix="cielo-location-names-", dir="/tmp") as temporary:
        temporary_directory = Path(temporary)
        ine_workbook = arguments.ine_workbook
        if ine_workbook is None:
            ine_workbook = temporary_directory / "ine-municipalities.xlsx"
            download(INE_WORKBOOK_URL, ine_workbook)

        # Restrict Wikidata lookups to the authoritative active INE code set.
        municipalities = read_ine_municipalities(ine_workbook)
        wikidata_response = arguments.wikidata_response
        if wikidata_response is None:
            labels = download_wikidata_labels(
                sorted(municipalities)
            )
        else:
            labels = read_wikidata_labels(wikidata_response)

        # Join the authoritative INE code set to reviewed Spanish labels.
        overrides = read_overrides()
        names = resolve_names(municipalities, labels, overrides)
        write_catalog(names, arguments.output)

    print(f"wrote {len(names)} municipality names to {arguments.output}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (
        KeyError,
        OSError,
        ValueError,
        RuntimeError,
        zipfile.BadZipFile,
    ) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(1)
