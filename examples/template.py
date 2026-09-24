#!/usr/bin/env python3
"""Skeleton pahiri task source (e.g. for Orbit or any tracker with an API).

    [[task_sources]]
    name = "orbit"
    command = "python3 ~/bin/orbit.py"

Print a JSON array of tickets on stdout; exit non-zero with a message on
stderr when something goes wrong (pahiri shows the last lines of stderr).
Only "id" is required. It becomes the task folder and git branch name.
"""
import json
import sys


def fetch():
    """Return a list of dicts: id, title, url, description.

    Replace this with a call to your tracker, e.g.
        import urllib.request
        req = urllib.request.Request(URL, headers={"Authorization": f"Bearer {TOKEN}"})
        data = json.load(urllib.request.urlopen(req, timeout=20))
    """
    return [
        {
            "id": "ORB-42",
            "title": "Example ticket",
            "url": "https://orbit.example.com/ORB-42",
            "description": "What needs doing, as plain text or Markdown.",
        }
    ]


def main():
    try:
        tickets = fetch()
    except Exception as exc:  # noqa: BLE001 - report anything to pahiri
        print(f"orbit: {exc}", file=sys.stderr)
        return 1
    json.dump(tickets, sys.stdout)
    return 0


if __name__ == "__main__":
    sys.exit(main())
