#!/bin/bash
# Runs cargo test and produces JUnit XML from the text output.
# Usage: ./scripts/test2junit.sh > results.xml
set -euo pipefail

TMPFILE=$(mktemp)
cargo test --workspace 2>&1 | tee "$TMPFILE" >&2

python3 -c "
import re, sys, xml.etree.ElementTree as ET
from xml.dom import minidom

lines = open(sys.argv[1]).readlines()
suites = {}
current_suite = 'unknown'
for line in lines:
    line = line.rstrip()
    m = re.match(r'^\s*Running unittests src/.*\((.+)\)', line)
    if not m:
        m = re.match(r'^(?:   )?Doc-tests (\S+)', line)
    if m:
        current_suite = m.group(1)
        if current_suite not in suites:
            suites[current_suite] = []
        continue
    m = re.match(r'^\s*test (\S+)\s+\.\.\.\s+(ok|FAILED|ignored)', line)
    if m:
        name, result = m.group(1), m.group(2)
        suites.setdefault(current_suite, []).append((name, result))

root = ET.Element('testsuites')
total = fail = 0
for suite_name, tests in suites.items():
    ts = ET.SubElement(root, 'testsuite', name=suite_name, tests=str(len(tests)))
    f = 0
    for name, result in tests:
        tc = ET.SubElement(ts, 'testcase', name=name, classname=suite_name)
        if result == 'FAILED':
            ET.SubElement(tc, 'failure', message=f'{name} failed')
            f += 1
        elif result == 'ignored':
            tc.set('status', 'skipped')
    ts.set('failures', str(f))
    total += len(tests)
    fail += f
root.set('tests', str(total))
root.set('failures', str(fail))
print(minidom.parseString(ET.tostring(root, encoding='unicode')).toprettyxml(indent='  '))
" "$TMPFILE"

rm -f "$TMPFILE"
