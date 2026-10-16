#!/usr/bin/env python3
"""Require immutable Actions references; syntax is additionally checked by actionlint."""
import pathlib,re,sys
errors=[]
for path in pathlib.Path('.github/workflows').glob('*.yml'):
    text=path.read_text()
    for match in re.finditer(r'uses:\s*([^\s#]+)',text):
        ref=match[1]
        if not ref.startswith('./') and not re.fullmatch(r'[^@]+@[0-9a-f]{40}',ref): errors.append(f'{path}: mutable action {ref}')
for error in errors: print(error,file=sys.stderr)
sys.exit(bool(errors))
