#!/usr/bin/env python3
"""Collect upstream license notices from Cargo's resolved package sources."""
import json,pathlib,shutil,sys
work=pathlib.Path(sys.argv[1])
metadata=json.loads((work/'cargo-metadata.json').read_text())
destination=work/'licenses'; destination.mkdir()
inventory=[]
nodes={node['id']:node for node in metadata['resolve']['nodes']}
reachable=set()
def visit(package_id):
    if package_id in reachable: return
    reachable.add(package_id)
    for dependency in nodes[package_id]['deps']:
        if any(kind['kind']!='dev' for kind in dependency['dep_kinds']): visit(dependency['pkg'])
visit(metadata['resolve']['root'])
for package in metadata['packages']:
    if package['id'] not in reachable: continue
    source=pathlib.Path(package['manifest_path']).parent
    name=package['name']+'-'+package['version']
    notices=[]
    for path in source.iterdir():
        if path.is_file() and path.name.lower().startswith(('license','licence','copying','notice','authors')):
            directory=destination/name; directory.mkdir(exist_ok=True)
            shutil.copy2(path,directory/path.name); notices.append(path.name)
    if not notices: raise SystemExit('Missing upstream license notices for '+name)
    inventory.append({'name':package['name'],'version':package['version'],'license':package.get('license'),'repository':package.get('repository'),'notices':notices})
(destination/'inventory.json').write_text(json.dumps(inventory,indent=2)+'\n')
(work/'cargo-metadata.json').unlink()
