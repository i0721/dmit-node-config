#!/usr/bin/env python3
import pathlib,subprocess,tempfile,sys
root=pathlib.Path(__file__).resolve().parents[1]
binary=str(pathlib.Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory() as directory:
    work=pathlib.Path(directory); config=work/'config'; config.mkdir(mode=0o700)
    # Public, nonproduction fixture; no host services are touched.
    for source,name in [('config/node.example.toml','node.toml'),('config/secrets.example.json','secrets.json')]:
        text=(root/source).read_text()
        if name=='secrets.json':
            import base64,json
            text=json.dumps({'xhttp_uuid':'10a6cfe1-d381-4c46-865e-998b28b801a1','ws_uuid':'20a6cfe1-d381-4c46-865e-998b28b801a2','reality_uuid':'30a6cfe1-d381-4c46-865e-998b28b801a3','reality_private_key':base64.urlsafe_b64encode(bytes([7]*32)).decode().rstrip('='),'reality_short_id':'0123456789abcdef'})
        path=config/name; path.write_text(text); path.chmod(0o600)
    subprocess.run([binary,'--config-dir',str(config),'--state-dir',str(work/'state'),'validate'],check=True)
