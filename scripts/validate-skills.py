#!/usr/bin/env python3
"""Validate all SKILL.md files in skills/ and optional-skills/ meet the format requirements.

Exit codes:
  0 - all skills valid
  1 - validation errors found

Called by CI to enforce the >=200 floor and format conformance (R32.4).
"""
import os
import sys
import re

SKILLS_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "skills")
OPTIONAL_SKILLS_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "optional-skills")
MIN_BUNDLED_SKILLS = 200

REQUIRED_FRONT_MATTER = {"name", "description", "category"}

def parse_skill(path):
    errors = []
    with open(path) as f:
        content = f.read()

    if not content.startswith("---"):
        errors.append(f"{path}: missing front-matter opening '---'")
        return errors

    end = content.find("\n---", 3)
    if end == -1:
        errors.append(f"{path}: missing front-matter closing '---'")
        return errors

    front = content[3:end].strip()
    body = content[end + 4:].strip()

    if not body:
        errors.append(f"{path}: missing body content after front-matter")

    fields = {}
    for line in front.split("\n"):
        line = line.strip()
        if not line:
            continue
        if ":" not in line:
            continue
        key, value = line.split(":", 1)
        fields[key.strip()] = value.strip()

    for required in REQUIRED_FRONT_MATTER:
        if required not in fields:
            errors.append(f"{path}: missing required field '{required}'")
        elif not fields[required]:
            errors.append(f"{path}: field '{required}' is empty")

    return errors

def scan_dir(base_dir):
    errors = []
    count = 0
    for root, dirs, files in os.walk(base_dir):
        for f in sorted(files):
            if f.endswith(".md"):
                path = os.path.join(root, f)
                errors.extend(parse_skill(path))
                count += 1
    return count, errors

def main():
    all_errors = []
    total_count = 0

    bundled_count, bundled_errors = scan_dir(SKILLS_DIR)
    all_errors.extend(bundled_errors)
    total_count += bundled_count

    optional_count, optional_errors = scan_dir(OPTIONAL_SKILLS_DIR)
    all_errors.extend(optional_errors)

    print(f"Scanned {bundled_count} bundled skills and {optional_count} optional skills")

    if bundled_count < MIN_BUNDLED_SKILLS:
        all_errors.append(
            f"Only {bundled_count} bundled skills found, minimum is {MIN_BUNDLED_SKILLS}"
        )

    if all_errors:
        for err in all_errors:
            print(f"  ERROR: {err}", file=sys.stderr)
        print(f"\n{len(all_errors)} error(s) found", file=sys.stderr)
        return 1

    print("All skills valid!")
    return 0

if __name__ == "__main__":
    sys.exit(main())
