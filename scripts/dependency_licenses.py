# SPDX-License-Identifier: MPL-2.0
"""Collect dependency license/notice texts into a release staging directory."""
import json
from pathlib import Path
import shutil
import subprocess
import sys

output=Path(sys.argv[1]);output.mkdir(parents=True,exist_ok=True)
target=sys.argv[2] if len(sys.argv)>2 else next(line.split(': ',1)[1] for line in subprocess.check_output(['rustc','-vV'],text=True).splitlines() if line.startswith('host: '))
metadata=json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1','--filter-platform',target]))
nodes={n['id']:n for n in metadata['resolve']['nodes']}
active=set();pending=[metadata['resolve']['root']]
while pending:
    id=pending.pop()
    if id in active:continue
    active.add(id)
    pending.extend(d['pkg'] for d in nodes[id]['deps'] if any(k['kind']!='dev' for k in d['dep_kinds']))
index=[]
for package in metadata['packages']:
    if package['name']=='ru-dbviewer' or package['id'] not in active:continue
    root=Path(package['manifest_path']).parent
    destination=output/f"{package['name']}-{package['version']}"
    copied=[]
    for path in root.rglob('*'):
        if not path.is_file() or not path.name.upper().startswith(('LICENSE','COPYING','NOTICE','COPYRIGHT')):continue
        relative=path.relative_to(root);target=destination/relative;target.parent.mkdir(parents=True,exist_ok=True)
        shutil.copyfile(path,target);copied.append(str(relative))
    if not copied and package['name'] in ('objc2-core-foundation','objc2-system-configuration') and package['version']=='0.3.2':
        fallback=Path(__file__).resolve().parents[1]/'third_party/objc2-LICENSE.md'
        destination.mkdir(parents=True,exist_ok=True);shutil.copyfile(fallback,destination/'LICENSE.md');shutil.copyfile(fallback.parent/'Apache-2.0.txt',destination/'LICENSE-APACHE');copied=['LICENSE.md','LICENSE-APACHE']
    if not copied:
        raise SystemExit(f"No license text found for {package['name']} {package['version']}; review before packaging")
    index.append({'name':package['name'],'version':package['version'],'license':package['license'],'repository':package.get('repository'),'files':copied})
(output/'index.json').write_text(json.dumps(index,indent=2)+'\n')
