#!/usr/bin/env python3
"""Relevance harness: relevance is measured, not asserted.

Reads a TSV of `query<TAB>expected-identity` pairs drawn from the real
archive (private, not committed), runs each against a live mailgrep, and
reports hit-rate@k and mean reciprocal rank. Never fails a build; it tells
you whether a boost change made things better or worse.

Usage: scripts/relevance.py queries.tsv [--base http://localhost:8025] [--account 1] [--k 10]
"""
import argparse
import json
import sys
import urllib.parse
import urllib.request


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("tsv")
    ap.add_argument("--base", default="http://localhost:8025")
    ap.add_argument("--account", default="1")
    ap.add_argument("--k", type=int, default=10)
    args = ap.parse_args()

    rows = []
    with open(args.tsv) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            query, expected = line.split("\t", 1)
            rows.append((query, expected.strip()))

    hits = 0
    rr_sum = 0.0
    for query, expected in rows:
        url = f"{args.base}/api/search?account={args.account}&limit={args.k}&q=" + urllib.parse.quote(query)
        with urllib.request.urlopen(url) as resp:
            data = json.load(resp)
        rank = None
        for i, r in enumerate(data["results"], 1):
            detail = json.load(urllib.request.urlopen(f"{args.base}/api/message/{r['id']}"))
            if detail["identity"] == expected or detail.get("msgid") == expected:
                rank = i
                break
        if rank:
            hits += 1
            rr_sum += 1.0 / rank
            print(f"  hit@{rank:<3} {query}")
        else:
            print(f"  MISS   {query}")

    n = len(rows) or 1
    print(f"\nqueries: {len(rows)}  hit-rate@{args.k}: {hits / n:.2%}  MRR: {rr_sum / n:.3f}")


if __name__ == "__main__":
    sys.exit(main())
