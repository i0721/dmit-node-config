#!/usr/bin/env python3
"""Validate repository Markdown targets and GitHub-style heading anchors."""
import pathlib, re, subprocess, sys, unicodedata
from urllib.parse import unquote, urlsplit

root=pathlib.Path(__file__).resolve().parents[1]
files=set(subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard'],cwd=root,text=True).splitlines())
errors=[]
def anchors(path):
    result=set(); counts={}; fence=False
    for line in path.read_text(encoding='utf-8-sig').splitlines():
        if line.lstrip().startswith(('```','~~~')): fence=not fence
        if fence: continue
        match=re.match(r'^#{1,6}\s+(.+?)\s*#*$',line)
        if not match: continue
        heading=re.sub(r'<[^>]*>','',match[1]).lower()
        heading=''.join(c for c in heading if c in '-_ ' or not unicodedata.category(c).startswith(('P','S','C'))).replace(' ','-')
        suffix=counts.get(heading,0); counts[heading]=suffix+1
        result.add(heading+('-'+str(suffix) if suffix else ''))
    return result
for name in sorted(files):
    path=root/name
    if path.suffix!='.md' or not path.is_file(): continue
    text=path.read_text(encoding='utf-8-sig')
    text=re.sub(r'```.*?```','',text,flags=re.S)
    for match in re.finditer(r'!?\[[^\]]*\]\((<?[^)\n]+>?)\)',text):
        link=match[1].strip().strip('<>')
        parsed=urlsplit(link)
        if parsed.scheme or parsed.netloc: continue
        target=(path.parent/unquote(parsed.path)).resolve() if parsed.path else path
        if not target.is_relative_to(root) or not target.exists(): errors.append(f'{name}: missing target {link}')
        elif parsed.fragment and target.suffix=='.md' and unquote(parsed.fragment) not in anchors(target): errors.append(f'{name}: missing anchor {link}')
    if name not in ['scripts/check-docs.py'] and re.search(r'yushulinfeng\.com|rin721qwq@|179\.255\.137\.198|54b3415431cdb76e0bd76f130f733a27',text): errors.append(f'{name}: real deployment identifier')
for error in errors: print(error,file=sys.stderr)
print(f'Checked Markdown files and anchors: {len(errors)} errors')
sys.exit(bool(errors))
