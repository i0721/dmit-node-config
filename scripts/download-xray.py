#!/usr/bin/env python3
import hashlib,pathlib,platform,sys,tempfile,tomllib,urllib.request,zipfile
root=pathlib.Path(__file__).resolve().parents[1]
lock=tomllib.loads((root/'versions.lock.toml').read_text())['xray']
machine=platform.machine()
arch={'x86_64':'amd64','aarch64':'arm64'}.get(machine)
if not arch: raise SystemExit('Unsupported Xray architecture: '+machine)
with tempfile.TemporaryDirectory() as work:
    archive=pathlib.Path(work)/'xray.zip'
    urllib.request.urlretrieve(lock[f'linux_{arch}_url'],archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest()!=lock[f'linux_{arch}_archive_sha256']: raise SystemExit('Xray archive digest mismatch')
    with zipfile.ZipFile(archive) as package: binary=package.read('xray')
    output=pathlib.Path(sys.argv[1]); output.write_bytes(binary); output.chmod(0o755)
print('Verified official Xray '+lock['version']+' '+arch)
