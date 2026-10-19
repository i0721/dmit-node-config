// Build-time only. The signed release archive carries static files and their digest manifest.
import {createHash} from 'node:crypto';
import {readFile,readdir,stat,mkdir,copyFile,writeFile} from 'node:fs/promises';
import {resolve,join,relative} from 'node:path';
const root=resolve(import.meta.dirname,'..');
const source=join(root,'webui/apps/web/dist');
const destination=resolve(process.argv[2]??join(root,'dist/admin-static'));
const version=(await readFile(join(root,'Cargo.toml'),'utf8')).match(/^version = "([^"]+)"/m)?.[1];
if(!version)throw new Error('Package version required');
const files={};let total=0;
async function walk(dir){
  for(const entry of await readdir(dir,{withFileTypes:true})){
    const from=join(dir,entry.name);const name=relative(source,from).replaceAll('\\','/');
    if(entry.isDirectory()){await walk(from);continue;}
    if(!entry.isFile()||!name.split('/').every(p=>/^[A-Za-z0-9_-][A-Za-z0-9_!.-]*$/.test(p)))throw new Error('Invalid static file');
    if(!/\.(html|js|css|json|txt|ico|png|jpg|jpeg|webp|svg|woff|woff2)$/.test(name))throw new Error('Unsupported static asset; rebuild the export before packaging');
    total+=(await stat(from)).size;
    if(total>100*1024*1024||Object.keys(files).length>=10000)throw new Error('Static limit');
    files[name]=createHash('sha256').update(await readFile(from)).digest('hex');
    const to=join(destination,'webui',name);await mkdir(resolve(to,'..'),{recursive:true});await copyFile(from,to);
  }
}
await walk(source);
if(!files['index.html']||!Object.keys(files).some(p=>p.startsWith('_next/static/')))throw new Error('Build static UI first');
await mkdir(destination,{recursive:true});
const report=JSON.parse(await readFile(join(root,'dist/admin-frontend-licenses.json'),'utf8'));
let notices='DMIT Console uses rin721/console-web at commit 60189fd7ccbc1c034034ed0ca5ae30d61a448026.\nUpstream source: https://github.com/rin721/console-web\n\n';
for(const [license,entries]of Object.entries(report)){
  for(const entry of entries){
    notices+=`${entry.name} ${entry.versions.join(', ')}\nLicense: ${license}\n${entry.homepage??''}\n`;
    for(const dir of entry.paths){
      for(const name of (await readdir(dir)).filter(n=>/^(licen[sc]e|notice|copying)(\..*)?$/i.test(n))){
        const path=join(dir,name);if((await stat(path)).isFile())notices+=(await readFile(path,'utf8'))+'\n';
      }
    }
    notices+='\n';
  }
}
await writeFile(join(destination,'webui/THIRD_PARTY.txt'),notices);
files['THIRD_PARTY.txt']=createHash('sha256').update(notices).digest('hex');
await writeFile(join(destination,'webui-manifest.json'),JSON.stringify({version,files},null,2)+'\n');
console.log(`Static WebUI ${version}: ${Object.keys(files).length} files, ${total} bytes`);
